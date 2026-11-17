//! Per-feature setup: its own code folder with its own knowledge graph, its own model, and skills uploaded by the user.
mod common;
use ao_core::db::DocRepo;
use common::env;
use serde_json::json;

fn feature(extra: serde_json::Value) -> String {
    let mut f = json!({"title": "f", "status": "approved"});
    f.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    DocRepo::new("features").unwrap().create(f, None).unwrap()["id"].as_str().unwrap().to_string()
}

fn repo(file: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(file), "def go():\n    return 1\n").unwrap();
    d
}

#[tokio::test]
async fn a_feature_keeps_its_own_folder_and_it_must_exist() {
    let e = env();
    let c = e.c.authed().await;
    let fid = feature(json!({}));
    let r = repo("a.py");
    let (st, out) = c.put(&format!("/api/features/{fid}/setup"), json!({"repo_path": r.path().to_string_lossy()})).await;
    assert_eq!(st, 200, "{out}");
    assert_eq!(c.get(&format!("/api/features/{fid}/setup")).await.1["repo_path"], r.path().to_string_lossy().to_string());
    assert_eq!(c.put(&format!("/api/features/{fid}/setup"), json!({"repo_path": "Z:/nowhere/at/all"})).await.0, 400);
    assert_eq!(c.put("/api/features/ghost/setup", json!({"repo_path": ""})).await.0, 404);
}

#[tokio::test]
async fn each_feature_builds_a_graph_of_its_own_folder_not_the_global_one() {
    let e = env();
    let c = e.c.authed().await;
    let global = repo("g1.py");
    std::fs::write(global.path().join("g2.py"), "x = 1\n").unwrap();
    c.put("/api/settings", json!({"repo_path": global.path().to_string_lossy()})).await;
    let (a, b) = (repo("a.py"), repo("b.py"));
    let (fa, fb) = (feature(json!({})), feature(json!({})));
    c.put(&format!("/api/features/{fa}/setup"), json!({"repo_path": a.path().to_string_lossy()})).await;
    c.put(&format!("/api/features/{fb}/setup"), json!({"repo_path": b.path().to_string_lossy()})).await;
    assert_eq!(c.get(&format!("/api/features/{fa}/graph")).await.1["built"], false);
    let built = c.post(&format!("/api/features/{fa}/graph/build"), json!({})).await;
    assert_eq!(built.0, 200, "{:?}", built.1);
    assert_eq!(built.1["files"], 1);
    assert_eq!(built.1["built"], true);
    assert_eq!(c.get(&format!("/api/features/{fb}/graph")).await.1["built"], false, "B has not been built");
    // a feature with no folder of its own falls back to the global one
    let fc = feature(json!({}));
    assert_eq!(c.post(&format!("/api/features/{fc}/graph/build"), json!({})).await.1["files"], 2);
}

#[tokio::test]
async fn a_feature_can_pick_its_own_model_and_go_back_to_the_role_defaults() {
    let e = env();
    let c = e.c.authed().await;
    let pid = c.post("/api/providers", json!({"name": "p", "type": "openai", "base_url": "https://x/v1", "api_key": "sk-test-abcdefghijkl"})).await.1["id"].as_str().unwrap().to_string();
    let fid = feature(json!({}));
    let url = format!("/api/features/{fid}/setup");
    assert_eq!(c.put(&url, json!({"model": {"provider_id": pid, "model": "big"}})).await.0, 200);
    assert_eq!(c.get(&url).await.1["model"], json!({"provider_id": pid, "model": "big"}));
    assert_eq!(c.put(&url, json!({"model": {"provider_id": "ghost", "model": "big"}})).await.0, 400);
    assert_eq!(c.put(&url, json!({"model": null})).await.0, 200);
    assert!(c.get(&url).await.1["model"].is_null());
}

#[tokio::test]
async fn an_uploaded_skill_shows_up_in_the_catalog_and_can_be_chosen_for_a_feature() {
    let e = env();
    let c = e.c.authed().await;
    let up = c.post("/api/skills/upload", json!({"name": "postgres-tips", "description": "How we use Postgres", "body": "Use indexes."})).await;
    assert_eq!(up.0, 200, "{:?}", up.1);
    let list = c.get("/api/skills").await.1;
    assert!(list.to_string().contains("postgres-tips"), "{list}");
    let fid = feature(json!({}));
    let r = c.put(&format!("/api/features/{fid}/skills"), json!({"skills": ["postgres-tips"]})).await;
    assert_eq!(r.0, 200, "{:?}", r.1);
    assert_eq!(r.1["selected"], json!(["postgres-tips"]));
    // names are safe folder names, and one never silently replaces another
    assert_eq!(c.post("/api/skills/upload", json!({"name": "../evil", "description": "d", "body": "b"})).await.0, 400);
    assert_eq!(c.post("/api/skills/upload", json!({"name": "postgres-tips", "description": "d", "body": "b"})).await.0, 409);
    assert_eq!(c.post("/api/skills/upload", json!({"name": "empty", "description": "d", "body": "  "})).await.0, 400);
}
