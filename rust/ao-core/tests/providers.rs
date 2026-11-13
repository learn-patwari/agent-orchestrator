//! Phase 3: model providers. Ported from backend/tests/test_providers.py, test_claude_cli.py, test_presets.py.
//! The HTTP transport is injected, like httpx.MockTransport in the Python tests.
use ao_core::providers::base::{Completion, HttpRequest, HttpResponse, Kind, Message, Provider, ProviderError, ToolCall, ToolSpec, Transport};
use ao_core::providers::claude_cli::{normalise_exe, ClaudeCliProvider, RunError, Runner};
use ao_core::providers::http::{HttpProvider, Protocol};
use ao_core::providers::presets::PRESETS;
use ao_core::providers::pricing::cost;
use ao_core::providers::{build, ADAPTERS};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn tool() -> ToolSpec {
    ToolSpec::new("read_file", "read a file", json!({"type": "object", "properties": {"path": {"type": "string"}}}))
}

fn user(s: &str) -> Message {
    Message::new("user", s)
}

type Seen = Arc<Mutex<Vec<HttpRequest>>>;

fn transport(f: impl Fn(&HttpRequest) -> Result<HttpResponse, String> + Send + Sync + 'static) -> (Transport, Seen) {
    let seen: Seen = Arc::new(Mutex::new(vec![]));
    let s = seen.clone();
    (
        Arc::new(move |r: &HttpRequest| {
            s.lock().unwrap().push(r.clone());
            f(r)
        }),
        seen,
    )
}

fn ok(v: Value) -> Result<HttpResponse, String> {
    Ok(HttpResponse { status: 200, body: v.to_string() })
}

fn status(code: u16, body: &str) -> Result<HttpResponse, String> {
    Ok(HttpResponse { status: code, body: body.to_string() })
}

fn openai(t: Transport) -> HttpProvider {
    HttpProvider::new(Protocol::OpenAi, "https://llm.example/v1", "sk-test-123456789012345678", Some(t)).backoff(0.0)
}

fn anthropic(t: Transport) -> HttpProvider {
    HttpProvider::new(Protocol::Anthropic, "https://llm.example", "sk-test-123456789012345678", Some(t)).backoff(0.0)
}

fn header<'a>(r: &'a HttpRequest, name: &str) -> &'a str {
    r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).unwrap()
}

// ---------------------------------------------------------------- HTTP adapters

#[test]
fn openai_complete_with_tools_and_usage() {
    let (t, seen) = transport(|_| {
        ok(json!({
            "model": "m1", "usage": {"prompt_tokens": 10, "completion_tokens": 5},
            "choices": [{"finish_reason": "tool_calls", "message": {"content": null, "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\": \"a.txt\"}"}}]}}]}))
    });
    let c = openai(t).complete("m1", &[Message::new("system", "s"), user("hi")], Some(&[tool()]), 1024, None).unwrap();
    let r = seen.lock().unwrap()[0].clone();
    assert_eq!(r.url, "https://llm.example/v1/chat/completions");
    assert!(header(&r, "authorization").starts_with("Bearer sk-test"));
    assert_eq!(r.body["tools"][0]["function"]["name"], "read_file");
    assert_eq!(c.tool_calls, vec![ToolCall::new("c1", "read_file", json!({"path": "a.txt"}))]);
    assert_eq!((c.usage.input_tokens, c.usage.output_tokens), (10, 5));
    assert_eq!(c.stop_reason, "tool_calls");
}

#[test]
fn openai_bad_tool_arguments_are_kept_raw() {
    let (t, _) = transport(|_| {
        ok(json!({"choices": [{"message": {"content": "", "tool_calls": [
            {"id": "c", "function": {"name": "read_file", "arguments": "{oops"}}]}}]}))
    });
    let c = openai(t).complete("m", &[user("x")], None, 1024, None).unwrap();
    assert_eq!(c.tool_calls[0].arguments, json!({"_raw": "{oops"}));
}

#[test]
fn openai_tool_result_roundtrip_mapping() {
    let (t, seen) = transport(|_| ok(json!({"choices": [{"message": {"content": "done"}}]})));
    let msgs = [
        user("go"),
        Message::assistant("", vec![ToolCall::new("c1", "read_file", json!({"path": "a"}))]),
        Message::tool("contents", "c1"),
    ];
    openai(t).complete("m", &msgs, None, 1024, None).unwrap();
    let b = seen.lock().unwrap()[0].body.clone();
    // Python's json.dumps spacing, so both backends send the same bytes
    assert_eq!(b["messages"][1]["tool_calls"][0]["function"]["arguments"], "{\"path\": \"a\"}");
    assert_eq!(b["messages"][2], json!({"role": "tool", "tool_call_id": "c1", "content": "contents"}));
    assert_eq!(b["messages"][1]["content"], Value::Null);
    assert_eq!(b["max_tokens"], 1024);
    assert!(b.get("temperature").is_none() && b.get("stream").is_none());
}

