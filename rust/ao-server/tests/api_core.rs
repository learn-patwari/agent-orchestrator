//! Route group 1: health, auth, settings, providers, role models, prices, usage, secrets, skills, roles.
//! Ported from backend/tests/{test_platform,test_provider_api,test_presets}.py.
mod common;
use ao_core::providers::base::{Completion, Message, Provider, ProviderError, ToolSpec};
use common::{env, env_with, ok_json};
use serde_json::{json, Value};
use std::sync::Arc;

#[tokio::test]
async fn requires_auth() {
    let e = env();
    let c = &e.c;
    assert_eq!(c.get("/api/settings").await.0, 401);
    assert_eq!(c.get("/api/providers").await, (401, json!({"detail": "not authenticated"})));
    assert_eq!(c.get("/api/health").await, (200, json!({"ok": true})));
}

#[tokio::test]
async fn setup_login_flow() {
    let e = env();
    let c = &e.c;
    assert_eq!(c.get("/api/auth/status").await.1, json!({"setup_done": false}));
    assert_eq!(c.post("/api/auth/setup", json!({"password": "short"})).await.0, 400);
    assert_eq!(c.post("/api/auth/setup", json!({"password": "correct-horse-1"})).await.0, 200);
    assert_eq!(c.get("/api/settings").await.0, 200); // cookie set by setup
    assert_eq!(c.post("/api/auth/setup", json!({"password": "another-pass-2"})).await.0, 409);
    c.act("/api/auth/logout").await;
    c.clear_cookies();
    assert_eq!(c.get("/api/settings").await.0, 401);
    assert_eq!(c.post("/api/auth/login", json!({"password": "wrong-pass-123"})).await.0, 401);
    assert_eq!(c.post("/api/auth/login", json!({"password": "correct-horse-1"})).await.0, 200);
    assert_eq!(c.get("/api/settings").await.0, 200);
    assert_eq!(c.post("/api/auth/login", json!({})).await.0, 422); // a missing field is a validation error
}

#[tokio::test]
async fn logout_drops_the_session_cookie() {
    let e = env();
    let c = e.c.authed().await;
    c.act("/api/auth/logout").await;
    assert_eq!(c.get("/api/settings").await.0, 401);
}

#[tokio::test]
async fn a_tampered_token_is_rejected() {
    let e = env();
    let c = e.c.authed().await;
    c.set_cookie("9999999999.abcd.deadbeef");
    assert_eq!(c.get("/api/settings").await.0, 401);
}

#[tokio::test]
async fn settings_defaults_and_flags() {
    let e = env();
    let c = e.c.authed().await;
    let s = c.get("/api/settings").await.1;
    assert!(s["feature_flags"]["external_providers"] == false && s["integration_branch"] == "internal");
    assert_eq!(c.put("/api/settings", json!({"merge_style": "squash"})).await.1["merge_style"], "squash");
    let (st, body) = c.put("/api/settings", json!({"bogus": 1})).await;
    assert_eq!((st, body), (400, json!({"detail": "unknown settings: ['bogus']"})));
    c.put("/api/settings", json!({"feature_flags": {"x": true}})).await;
    let flags = c.get("/api/settings").await.1["feature_flags"].clone();
    assert!(flags["x"] == true && flags["external_providers"] == false); // flags merge key by key
}

#[tokio::test]
async fn secret_scan() {
    let e = env();
    let c = e.c.authed().await;
    assert_eq!(c.post("/api/secrets/scan", json!({"text": "my password: hunter2hunter2"})).await.1["looks_like_secret"], true);
    assert_eq!(c.post("/api/secrets/scan", json!({"text": "build a login page"})).await.1["looks_like_secret"], false);
}

async fn add(c: &common::Client, kind: &str) -> (u16, Value) {
    c.post("/api/providers", json!({"name": "p", "type": kind, "base_url": "https://llm.example/v1/", "api_key": "sk-secret-abcdefghijkl1234"})).await
}

#[tokio::test]
async fn the_provider_key_is_never_returned_and_encrypted_at_rest() {
    let e = env();
    let c = e.c.authed().await;
    let (st, body) = add(c, "openai").await;
    assert_eq!(st, 200);
    assert!(body["api_key"] == "****1234" && body["base_url"] == "https://llm.example/v1" && body.get("api_key_enc").is_none());
    assert!(!c.get("/api/providers").await.1.to_string().contains("sk-secret"));
    let stored = ao_core::db::DocRepo::new("providers").unwrap().list().unwrap();
    assert!(!serde_json::to_string(&stored).unwrap().contains("sk-secret")); // stored encrypted
}

const OK: &str = r#"{"model": "m1", "usage": {"prompt_tokens": 4, "completion_tokens": 2}, "choices": [{"message": {"content": "ok"}}]}"#;

#[tokio::test]
async fn test_connection_success_and_auth_failure() {
    let e = env();
    let c = e.c.authed().await;
    let pid = add(c, "openai").await.1["id"].as_str().unwrap().to_string();
    e.c.upstream.set(|_| ok_json(200, serde_json::from_str(OK).unwrap()));
    let r = c.post(&format!("/api/providers/{pid}/test"), json!({"model": "m1"})).await.1;
    assert!(r["ok"] == true && r["usage"] == json!({"input_tokens": 4, "output_tokens": 2}), "{r}");

    e.c.upstream.set(|_| ao_core::providers::base::HttpResponse { status: 401, body: "bad key sk-secret-abcdefghijkl1234".into() });
    let r = c.post(&format!("/api/providers/{pid}/test"), json!({"model": "m1"})).await.1;
    assert!(r["ok"] == false && r["error_kind"] == "auth", "{r}");
    assert!(!r["error"].as_str().unwrap().contains("sk-secret")); // redacted
    assert_eq!(c.post("/api/providers/nope/test", json!({"model": "m"})).await.0, 404);
}

