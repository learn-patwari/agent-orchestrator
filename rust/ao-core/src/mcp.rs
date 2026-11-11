//! MCP client: stdio and HTTP transports, per-role allow-lists, secrets by reference.
//!
//! A server is configured by name with the roles that may use it. A role sees a server's tools (named
//! `mcp__<server>__<tool>`) only if the server lists that role; the app works with zero servers, and one server that
//! fails to start never removes an agent's other tools. Secrets are never stored: `env_refs` maps a variable the server
//! needs to the name of an environment variable on this machine, resolved only when the server is started. Whatever a
//! tool returns is untrusted data (the run loop wraps it like every tool result).
use crate::db::{DbError, DocRepo};
use crate::providers::base::{py_dumps, HttpRequest, ToolSpec, Transport};
use crate::providers::http::default_transport;
use crate::runtime::tools::{McpTools, ToolError};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

pub const TIMEOUT: u64 = 20;
static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z0-9][a-z0-9_-]*$").expect("valid pattern"));

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

fn err<T>(m: impl Into<String>) -> Result<T, McpError> {
    Err(McpError::Msg(m.into()))
}

fn rpc_result(msg: &Value) -> Result<Value, McpError> {
    if let Some(e) = msg.get("error") {
        return err(e.get("message").map_or(e.to_string(), |m| m.as_str().map_or(m.to_string(), String::from)));
    }
    Ok(msg.get("result").cloned().unwrap_or(json!({})))
}

pub trait McpTransport: Send {
    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), McpError>;
    fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value, McpError>;
    fn close(&mut self);
}

pub struct StdioTransport {
    command: Vec<String>,
    env: Vec<(String, String)>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Option<Receiver<Option<Value>>>,
    id: i64,
}

impl StdioTransport {
    pub fn new(command: &[String], env: &[(String, String)]) -> Self {
        StdioTransport { command: command.to_vec(), env: env.to_vec(), child: None, stdin: None, lines: None, id: 0 }
    }

    fn start(&mut self) -> Result<(), McpError> {
        let Some((prog, args)) = self.command.split_first() else { return err("could not start the server: empty command") };
        let mut child = Command::new(prog)
            .args(args)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError::Msg(format!("could not start the server: {e}")))?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Msg("could not start the server: no output pipe".into()))?;
        self.stdin = child.stdin.take();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if tx.send(Some(v)).is_err() {
                        return;
                    }
                } // not JSON-RPC: ignore stray output
            }
            let _ = tx.send(None); // the server closed its output
        });
        self.child = Some(child);
        self.lines = Some(rx);
        Ok(())
    }

    fn send(&mut self, msg: &Value) -> Result<(), McpError> {
        if self.child.is_none() {
            self.start()?;
        }
        let Some(stdin) = self.stdin.as_mut() else { return err("the server stopped: no input pipe") };
        writeln!(stdin, "{}", py_dumps(msg)).and_then(|_| stdin.flush()).map_err(|e| McpError::Msg(format!("the server stopped: {e}")))
    }
}

impl McpTransport for StdioTransport {
    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params.unwrap_or(json!({}))}))
    }

    fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        self.id += 1;
        let id = self.id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params.unwrap_or(json!({}))}))?;
        loop {
            let got = self.lines.as_ref().map(|rx| rx.recv_timeout(Duration::from_secs(TIMEOUT)));
            match got {
                Some(Ok(Some(msg))) => {
                    if msg.get("id").and_then(|i| i.as_i64()) == Some(id) {
                        return rpc_result(&msg);
                    }
                }
                Some(Ok(None)) | Some(Err(RecvTimeoutError::Disconnected)) | None => return err("the server closed the connection"),
                Some(Err(RecvTimeoutError::Timeout)) => return err(format!("the server did not answer {method} in {TIMEOUT} s")),
            }
        }
    }

    fn close(&mut self) {
        if let Some(mut c) = self.child.take() {
            if matches!(c.try_wait(), Ok(None)) {
                let _ = c.kill();
            }
            let _ = c.wait();
        }
        self.stdin = None;
        self.lines = None;
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct HttpTransport {
    url: String,
    send: Transport,
    headers: Vec<(String, String)>,
    id: i64,
}

impl HttpTransport {
    pub fn new(url: &str, transport: Option<Transport>) -> Self {
        HttpTransport { url: url.into(), send: transport.unwrap_or_else(|| default_transport(TIMEOUT)), headers: vec![("content-type".into(), "application/json".into())], id: 0 }
    }

    fn post(&self, body: Value) -> Result<crate::providers::base::HttpResponse, String> {
        (self.send)(&HttpRequest { url: self.url.clone(), headers: self.headers.clone(), body })
    }
}

impl McpTransport for HttpTransport {
    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        let _ = self.post(json!({"jsonrpc": "2.0", "method": method, "params": params.unwrap_or(json!({}))})); // a failed notification is ignored
        Ok(())
    }

    fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        self.id += 1;
        let did_not = |e: String| McpError::Msg(format!("the server did not answer {method}: {e}"));
        let r = self.post(json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params.unwrap_or(json!({}))})).map_err(did_not)?;
        if !(200..300).contains(&r.status) {
            return Err(did_not(format!("HTTP {}", r.status)));
        }
        let msg: Value = serde_json::from_str(&r.body).map_err(|e| did_not(e.to_string()))?;
        rpc_result(&msg)
    }

    fn close(&mut self) {}
}