#[test]
fn anthropic_complete_and_mapping() {
    let (t, seen) = transport(|_| {
        ok(json!({
            "model": "claude-x", "stop_reason": "tool_use", "usage": {"input_tokens": 7, "output_tokens": 3},
            "content": [{"type": "text", "text": "ok "}, {"type": "tool_use", "id": "t1", "name": "read_file", "input": {"path": "b"}}]}))
    });
    let msgs = [
        Message::new("system", "be brief"),
        user("hi"),
        Message::assistant("", vec![ToolCall::new("t0", "read_file", json!({"path": "a"}))]),
        Message::tool("r1", "t0"),
        Message::tool("r2", "t0b"),
    ];
    let c = anthropic(t).complete("claude-x", &msgs, Some(&[tool()]), 1024, None).unwrap();
    let r = seen.lock().unwrap()[0].clone();
    let b = &r.body;
    assert_eq!(r.url, "https://llm.example/v1/messages");
    assert!(header(&r, "x-api-key").starts_with("sk-test"));
    assert_eq!(header(&r, "anthropic-version"), "2023-06-01");
    assert_eq!(b["system"][0]["text"], "be brief");
    assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral", "the unchanging prompt prefix is cached");
    assert_eq!(b["tools"][0]["input_schema"]["type"], "object");
    let roles: Vec<_> = b["messages"].as_array().unwrap().iter().map(|m| m["role"].clone()).collect();
    assert_eq!(roles, vec![json!("user"), json!("assistant"), json!("user")]);
    assert_eq!(b["messages"][2]["content"].as_array().unwrap().len(), 2); // tool results merged into one user turn
    assert_eq!(c.text, "ok ");
    assert_eq!(c.tool_calls[0].arguments, json!({"path": "b"}));
    assert_eq!((c.usage.input_tokens, c.usage.output_tokens), (7, 3));
}

