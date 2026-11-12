//! The exported UI is served from the API's port; the API keeps priority.
mod common;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use tower::ServiceExt;

async fn get(r: &axum::Router, p: &str) -> (u16, String) {
    let resp = r.clone().oneshot(Request::builder().uri(p).body(Body::empty()).unwrap()).await.unwrap();
    let st = resp.status().as_u16();
    (st, String::from_utf8_lossy(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).into_owned())
}

#[tokio::test]
async fn pages_assets_and_the_api_share_one_port() {
    let _e = common::env();
    let ui = tempfile::tempdir().unwrap();
    std::fs::write(ui.path().join("index.html"), "<h1>home</h1>").unwrap();
    std::fs::write(ui.path().join("chat.html"), "<h1>chat</h1>").unwrap();
    std::fs::create_dir_all(ui.path().join("_next/static")).unwrap();
    std::fs::write(ui.path().join("_next/static/a.js"), "x=1").unwrap();
    std::fs::write(ui.path().join("404.html"), "nope").unwrap();
    let r = ao_server::app_with_ui(ao_server::AppState::default(), Some(ui.path().to_path_buf()));
    assert_eq!(get(&r, "/").await, (200, "<h1>home</h1>".into()));
    assert_eq!(get(&r, "/chat").await, (200, "<h1>chat</h1>".into()));
    assert_eq!(get(&r, "/_next/static/a.js").await, (200, "x=1".into()));
    assert_eq!(get(&r, "/missing").await, (404, "nope".into()));
    assert_eq!(get(&r, "/api/health").await, (200, r#"{"ok":true}"#.into()));
    assert_eq!(get(&r, "/api/nothing").await, (404, r#"{"detail":"Not Found"}"#.into()));
    assert_eq!(get(&r, "/..%2f..%2fCargo.toml").await.0, 404); // no escaping the UI folder
    assert_eq!(get(&r, "/../Cargo.toml").await.0, 404);
}
