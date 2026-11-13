//! Features, registry, team, office, chat, notifications and schedules.
//! Ported from backend/tests/{test_features_api,test_agent_registry,test_office,test_notify_sched}.py.
mod common;
use ao_core::db::DocRepo;
use ao_core::notify::Notifier;
use common::{env, ok_json, Client};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const ANALYSIS: &str = r#"{"restatement": "A tax report page.", "why": "Finance needs it.",
 "impact": {"modules": ["reports"], "data": ["invoices"], "security": ["role checks"], "risks": ["slow query"]},
 "questions": [
  {"title": "DB?", "body": "Which database?", "options": ["sqlite", "mongo"], "default": "sqlite"},
  {"title": "Format?", "body": "Export format?", "options": ["csv", "pdf"], "default": "csv"}]}"#;
const PLAN: &str = r#"{"phases": [{"id": "P1", "name": "Backend", "tasks": [
  {"id": "t1", "title": "API", "files": ["api.py"], "depends_on": []},
  {"id": "t2", "title": "UI", "files": ["ui.tsx"], "depends_on": ["t1"]}]}]}"#;

fn completion(text: &str, tool_calls: Option<Value>) -> ao_core::providers::base::HttpResponse {
    let mut msg = json!({"content": if text.is_empty() { Value::Null } else { json!(text) }});
    if let Some(t) = tool_calls {
        msg["tool_calls"] = t;
    }
    ok_json(200, json!({"model": "m", "usage": {"prompt_tokens": 5, "completion_tokens": 2}, "choices": [{"message": msg}]}))
}

fn reply(e: &common::Env, text: &'static str) {
    e.c.upstream.set(move |_| completion(text, None));
}

async fn provider(c: &Client) -> String {
    c.post("/api/providers", json!({"name": "p", "type": "openai", "base_url": "https://x/v1", "api_key": "sk-test-abcdefghijkl"})).await.1["id"].as_str().unwrap().to_string()
}

async fn arch(c: &Client) {
    let pid = provider(c).await;
    c.put("/api/role-models/architect", json!({"provider_id": pid, "model": "m"})).await;
}

#[tokio::test]
async fn feature_endpoints_require_auth_and_an_architect_model() {
    let e = env();
    let c = &e.c;
    assert_eq!(c.get("/api/features").await.0, 401);
    assert_eq!(c.post("/api/features", json!({"title": "t", "text": "x"})).await.0, 401);
    c.authed().await;
    assert_eq!(c.post("/api/features", json!({"title": "t", "text": "x"})).await.0, 400);
}

#[tokio::test]
async fn the_full_feature_flow_over_the_api_to_approval() {
    let e = env();
    let c = e.c.authed().await;
    arch(c).await;
    reply(&e, ANALYSIS);
    let f = c.post("/api/features", json!({"title": "Tax report", "text": "Add a tax report"})).await.1;
    let fid = f["id"].as_str().unwrap().to_string();
    assert!(f["status"] == "questioning" && f["analysis"]["restatement"].is_string(), "{f}");
    let q = c.get("/api/inbox?status=open").await.1;
    assert_eq!(q.as_array().unwrap().iter().map(|i| i["title"].clone()).collect::<Vec<_>>(), vec![json!("DB?")]);
    c.post(&format!("/api/inbox/{}/answer", q[0]["id"].as_str().unwrap()), json!({"answer": "mongo"})).await;
    let q = c.get("/api/inbox?status=open").await.1;
    assert_eq!(q.as_array().unwrap().iter().map(|i| i["title"].clone()).collect::<Vec<_>>(), vec![json!("Format?")]); // the next question appeared by itself
    c.act(&format!("/api/inbox/{}/accept-default", q[0]["id"].as_str().unwrap())).await;
    assert_eq!(c.get(&format!("/api/features/{fid}")).await.1["status"], "ready");
    reply(&e, PLAN);
    let (st, fin) = c.act(&format!("/api/features/{fid}/finalize")).await;
    assert!(st == 200 && fin["status"] == "srs_draft", "{fin}");
    let srs = c.get(&format!("/api/features/{fid}/srs")).await.1;
    let srs = srs.as_str().unwrap();
    assert!(srs.contains("# SRS: Tax report") && srs.contains("D-1") && srs.contains("[A]"), "{srs}");
    let open = c.get("/api/inbox?status=open").await.1;
    let gate = open.as_array().unwrap().iter().find(|i| i["kind"] == "approval").unwrap().clone();
    assert_eq!(c.post(&format!("/api/inbox/{}/decide", gate["id"].as_str().unwrap()), json!({"approve": true})).await.0, 200);
    assert_eq!(c.get(&format!("/api/features/{fid}")).await.1["status"], "approved");
    let listed = c.get("/api/features").await.1;
    assert_eq!(listed.as_array().unwrap().iter().map(|x| x["id"].clone()).collect::<Vec<_>>(), vec![json!(fid)]);
    assert!(listed[0].get("srs_md").is_none()); // the list leaves the long text out
}