#[tokio::test]
async fn external_providers_are_hidden_behind_the_flag() {
    let e = env();
    let c = e.c.authed().await;
    assert_eq!(c.get("/api/providers/types").await.1["types"], json!(["openai", "anthropic", "claude_cli"]));
    assert_eq!(add(c, "external").await.0, 400);
    c.put("/api/settings", json!({"feature_flags": {"external_providers": true}})).await;
    assert!(c.get("/api/providers/types").await.1["types"].as_array().unwrap().contains(&json!("external")));
    c.put("/api/settings", json!({"feature_flags": {"external_providers": false}})).await;
    assert_eq!(add(c, "external").await.0, 400);
}

#[tokio::test]
async fn role_models_prices_and_usage_cost() {
    let e = env();
    let c = e.c.authed().await;
    let pid = add(c, "openai").await.1["id"].as_str().unwrap().to_string();
    assert_eq!(c.put("/api/role-models/developer_1", json!({"provider_id": "nope", "model": "m"})).await.0, 400);
    assert_eq!(c.put("/api/role-models/ghost", json!({"provider_id": pid, "model": "m"})).await.0, 404);
    let m = c.put("/api/role-models/developer_1", json!({"provider_id": pid, "model": "m1"})).await.1;
    assert_eq!(m["developer_1"]["model"], "m1");
    let listed = c.get("/api/role-models").await.1;
    assert!(listed["roles"].as_array().unwrap().contains(&json!("tester")) && listed["models"]["developer_1"]["provider_id"] == pid.as_str());
    assert_eq!(c.put("/api/prices", json!({"m1": {"input": 1.0}})).await.0, 400);
    c.put("/api/prices", json!({"m1": {"input": 2.0, "output": 4.0}})).await;
    ao_core::providers::record_call("developer_1", &pid, "m1", ao_core::providers::base::Usage { input_tokens: 1_000_000, output_tokens: 500_000 }, None).unwrap();
    let u = c.get("/api/usage").await.1["by_role"]["developer_1"].clone();
    assert!(u["calls"] == 1 && u["cost_usd"] == 4.0, "{u}");
    assert_eq!(c.get("/api/prices").await.1["m1"]["output"], 4.0);
}

struct Boom;

impl Provider for Boom {
    fn complete(&self, _: &str, _: &[Message], _: Option<&[ToolSpec]>, _: u32, _: Option<f64>) -> Result<Completion, ProviderError> {
        unreachable!()
    }
    fn stream(&self, _: &str, _: &[Message], _: Option<&[ToolSpec]>, _: u32, _: Option<f64>) -> Result<Vec<String>, ProviderError> {
        Ok(vec![])
    }
    fn test_connection(&self, _: &str) -> Result<Value, ProviderError> {
        panic!("boom sk-secret-abcdefghijkl1234")
    }
}

#[tokio::test]
async fn an_unexpected_adapter_crash_is_reported_not_a_500() {
    let e = env_with(|st| st.provider_factory = Some(Arc::new(|_| Ok(Box::new(Boom)))));
    let c = e.c.authed().await;
    let pid = add(c, "openai").await.1["id"].as_str().unwrap().to_string();
    let (st, body) = c.post(&format!("/api/providers/{pid}/test"), json!({"model": "m"})).await;
    assert_eq!(st, 200);
    assert!(body["ok"] == false && body["error"].as_str().unwrap().contains("boom") && !body["error"].as_str().unwrap().contains("sk-secret"), "{body}");
}

#[tokio::test]
async fn presets_need_a_login_and_a_local_preset_needs_no_key() {
    let e = env();
    let c = &e.c;
    assert_eq!(c.get("/api/providers/presets").await.0, 401);
    c.authed().await;
    let (st, body) = c.get("/api/providers/presets").await;
    assert!(st == 200 && body["presets"].as_array().unwrap().len() == ao_core::providers::presets::PRESETS.len());
    let r = c.post("/api/providers", json!({"name": "Local Ollama", "type": "openai", "base_url": "http://localhost:11434/v1"})).await;
    assert_eq!(r.0, 200);
    assert_eq!(c.delete(&format!("/api/providers/{}", r.1["id"].as_str().unwrap())).await.1, json!({"ok": true}));
    assert_eq!(c.delete("/api/providers/zzz").await.0, 404);
}

#[tokio::test]
async fn roles_and_skills_are_listed_from_the_configured_folders() {
    let e = env();
    let c = e.c.authed().await;
    let roles = c.get("/api/roles").await.1;
    let names: Vec<&str> = roles.as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    for n in ["orchestrator", "architect", "developer_1", "developer_2", "tester", "cicd", "incident_reporter", "production_support"] {
        assert!(names.contains(&n), "{n}");
    }
    assert!(roles.as_array().unwrap().iter().all(|r| r["problems"].is_array() && r["tools"].is_array()));
    assert!(c.get("/api/skills").await.1.is_array());
}

#[tokio::test]
async fn bad_json_is_a_validation_error() {
    let e = env();
    let c = e.c.authed().await;
    let (st, _) = c.call("PUT", "/api/settings", Some(json!([1, 2]))).await;
    assert_eq!(st, 422);
}
