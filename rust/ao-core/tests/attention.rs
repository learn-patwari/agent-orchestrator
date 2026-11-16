//! The attention feed: everything that waits for the human, each with a link to the exact place.
use ao_core::attention;
use ao_core::db::{self, now, DocRepo};
use ao_core::inbox::{request_gate, Inbox};
use serde_json::json;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn with_env<R>(f: impl FnOnce() -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    f()
}

fn kinds() -> Vec<String> {
    attention::items().unwrap().iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn nothing_waiting_means_an_empty_feed_and_a_zero_count() {
    with_env(|| {
        assert!(attention::items().unwrap().is_empty());
        assert_eq!(attention::count().unwrap(), 0);
    });
}

#[test]
fn an_srs_approval_links_to_its_feature_and_a_plain_one_links_to_the_inbox() {
    with_env(|| {
        let fid = DocRepo::new("features").unwrap().create(json!({"title": "notify", "status": "srs_draft"}), None).unwrap()["id"].as_str().unwrap().to_string();
        let gate = request_gate("srs_approval", "Approve the SRS", None).unwrap();
        DocRepo::new("inbox").unwrap().update(gate["id"].as_str().unwrap(), json!({"feature_id": fid})).unwrap();
        request_gate("merge_main", "Merge to main", None).unwrap();
        let items = attention::items().unwrap();
        assert_eq!(items.len(), 2);
        let srs = items.iter().find(|i| i["link"].as_str().unwrap().starts_with("/features")).unwrap();
        assert_eq!(srs["kind"], "approval");
        assert_eq!(srs["link"], format!("/features/?id={fid}"));
        assert!(items.iter().any(|i| i["link"] == "/inbox/"));
    });
}

#[test]
fn open_questions_count_and_answered_ones_drop_out() {
    with_env(|| {
        let q = Inbox::new(false).ask_with("DB?", "Which database?", &["sqlite", "mongo"], Some("sqlite"), None, json!({})).unwrap();
        assert_eq!(kinds(), vec!["question"]);
        Inbox::new(false).answer(q["id"].as_str().unwrap(), "mongo").unwrap();
        assert_eq!(attention::count().unwrap(), 0);
    });
}

#[test]
fn finished_and_failed_runs_show_until_seen_and_old_ones_never_do() {
    with_env(|| {
        let runs = DocRepo::new("runs").unwrap();
        let done = runs.create(json!({"role": "developer_1", "task": "Create hello.txt", "status": "done", "finished_at": now()}), None).unwrap();
        runs.create(json!({"role": "tester", "task": "Run tests", "status": "failed", "error": "429", "finished_at": now()}), None).unwrap();
        runs.create(json!({"role": "cicd", "task": "old", "status": "done", "finished_at": now() - 3.0 * 86400.0}), None).unwrap();
        runs.create(json!({"role": "architect", "task": "busy", "status": "running"}), None).unwrap();
        let mut k = kinds();
        k.sort();
        assert_eq!(k, vec!["task_done", "task_failed"]);
        attention::mark_seen(&[format!("run:{}", done["id"].as_str().unwrap())]).unwrap();
        assert_eq!(kinds(), vec!["task_failed"]);
        let failed = &attention::items().unwrap()[0];
        assert!(failed["title"].as_str().unwrap().contains("tester") && failed["detail"].as_str().unwrap().contains("429"), "{failed}");
    });
}

#[test]
fn the_newest_item_comes_first() {
    with_env(|| {
        let runs = DocRepo::new("runs").unwrap();
        runs.create(json!({"role": "a", "task": "t", "status": "done", "finished_at": now() - 100.0}), None).unwrap();
        request_gate("merge_main", "Merge", None).unwrap();
        assert_eq!(kinds(), vec!["approval", "task_done"]);
    });
}