pub struct McpClient {
    t: Mutex<Box<dyn McpTransport>>,
}

impl McpClient {
    pub fn new(t: Box<dyn McpTransport>) -> Self {
        McpClient { t: Mutex::new(t) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut dyn McpTransport) -> R) -> R {
        let mut g = self.t.lock().unwrap_or_else(|e| e.into_inner());
        f(g.as_mut())
    }

    pub fn initialize(&self) -> Result<Value, McpError> {
        self.with(|t| {
            let out = t.request("initialize", Some(json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "agent-orchestrator", "version": "1"}})))?;
            t.notify("notifications/initialized", None)?;
            Ok(out)
        })
    }

    pub fn list_tools(&self) -> Result<Vec<Value>, McpError> {
        let r = self.with(|t| t.request("tools/list", None))?;
        Ok(r["tools"].as_array().cloned().unwrap_or_default())
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> Result<String, McpError> {
        let res = self.with(|t| t.request("tools/call", Some(json!({"name": name, "arguments": args}))))?;
        let text = res["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "text").map(|b| b["text"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
        let failed = !(res["isError"].is_null() || res["isError"] == false);
        if failed {
            return err(if text.is_empty() { "the tool reported an error".to_string() } else { text });
        }
        Ok(text)
    }

    pub fn close(&self) {
        self.with(|t| t.close());
    }
}

pub struct McpServers;

impl McpServers {
    fn repo(&self) -> Result<DocRepo, McpError> {
        Ok(DocRepo::new("mcp_servers")?)
    }

    pub fn list(&self) -> Result<Vec<Value>, McpError> {
        Ok(self.repo()?.list()?)
    }

    pub fn get(&self, name: &str) -> Result<Option<Value>, McpError> {
        Ok(self.repo()?.get(name)?)
    }

    pub fn save(&self, name: &str, cfg: &Value) -> Result<Value, McpError> {
        if !NAME.is_match(name) || name.contains("__") {
            return err("a server name uses letters, digits, - and _ (no double underscore)");
        }
        let transport = cfg["transport"].as_str().unwrap_or("");
        match transport {
            "stdio" => {
                let ok = cfg["command"].as_array().is_some_and(|c| !c.is_empty() && c.iter().all(|x| x.as_str().is_some_and(|s| !s.is_empty())));
                if !ok {
                    return err("stdio needs a command: a non-empty list of strings");
                }
            }
            "http" => {
                let url = cfg["url"].as_str().unwrap_or("");
                let host_ok = url.split_once("://").is_some_and(|(sc, rest)| (sc == "http" || sc == "https") && !rest.split(['/', '?', '#']).next().unwrap_or("").is_empty());
                if !host_ok {
                    return err("http needs an http or https url");
                }
            }
            _ => return err("transport must be stdio or http"),
        }
        let roles: Vec<String> = cfg["roles"].as_array().into_iter().flatten().map(crate::runtime::py_str).collect();
        let env_refs: serde_json::Map<String, Value> = cfg["env_refs"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), json!(crate::runtime::py_str(v)))).collect();
        let doc = json!({"name": name, "transport": transport, "command": cfg.get("command").cloned().unwrap_or(Value::Null),
                         "url": cfg.get("url").cloned().unwrap_or(Value::Null), "roles": roles,
                         "enabled": cfg.get("enabled").cloned().unwrap_or(json!(true)), "env_refs": env_refs});
        let repo = self.repo()?;
        if repo.get(name)?.is_some() {
            return Ok(repo.update(name, doc)?.unwrap_or(Value::Null));
        }
        Ok(repo.create(doc, Some(name))?)
    }

    pub fn delete(&self, name: &str) -> Result<bool, McpError> {
        let repo = self.repo()?;
        if repo.get(name)?.is_none() {
            return Ok(false);
        }
        Ok(repo.delete(name)?)
    }
}

