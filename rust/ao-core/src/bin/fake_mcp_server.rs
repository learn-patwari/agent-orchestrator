//! A tiny MCP server over stdio for tests: newline-delimited JSON-RPC 2.0 with initialize, tools/list, tools/call.
//! It is test support only and is not part of the app.
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn reply(id: &Value, result: Result<Value, Value>) {
    let mut m = json!({"jsonrpc": "2.0", "id": id});
    match result {
        Ok(r) => m["result"] = r,
        Err(e) => m["error"] = e,
    }
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{m}");
    let _ = out.flush();
}

fn main() {
    let tools = json!([
        {"name": "echo", "description": "Echo the text back.",
         "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
        {"name": "env_value", "description": "Return the value of an environment variable.",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}},
    ]);
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = req.get("id").filter(|i| !i.is_null()) else { continue }; // a notification such as notifications/initialized
        match req["method"].as_str().unwrap_or("") {
            "initialize" => reply(id, Ok(json!({"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}))),
            "tools/list" => reply(id, Ok(json!({"tools": tools}))),
            "tools/call" => {
                let p = &req["params"];
                let a = &p["arguments"];
                match p["name"].as_str().unwrap_or("") {
                    "echo" => reply(id, Ok(json!({"content": [{"type": "text", "text": format!("echo: {}", a["text"].as_str().unwrap_or(""))}]}))),
                    "env_value" => {
                        let v = std::env::var(a["name"].as_str().unwrap_or("")).unwrap_or_else(|_| "(unset)".into());
                        reply(id, Ok(json!({"content": [{"type": "text", "text": v}]})));
                    }
                    other => reply(id, Err(json!({"code": -32601, "message": format!("unknown tool {other}")}))),
                }
            }
            other => reply(id, Err(json!({"code": -32601, "message": format!("unknown method {other}")}))),
        }
    }
}
