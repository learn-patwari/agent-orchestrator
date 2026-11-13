//! The two HTTP protocols: OpenAI style (`/chat/completions`) and Anthropic style (`/v1/messages`).
//! One struct, one protocol switch; the request and response mapping match backend/app/providers/*_compat.py.
use super::base::*;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAi,
    Anthropic,
}

pub struct HttpProvider {
    protocol: Protocol,
    base_url: String,
    api_key: String,
    transport: Transport,
    max_retries: u32,
    backoff: f64,
}

/// The real network call (ureq, rustls). Any HTTP status is an answer; only connection failures are `Err`.
pub fn default_transport(timeout_secs: u64) -> Transport {
    Arc::new(move |req: &HttpRequest| {
        let mut r = ureq::post(&req.url).timeout(Duration::from_secs(timeout_secs));
        for (k, v) in &req.headers {
            r = r.set(k, v);
        }
        match r.send_string(&req.body.to_string()) {
            Ok(resp) => {
                let status = resp.status();
                Ok(HttpResponse { status, body: resp.into_string().map_err(|e| e.to_string())? })
            }
            Err(ureq::Error::Status(status, resp)) => Ok(HttpResponse { status, body: resp.into_string().unwrap_or_default() }),
            Err(e) => Err(e.to_string()),
        }
    })
}

impl HttpProvider {
    pub fn new(protocol: Protocol, base_url: &str, api_key: &str, transport: Option<Transport>) -> Self {
        HttpProvider {
            protocol,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            transport: transport.unwrap_or_else(|| default_transport(120)),
            max_retries: 3,
            backoff: 0.5,
        }
    }

    pub fn backoff(mut self, seconds: f64) -> Self {
        self.backoff = seconds;
        self
    }

    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    fn request(&self, model: &str, messages: &[Message], tools: Option<&[ToolSpec]>, max_tokens: u32, temperature: Option<f64>, stream: bool) -> HttpRequest {
        let tools = tools.filter(|t| !t.is_empty());
        let mut body = Map::new();
        body.insert("model".into(), json!(model));
        let (url, headers) = match self.protocol {
            Protocol::OpenAi => {
                body.insert("messages".into(), Value::Array(messages.iter().map(openai_message).collect()));
                body.insert("max_tokens".into(), json!(max_tokens));
                if let Some(t) = temperature {
                    body.insert("temperature".into(), json!(t));
                }
                if let Some(ts) = tools {
                    let specs = ts.iter().map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}));
                    body.insert("tools".into(), Value::Array(specs.collect()));
                }
                (
                    format!("{}/chat/completions", self.base_url),
                    vec![("Authorization".to_string(), format!("Bearer {}", self.api_key)), ("Content-Type".into(), "application/json".into())],
                )
            }
            Protocol::Anthropic => {
                let system: Vec<&str> = messages.iter().filter(|m| m.role == "system").map(|m| m.content.as_str()).collect();
                body.insert("messages".into(), Value::Array(anthropic_messages(messages)));
                body.insert("max_tokens".into(), json!(max_tokens));
                if !system.is_empty() && !system.join("\n\n").is_empty() {
                    body.insert("system".into(), json!(system.join("\n\n")));
                }
                if let Some(t) = temperature {
                    body.insert("temperature".into(), json!(t));
                }
                if let Some(ts) = tools {
                    let specs = ts.iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters}));
                    body.insert("tools".into(), Value::Array(specs.collect()));
                }
                (
                    format!("{}/v1/messages", self.base_url),
                    vec![
                        ("x-api-key".to_string(), self.api_key.clone()),
                        ("anthropic-version".into(), "2023-06-01".into()),
                        ("Content-Type".into(), "application/json".into()),
                    ],
                )
            }
        };
        if stream {
            body.insert("stream".into(), json!(true));
        }
        HttpRequest { url, headers, body: Value::Object(body) }
    }

    /// POST with retries on rate limits, server errors and network failures (backoff doubles each time).
    fn post(&self, req: &HttpRequest) -> Result<HttpResponse, ProviderError> {
        let mut last: Option<ProviderError> = None;
        for attempt in 0..=self.max_retries {
            match (self.transport)(req) {
                Err(e) => last = Some(ProviderError::new(Kind::Network, e)),
                Ok(r) if r.status < 400 => return Ok(r),
                Ok(r) => {
                    let err = classify(r.status, &r.body);
                    if !matches!(err.kind, Kind::RateLimit | Kind::Server) {
                        return Err(err); // auth/model/bad request: retrying will not help
                    }
                    last = Some(err);
                }
            }
            if attempt < self.max_retries && self.backoff > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(self.backoff * f64::from(1u32 << attempt.min(16))));
            }
        }
        Err(last.expect("at least one attempt"))
    }

    fn parse(&self, data: &Value) -> Result<Completion, ProviderError> {
        let bad = |what: &str| ProviderError::new(Kind::Server, format!("unexpected reply from the model API: {what}"));
        let model = data["model"].as_str().unwrap_or("").to_string();
        let u = &data["usage"];
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        match self.protocol {
            Protocol::OpenAi => {
                let choice = data["choices"].get(0).ok_or_else(|| bad("no choices"))?;
                let msg = &choice["message"];
                let mut calls = vec![];
                for tc in msg["tool_calls"].as_array().into_iter().flatten() {
                    let raw = match tc["function"]["arguments"].as_str() {
                        Some(s) if !s.is_empty() => s,
                        _ => "{}",
                    };
                    let args = serde_json::from_str(raw).unwrap_or_else(|_| json!({"_raw": raw}));
                    calls.push(ToolCall::new(tc["id"].as_str().unwrap_or(""), tc["function"]["name"].as_str().unwrap_or(""), args));
                }
                Ok(Completion {
                    text: msg["content"].as_str().unwrap_or("").to_string(),
                    tool_calls: calls,
                    usage: Usage { input_tokens: n("prompt_tokens"), output_tokens: n("completion_tokens") },
                    stop_reason: choice["finish_reason"].as_str().unwrap_or("").to_string(),
                    model,
                })
            }
            Protocol::Anthropic => {
                let (mut text, mut calls) = (String::new(), vec![]);
                for b in data["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => text.push_str(b["text"].as_str().unwrap_or("")),
                        Some("tool_use") => {
                            let input = if b["input"].is_null() { json!({}) } else { b["input"].clone() };
                            calls.push(ToolCall::new(b["id"].as_str().unwrap_or(""), b["name"].as_str().unwrap_or(""), input));
                        }
                        _ => {}
                    }
                }
                Ok(Completion {
                    text,
                    tool_calls: calls,
                    usage: Usage { input_tokens: n("input_tokens"), output_tokens: n("output_tokens") },
                    stop_reason: data["stop_reason"].as_str().unwrap_or("").to_string(),
                    model,
                })
            }
        }
    }

    fn stream_delta(&self, data: &Value) -> Option<String> {
        let d = match self.protocol {
            Protocol::OpenAi => data["choices"].get(0).map(|c| &c["delta"]["content"]),
            Protocol::Anthropic if data["type"] == "content_block_delta" => Some(&data["delta"]["text"]),
            Protocol::Anthropic => None,
        };
        d.and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from)
    }
}

