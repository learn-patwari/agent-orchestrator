//! Phase 2b: releases, incidents and the common rules. Ported from backend/tests/test_releases.py,
//! test_incidents.py (the Incidents class) and test_writer_and_guards.py (the rules).
use ao_core::db::{self, DocRepo};
use ao_core::incidents::{IncidentError, Incidents};
use ao_core::inbox::Inbox;
use ao_core::releases::{generate_notes, next_version, suggest_bump};
use ao_core::rules::{OWNERSHIP, RULES_FOR_ALL};
use serde_json::{json, Value};
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn with_db<R>(f: impl FnOnce() -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", dir.path());
    db::init_db().unwrap();
    f()
}

// ---------------------------------------------------------------- releases (pure)

#[test]
fn a_breaking_change_suggests_major() {
    assert_eq!(suggest_bump(&["feat: add x", "fix!: drop old api"]), Some("major"));
    assert_eq!(suggest_bump(&["feat: add x\n\nBREAKING CHANGE: removed y"]), Some("major"));
}

#[test]
fn a_feature_suggests_minor_and_a_fix_patch() {
    assert_eq!(suggest_bump(&["fix: typo", "feat(ui): button"]), Some("minor"));
    assert_eq!(suggest_bump(&["fix: typo", "docs: readme"]), Some("patch"));
}

#[test]
fn no_commits_suggest_nothing() {
    assert_eq!(suggest_bump(&[]), None);
    assert_eq!(suggest_bump(&["just some words"]), None);
}

#[test]
fn next_version_bumps() {
    assert_eq!(next_version(Some("1.2.3"), "major").unwrap(), "2.0.0");
    assert_eq!(next_version(Some("1.2.3"), "minor").unwrap(), "1.3.0");
    assert_eq!(next_version(Some("1.2.3"), "patch").unwrap(), "1.2.4");
    assert_eq!(next_version(Some("v0.9.0"), "minor").unwrap(), "0.10.0");
    assert_eq!(next_version(None, "minor").unwrap(), "0.1.0");
}

#[test]
fn next_version_rejects_bad_input() {
    assert!(next_version(Some("1.2"), "patch").is_err());
    assert!(next_version(Some("1.2.3"), "huge").is_err());
}

#[test]
fn notes_group_by_type_and_list_breaking_first() {
    let notes = generate_notes("1.3.0", &["feat: add export", "fix: crash on empty", "fix!: rename flag", "chore: deps"]);
    assert!(notes.starts_with("# 1.3.0"));
    let at = |s: &str| notes.find(s).unwrap();
    assert!(at("Breaking changes") < at("Features") && at("Features") < at("Fixes"));
    for line in ["- add export", "- crash on empty", "- rename flag"] {
        assert!(notes.contains(line), "{line}");
    }
    assert!(!notes.contains("deps"));
}

#[test]
fn notes_for_an_empty_release_say_so() {
    assert!(generate_notes("1.0.1", &["chore: deps"]).contains("No user-facing changes"));
}

// ---------------------------------------------------------------- rules

#[test]
fn the_common_rules_say_what_the_orchestrator_asked() {
    for must in ["do not fix it", "incident_reporter", "technical_writer", "architect", "Stay in your lane", "never the user's approval", "Not verified"] {
        assert!(RULES_FOR_ALL.contains(must), "{must}");
    }
    assert!(OWNERSHIP.contains("WHO OWNS WHAT"));
    assert!(OWNERSHIP.contains("release notes") && OWNERSHIP.contains("user guides"));
    assert!(OWNERSHIP.contains("technical_writer"));
}

// ---------------------------------------------------------------- incidents