#[test]
fn error_taxonomy_without_retry() {
    for (code, kind) in [(401, Kind::Auth), (403, Kind::Auth), (404, Kind::Model)] {
        let (t, seen) = transport(move |_| status(code, "nope"));
        let e = openai(t).complete("m", &[user("x")], None, 1024, None).unwrap_err();
        assert_eq!(e.kind, kind);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
    let (t, _) = transport(|_| status(400, "unknown model xyz"));
    assert_eq!(openai(t).complete("m", &[user("x")], None, 1024, None).unwrap_err().kind, Kind::Model);
    let (t, _) = transport(|_| status(400, "bad json"));
    let e = openai(t).complete("m", &[user("x")], None, 1024, None).unwrap_err();
    assert_eq!((e.kind, e.message.as_str()), (Kind::BadRequest, "HTTP 400: bad json"));
    assert_eq!(Kind::RateLimit.as_str(), "rate_limit");
}

#[test]
fn retries_on_rate_limit_then_succeeds_and_gives_up_when_exhausted() {
    let n = Arc::new(Mutex::new(0));
    let n2 = n.clone();
    let (t, _) = transport(move |_| {
        let mut i = n2.lock().unwrap();
        *i += 1;
        if *i < 3 {
            status(429, "slow down")
        } else {
            ok(json!({"choices": [{"message": {"content": "hi"}}]}))
        }
    });
    assert_eq!(openai(t).complete("m", &[user("x")], None, 1024, None).unwrap().text, "hi");
    assert_eq!(*n.lock().unwrap(), 3);
    let (t, seen) = transport(|_| status(429, ""));
    let e = openai(t).max_retries(1).complete("m", &[user("x")], None, 1024, None).unwrap_err();
    assert_eq!(e.kind, Kind::RateLimit);
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn a_transport_failure_is_a_network_error() {
    let (t, _) = transport(|_| Err("refused".into()));
    let e = openai(t).max_retries(0).complete("m", &[user("x")], None, 1024, None).unwrap_err();
    assert_eq!(e.kind, Kind::Network);
}

#[test]
fn streaming_both_styles() {
    let (t, seen) = transport(|_| {
        let sse: String =
            ["Hel", "lo"].iter().map(|t| format!("data: {}\n\n", json!({"choices": [{"delta": {"content": t}}]}))).collect();
        status(200, &format!("{sse}data: [DONE]\n\n"))
    });
    let out = openai(t).stream("m", &[user("x")], None, 1024, None).unwrap();
    assert_eq!(out.concat(), "Hello");
    assert_eq!(seen.lock().unwrap()[0].body["stream"], true);

    let (t, _) = transport(|_| {
        let mut s = String::new();
        for x in ["Wor", "ld"] {
            s += &format!(
                "event: content_block_delta\ndata: {}\n\n",
                json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": x}})
            );
        }
        s += "event: message_stop\ndata: {\"type\": \"message_stop\"}\n\n";
        status(200, &s)
    });
    assert_eq!(anthropic(t).stream("m", &[user("x")], None, 1024, None).unwrap().concat(), "World");
}

#[test]
fn test_connection_reports_model_and_usage() {
    let (t, seen) = transport(|_| {
        ok(json!({"model": "m1", "usage": {"prompt_tokens": 4, "completion_tokens": 1}, "choices": [{"message": {"content": "ok"}}]}))
    });
    let r = openai(t).test_connection("m").unwrap();
    assert_eq!(r, json!({"ok": true, "model": "m1", "usage": {"input_tokens": 4, "output_tokens": 1}}));
    let b = seen.lock().unwrap()[0].body.clone();
    assert_eq!((b["max_tokens"].clone(), b["messages"][0]["content"].clone()), (json!(8), json!("Reply with the word ok.")));
}

#[test]
fn cost_from_price_table() {
    let t = json!({"m": {"input": 3.0, "output": 15.0}});
    assert_eq!(cost("m", 1_000_000, 100_000, &t), Some(4.5));
    assert_eq!(cost("unknown", 10, 10, &t), None);
}

// ---------------------------------------------------------------- presets and registry

#[test]
fn presets_cover_the_major_providers_and_custom() {
    let ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
    for want in [
        "openai", "anthropic", "gemini", "azure_foundry", "mistral", "groq", "xai", "deepseek", "openrouter", "together",
        "fireworks", "perplexity", "cohere", "ollama", "lmstudio", "claude_cli", "custom",
    ] {
        assert!(ids.contains(&want), "{want}");
    }
    let mut u = ids.clone();
    u.sort();
    u.dedup();
    assert_eq!(u.len(), ids.len());
}

#[test]
fn every_preset_uses_an_offered_protocol_and_a_sane_url() {
    for p in PRESETS {
        assert!(ADAPTERS.contains(&p.kind), "{}", p.id);
        if !["custom", "claude_cli", "azure_foundry"].contains(&p.id) {
            assert!(p.base_url.starts_with("https://") || p.base_url.starts_with("http://localhost"), "{}", p.id);
        }
        assert!(!p.name.is_empty());
    }
    let by = |id: &str| PRESETS.iter().find(|p| p.id == id).unwrap();
    assert!(!by("ollama").needs_key && !by("lmstudio").needs_key && !by("claude_cli").needs_key);
    assert!(by("custom").base_url.is_empty() && by("openai").needs_key);
}

#[test]
fn presets_serialise_like_the_python_api() {
    let v = ao_core::providers::presets::as_json();
    assert_eq!(
        v[0],
        json!({"id": "openai", "name": "OpenAI", "type": "openai", "base_url": "https://api.openai.com/v1", "needs_key": true, "note": ""})
    );
    assert_eq!(v.as_array().unwrap().len(), PRESETS.len());
}

#[test]
fn the_registry_builds_each_type_and_refuses_unknown_and_external() {
    static ENV: Mutex<()> = Mutex::new(());
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", dir.path());
    let enc = |s: &str| ao_core::secrets::encrypt(s);
    for (t, url) in [("openai", "https://x/v1"), ("anthropic", "https://x"), ("claude_cli", "")] {
        let rec = json!({"type": t, "base_url": url, "api_key_enc": enc("")});
        assert!(build(&rec).is_ok(), "{t}");
    }
    let e = build(&json!({"type": "external", "base_url": "", "api_key_enc": enc("")})).err().unwrap();
    assert_eq!(e.to_string(), "external providers are not available yet");
    let e = build(&json!({"type": "nope", "base_url": "", "api_key_enc": enc("")})).err().unwrap();
    assert_eq!(e.to_string(), "unknown provider type nope");
}

// ---------------------------------------------------------------- Claude login provider

type Calls = Arc<Mutex<Vec<(Vec<String>, String, HashMap<String, String>)>>>;

fn fake(result: &str, code: i32, stderr: &str, is_error: bool) -> (Runner, Calls) {
    let calls: Calls = Arc::new(Mutex::new(vec![]));
    let c = calls.clone();
    let result = result.to_string();
    let stderr = stderr.to_string();
    let r: Runner = Arc::new(move |argv: &[String], stdin: &str, env: &HashMap<String, String>, _t: f64| {
        c.lock().unwrap().push((argv.to_vec(), stdin.to_string(), env.clone()));
        let out = json!({"type": "result", "is_error": is_error, "result": result, "usage": {"input_tokens": 7, "output_tokens": 3}});
        Ok((code, out.to_string(), stderr.clone()))
    });
    (r, calls)
}

fn cli(r: Runner) -> ClaudeCliProvider {
    ClaudeCliProvider::new("C:/x/claude.exe", Some(r))
}

fn run(p: &ClaudeCliProvider, msgs: &[Message], tools: Option<&[ToolSpec]>) -> Result<Completion, ProviderError> {
    p.complete("sonnet", msgs, tools, 1024, None)
}

fn arg<'a>(a: &'a [String], flag: &str) -> &'a str {
    &a[a.iter().position(|x| x == flag).unwrap() + 1]
}