fn openai_message(m: &Message) -> Value {
    if m.role == "tool" {
        json!({"role": "tool", "tool_call_id": m.tool_call_id, "content": m.content})
    } else if m.role == "assistant" && !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|t| json!({"id": t.id, "type": "function", "function": {"name": t.name, "arguments": py_dumps(&t.arguments)}}))
            .collect();
        let content = if m.content.is_empty() { Value::Null } else { json!(m.content) };
        json!({"role": "assistant", "content": content, "tool_calls": calls})
    } else {
        json!({"role": m.role, "content": m.content})
    }
}

fn anthropic_messages(messages: &[Message]) -> Vec<Value> {
    let mut msgs: Vec<Value> = vec![];
    for m in messages.iter().filter(|m| m.role != "system") {
        if m.role == "tool" {
            let block = json!({"type": "tool_result", "tool_use_id": m.tool_call_id, "content": m.content});
            if let Some(Value::Array(content)) = msgs.last_mut().filter(|l| l["role"] == "user").and_then(|l| l.get_mut("content")) {
                content.push(block); // merge consecutive tool results
            } else {
                msgs.push(json!({"role": "user", "content": [block]}));
            }
        } else if m.role == "assistant" && !m.tool_calls.is_empty() {
            let mut blocks = vec![];
            if !m.content.is_empty() {
                blocks.push(json!({"type": "text", "text": m.content}));
            }
            blocks.extend(m.tool_calls.iter().map(|t| json!({"type": "tool_use", "id": t.id, "name": t.name, "input": t.arguments})));
            msgs.push(json!({"role": "assistant", "content": blocks}));
        } else {
            msgs.push(json!({"role": m.role, "content": m.content}));
        }
    }
    msgs
}

impl Provider for HttpProvider {
    fn complete(&self, model: &str, messages: &[Message], tools: Option<&[ToolSpec]>, max_tokens: u32, temperature: Option<f64>) -> Result<Completion, ProviderError> {
        let req = self.request(model, messages, tools, max_tokens, temperature, false);
        let r = self.post(&req)?;
        let data: Value = serde_json::from_str(&r.body).map_err(|e| ProviderError::new(Kind::Server, format!("the model API did not answer with JSON: {e}")))?;
        self.parse(&data)
    }

    /// Text deltas, in order. The reply is read in full before it is split (no incremental delivery yet).
    fn stream(&self, model: &str, messages: &[Message], tools: Option<&[ToolSpec]>, max_tokens: u32, temperature: Option<f64>) -> Result<Vec<String>, ProviderError> {
        let req = self.request(model, messages, tools, max_tokens, temperature, true);
        let r = (self.transport)(&req).map_err(|e| ProviderError::new(Kind::Network, e))?;
        if r.status >= 400 {
            return Err(classify(r.status, &r.body));
        }
        let mut out = vec![];
        for line in r.body.lines() {
            if let Some(raw) = line.strip_prefix("data:") {
                let raw = raw.trim();
                if raw == "[DONE]" {
                    break;
                }
                let Ok(v) = serde_json::from_str::<Value>(raw) else { continue };
                if let Some(d) = self.stream_delta(&v) {
                    out.push(d);
                }
            }
        }
        Ok(out)
    }
}
