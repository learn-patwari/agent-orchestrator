//! Route group 2: plugins, MCP servers, budget, profiles, skill pickers.
//! Ported from backend/tests/{test_plugins,test_mcp,test_budget,test_profiles}.py.
mod common;
use ao_core::db::DocRepo;
use common::env;
use serde_json::{json, Value};
use std::path::Path;

fn make_plugin(root: &Path, name: &str, role_skills: &str) -> String {
    let p = root.join(name);
    std::fs::create_dir_all(p.join("roles")).unwrap();
    std::fs::create_dir_all(p.join("skills/pg")).unwrap();
    std::fs::write(p.join("skills/pg/SKILL.md"), "---\nname: pg\ndescription: postgres tips\n---\nUse indexes.").unwrap();
    std::fs::write(p.join("roles/dba.md"), format!("---\nname: dba\ndescription: database person\ntools: [read_file]\nskills: {role_skills}\n---\nYou tune databases.")).unwrap();
    std::fs::write(p.join("plugin.json"), json!({"name": name, "version": "1.0.0", "description": "demo plugin"}).to_string()).unwrap();
    p.to_string_lossy().into_owned()
}

fn role_names(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn plugin_api_installs_enables_and_the_roles_and_skills_appear() {
    let e = env();
    let c = e.c.authed().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = make_plugin(tmp.path(), "dbtools", "[pg]");
    assert_eq!(c.post("/api/plugins/install", json!({"path": p})).await.1["problems"], json!([]));
    assert_eq!(c.post("/api/plugins/install", json!({"path": tmp.path().join("ghost").to_string_lossy()})).await.0, 400);
    assert!(!role_names(&c.get("/api/roles").await.1).contains(&"dba".to_string())); // installed, not enabled
    assert_eq!(c.act("/api/plugins/dbtools/enable").await.1["enabled"], true);
    let roles = c.get("/api/roles").await.1;
    let dba = roles.as_array().unwrap().iter().find(|r| r["name"] == "dba").expect("dba listed").clone();
    assert_eq!(dba["problems"], json!([])); // its skill "pg" is found through the plugin
    let listed = c.get("/api/plugins").await.1;
    assert_eq!(role_names(&listed["installed"]), vec!["dbtools"]);
    assert!(role_names(&listed["available"]).contains(&"software-delivery".to_string()));
    assert_eq!(c.act("/api/plugins/ghost/enable").await.0, 404);
    assert_eq!(c.act("/api/plugins/dbtools/disable").await.1["enabled"], false);
    assert!(!role_names(&c.get("/api/roles").await.1).contains(&"dba".to_string()));
    assert_eq!(c.delete("/api/plugins/dbtools").await.0, 200);
    assert_eq!(c.delete("/api/plugins/dbtools").await.0, 404);
}

#[tokio::test]
async fn agent_builder_creates_an_installed_plugin_with_the_role() {
    let e = env();
    let c = e.c.authed().await;
    let body = json!({"name": "dba", "description": "Tunes databases", "instructions": "You tune databases.", "tools": ["read_file", "list_dir"], "skills": []});
    let (st, r) = c.post("/api/agents/build", body).await;
    assert!(st == 200 && r["problems"] == json!([]) && r["plugin"] == "agent-dba", "{r}");
    assert_eq!(c.act("/api/plugins/agent-dba/enable").await.1["enabled"], true);
    let roles = c.get("/api/roles").await.1;
    let dba = roles.as_array().unwrap().iter().find(|x| x["name"] == "dba").unwrap().clone();
    assert!(dba["description"] == "Tunes databases" && dba["tools"] == json!(["read_file", "list_dir"]));
}

#[tokio::test]
async fn agent_builder_refuses_unknown_tools_missing_skills_and_bad_names() {
    let e = env();
    let c = e.c.authed().await;
    let base = json!({"name": "dba", "description": "d", "instructions": "x", "tools": ["read_file"], "skills": []});
    let with = |k: &str, v: Value| {
        let mut b = base.clone();
        b[k] = v;
        b
    };
    let (st, r) = c.post("/api/agents/build", with("tools", json!(["flying"]))).await;
    assert!(st == 400 && r["detail"].as_str().unwrap().contains("unknown tool flying"), "{r}");
    assert_eq!(c.post("/api/agents/build", with("skills", json!(["ghost"]))).await.0, 400);
    assert_eq!(c.post("/api/agents/build", with("name", json!("Bad Name!"))).await.0, 400);
    assert_eq!(c.post("/api/agents/build", with("instructions", json!("  "))).await.0, 400);
    assert_eq!(c.get("/api/plugins").await.1["installed"], json!([])); // nothing half-installed
}

#[tokio::test]
async fn a_plugin_can_be_exported_as_a_zip() {
    let e = env();
    let c = e.c.authed().await;
    c.post("/api/agents/build", json!({"name": "dba", "description": "d", "instructions": "You tune.", "tools": ["read_file"], "skills": []})).await;
    let (st, body) = c.get("/api/plugins/agent-dba/export").await;
    assert!(st == 200 && body.as_str().is_some_and(|s| s.starts_with("PK")), "{body}"); // a zip archive
    assert_eq!(c.get("/api/plugins/ghost/export").await.0, 404);
}

#[tokio::test]
async fn mcp_servers_are_saved_listed_and_deleted() {
    let e = env();
    let c = e.c.authed().await;
    let cfg = json!({"transport": "stdio", "command": ["server", "--x"], "roles": ["developer_1"]});
    let saved = c.put("/api/mcp/servers/files", cfg).await.1;
    assert!(saved["name"] == "files" && saved["enabled"] == true, "{saved}");
    assert_eq!(c.get("/api/mcp/servers").await.1.as_array().unwrap().len(), 1);
    assert_eq!(c.put("/api/mcp/servers/bad__name", json!({"transport": "stdio", "command": ["x"]})).await.0, 400);
    assert_eq!(c.put("/api/mcp/servers/web", json!({"transport": "http", "url": "ftp://x"})).await.0, 400);
    assert_eq!(c.put("/api/mcp/servers/web", json!({"transport": "carrier-pigeon"})).await.0, 400);
    assert_eq!(c.delete("/api/mcp/servers/files").await.1, json!({"ok": true}));
    assert_eq!(c.delete("/api/mcp/servers/files").await.0, 404);
}

#[tokio::test]
async fn mcp_test_reports_a_server_that_cannot_start_without_a_500() {
    let e = env();
    let c = e.c.authed().await;
    c.put("/api/mcp/servers/dead", json!({"transport": "stdio", "command": ["no-such-program-xyz"]})).await;
    let (st, r) = c.act("/api/mcp/servers/dead/test").await;
    assert!(st == 200 && r["ok"] == false && r["tools"] == json!([]) && r["error"].is_string(), "{r}");
    assert_eq!(c.act("/api/mcp/servers/ghost/test").await.0, 404);
}

#[tokio::test]
async fn mcp_test_lists_tools_over_http() {
    let e = env();
    let c = e.c.authed().await;
    e.c.upstream.set(|req| {
        let m: Value = req.body.clone();
        let result = match m["method"].as_str() {
            Some("tools/list") => json!({"tools": [{"name": "echo", "description": "say it back"}]}),
            _ => json!({}),
        };
        common::ok_json(200, json!({"jsonrpc": "2.0", "id": m["id"], "result": result}))
    });
    c.put("/api/mcp/servers/web", json!({"transport": "http", "url": "https://mcp.example/rpc"})).await;
    let r = c.act("/api/mcp/servers/web/test").await.1;
    assert!(r["ok"] == true && r["tools"] == json!([{"name": "echo", "description": "say it back"}]), "{r}");
}

fn feature(id: &str) {
    DocRepo::new("features").unwrap().create(json!({"title": "f", "state": "building"}), Some(id)).unwrap();
}

#[tokio::test]
async fn feature_budget_is_set_read_and_validated() {
    let e = env();
    let c = e.c.authed().await;
    feature("f1");
    assert_eq!(c.get("/api/features/ghost/budget").await.0, 404);
    let s = c.get("/api/features/f1/budget").await.1;
    assert!(s["limit"].is_null() && s["used"] == 0 && s["held"] == false, "{s}");
    let s = c.put("/api/features/f1/budget", json!({"limit": 5000})).await.1;
    assert!(s["limit"] == 5000 && s["remaining"] == 5000, "{s}");
    assert_eq!(c.put("/api/features/f1/budget", json!({"limit": 0})).await.0, 400);
    assert_eq!(c.put("/api/features/ghost/budget", json!({"limit": 5})).await.0, 404);
    assert!(c.put("/api/features/f1/budget", json!({})).await.1["limit"].is_null()); // no limit clears it
    assert_eq!(c.get("/api/budget").await.0, 200);
}

fn profile(n: i64) -> Value {
    json!({"max_steps": n, "max_total_tokens": 1000, "max_output_tokens": 100, "compact_at_chars": 5000, "keep_recent": 3})
}

#[tokio::test]
async fn profiles_list_save_activate_and_delete() {
    let e = env();
    let c = e.c.authed().await;
    let v = c.get("/api/profiles").await.1;
    assert_eq!(v["active"], "balanced");
    assert_eq!(v["profiles"].as_array().unwrap().len(), 3);
    let v = c.put("/api/profiles/mine", profile(7)).await.1;
    assert_eq!(v["profiles"].as_array().unwrap().len(), 4);
    assert_eq!(c.put("/api/profiles/lean", profile(7)).await.0, 400); // built-ins are read only
    assert_eq!(c.put("/api/profiles/bad", json!({"max_steps": 1})).await.0, 400);
    assert_eq!(c.post("/api/profiles/active", json!({"name": "mine"})).await.1["active"], "mine");
    assert_eq!(c.post("/api/profiles/active", json!({"name": "ghost"})).await.0, 404);
    assert_eq!(c.delete("/api/profiles/lean").await.0, 400);
    assert_eq!(c.delete("/api/profiles/ghost").await.0, 404);
    assert_eq!(c.delete("/api/profiles/mine").await.1["active"], "balanced"); // deleting the active one falls back
}

#[tokio::test]
async fn feature_and_task_skills_are_validated_against_the_catalog() {
    let e = env();
    let c = e.c.authed().await;
    feature("f1");
    let tmp = tempfile::tempdir().unwrap();
    let p = make_plugin(tmp.path(), "dbtools", "[pg]");
    c.post("/api/plugins/install", json!({"path": p})).await;
    c.act("/api/plugins/dbtools/enable").await;
    let v = c.get("/api/features/f1/skills").await.1;
    assert_eq!(v["selected"], json!([]));
    assert!(v["available"].as_array().unwrap().iter().any(|s| s["name"] == "pg" && s["tokens"].as_u64().unwrap() > 0));
    let v = c.put("/api/features/f1/skills", json!({"skills": ["pg", "pg"]})).await.1;
    assert_eq!(v["selected"], json!(["pg"])); // de-duplicated
    let (st, r) = c.put("/api/features/f1/skills", json!({"skills": ["ghost"]})).await;
    assert!(st == 400 && r["detail"] == "unknown skill(s): ghost", "{r}");
    assert_eq!(c.put("/api/features/ghost/skills", json!({"skills": []})).await.0, 404);
    DocRepo::new("tasks").unwrap().create(json!({"title": "t"}), Some("t1")).unwrap();
    let r = c.put("/api/pipeline/tasks/t1/skills", json!({"skills": ["pg"]})).await.1;
    assert!(r["selected"] == json!(["pg"]) && r["tokens"].as_u64().unwrap() > 0, "{r}");
    assert_eq!(c.put("/api/pipeline/tasks/t1/skills", json!({"skills": ["ghost"]})).await.0, 400);
    assert_eq!(c.put("/api/pipeline/tasks/nope/skills", json!({"skills": []})).await.0, 404);
}