#[test]
fn cli_plain_completion_returns_text_and_usage() {
    let (r, _) = fake("hi there", 0, "", false);
    let c = run(&cli(r), &[user("hello")], None).unwrap();
    assert!(c.text == "hi there" && c.tool_calls.is_empty());
    assert_eq!((c.usage.input_tokens, c.usage.output_tokens), (7, 3));
}

#[test]
fn cli_command_disables_tools_sets_model_system_and_sends_the_prompt_on_stdin() {
    let (r, calls) = fake("x", 0, "", false);
    run(&cli(r), &[Message::new("system", "be brief"), user("hello")], None).unwrap();
    let (a, stdin, _) = calls.lock().unwrap()[0].clone();
    assert!(a[0] == "C:/x/claude.exe" && a.contains(&"-p".to_string()));
    assert_eq!(arg(&a, "--model"), "sonnet");
    assert_eq!(arg(&a, "--system-prompt"), "be brief");
    assert_eq!(arg(&a, "--tools"), "");
    assert_eq!(arg(&a, "--output-format"), "json");
    assert!(a.contains(&"--no-session-persistence".to_string()));
    assert!(stdin.contains("hello") && !a.join(" ").contains("hello"));
}

#[test]
fn cli_conversation_is_flattened_in_order_with_the_tool_list() {
    let (r, calls) = fake("x", 0, "", false);
    let msgs = [
        user("read a.txt"),
        Message::assistant("", vec![ToolCall::new("c1", "read_file", json!({"path": "a.txt"}))]),
        Message::tool("file body", "c1"),
        user("summarise"),
    ];
    run(&cli(r), &msgs, Some(&[tool()])).unwrap();
    let s = calls.lock().unwrap()[0].1.clone();
    let at = |x: &str| s.find(x).unwrap();
    assert!(at("read a.txt") < at("file body") && at("file body") < at("summarise"));
    assert!(s.contains("(called tool read_file with {\"path\": \"a.txt\"})"));
}

#[test]
fn cli_tool_call_reply_is_parsed_from_fenced_json() {
    let reply = "Reading it.\n```json\n{\"tool_calls\": [{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.txt\"}}]}\n```";
    let (r, _) = fake(reply, 0, "", false);
    let c = run(&cli(r), &[user("go")], Some(&[tool()])).unwrap();
    assert_eq!(c.tool_calls.len(), 1);
    assert_eq!((c.tool_calls[0].name.as_str(), &c.tool_calls[0].arguments), ("read_file", &json!({"path": "a.txt"})));
    assert!(!c.tool_calls[0].id.is_empty() && !c.text.contains("tool_calls"));
    assert_eq!(c.stop_reason, "tool_use");
}

#[test]
fn cli_unknown_tools_are_ignored_and_json_without_tools_stays_text() {
    let (r, _) = fake("{\"tool_calls\": [{\"name\": \"rm_rf\", \"arguments\": {}}]}", 0, "", false);
    assert!(run(&cli(r), &[user("go")], Some(&[tool()])).unwrap().tool_calls.is_empty());
    let (r, _) = fake("{\"tool_calls\": [{\"name\": \"read_file\", \"arguments\": {}}]}", 0, "", false);
    let c = run(&cli(r), &[user("go")], None).unwrap();
    assert!(c.tool_calls.is_empty() && c.text.contains("tool_calls"));
}

