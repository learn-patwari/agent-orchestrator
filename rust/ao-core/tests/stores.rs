//! Phase 2a: notify, handoffs, inbox and gates, scheduler, quota. Ported from
//! backend/tests/test_notify_sched.py, test_inbox.py and test_quota.py.
use ao_core::db::{self, DocRepo};
use ao_core::handoffs;
use ao_core::inbox::{check_gate, request_gate, GateError, Inbox, InboxError, GATED};
use ao_core::notify::Notifier;
use ao_core::quota::{limits_for, Metric, QuotaError, QuotaGuard, Usage};
use ao_core::scheduler::{budget_percent_used, Scheduler};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());
const DAY: f64 = 86400.0;

fn with_db<R>(f: impl FnOnce() -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", dir.path());
    db::init_db().unwrap();
    f()
}

fn kinds(items: &[Value]) -> Vec<String> {
    items.iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect()
}

// ---------------------------------------------------------------- notify and handoffs

#[test]
fn inbox_items_create_unread_notifications_but_status_items_do_not() {
    with_db(|| {
        Inbox::new(false).ask("DB?", "Which?", &[], Some("a"), None).unwrap();
        request_gate("merge_main", "merge PR 5", None).unwrap();
        Inbox::new(false).post("status", "build done", "", json!({})).unwrap();
        assert_eq!(kinds(&Notifier::new().unread().unwrap()), vec!["question", "approval"]);
    });
}

#[test]
fn mark_read_and_dedupe_by_key() {
    with_db(|| {
        let nt = Notifier::new();
        let a = nt.send("run_failed", "Run r1 failed", "", Some("run:r1")).unwrap().unwrap();
        assert!(nt.send("run_failed", "Run r1 failed", "", Some("run:r1")).unwrap().is_none());
        nt.mark_read(a["id"].as_str().unwrap()).unwrap();
        assert!(nt.unread().unwrap().is_empty());
        assert!(nt.send("run_failed", "again", "", Some("run:r1")).unwrap().is_some());
    });
}

#[test]
fn handoffs_have_strictly_increasing_times() {
    with_db(|| {
        let a = handoffs::record("tester", "developer_1", "QC failed", None).unwrap();
        let b = handoffs::record("developer_1", "tester", "fixed", Some("r1")).unwrap();
        assert!(b["at"].as_f64().unwrap() > a["at"].as_f64().unwrap());
        assert_eq!((b["from"].clone(), b["to"].clone(), b["run_id"].clone()), (json!("developer_1"), json!("tester"), json!("r1")));
        assert_eq!(a["run_id"], Value::Null);
    });
}

// ---------------------------------------------------------------- inbox

const OPTS: [&str; 2] = ["sqlite", "mongo"];

#[test]
fn a_question_answered_explicitly_records_no_assumption() {
    with_db(|| {
        let ib = Inbox::new(false);
        let q = ib.ask("DB?", "Which database?", &OPTS, Some("sqlite"), None).unwrap();
        assert_eq!(q["status"], "open");
        let done = ib.answer(q["id"].as_str().unwrap(), "mongo").unwrap();
        assert_eq!((done["status"].clone(), done["answer"].clone(), done["resolved_by"].clone()), (json!("resolved"), json!("mongo"), json!("human")));
        assert!(ib.assumptions().unwrap().is_empty());
    });
}

#[test]
fn accepting_the_default_records_an_assumption() {
    with_db(|| {
        let ib = Inbox::new(false);
        let q = ib.ask("DB?", "Which database?", &OPTS, Some("sqlite"), None).unwrap();
        ib.accept_default(q["id"].as_str().unwrap()).unwrap();
        let a = ib.assumptions().unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!((a[0]["assumed"].clone(), a[0]["status"].clone(), a[0]["item_id"].clone()), (json!("sqlite"), json!("assumed"), q["id"].clone()));
    });
}

#[test]
fn decide_with_defaults_resolves_questions_but_never_gates() {
    with_db(|| {
        let ib = Inbox::new(true);
        let q = ib.ask("DB?", "Which database?", &OPTS, Some("sqlite"), None).unwrap();
        assert_eq!((q["status"].clone(), q["resolved_by"].clone()), (json!("resolved"), json!("auto_default")));
        assert_eq!(ib.assumptions().unwrap()[0]["assumed"], "sqlite");
        assert_eq!(ib.ask("Name?", "Product name?", &[], None, None).unwrap()["status"], "open");
        assert_eq!(request_gate("merge_main", "merge PR 5 into main", None).unwrap()["status"], "open");
    });
}