/// Starts a configured server and returns an initialised client.
pub fn connect(server: &Value, http: Option<Transport>) -> Result<McpClient, McpError> {
    let t: Box<dyn McpTransport> = if server["transport"] == "stdio" {
        let mut env = vec![];
        for (var, r) in server["env_refs"].as_object().into_iter().flatten() {
            let r = r.as_str().unwrap_or("");
            match std::env::var(r) {
                Ok(v) => env.push((var.clone(), v)),
                Err(_) => return err(format!("environment variable {r} is not set on this machine")),
            }
        }
        let command: Vec<String> = server["command"].as_array().into_iter().flatten().map(crate::runtime::py_str).collect();
        Box::new(StdioTransport::new(&command, &env))
    } else {
        Box::new(HttpTransport::new(server["url"].as_str().unwrap_or(""), http))
    };
    let c = McpClient::new(t);
    if let Err(e) = c.initialize() {
        c.close();
        return Err(e);
    }
    Ok(c)
}

/// The MCP tools one role may use in one run. Connects lazily; `close()` stops the servers it started.
pub struct RoleMcp {
    role: String,
    clients: Mutex<HashMap<String, McpClient>>,
    problems: Mutex<HashMap<String, String>>,
    specs: Mutex<Option<Vec<ToolSpec>>>,
}

impl RoleMcp {
    pub fn new(role: &str) -> Self {
        RoleMcp { role: role.into(), clients: Default::default(), problems: Default::default(), specs: Default::default() }
    }

    pub fn problems(&self) -> HashMap<String, String> {
        self.problems.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn allowed(&self) -> Vec<Value> {
        McpServers.list().unwrap_or_default().into_iter().filter(|s| s["enabled"] != false && s["roles"].as_array().is_some_and(|r| r.iter().any(|x| x == self.role.as_str()))).collect()
    }

    fn with_client<R>(&self, server: &Value, f: impl FnOnce(&McpClient) -> Result<R, McpError>) -> Result<R, McpError> {
        let name = server["name"].as_str().unwrap_or("").to_string();
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if !clients.contains_key(&name) {
            clients.insert(name.clone(), connect(server, None)?);
        }
        f(&clients[&name])
    }
}

impl McpTools for RoleMcp {
    fn specs(&self) -> Vec<ToolSpec> {
        let mut cache = self.specs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = cache.as_ref() {
            return c.clone();
        }
        let mut out = vec![];
        for s in self.allowed() {
            let name = s["name"].as_str().unwrap_or("").to_string();
            match self.with_client(&s, |c| c.list_tools()) {
                Ok(tools) => {
                    for t in tools {
                        let schema = t.get("inputSchema").filter(|v| v.as_object().is_some_and(|o| !o.is_empty())).cloned().unwrap_or(json!({"type": "object", "properties": {}}));
                        out.push(ToolSpec::new(&format!("mcp__{name}__{}", t["name"].as_str().unwrap_or("")), t["description"].as_str().unwrap_or(""), schema));
                    }
                }
                Err(e) => {
                    self.problems.lock().unwrap_or_else(|e| e.into_inner()).insert(name, e.to_string());
                }
            }
        }
        *cache = Some(out.clone());
        out
    }

    fn call(&self, name: &str, args: &Value) -> Result<String, ToolError> {
        let parts: Vec<&str> = name.splitn(3, "__").collect();
        if parts.len() != 3 {
            return Err(ToolError(format!("unknown MCP tool {name}")));
        }
        let server = McpServers.get(parts[1]).ok().flatten().ok_or_else(|| ToolError(format!("unknown MCP server {}", parts[1])))?;
        let listed = server["roles"].as_array().is_some_and(|r| r.iter().any(|x| x == self.role.as_str()));
        if server["enabled"] == false || !listed {
            return Err(ToolError(format!("MCP server {} is not allowed for the {} role", parts[1], self.role)));
        }
        self.with_client(&server, |c| c.call_tool(parts[2], args)).map_err(|e| ToolError(format!("MCP tool failed: {e}")))
    }

    fn close(&self) {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        for c in clients.values() {
            c.close();
        }
        clients.clear();
    }
}
