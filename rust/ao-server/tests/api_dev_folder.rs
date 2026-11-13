//! Where agents write code: the development folder (`repo_path`) and where task worktrees go (`worktree_root`).
mod common;
use common::env;
use serde_json::json;
use std::process::Command;

fn git_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(d.path()).status().unwrap().success());
    d
}

#[tokio::test]
async fn a_path_that_is_not_a_folder_is_refused_and_nothing_is_saved() {
    let e = env();
    let c = e.c.authed().await;
    let r = c.put("/api/settings", json!({"repo_path": "C:/definitely/not/here"})).await;
    assert_eq!(r.0, 400);
    assert!(r.1["detail"].as_str().unwrap().contains("repo_path is not a folder"), "{:?}", r.1);
    assert_eq!(c.get("/api/settings").await.1["repo_path"], "");
}

#[tokio::test]
async fn pasted_quotes_and_spaces_are_cleaned_and_a_blank_clears_it() {
    let e = env();
    let c = e.c.authed().await;
    let d = git_repo();
    let p = d.path().to_string_lossy().into_owned();
    let saved = c.put("/api/settings", json!({"repo_path": format!("  \"{p}\"  ")})).await;
    assert_eq!(saved.0, 200, "{:?}", saved.1);
    assert_eq!(saved.1["repo_path"], p.as_str());
    assert_eq!(c.put("/api/settings", json!({"repo_path": "  "})).await.1["repo_path"], "");
}

#[tokio::test]
async fn worktrees_may_not_live_inside_the_repository() {
    let e = env();
    let c = e.c.authed().await;
    let d = git_repo();
    let p = d.path().to_string_lossy().into_owned();
    c.put("/api/settings", json!({"repo_path": p})).await;
    let inside = d.path().join("wt").to_string_lossy().into_owned();
    let r = c.put("/api/settings", json!({"worktree_root": inside})).await;
    assert_eq!(r.0, 400);
    assert!(r.1["detail"].as_str().unwrap().contains("must not be inside"), "{:?}", r.1);
    let elsewhere = tempfile::tempdir().unwrap();
    let ok = c.put("/api/settings", json!({"worktree_root": elsewhere.path().join("new-folder").to_string_lossy()})).await;
    assert_eq!(ok.0, 200, "a folder that does not exist yet is fine, it is created when needed");
}

#[tokio::test]
async fn the_dev_folder_report_says_whether_it_is_a_git_repository_and_where_worktrees_go() {
    let e = env();
    let c = e.c.authed().await;
    let none = c.get("/api/dev-folder").await;
    assert_eq!(none.0, 200);
    assert_eq!(none.1["repo_path"], "");
    assert_eq!(none.1["exists"], false);
    assert!(none.1["worktree_root"].as_str().unwrap().replace('\\', "/").ends_with("/worktrees"), "{:?}", none.1);

    let plain = tempfile::tempdir().unwrap();
    c.put("/api/settings", json!({"repo_path": plain.path().to_string_lossy()})).await;
    let r = c.get("/api/dev-folder").await.1;
    assert_eq!((r["exists"].clone(), r["is_git"].clone()), (json!(true), json!(false)));

    let g = git_repo();
    c.put("/api/settings", json!({"repo_path": g.path().to_string_lossy()})).await;
    assert_eq!(c.get("/api/dev-folder").await.1["is_git"], true);
}

#[tokio::test]
async fn the_dev_folder_route_needs_a_session() {
    let e = env();
    assert_eq!(e.c.get("/api/dev-folder").await.0, 401);
}