#[test]
fn an_assumption_can_be_overruled_later() {
    with_db(|| {
        let ib = Inbox::new(false);
        let q = ib.ask("DB?", "Which?", &OPTS, Some("sqlite"), None).unwrap();
        ib.accept_default(q["id"].as_str().unwrap()).unwrap();
        let a = ib.assumptions().unwrap().remove(0);
        let out = ib.overrule(a["id"].as_str().unwrap(), "mongo").unwrap();
        assert_eq!((out["status"].clone(), out["assumed"].clone(), out["overruled_to"].clone()), (json!("overruled"), json!("sqlite"), json!("mongo")));
    });
}

#[test]
fn an_item_cannot_be_resolved_twice_or_answered_when_unknown() {
    with_db(|| {
        let ib = Inbox::new(false);
        let q = ib.ask("x", "x", &[], Some("a"), None).unwrap();
        let id = q["id"].as_str().unwrap();
        ib.answer(id, "b").unwrap();
        assert!(matches!(ib.answer(id, "c"), Err(InboxError::Conflict(_))));
        assert!(matches!(ib.answer("nope", "c"), Err(InboxError::NotFound(_))));
        let no_default = ib.ask("y", "y", &[], None, None).unwrap();
        assert!(matches!(ib.accept_default(no_default["id"].as_str().unwrap()), Err(InboxError::Invalid(_))));
    });
}

#[test]
fn a_gate_blocks_until_approved_and_is_single_use() {
    with_db(|| {
        let g = request_gate("merge_main", "merge PR 5 into main", Some("r1")).unwrap();
        let id = g["id"].as_str().unwrap();
        assert!(matches!(check_gate("merge_main", Some(id)), Err(GateError::Blocked(_)))); // still open
        assert!(matches!(check_gate("merge_main", None), Err(GateError::Blocked(_)))); // no approval at all
        Inbox::new(false).decide(id, true).unwrap();
        check_gate("merge_main", Some(id)).unwrap(); // passes once
        assert!(matches!(check_gate("merge_main", Some(id)), Err(GateError::Blocked(_)))); // consumed
    });
}

#[test]
fn a_rejected_or_wrong_action_gate_stays_blocked() {
    with_db(|| {
        let g = request_gate("production_config", "change env", None).unwrap();
        Inbox::new(false).decide(g["id"].as_str().unwrap(), false).unwrap();
        assert!(check_gate("production_config", g["id"].as_str()).is_err());
        let g2 = request_gate("credentials", "need token", None).unwrap();
        Inbox::new(false).decide(g2["id"].as_str().unwrap(), true).unwrap();
        assert!(check_gate("merge_main", g2["id"].as_str()).is_err());
    });
}

#[test]
fn an_unknown_gate_action_is_rejected() {
    with_db(|| {
        assert!(matches!(request_gate("make_coffee", "x", None), Err(InboxError::Invalid(_))));
        for a in ["merge_main", "production_config", "credentials", "destructive_git", "delete_data", "srs_approval"] {
            assert!(GATED.iter().any(|(k, _)| *k == a), "{a}");
        }
    });
}

#[test]
fn the_audit_trail_records_requests_decisions_and_blocks() {
    with_db(|| {
        let g = request_gate("merge_main", "merge", Some("r9")).unwrap();
        let id = g["id"].as_str().unwrap();
        assert!(check_gate("merge_main", Some(id)).is_err());
        Inbox::new(false).decide(id, true).unwrap();
        check_gate("merge_main", Some(id)).unwrap();
        let events: Vec<_> = DocRepo::new("audit").unwrap().list().unwrap().iter().map(|e| e["event"].as_str().unwrap().to_string()).collect();
        assert_eq!(events, vec!["gate_requested", "gate_blocked", "gate_approved", "gate_passed"]);
    });
}

#[test]
fn only_approvals_are_approved_and_only_questions_answered() {
    with_db(|| {
        let ib = Inbox::new(false);
        let q = ib.ask("q", "q", &[], None, None).unwrap();
        assert!(matches!(ib.decide(q["id"].as_str().unwrap(), true), Err(InboxError::Invalid(_))));
        let g = request_gate("merge_main", "m", None).unwrap();
        assert!(matches!(ib.answer(g["id"].as_str().unwrap(), "yes"), Err(InboxError::Invalid(_))));
        assert_eq!(ib.list(Some("open")).unwrap().len(), 2);
        assert_eq!(ib.list(Some("resolved")).unwrap().len(), 0);
    });
}

// ---------------------------------------------------------------- scheduler

fn no_run(_: &Value) -> Result<(), String> {
    Ok(())
}