#[tokio::test]
async fn finalize_before_the_questions_are_answered_is_a_conflict_and_unknown_is_404() {
    let e = env();
    let c = e.c.authed().await;
    arch(c).await;
    reply(&e, ANALYSIS);
    let f = c.post("/api/features", json!({"title": "t", "text": "x"})).await.1;
    assert_eq!(c.act(&format!("/api/features/{}/finalize", f["id"].as_str().unwrap())).await.0, 409);
    assert_eq!(c.get("/api/features/nope").await.0, 404);
    assert_eq!(c.get(&format!("/api/features/{}/srs", f["id"].as_str().unwrap())).await.0, 404); // no SRS yet
}

#[tokio::test]
async fn an_architect_that_answers_nonsense_is_a_502_and_leaves_nothing_behind() {
    let e = env();
    let c = e.c.authed().await;
    arch(c).await;
    reply(&e, "this is not json");
    let (st, r) = c.post("/api/features", json!({"title": "t", "text": "x"})).await;
    assert!(st == 502 && r["detail"].as_str().unwrap().starts_with("architect failed"), "{r}");
    assert_eq!(c.get("/api/features").await.1, json!([]));
}

#[tokio::test]
async fn the_registry_api_end_to_end() {
    let e = env();
    let c = e.c.authed().await;
    let key = c.get("/api/registry/key").await.1["key"].as_str().unwrap().to_string();
    let body = json!({"name": "scout", "endpoint": "https://agents.example/scout", "capabilities": ["code search"], "description": "d"});
    // the register call is open to agents, but needs the registry key header
    c.clear_cookies();
    assert_eq!(c.post("/api/registry/register", body.clone()).await.0, 401);
    assert_eq!(c.call_with("POST", "/api/registry/register", Some(body.clone()), &[("x-registry-key", "nope")]).await.0, 401);
    let reg = c.call_with("POST", "/api/registry/register", Some(body), &[("x-registry-key", &key)]).await.1;
    assert!(reg["status"] == "pending" && reg["token"].is_string(), "{reg}");
    let titles: Vec<Value> = DocRepo::new("inbox").unwrap().list().unwrap().iter().map(|i| i["title"].clone()).collect();
    assert_eq!(titles, vec![json!("External agent wants to join: scout")]);
    assert_eq!(c.post("/api/registry/heartbeat", json!({"name": "scout", "token": "bad"})).await.0, 401);
    assert_eq!(c.post("/api/registry/heartbeat", json!({"name": "scout", "token": reg["token"]})).await.1, json!({"ok": true}));
    c.post("/api/auth/login", json!({"password": "correct-horse-1"})).await;
    assert_eq!(c.get("/api/registry").await.1[0]["live"], false); // pending
    assert_eq!(c.act("/api/registry/scout/approve").await.1["status"], "approved");
    assert_eq!(c.get("/api/registry").await.1[0]["live"], true);
    assert_eq!(c.act("/api/registry/ghost/approve").await.0, 404);
    c.put("/api/settings", json!({"feature_flags": {"external_providers": true}})).await;
    e.c.upstream.set(|_| ok_json(200, json!({"reply": "done"})));
    let r = c.post("/api/registry/scout/task", json!({"task": "find x"})).await;
    assert!(r.0 == 200 && r.1["reply"].as_str().unwrap().contains("done"), "{r:?}");
    e.c.upstream.set(|_| ok_json(500, json!({})));
    assert_eq!(c.post("/api/registry/scout/task", json!({"task": "again"})).await.0, 502); // the agent failed
    assert_eq!(c.act("/api/registry/scout/reject").await.1["status"], "rejected");
    assert_eq!(c.post("/api/registry/scout/task", json!({"task": "again"})).await.0, 409);
}

async fn setup_roles(c: &Client, tmp: &std::path::Path) -> String {
    let roles = tmp.join("roles");
    std::fs::create_dir_all(&roles).unwrap();
    std::fs::write(roles.join("developer.md"), "---\nname: developer\ndescription: dev\n---\nYou develop.").unwrap();
    std::fs::write(roles.join("orchestrator.md"), "---\nname: orchestrator\ndescription: boss\n---\nLead.").unwrap();
    c.put("/api/settings", json!({"role_dirs": [roles.to_string_lossy()], "skill_paths": [tmp.join("skills").to_string_lossy()], "workspace_roots": [tmp.to_string_lossy()]})).await;
    provider(c).await
}

