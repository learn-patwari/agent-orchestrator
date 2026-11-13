//! The shared read-only snapshot: agents read facts from it, the database stays the one place that is written.
use ao_core::db::{self, DocRepo};
use ao_core::snapshot::{self, inbox_for, post, rebuild, MAX_TEXT};
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

fn run(role: &str, task: &str, status: &str) -> String {
    let r = DocRepo::new("runs").unwrap().create(json!({"role": role, "task": task, "status": status, "steps": 3, "error": null}), None).unwrap();
    r["id"].as_str().unwrap().to_string()
}

#[test]
fn a_rebuild_shows_each_roles_latest_run_and_roles_with_no_run_as_idle() {
    with_env(|| {
        run("architect", "old job", "done");
        run("architect", "design the API", "running");
        let s = rebuild(&["architect".into(), "tester".into()]).unwrap();
        let row = |r: &str| s.team.iter().find(|x| x["role"] == r).unwrap().clone();
        assert_eq!(row("architect")["state"], "running");
        assert_eq!(row("architect")["task"], "design the API");
        assert_eq!(row("tester")["state"], "idle");
        assert_eq!(s.runs.len(), 2);
    });
}

#[test]
fn readers_keep_a_stable_copy_until_the_next_publish() {
    with_env(|| {
        let first = rebuild(&["tester".into()]).unwrap();
        run("tester", "run the suite", "running"); // a write after the snapshot was taken
        let still = snapshot::current();
        assert_eq!(still.version, first.version);
        assert_eq!(still.runs.len(), 0, "a write is not visible until the next publish");
        let next = rebuild(&["tester".into()]).unwrap();
        assert!(next.version > first.version);
        assert_eq!(snapshot::current().runs.len(), 1);
        assert_eq!(first.runs.len(), 0, "a copy a reader already holds never changes");
    });
}

#[test]
fn messages_between_agents_go_through_the_mailbox_and_arrive_in_the_next_snapshot() {
    with_env(|| {
        post("architect", "developer_1", "the schema is in docs/schema.md").unwrap();
        post("tester", "developer_1", "login test fails on empty password").unwrap();
        post("architect", "tester", "use the staging data").unwrap();
        assert!(inbox_for(&rebuild(&[]).unwrap(), "developer_1").len() == 2);
        let s = snapshot::current();
        let mine = inbox_for(&s, "developer_1");
        assert_eq!(mine[0]["from"], "architect");
        assert_eq!(mine[1]["text"], "login test fails on empty password");
        assert_eq!(inbox_for(&s, "tester").len(), 1);
        assert!(inbox_for(&s, "nobody").is_empty());
    });
}

#[test]
fn a_message_needs_a_recipient_and_text_and_long_text_is_cut() {
    with_env(|| {
        assert!(post("a", "", "hi").is_err());
        assert!(post("a", "b", "  ").is_err());
        let m = post("a", "b", &"x".repeat(MAX_TEXT * 3)).unwrap();
        assert!(m["text"].as_str().unwrap().chars().count() <= MAX_TEXT + 1);
    });
}

#[test]
fn long_reports_are_shortened_in_the_snapshot_but_not_in_the_database() {
    with_env(|| {
        let id = run("tester", &"t".repeat(900), "done");
        DocRepo::new("runs").unwrap().update(&id, json!({"result": "r".repeat(5000)})).unwrap();
        let s = rebuild(&["tester".into()]).unwrap();
        assert!(s.runs[0]["task"].as_str().unwrap().chars().count() <= 130);
        assert!(s.runs[0]["result"].as_str().unwrap().chars().count() <= 700);
        assert_eq!(DocRepo::new("runs").unwrap().get(&id).unwrap().unwrap()["result"].as_str().unwrap().len(), 5000);
    });
}

#[test]
fn the_publisher_refreshes_the_snapshot_on_its_own() {
    with_env(|| {
        let before = rebuild(&[]).unwrap().version;
        let p = snapshot::start_publisher(std::time::Duration::from_millis(20), Vec::new);
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(snapshot::current().version > before + 2);
        drop(p); // stops the thread
        std::thread::sleep(std::time::Duration::from_millis(80));
        let v = snapshot::current().version;
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert_eq!(snapshot::current().version, v, "a stopped publisher publishes nothing more");
    });
}
