//! The `state` and `tell` tools: agents read facts from the shared snapshot and leave each other messages, with no model call.
use ao_core::db::{self, DocRepo};
use ao_core::runtime::roles::default_tools;
use ao_core::runtime::tools::{ToolBox, Workspace};
use ao_core::snapshot::rebuild;
use serde_json::json;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn with_box<R>(role: &str, f: impl FnOnce(&ToolBox) -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    f(&ToolBox::new(Workspace::new(ws.path()), None).role(role))
}

const BOTH: [&str; 2] = ["state", "tell"];

#[test]
fn every_role_can_read_state_and_leave_a_message() {
    for role in ["developer_1", "tester", "architect", "orchestrator", "cicd", "someone_new"] {
        let t = default_tools(role);
        assert!(t.contains(&"state".to_string()) && t.contains(&"tell".to_string()), "{role}");
    }
}

#[test]
fn state_answers_team_runs_and_reports_from_the_snapshot() {
    with_box("tester", |t| {
        let id = DocRepo::new("runs").unwrap().create(json!({"role": "architect", "task": "design the API", "status": "done", "steps": 4, "error": null, "result": "API drafted in docs/api.md"}), None).unwrap()["id"].as_str().unwrap().to_string();
        rebuild(&["architect".into(), "tester".into()]).unwrap();
        let team = t.call(&BOTH, "state", &json!({"action": "team"})).unwrap();
        assert!(team.contains("architect") && team.contains("tester") && team.contains("idle"), "{team}");
        let runs = t.call(&BOTH, "state", &json!({"action": "runs"})).unwrap();
        assert!(runs.contains("design the API"), "{runs}");
        let rep = t.call(&BOTH, "state", &json!({"action": "report", "query": id})).unwrap();
        assert!(rep.contains("API drafted"), "{rep}");
        assert!(t.call(&BOTH, "state", &json!({"action": "report", "query": "nope"})).unwrap_err().to_string().contains("no run"));
        assert!(t.call(&BOTH, "state", &json!({"action": "explode"})).unwrap_err().to_string().contains("unknown state action"));
    });
}

#[test]
fn a_message_from_one_agent_reaches_the_other_through_the_next_snapshot() {
    with_box("architect", |sender| {
        let sent = sender.call(&BOTH, "tell", &json!({"to": "developer_1", "text": "schema is in docs/schema.md"})).unwrap();
        assert!(sent.contains("developer_1"), "{sent}");
        let ws = tempfile::tempdir().unwrap();
        let dev = ToolBox::new(Workspace::new(ws.path()), None).role("developer_1");
        let before = dev.call(&BOTH, "state", &json!({"action": "inbox"})).unwrap();
        assert!(!before.contains("schema is in"), "not yet published: {before}");
        rebuild(&[]).unwrap();
        let after = dev.call(&BOTH, "state", &json!({"action": "inbox"})).unwrap();
        assert!(after.contains("schema is in docs/schema.md") && after.contains("architect"), "{after}");
        assert!(after.contains("untrusted"), "messages are data, not instructions: {after}");
        assert!(sender.call(&BOTH, "state", &json!({"action": "inbox"})).unwrap().contains("no messages"));
    });
}

#[test]
fn tell_refuses_an_empty_message_and_a_role_without_the_tool() {
    with_box("tester", |t| {
        assert!(t.call(&BOTH, "tell", &json!({"to": "architect", "text": ""})).is_err());
        assert!(t.call(&BOTH, "tell", &json!({"text": "hi"})).is_err());
        assert!(t.call(&["state"], "tell", &json!({"to": "a", "text": "hi"})).is_err());
    });
}