#[tokio::test]
async fn the_office_feed_lists_the_team_and_every_hand_off() {
    let e = env();
    std::env::set_var("AO_SYNC_RUNS", "1");
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    let pid = setup_roles(c, tmp.path()).await;
    for n in ["developer", "orchestrator"] {
        c.put(&format!("/api/role-models/{n}"), json!({"provider_id": pid, "model": "m"})).await;
    }
    let fid = DocRepo::new("features").unwrap().create(json!({"title": "f", "status": "approved"}), None).unwrap()["id"].as_str().unwrap().to_string();
    let call = json!([{"id": "t1", "type": "function", "function": {"name": "delegate", "arguments": json!({"role": "developer", "task": "Create hello.txt", "area": "root", "done_criteria": ["exists"], "feature_id": fid}).to_string()}}]);
    let replies = Arc::new(Mutex::new(vec![completion("Started.", None), completion("hello done", None), completion("", Some(call))]));
    e.c.upstream.set(move |_| replies.lock().unwrap().pop().unwrap_or_else(|| completion("ok", None)));
    let (st, out) = c.post("/api/chat", json!({"text": "go"})).await;
    std::env::remove_var("AO_SYNC_RUNS");
    assert_eq!(st, 200, "{out}");
    let feed = c.get("/api/office").await.1;
    let roles: Vec<&str> = feed["team"].as_array().unwrap().iter().map(|t| t["role"].as_str().unwrap()).collect();
    assert!(roles.contains(&"developer") && roles.contains(&"orchestrator"));
    let hs = feed["handoffs"].as_array().unwrap();
    let pairs: Vec<(&str, &str)> = hs.iter().map(|h| (h["from"].as_str().unwrap(), h["to"].as_str().unwrap())).collect();
    assert_eq!(pairs, vec![("orchestrator", "developer"), ("developer", "orchestrator")], "{feed}");
    assert!(hs[0]["label"].as_str().unwrap().starts_with("Create hello.txt") && hs[1]["label"] == "done");
    assert!(hs[0]["at"].as_f64() <= hs[1]["at"].as_f64());
    let history = c.get("/api/chat").await.1;
    assert!(history.as_array().unwrap().iter().any(|m| m["role"] == "user" && m["content"] == "go"), "{history}");
}

#[tokio::test]
async fn the_office_feed_is_empty_before_any_work_and_chat_needs_a_model() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    setup_roles(c, tmp.path()).await;
    let feed = c.get("/api/office").await.1;
    assert!(feed["handoffs"] == json!([]) && feed["open_incidents"] == 0 && feed["pending_prod"] == json!([]), "{feed}");
    assert_eq!(c.post("/api/chat", json!({"text": "hi"})).await.0, 400);
    assert_eq!(c.get("/api/team").await.1.as_array().unwrap().len(), 2);
    assert_eq!(c.get("/api/status").await.1[0]["state"], "idle");
}

#[tokio::test]
async fn team_check_asks_each_model_once() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    let pid = setup_roles(c, tmp.path()).await;
    c.put("/api/role-models/developer", json!({"provider_id": pid, "model": "m"})).await;
    reply(&e, "ok");
    let r = c.act("/api/team/check").await.1;
    let rows = r.as_array().unwrap();
    assert!(rows.iter().any(|x| x["role"] == "developer" && x["responding"] == true), "{r}");
    assert!(rows.iter().any(|x| x["role"] == "orchestrator" && x["responding"] == false && x["detail"] == "no model assigned"), "{r}");
}

#[tokio::test]
async fn notifications_and_schedules() {
    let e = env();
    let c = e.c.authed().await;
    let n = Notifier::new().send("status", "hello", "body", None).unwrap().unwrap();
    assert_eq!(c.get("/api/notifications?unread=true").await.1.as_array().unwrap().len(), 1);
    assert_eq!(c.act(&format!("/api/notifications/{}/read", n["id"].as_str().unwrap())).await.0, 200);
    assert_eq!(c.get("/api/notifications?unread=true").await.1, json!([]));
    assert_eq!(c.get("/api/notifications").await.1.as_array().unwrap().len(), 1);
    assert_eq!(c.act("/api/notifications/nope/read").await.0, 404);
    let (st, r) = c.post("/api/schedules", json!({"name": "x", "payload": {"kind": "launch"}, "run_at": 1.0})).await;
    assert!(st == 400 && r["detail"] == "payload.kind must be one of ['reminder']", "{r}");
    let ok = c.post("/api/schedules", json!({"name": "nudge", "payload": {"kind": "reminder", "title": "t"}, "run_at": 10.0, "every_s": 60.0})).await;
    assert!(ok.0 == 200 && ok.1["status"] == "scheduled" && ok.1["every_s"] == 60.0, "{ok:?}");
    assert_eq!(c.get("/api/schedules").await.1.as_array().unwrap().len(), 1);
    assert_eq!(c.post("/api/schedules", json!({"name": "x"})).await.0, 422);
}

#[tokio::test]
async fn the_background_tick_runs_due_reminders_once() {
    let e = env();
    let c = e.c.authed().await;
    c.post("/api/schedules", json!({"name": "stand-up", "payload": {"kind": "reminder", "text": "9am"}, "run_at": 1.0})).await;
    ao_server::background::tick_once(&ao_server::AppState::default());
    ao_server::background::tick_once(&ao_server::AppState::default());
    let inbox = c.get("/api/inbox").await.1;
    assert_eq!(inbox.as_array().unwrap().iter().filter(|i| i["title"] == "stand-up").count(), 1, "{inbox}");
    assert_eq!(c.get("/api/notifications").await.1.as_array().unwrap().iter().filter(|n| n["kind"] == "reminder").count(), 1);
    assert_eq!(c.get("/api/schedules").await.1[0]["status"], "done");
}