#[test]
fn cli_failures_are_classified() {
    let (r, _) = fake("Invalid API key - Please run /login", 1, "", true);
    assert_eq!(run(&cli(r), &[user("x")], None).unwrap_err().kind, Kind::Auth);
    let (r, _) = fake("Claude usage limit reached", 1, "", true);
    assert_eq!(run(&cli(r), &[user("x")], None).unwrap_err().kind, Kind::RateLimit);
    let (r, _) = fake("boom", 1, "", true);
    assert_eq!(run(&cli(r), &[user("x")], None).unwrap_err().kind, Kind::Server);
}

#[test]
fn cli_timeout_and_missing_program_are_network_errors() {
    let slow: Runner = Arc::new(|_, _, _, _| Err(RunError::Timeout("took too long".into())));
    let missing: Runner = Arc::new(|_, _, _, _| Err(RunError::NotFound("claude".into())));
    assert_eq!(run(&cli(slow), &[user("x")], None).unwrap_err().kind, Kind::Network);
    let e = run(&cli(missing), &[user("x")], None).unwrap_err();
    assert_eq!(e.kind, Kind::Network);
    assert!(e.message.contains("could not run claude (C:/x/claude.exe)"));
}

#[test]
fn cli_removes_the_api_key_variable_so_the_subscription_login_is_used() {
    static ENV: Mutex<()> = Mutex::new(());
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (r, calls) = fake("x", 0, "", false);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-should-not-leak");
    run(&cli(r), &[user("x")], None).unwrap();
    std::env::remove_var("ANTHROPIC_API_KEY");
    assert!(!calls.lock().unwrap()[0].2.contains_key("ANTHROPIC_API_KEY"));
}

#[test]
fn cli_tools_offered_add_a_last_reminder_that_no_native_tools_exist() {
    let (r, calls) = fake("ok", 0, "", false);
    run(&cli(r), &[user("go")], Some(&[tool()])).unwrap();
    assert!(calls.lock().unwrap()[0].1.trim_end().ends_with("no native tools exist."));
    let (r, calls) = fake("ok", 0, "", false);
    run(&cli(r), &[user("go")], None).unwrap();
    assert!(!calls.lock().unwrap()[0].1.contains("Reminder"));
}

#[test]
fn cli_system_prompt_gets_the_tool_rules_and_tool_descriptions() {
    let (r, calls) = fake("ok", 0, "", false);
    run(&cli(r), &[Message::new("system", "sys"), user("go")], Some(&[tool()])).unwrap();
    let a = calls.lock().unwrap()[0].0.clone();
    let sys = arg(&a, "--system-prompt");
    assert!(sys.starts_with("sys\n\nYou have NO built-in tools"));
    assert!(sys.contains("- read_file: read a file Parameters: {\"type\": \"object\", \"properties\": {\"path\": {\"type\": \"string\"}}}"));
}

#[test]
fn pasted_program_paths_are_cleaned() {
    assert_eq!(normalise_exe("& \"C:\\a b\\claude.exe\""), "C:\\a b\\claude.exe");
    assert_eq!(normalise_exe("  'C:/x/claude.exe' "), "C:/x/claude.exe");
    assert_eq!(normalise_exe("&"), "");
    assert_eq!(normalise_exe("claude"), "claude");
}

#[test]
fn cached_input_tokens_are_counted_so_usage_matches_what_the_provider_bills() {
    // the claude program reports most of its input as cache tokens, apart from `input_tokens`
    let r: Runner = Arc::new(|_a: &[String], _s: &str, _e: &HashMap<String, String>, _t: f64| {
        let out = json!({"type": "result", "is_error": false, "result": "hi", "usage": {"input_tokens": 2, "cache_creation_input_tokens": 9000, "cache_read_input_tokens": 30000, "output_tokens": 50}});
        Ok((0, out.to_string(), String::new()))
    });
    let c = run(&cli(r), &[user("hello")], None).unwrap();
    assert_eq!((c.usage.input_tokens, c.usage.output_tokens), (39002, 50));

    let (t, _) = transport(|_| ok(json!({"model": "m", "stop_reason": "end_turn", "content": [{"type": "text", "text": "x"}],
        "usage": {"input_tokens": 5, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 400, "output_tokens": 7}})));
    let c = anthropic(t).complete("m", &[user("hi")], None, 100, None).unwrap();
    assert_eq!((c.usage.input_tokens, c.usage.output_tokens), (505, 7));
}
