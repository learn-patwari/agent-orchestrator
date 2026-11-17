//! Chat with one agent from the Office: its own saved sessions, its own role prompt and model, and no delegation.
mod common;
use common::{env, ok_json, Client};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn agent(c: &Client, role: &str) {
    let pid = c.post("/api/providers", json!({"name": "p", "type": "openai", "base_url": "https://x/v1", "api_key": "sk-test-abcdefghijkl"})).await.1["id"].as_str().unwrap().to_string();
    c.put(&format!("/api/role-models/{role}"), json!({"provider_id": pid, "model": "dev-model"})).await;
}

fn answer(text: &str) -> common_resp::R {
    ok_json(200, json!({"model": "m", "usage": {"prompt_tokens": 5, "completion_tokens": 2}, "choices": [{"message": {"content": text}}]}))
}
mod common_resp {
    pub type R = ao_core::providers::base::HttpResponse;
}

#[tokio::test]
async fn a_message_starts_a_session_and_the_reply_uses_the_agents_own_model() {
    let e = env();
    let c = e.c.authed().await;
    agent(c, "developer_1").await;
    let seen: Arc<Mutex<Vec<Value>>> = Default::default();
    let s2 = seen.clone();
    e.c.upstream.set(move |req| {
        s2.lock().unwrap().push(req.body.clone());
        answer("I changed the button.")
    });
    let (st, out) = c.post("/api/agents/developer_1/chat", json!({"text": "what did you change?"})).await;
    assert_eq!(st, 200, "{out}");
    assert_eq!(out["reply"], "I changed the button.");
    assert!(out["session_id"].as_str().is_some_and(|s| !s.is_empty()));
    let body = seen.lock().unwrap()[0].clone();
    assert_eq!(body["model"], "dev-model");
    assert!(body.get("tools").is_none(), "only the orchestrator can start other agents");
}

#[tokio::test]
async fn sessions_are_kept_apart_listed_and_resumable() {
    let e = env();
    let c = e.c.authed().await;
    agent(c, "developer_1").await;
    e.c.upstream.set(|_| answer("ok"));
    let a = c.post("/api/agents/developer_1/chat", json!({"text": "first topic"})).await.1["session_id"].as_str().unwrap().to_string();
    let b = c.post("/api/agents/developer_1/chat", json!({"text": "second topic"})).await.1["session_id"].as_str().unwrap().to_string();
    assert_ne!(a, b);
    c.post("/api/agents/developer_1/chat", json!({"text": "more on first", "session_id": a})).await;
    let list = c.get("/api/agents/developer_1/chat/sessions").await.1;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let hist = c.get(&format!("/api/agents/developer_1/chat?session={a}")).await.1;
    let texts: Vec<&str> = hist.as_array().unwrap().iter().map(|m| m["content"].as_str().unwrap()).collect();
    assert_eq!(texts, ["first topic", "ok", "more on first", "ok"]);
    // not mixed into the orchestrator's chat or another agent's sessions
    assert_eq!(c.get("/api/chat").await.1, json!([]));
    assert_eq!(c.get("/api/agents/tester/chat/sessions").await.1, json!([]));
}

#[tokio::test]
async fn an_agent_without_a_model_or_an_unknown_role_is_refused() {
    let e = env();
    let c = e.c.authed().await;
    assert_eq!(c.post("/api/agents/developer_1/chat", json!({"text": "hi"})).await.0, 400);
    assert_eq!(c.post("/api/agents/nobody/chat", json!({"text": "hi"})).await.0, 404);
}
