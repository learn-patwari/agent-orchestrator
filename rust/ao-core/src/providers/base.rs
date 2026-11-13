//! Neutral provider shapes (FR-6) and the error taxonomy. Adapters map these to a vendor API.
use serde_json::{json, ser::Formatter, Value};
use std::io;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Unknown,
    Auth,
    Model,
    RateLimit,
    Network,
    BadRequest,
    Server,
}

impl Kind {
    /// The `kind` strings the Python backend exposes.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Unknown => "unknown",
            Kind::Auth => "auth",
            Kind::Model => "model",
            Kind::RateLimit => "rate_limit",
            Kind::Network => "network",
            Kind::BadRequest => "bad_request",
            Kind::Server => "server",
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ProviderError {
    pub kind: Kind,
    pub message: String,
}

impl ProviderError {
    pub fn new(kind: Kind, message: impl Into<String>) -> Self {
        ProviderError { kind, message: message.into() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON schema
}

impl ToolSpec {
    pub fn new(name: &str, description: &str, parameters: Value) -> Self {
        ToolSpec { name: name.into(), description: description.into(), parameters }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl ToolCall {
    pub fn new(id: &str, name: &str, arguments: Value) -> Self {
        ToolCall { id: id.into(), name: name.into(), arguments }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: String, // system | user | assistant | tool
    pub content: String,
    pub tool_calls: Vec<ToolCall>,     // on assistant
    pub tool_call_id: Option<String>, // on tool
}

impl Message {
    pub fn new(role: &str, content: &str) -> Self {
        Message { role: role.into(), content: content.into(), tool_calls: vec![], tool_call_id: None }
    }
    pub fn assistant(content: &str, tool_calls: Vec<ToolCall>) -> Self {
        Message { tool_calls, ..Message::new("assistant", content) }
    }
    pub fn tool(content: &str, tool_call_id: &str) -> Self {
        Message { tool_call_id: Some(tool_call_id.into()), ..Message::new("tool", content) }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
    pub stop_reason: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Sends one POST. `Err` means the request never got an answer (connection, timeout).
pub type Transport = Arc<dyn Fn(&HttpRequest) -> Result<HttpResponse, String> + Send + Sync>;

pub trait Provider: Send + Sync {
    fn complete(&self, model: &str, messages: &[Message], tools: Option<&[ToolSpec]>, max_tokens: u32, temperature: Option<f64>) -> Result<Completion, ProviderError>;
    fn stream(&self, model: &str, messages: &[Message], tools: Option<&[ToolSpec]>, max_tokens: u32, temperature: Option<f64>) -> Result<Vec<String>, ProviderError>;

    fn test_connection(&self, model: &str) -> Result<Value, ProviderError> {
        let c = self.complete(model, &[Message::new("user", "Reply with the word ok.")], None, 8, None)?;
        let m = if c.model.is_empty() { model.to_string() } else { c.model.clone() };
        Ok(json!({"ok": true, "model": m, "usage": {"input_tokens": c.usage.input_tokens, "output_tokens": c.usage.output_tokens}}))
    }
}

pub fn classify(status: u16, body: &str) -> ProviderError {
    let msg = format!("HTTP {status}: {}", body.chars().take(300).collect::<String>());
    let kind = match status {
        401 | 403 => Kind::Auth,
        404 => Kind::Model,
        429 => Kind::RateLimit,
        s if s >= 500 => Kind::Server,
        400 | 422 if body.to_lowercase().contains("model") => Kind::Model,
        _ => Kind::BadRequest,
    };
    ProviderError::new(kind, msg)
}

// ---- Python-compatible JSON text (json.dumps defaults: ", " and ": " separators, ASCII only) ----

struct PyFormatter;

impl Formatter for PyFormatter {
    fn begin_object_key<W: ?Sized + io::Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }
    fn begin_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }
    fn write_string_fragment<W: ?Sized + io::Write>(&mut self, w: &mut W, fragment: &str) -> io::Result<()> {
        for ch in fragment.chars() {
            if ch.is_ascii() {
                w.write_all(&[ch as u8])?;
            } else {
                let mut buf = [0u16; 2];
                for unit in ch.encode_utf16(&mut buf) {
                    write!(w, "\\u{:04x}", unit)?;
                }
            }
        }
        Ok(())
    }
}

/// Same text as Python's `json.dumps(value)`, so both backends send identical tool-argument strings.
pub fn py_dumps(v: &Value) -> String {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PyFormatter);
    serde::Serialize::serialize(v, &mut ser).expect("a Value always serialises");
    String::from_utf8(out).expect("ascii")
}
