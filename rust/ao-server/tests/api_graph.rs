//! The knowledge graph API: build it for the configured repository, read its status and what it has saved.
mod common;
use common::env;
use serde_json::json;

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("app")).unwrap();
    std::fs::write(d.path().join("app/db.py"), "def get():\n    return 1\n").unwrap();
    std::fs::write(d.path().join("app/main.py"), "from app import db\n\ndef start():\n    return db.get()\n").unwrap();
    d
}

#[tokio::test]
async fn the_graph_needs_a_repository_then_builds_and_reports() {
    let e = env();
    let c = e.c.authed().await;
    let before = c.get("/api/graph").await;
    assert_eq!(before.0, 200);
    assert_eq!(before.1["built"], false);
    assert_eq!(before.1["reason"], "set repo_path in settings first");
    assert_eq!(c.post("/api/graph/build", json!({})).await.0, 400);

    let r = repo();
    c.put("/api/settings", json!({"repo_path": r.path().to_string_lossy()})).await;
    let built = c.post("/api/graph/build", json!({})).await;
    assert_eq!(built.0, 200, "{:?}", built.1);
    assert_eq!(built.1["built"], true);
    assert_eq!(built.1["files"], 2);
    assert_eq!(built.1["symbols"], 2); // get and start
    assert_eq!(built.1["import_links"], 1);
    assert_eq!(built.1["tokens_saved"], 0);
    let again = c.get("/api/graph").await.1;
    assert_eq!(again["files"], 2);
    assert!(again["built_at"].as_f64().unwrap() > 0.0);
    assert_eq!(again["by_language"]["python"], 2);
}

#[tokio::test]
async fn a_saved_folder_that_was_deleted_later_is_a_clear_error() {
    let e = env();
    let c = e.c.authed().await;
    let r = repo();
    assert_eq!(c.put("/api/settings", json!({"repo_path": r.path().to_string_lossy()})).await.0, 200);
    drop(r); // the folder goes away after it was saved
    let r = c.post("/api/graph/build", json!({})).await;
    assert_eq!(r.0, 400);
    assert!(r.1["detail"].as_str().unwrap().contains("not a folder"), "{:?}", r.1);
}

#[tokio::test]
async fn graph_routes_need_a_session() {
    let e = env();
    assert_eq!(e.c.get("/api/graph").await.0, 401);
    assert_eq!(e.c.post("/api/graph/build", json!({})).await.0, 401);
}