fn handoffs() -> Vec<(String, String)> {
    let mut h = DocRepo::new("handoffs").unwrap().list().unwrap();
    h.sort_by(|a, b| a["at"].as_f64().partial_cmp(&b["at"].as_f64()).unwrap());
    h.iter().map(|x| (x["from"].as_str().unwrap().to_string(), x["to"].as_str().unwrap().to_string())).collect()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

fn make_task(owner: &str) -> String {
    let t = DocRepo::new("tasks").unwrap()
        .create(json!({"name": "login", "owner": owner, "reviewer": "developer_1", "status": "approved", "release": "in_prod"}), None)
        .unwrap();
    t["id"].as_str().unwrap().to_string()
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_string()
}

#[test]
fn a_reported_incident_starts_at_l1_and_goes_to_production_support() {
    with_db(|| {
        let inc = Incidents.report("Login 500", "high", "POST /login returns 500", "prod", None).unwrap();
        assert_eq!((inc["level"].clone(), inc["status"].clone(), inc["severity"].clone()), (json!(1), json!("open"), json!("high")));
        assert_eq!(handoffs(), vec![pair("incident_reporter", "production_support")]);
    });
}

#[test]
fn the_same_open_incident_is_not_reported_twice() {
    with_db(|| {
        let i = Incidents;
        let a = i.report("Login 500", "high", "e", "prod", None).unwrap();
        let b = i.report("login 500 ", "high", "e again", "prod", None).unwrap();
        assert_eq!(a["id"], b["id"]);
        assert_eq!(i.list(true).unwrap().len(), 1);
        assert_eq!(handoffs().len(), 1);
        i.resolve(&id(&a)).unwrap();
        assert_ne!(i.report("Login 500", "high", "back", "prod", None).unwrap()["id"], a["id"]); // a resolved one can come back
    });
}

#[test]
fn bad_reports_are_refused() {
    with_db(|| {
        assert!(matches!(Incidents.report("x", "catastrophic", "e", "prod", None), Err(IncidentError::Invalid(m)) if m.contains("severity")));
        assert!(matches!(Incidents.report("  ", "low", "e", "prod", None), Err(IncidentError::Invalid(_))));
        assert!(matches!(Incidents.get("nope"), Err(IncidentError::Invalid(m)) if m.contains("unknown incident")));
    });
}

#[test]
fn escalation_goes_l1_to_l4_then_asks_the_human_and_stops() {
    with_db(|| {
        let i = Incidents;
        let inc = i.report("Slow", "medium", "e", "prod", None).unwrap();
        let iid = id(&inc);
        assert_eq!(i.escalate(&iid, "needs a code change").unwrap()["level"], 2);
        assert_eq!(i.escalate(&iid, "design issue").unwrap()["level"], 3);
        assert_eq!(handoffs().last().unwrap(), &pair("production_support", "architect")); // L3 is the architect's call
        assert_eq!(i.escalate(&iid, "outage").unwrap()["level"], 4);
        let open: Vec<_> = Inbox::new(false).list(Some("open")).unwrap().iter().map(|x| x["title"].clone()).collect();
        assert_eq!(open, vec![json!("Incident needs you: Slow")]);
        assert!(matches!(i.escalate(&iid, "more"), Err(IncidentError::Invalid(m)) if m.contains("L4")));
        let levels: Vec<_> = i.get(&iid).unwrap()["history"].as_array().unwrap().iter().map(|h| h["level"].clone()).collect();
        assert_eq!(levels, vec![json!(2), json!(3), json!(4)]);
    });
}

#[test]
fn a_fix_goes_to_the_developer_who_owns_the_feature_then_resolves() {
    with_db(|| {
        let i = Incidents;
        let inc = i.report("Login 500", "high", "e", "prod", Some(&make_task("developer_2"))).unwrap();
        let out = i.hand_to_developer(&id(&inc)).unwrap();
        assert_eq!((out["status"].clone(), out["fix_owner"].clone()), (json!("fixing"), json!("developer_2")));
        assert_eq!(handoffs().last().unwrap(), &pair("production_support", "developer_2"));
        assert_eq!(i.resolve(&id(&inc)).unwrap()["status"], "resolved");
        assert!(i.resolve(&id(&inc)).is_err());
    });
}

#[test]
fn a_fix_needs_the_task_that_owns_the_feature() {
    with_db(|| {
        let inc = Incidents.report("Mystery", "low", "e", "prod", None).unwrap();
        assert!(matches!(Incidents.hand_to_developer(&id(&inc)), Err(IncidentError::Invalid(m)) if m.contains("task")));
    });
}

#[test]
fn a_failing_production_test_report_raises_an_incident_once() {
    with_db(|| {
        let i = Incidents;
        let tid = make_task("developer_2");
        assert!(i.production_tests(&tid, 10, 0, "smoke").unwrap().is_none());
        let inc = i.production_tests(&tid, 7, 3, "smoke").unwrap().unwrap();
        assert_eq!((inc["source"].clone(), inc["task_id"].clone()), (json!("prod"), json!(tid)));
        assert!(inc["evidence"].as_str().unwrap().contains("3 failing"));
        assert_eq!(i.production_tests(&tid, 7, 3, "smoke").unwrap().unwrap()["id"], inc["id"]);
        assert_eq!(i.list(true).unwrap().len(), 1);
        assert!(matches!(i.production_tests("zzz", 1, 1, "a"), Err(IncidentError::Invalid(m)) if m.contains("unknown task")));
    });
}

#[test]
fn open_incidents_can_be_listed_without_resolved_ones() {
    with_db(|| {
        let i = Incidents;
        let a = i.report("One", "low", "e", "prod", None).unwrap();
        i.report("Two", "low", "e", "prod", None).unwrap();
        i.resolve(&id(&a)).unwrap();
        assert_eq!(i.list(true).unwrap().len(), 2);
        assert_eq!(i.list(false).unwrap().len(), 1);
    });
}

#[test]
fn the_rules_text_is_identical_to_the_python_backends() {
    // fixtures/rules.json is dumped from backend/app/rules.py; it keeps the two backends from drifting apart
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rules.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    assert_eq!(RULES_FOR_ALL, v["rules"].as_str().unwrap());
    assert_eq!(OWNERSHIP, v["ownership"].as_str().unwrap());
}
