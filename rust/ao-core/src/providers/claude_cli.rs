//! Claude through the local `claude` program (the user's own login, no API key). Port of claude_cli.py.
//! Each call runs `claude -p` once with the program's own tools off; ours go through a JSON protocol in the
//! prompt: the model answers with {"tool_calls": [...]} and only tools offered in this call are accepted.
use super::base::*;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

pub const TOOL_RULES: &str = concat!(
    "You have NO built-in tools in this session; never try to call one, a native call fails. The tools below are ",
    "not native: you call one by writing, as your whole reply, a single JSON object and nothing else: ",
    "{\"tool_calls\": [{\"name\": \"<tool>\", \"arguments\": {...}}]}. Do this whenever the task needs one of them. ",
    "If no tool is needed, answer in plain text.\nTools:\n"
);
pub const REMINDER: &str = "\n\n[Reminder] Reply with the JSON tool_calls object if you need a listed tool; no native tools exist.";

static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```(?:json)?\s*(\{.*?\})\s*```").expect("valid pattern"));

#[derive(Debug)]
pub enum RunError {
    Timeout(String),
    NotFound(String),
    Io(String),
}

/// (argv, stdin, env, timeout seconds) -> (exit code, stdout, stderr)
pub type Runner = Arc<dyn Fn(&[String], &str, &HashMap<String, String>, f64) -> Result<(i32, String, String), RunError> + Send + Sync>;

/// What people paste is not always a path: a PowerShell call such as & "C:/.../claude.exe", or a quoted path.
pub fn normalise_exe(value: &str) -> String {
    let mut s = value.trim();
    if let Some(rest) = s.strip_prefix('&') {
        s = rest.trim();
    }
    s.trim_matches(|c| c == '"' || c == '\'').trim().to_string()
}

/// A cleaned path; a folder that holds the program stands for the program.
fn resolve(value: &str) -> String {
    let s = normalise_exe(value);
    if !s.is_empty() && Path::new(&s).is_dir() {
        for name in ["claude.exe", "claude"] {
            let p = Path::new(&s).join(name);
            if p.is_file() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    s
}

/// The program on PATH, else the newest copy bundled with the Claude desktop app (Windows).
pub fn find_claude() -> String {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let root = Path::new(&appdata).join("Claude").join("claude-code");
        let mut found: Vec<(std::time::SystemTime, std::path::PathBuf)> = vec![];
        for v in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            for h in std::fs::read_dir(v.path()).into_iter().flatten().flatten() {
                let exe = h.path().join("claude.exe");
                if let Ok(m) = std::fs::metadata(&exe) {
                    found.push((m.modified().unwrap_or(std::time::UNIX_EPOCH), exe));
                }
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, p)) = found.first() {
            return p.to_string_lossy().into_owned();
        }
    }
    "claude".to_string()
}

fn default_runner(argv: &[String], stdin: &str, env: &HashMap<String, String>, timeout: f64) -> Result<(i32, String, String), RunError> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { RunError::NotFound(e.to_string()) } else { RunError::Io(e.to_string()) })?;
    let mut pipe = child.stdin.take().expect("piped");
    let input = stdin.to_string();
    std::thread::spawn(move || {
        let _ = pipe.write_all(input.as_bytes());
    });
    let (mut out, mut err) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
    let o = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut s);
        s
    });
    let e = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut err, &mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed().as_secs_f64() > timeout => {
                let _ = child.kill();
                return Err(RunError::Timeout(format!("timed out after {timeout} seconds")));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(RunError::Io(e.to_string())),
        }
    };
    Ok((status.code().unwrap_or(-1), o.join().unwrap_or_default(), e.join().unwrap_or_default()))
}

fn flatten(messages: &[Message]) -> String {
    let mut parts = vec![];
    for m in messages {
        match m.role.as_str() {
            "user" => parts.push(format!("[User]\n{}", m.content)),
            "assistant" => {
                let calls: String = m.tool_calls.iter().map(|t| format!("\n(called tool {} with {})", t.name, py_dumps(&t.arguments))).collect();
                parts.push(format!("[Assistant]\n{}{}", m.content, calls));
            }
            "tool" => parts.push(format!("[Tool result]\n{}", m.content)),
            _ => {}
        }
    }
    parts.join("\n\n")
}

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn classify_text(text: &str) -> ProviderError {
    let low = text.to_lowercase();
    if ["/login", "not logged in", "invalid api key", "authenticat"].iter().any(|k| low.contains(k)) {
        return ProviderError::new(Kind::Auth, format!("Claude login needed: run `claude` once and sign in. ({})", head(text, 200)));
    }
    if ["limit", "rate", "overloaded"].iter().any(|k| low.contains(k)) {
        return ProviderError::new(Kind::RateLimit, head(text, 300));
    }
    ProviderError::new(Kind::Server, head(text, 300))
}

