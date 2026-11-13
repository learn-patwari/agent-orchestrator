//! Runs API and resume. Ported from backend/tests/test_runs_api.py (and the budget hold/resume parts of test_budget.py).
mod common;
use ao_core::db::DocRepo;
use common::{env, ok_json, Client};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn approved_fid() -> String {
    DocRepo::new("features").unwrap().create(json!({"title": "f", "status": "approved"}), None).unwrap()["id"].as_str().unwrap().to_string()
}

fn completion(text: &str, tool_calls: Option<Value>) -> ao_core::providers::base::HttpResponse {
    let mut msg = json!({"content": if text.is_empty() { Value::Null } else { json!(text) }});
    if let Some(t) = tool_calls {
        msg["tool_calls"] = t;
    }
    ok_json(200, json!({"model": "m", "usage": {"prompt_tokens": 5, "completion_tokens": 2}, "choices": [{"message": msg}]}))
}

async fn setup_role(c: &Client, tmp: &std::path::Path) -> String {
    let roles = tmp.join("roles");
    std::fs::create_dir_all(&roles).unwrap();
    std::fs::write(roles.join("developer.md"), "---\nname: developer\ndescription: dev\nskills: [ghost]\n---\nYou develop.").unwrap();
    c.put("/api/settings", json!({"role_dirs": [roles.to_string_lossy()], "skill_paths": [tmp.join("skills").to_string_lossy()], "workspace_roots": [tmp.to_string_lossy()]})).await;
    c.post("/api/providers", json!({"name": "p", "type": "openai", "base_url": "https://x/v1", "api_key": "sk-test-abcdefghijkl"})).await.1["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn roles_listing_reports_validation_problems() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    setup_role(c, tmp.path()).await;
    let r = c.get("/api/roles").await.1;
    assert!(r[0]["name"] == "developer" && r[0]["problems"] == json!(["missing skill ghost"]), "{r}");
    assert_eq!(c.get("/api/skills").await.1, json!([]));
}

#[tokio::test]
async fn a_run_needs_a_model_and_a_workspace() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    let pid = setup_role(c, tmp.path()).await;
    let body = json!({"role": "developer", "task": "t", "workspace": tmp.path().to_string_lossy(), "wait": true, "feature_id": approved_fid()});
    assert_eq!(c.post("/api/runs", body.clone()).await.0, 400); // no model for the role
    c.put("/api/role-models/developer", json!({"provider_id": pid, "model": "m"})).await;
    let mut nope = body.clone();
    nope["workspace"] = json!(tmp.path().join("nope").to_string_lossy());
    assert_eq!(c.post("/api/runs", nope).await.0, 400);
    let mut ghost = body.clone();
    ghost["role"] = json!("ghost");
    assert_eq!(c.post("/api/runs", ghost).await.0, 404);
    assert_eq!(c.post("/api/runs", json!({"role": "developer"})).await.0, 422);
}

#[tokio::test]
async fn development_waits_for_an_approved_srs() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    let pid = setup_role(c, tmp.path()).await;
    c.put("/api/role-models/developer", json!({"provider_id": pid, "model": "m"})).await;
    let draft = DocRepo::new("features").unwrap().create(json!({"title": "f", "status": "draft"}), None).unwrap()["id"].as_str().unwrap().to_string();
    for fid in [json!(null), json!(draft)] {
        let (st, body) = c.post("/api/runs", json!({"role": "developer", "task": "t", "workspace": tmp.path().to_string_lossy(), "wait": true, "feature_id": fid})).await;
        assert_eq!(st, 409, "{body}");
    }
}

#[tokio::test]
async fn a_run_end_to_end_via_the_api() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    let pid = setup_role(c, tmp.path()).await;
    c.put("/api/role-models/developer", json!({"provider_id": pid, "model": "m"})).await;
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let replies = Arc::new(Mutex::new(vec![
        completion("created hello.txt", None),
        completion("", Some(json!([{"id": "c1", "type": "function", "function": {"name": "write_file", "arguments": "{\"path\": \"hello.txt\", \"content\": \"hi\"}"}}]))),
    ]));
    e.c.upstream.set(move |_| replies.lock().unwrap().pop().unwrap_or_else(|| completion("done", None)));
    let (st, out) = c.post("/api/runs", json!({"role": "developer", "task": "make a file", "workspace": ws.to_string_lossy(), "wait": true, "feature_id": approved_fid()})).await;
    assert!(st == 200 && out["status"] == "done", "{out}");
    assert_eq!(std::fs::read_to_string(ws.join("hello.txt")).unwrap(), "hi");
    let id = out["id"].as_str().unwrap();
    let detail = c.get(&format!("/api/runs/{id}")).await.1;
    let roles: Vec<&str> = detail["log"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "tool", "assistant"]);
    assert_eq!(c.get("/api/usage").await.1["by_role"]["developer"]["calls"], 2);
    assert_eq!(c.get("/api/runs").await.1[0]["id"], id);
    assert_eq!(c.get("/api/runs/nope").await.0, 404);
}

#[tokio::test]
async fn a_held_feature_blocks_new_runs_and_resume_clears_it() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    let pid = setup_role(c, tmp.path()).await;
    c.put("/api/role-models/developer", json!({"provider_id": pid, "model": "m"})).await;
    let fid = approved_fid();
    DocRepo::new("features").unwrap().update(&fid, json!({"held": true, "token_limit": 100})).unwrap();
    let body = json!({"role": "developer", "task": "t", "workspace": tmp.path().to_string_lossy(), "wait": true, "feature_id": fid});
    let (st, r) = c.post("/api/runs", body).await;
    assert!(st == 429 && r["detail"].as_str().unwrap().contains("press Resume"), "{r}");
    let s = c.act(&format!("/api/features/{fid}/resume")).await;
    assert_eq!(s.0, 200);
    assert_eq!(s.1["held"], false);
    assert_eq!(c.act("/api/features/ghost/resume").await.0, 404);
}