#[test]
fn a_one_shot_job_runs_once_when_due() {
    with_db(|| {
        let s = Scheduler::new(80.0);
        s.add("continue", json!({"x": 1}), 100.0, None).unwrap();
        let mut ran: Vec<Value> = vec![];
        assert!(s.tick(50.0, None, &mut |j| { ran.push(j.clone()); Ok(()) }).unwrap().ran.is_empty());
        let out = s.tick(100.0, None, &mut |j| { ran.push(j.clone()); Ok(()) }).unwrap();
        assert_eq!(out.ran.iter().map(|j| j["name"].clone()).collect::<Vec<_>>(), vec![json!("continue")]);
        assert_eq!(ran[0]["payload"], json!({"x": 1}));
        assert!(s.tick(200.0, None, &mut no_run).unwrap().ran.is_empty());
        assert_eq!(s.list().unwrap()[0]["status"], "done");
    });
}

#[test]
fn a_recurring_job_reschedules_without_catching_up_every_missed_slot() {
    with_db(|| {
        let s = Scheduler::new(80.0);
        s.add("check-in", json!({}), 100.0, Some(60.0)).unwrap();
        let mut n = 0;
        s.tick(100.0, None, &mut |_| { n += 1; Ok(()) }).unwrap();
        s.tick(100.0, None, &mut |_| { n += 1; Ok(()) }).unwrap(); // same instant: not due again
        assert_eq!(n, 1);
        s.tick(1000.0, None, &mut |_| { n += 1; Ok(()) }).unwrap(); // long gap: runs once, not 15 times
        assert_eq!(n, 2);
        let j = &s.list().unwrap()[0];
        assert!(j["run_at"].as_f64().unwrap() > 1000.0 && j["status"] == "scheduled");
    });
}

#[test]
fn a_budget_at_or_above_the_threshold_pauses_jobs_and_notifies_once() {
    with_db(|| {
        let s = Scheduler::new(80.0);
        s.add("continue", json!({}), 0.0, None).unwrap();
        let mut n = 0;
        let out = s.tick(10.0, Some(85.0), &mut |_| { n += 1; Ok(()) }).unwrap();
        assert!(out.paused && n == 0);
        s.tick(11.0, Some(90.0), &mut |_| { n += 1; Ok(()) }).unwrap();
        assert_eq!(kinds(&Notifier::new().unread().unwrap()), vec!["budget_pause"]); // one notice, not one per tick
        let out = s.tick(12.0, Some(40.0), &mut |_| { n += 1; Ok(()) }).unwrap(); // window reset: resumes by itself
        assert!(!out.paused && n == 1);
        assert!(Notifier::new().unread().unwrap().is_empty()); // the stale notice is cleared
    });
}

#[test]
fn a_failing_job_is_recorded_and_does_not_stop_the_others() {
    with_db(|| {
        let s = Scheduler::new(80.0);
        s.add("bad", json!({}), 0.0, None).unwrap();
        s.add("good", json!({}), 0.0, None).unwrap();
        let mut ran = vec![];
        s.tick(1.0, None, &mut |j| {
            if j["name"] == "bad" { return Err("boom".into()); }
            ran.push(j["name"].as_str().unwrap().to_string());
            Ok(())
        }).unwrap();
        let st: HashMap<String, Value> = s.list().unwrap().into_iter().map(|j| (j["name"].as_str().unwrap().to_string(), j)).collect();
        assert_eq!(ran, vec!["good"]);
        assert_eq!(st["bad"]["status"], "failed");
        assert!(st["bad"]["error"].as_str().unwrap().contains("boom"));
    });
}

#[test]
fn budget_percent_used_comes_from_calls_and_the_daily_cap() {
    with_db(|| {
        assert_eq!(budget_percent_used(None, None).unwrap(), None); // no cap configured
        let now = 1_000_000.0;
        let r = DocRepo::new("llm_calls").unwrap();
        r.create(json!({"role": "x", "cost_usd": 2.0, "at": now}), None).unwrap();
        r.create(json!({"role": "x", "cost_usd": null, "at": now}), None).unwrap(); // unknown price ignored
        r.create(json!({"role": "x", "cost_usd": 50.0, "at": now - 3.0 * DAY}), None).unwrap(); // old
        assert_eq!(budget_percent_used(Some(10.0), Some(now)).unwrap(), Some(20.0));
    });
}

// ---------------------------------------------------------------- quota

fn vercel(stop: f64) -> QuotaGuard {
    QuotaGuard::new("vercel_hobby", stop, None).unwrap()
}