pub struct ClaudeCliProvider {
    pub exe: String,
    run: Runner,
    timeout: f64,
}

impl ClaudeCliProvider {
    pub fn new(base_url: &str, runner: Option<Runner>) -> Self {
        let r = resolve(base_url);
        ClaudeCliProvider { exe: if r.is_empty() { find_claude() } else { r }, run: runner.unwrap_or_else(|| Arc::new(default_runner)), timeout: 300.0 }
    }

    fn extract_calls(text: &str, allowed: &[&str]) -> (String, Vec<ToolCall>) {
        let m = FENCE.captures(text);
        let blob = m.as_ref().map(|c| c[1].to_string()).unwrap_or_else(|| text.trim().to_string());
        let Ok(obj) = serde_json::from_str::<Value>(&blob) else { return (text.to_string(), vec![]) };
        let Some(raw) = obj.get("tool_calls").and_then(|v| v.as_array()) else { return (text.to_string(), vec![]) };
        let calls: Vec<ToolCall> = raw
            .iter()
            .filter_map(|c| {
                let name = c.get("name")?.as_str()?;
                if !allowed.contains(&name) {
                    return None;
                }
                let args = match c.get("arguments") {
                    Some(a) if !a.is_null() && a != &json!({}) && a != &json!("") => a.clone(),
                    _ => json!({}),
                };
                Some(ToolCall::new(&format!("call_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]), name, args))
            })
            .collect();
        if calls.is_empty() {
            return (text.to_string(), vec![]);
        }
        let rest = m.map(|c| text.replace(&c[0], "")).unwrap_or_default().trim().to_string();
        (rest, calls)
    }
}

impl Provider for ClaudeCliProvider {
    fn complete(&self, model: &str, messages: &[Message], tools: Option<&[ToolSpec]>, _max_tokens: u32, _temperature: Option<f64>) -> Result<Completion, ProviderError> {
        let tools = tools.filter(|t| !t.is_empty());
        let mut system = messages.iter().filter(|m| m.role == "system").map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n\n");
        if let Some(ts) = tools {
            if !system.is_empty() {
                system.push_str("\n\n");
            }
            system.push_str(TOOL_RULES);
            let lines: Vec<String> = ts.iter().map(|t| format!("- {}: {} Parameters: {}", t.name, t.description, py_dumps(&t.parameters))).collect();
            system.push_str(&lines.join("\n"));
        }
        let argv: Vec<String> = ["-p", "--output-format", "json", "--tools", "", "--model"]
            .iter()
            .map(|s| s.to_string())
            .chain([model.to_string(), "--system-prompt".into(), system, "--no-session-persistence".into()])
            .collect();
        let argv: Vec<String> = std::iter::once(self.exe.clone()).chain(argv).collect();
        let env: HashMap<String, String> = std::env::vars().filter(|(k, _)| k != "ANTHROPIC_API_KEY").collect();
        let stdin = flatten(messages) + if tools.is_some() { REMINDER } else { "" };
        let (code, out, err) = (self.run)(&argv, &stdin, &env, self.timeout).map_err(|e| match e {
            RunError::Timeout(m) => ProviderError::new(Kind::Network, format!("claude did not answer in time: {m}")),
            RunError::NotFound(m) | RunError::Io(m) => ProviderError::new(Kind::Network, format!("could not run claude ({}): {m}", self.exe)),
        })?;
        let Ok(data) = serde_json::from_str::<Value>(&out) else {
            let t = if !err.is_empty() { &err } else if !out.is_empty() { &out } else { "claude returned no output" };
            return Err(classify_text(t));
        };
        let result_text = match &data["result"] {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            v => v.to_string(),
        };
        if code != 0 || data["is_error"].as_bool().unwrap_or(false) {
            return Err(classify_text(if result_text.is_empty() { &err } else { &result_text }));
        }
        let (mut text, mut calls) = (result_text, vec![]);
        if let Some(ts) = tools {
            let names: Vec<&str> = ts.iter().map(|t| t.name.as_str()).collect();
            (text, calls) = Self::extract_calls(&text, &names);
        }
        let u = &data["usage"];
        Ok(Completion {
            text,
            stop_reason: if calls.is_empty() { "end_turn" } else { "tool_use" }.to_string(),
            tool_calls: calls,
            usage: Usage { input_tokens: u["input_tokens"].as_u64().unwrap_or(0), output_tokens: u["output_tokens"].as_u64().unwrap_or(0) },
            model: model.to_string(),
        })
    }

    fn stream(&self, model: &str, messages: &[Message], tools: Option<&[ToolSpec]>, max_tokens: u32, temperature: Option<f64>) -> Result<Vec<String>, ProviderError> {
        Ok(vec![self.complete(model, messages, tools, max_tokens, temperature)?.text])
    }
}