#[test]
fn vercel_hobby_limits_are_seeded_from_the_documented_values() {
    let v = limits_for("vercel_hobby").unwrap();
    assert_eq!((v["deployments_per_day"].limit, v["deployments_per_day"].window_s), (100, DAY));
    assert_eq!(v["deployments_per_hour"].limit, 100);
    assert_eq!(v["concurrent_builds"].limit, 1);
}

#[test]
fn quota_allows_until_the_stop_threshold_then_blocks() {
    with_db(|| {
        let g = vercel(70.0);
        for i in 0..69 { g.record("deployments_per_day", Some(1000.0 + i as f64)).unwrap(); }
        g.check("deployments_per_day", Some(2000.0), 0).unwrap(); // 69 of 100: still fine
        g.record("deployments_per_day", Some(1100.0)).unwrap();
        match g.check("deployments_per_day", Some(2000.0), 0) {
            Err(QuotaError::Blocked(m)) => assert!(m.contains("70"), "{m}"),
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn old_events_fall_out_of_the_rolling_window() {
    with_db(|| {
        let g = vercel(70.0);
        for i in 0..80 { g.record("deployments_per_day", Some(100.0 + i as f64)).unwrap(); }
        assert!(g.check("deployments_per_day", Some(500.0), 0).is_err());
        g.check("deployments_per_day", Some(100.0 + 80.0 + DAY), 0).unwrap();
    });
}

#[test]
fn hour_and_day_windows_are_independent() {
    with_db(|| {
        let g = vercel(50.0);
        for i in 0..60 { g.record("deployments_per_hour", Some(10_000.0 + i as f64)).unwrap(); }
        assert!(g.check("deployments_per_hour", Some(10_100.0), 0).is_err());
        g.check("deployments_per_day", Some(10_100.0), 0).unwrap();
    });
}

#[test]
fn one_concurrent_build_blocks_a_second() {
    with_db(|| {
        let g = vercel(70.0);
        g.check("concurrent_builds", Some(1.0), 0).unwrap();
        assert!(g.check("concurrent_builds", Some(1.0), 1).is_err());
    });
}

#[test]
fn blocking_notifies_once_and_names_the_metric() {
    with_db(|| {
        let g = vercel(10.0);
        for i in 0..15 { g.record("deployments_per_day", Some(100.0 + i as f64)).unwrap(); }
        for _ in 0..3 { assert!(g.check("deployments_per_day", Some(200.0), 0).is_err()); }
        let n = Notifier::new().unread().unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0]["kind"], "quota_block");
        assert!(n[0]["body"].as_str().unwrap().contains("deployments_per_day"));
    });
}

#[test]
fn a_platform_limit_error_stops_everything_and_never_retries() {
    with_db(|| {
        let g = vercel(70.0);
        g.limit_hit("api-deployments-free-per-day", Some(500.0)).unwrap();
        for m in ["deployments_per_day", "deployments_per_hour"] {
            match g.check(m, Some(501.0), 0) {
                Err(QuotaError::Blocked(msg)) => assert!(msg.contains("limit error")),
                other => panic!("{other:?}"),
            }
        }
        g.check("deployments_per_day", Some(500.0 + DAY + 1.0), 0).unwrap();
    });
}

#[test]
fn an_unknown_platform_or_metric_is_an_error_not_a_free_pass() {
    with_db(|| {
        assert!(matches!(QuotaGuard::new("nope", 70.0, None), Err(QuotaError::UnknownPlatform(_))));
        assert!(matches!(vercel(70.0).check("made_up", Some(1.0), 0), Err(QuotaError::UnknownMetric(_))));
    });
}

#[test]
fn the_usage_report_shows_used_limit_and_percent() {
    with_db(|| {
        let g = vercel(70.0);
        for i in 0..5 { g.record("deployments_per_day", Some(100.0 + i as f64)).unwrap(); }
        assert_eq!(g.usage(Some(200.0)).unwrap()["deployments_per_day"], Usage { used: 5, limit: 100, percent: 5.0 });
        assert!(!g.usage(Some(200.0)).unwrap().contains_key("concurrent_builds")); // no window: not reported
    });
}

#[test]
fn a_custom_limits_table_can_be_supplied() {
    with_db(|| {
        let mut table: HashMap<String, HashMap<String, Metric>> = HashMap::new();
        table.insert("acme_free".into(), HashMap::from([("builds_per_day".to_string(), Metric { limit: 10, window_s: DAY })]));
        let g = QuotaGuard::new("acme_free", 50.0, Some(table)).unwrap();
        for i in 0..5 { g.record("builds_per_day", Some(100.0 + i as f64)).unwrap(); }
        assert!(g.check("builds_per_day", Some(200.0), 0).is_err());
    });
}
