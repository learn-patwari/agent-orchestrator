//! Phase 6a: budget, profiles, agent registry, BA helpers, features, settings.
//! Ported from backend/tests/{test_budget,test_profiles,test_agent_registry,test_ba_pure,test_features}.py.
use ao_core::agent_registry::{AgentRegistry, TTL};
use ao_core::ba::{build_srs, lanes, parse_json};
use ao_core::budget::Budget;
use ao_core::db::{self, DocRepo};
use ao_core::features::{require_approved, Features};
use ao_core::inbox::Inbox;
use ao_core::profiles::Profiles;
use ao_core::providers::base::{Completion, Message, Provider, ProviderError, ToolSpec, Usage};
use ao_core::settings;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

static ENV: Mutex<()> = Mutex::new(());

fn with_env<R>(f: impl FnOnce() -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    f()
}

// ---------------------------------------------------------------- budget

fn feature() -> String {
    DocRepo::new("features").unwrap().create(json!({"title": "f", "status": "approved"}), None).unwrap()["id"].as_str().unwrap().to_string()
}

fn spend(fid: &str, tokens: i64) {
    DocRepo::new("runs").unwrap().create(json!({"feature_id": fid, "tokens": tokens, "status": "done"}), None).unwrap();
}

fn call(tokens: i64, at: f64) {
    DocRepo::new("llm_calls").unwrap().create(json!({"role": "developer", "input_tokens": tokens, "output_tokens": 0, "at": at}), None).unwrap();
}

#[test]
fn no_limit_never_holds() {
    with_env(|| {
        let fid = feature();
        spend(&fid, 1_000_000_000);
        let b = Budget;
        assert_eq!(b.gate(Some(&fid)).unwrap(), None);
        let s = b.status(&fid).unwrap();
        assert!(s["limit"].is_null() && s["held"] == false);
    });
}

#[test]
fn the_feature_holds_once_its_runs_reach_the_limit() {
    with_env(|| {
        let fid = feature();
        let b = Budget;
        b.set(&fid, Some(1000), None).unwrap();
        spend(&fid, 400);
        assert_eq!(b.gate(Some(&fid)).unwrap(), None);
        let s = b.status(&fid).unwrap();
        assert!(s["used"] == 400 && s["remaining"] == 600);
        spend(&fid, 600);
        assert!(b.gate(Some(&fid)).unwrap().unwrap().contains("token limit"));
        assert_eq!(b.status(&fid).unwrap()["held"], true);
        assert!(b.gate(Some(&fid)).unwrap().is_some()); // stays held on later checks
        let n = ao_core::notify::Notifier::new().unread().unwrap();
        assert_eq!(n.iter().filter(|x| x["key"] == format!("hold:{fid}")).count(), 1); // told the human once
    });
}

#[test]
fn other_features_are_not_affected() {
    with_env(|| {
        let (a, c) = (feature(), feature());
        let b = Budget;
        b.set(&a, Some(100), None).unwrap();
        spend(&a, 500);
        spend(&c, 500);
        assert!(b.gate(Some(&a)).unwrap().is_some() && b.gate(Some(&c)).unwrap().is_none());
    });
}

#[test]
fn resume_gives_a_fresh_allowance_and_clears_the_hold() {
    with_env(|| {
        let fid = feature();
        let b = Budget;
        b.set(&fid, Some(1000), None).unwrap();
        spend(&fid, 1000);
        assert!(b.gate(Some(&fid)).unwrap().is_some());
        b.resume(&fid).unwrap();
        let s = b.status(&fid).unwrap();
        assert!(b.gate(Some(&fid)).unwrap().is_none() && s["used"] == 0 && s["held"] == false);
        // the hold notice is read once the human resumed
        assert!(ao_core::notify::Notifier::new().unread().unwrap().iter().all(|x| x["key"] != format!("hold:{fid}")));
        spend(&fid, 1000);
        assert!(b.gate(Some(&fid)).unwrap().is_some()); // the new window has its own limit
    });
}

#[test]
fn raising_or_removing_the_limit_lifts_the_hold() {
    with_env(|| {
        let fid = feature();
        let b = Budget;
        b.set(&fid, Some(100), None).unwrap();
        spend(&fid, 100);
        b.gate(Some(&fid)).unwrap();
        assert_eq!(b.set(&fid, Some(100), None).unwrap()["held"], true); // same limit: still held
        assert_eq!(b.set(&fid, Some(500), None).unwrap()["held"], false);
    });
}

#[test]
fn tick_resumes_only_after_the_resume_time() {
    with_env(|| {
        let fid = feature();
        let b = Budget;
        b.set(&fid, Some(100), Some(5000.0)).unwrap();
        spend(&fid, 100);
        assert!(b.gate(Some(&fid)).unwrap().is_some());
        assert!(b.tick(4999.0).unwrap().is_empty() && b.status(&fid).unwrap()["held"] == true);
        assert_eq!(b.tick(5000.0).unwrap(), vec![fid.clone()]);
        assert!(b.status(&fid).unwrap()["held"] == false && b.gate(Some(&fid)).unwrap().is_none());
    });
}

#[test]
fn a_hold_without_a_resume_time_waits_for_a_manual_resume() {
    with_env(|| {
        let fid = feature();
        let b = Budget;
        b.set(&fid, Some(100), None).unwrap();
        spend(&fid, 100);
        b.gate(Some(&fid)).unwrap();
        assert!(b.tick(1e12).unwrap().is_empty() && b.status(&fid).unwrap()["held"] == true);
    });
}

#[test]
fn bad_limits_are_refused() {
    with_env(|| {
        let fid = feature();
        for bad in [0, -5] {
            assert!(Budget.set(&fid, Some(bad), None).unwrap_err().to_string().contains("above zero"));
        }
        assert_eq!(Budget.set("nope", Some(100), None).unwrap_err().to_string(), "unknown feature nope");
        Budget.set(&fid, None, None).unwrap(); // removing the limit is allowed
    });
}

#[test]
fn daily_limit_counts_only_the_last_24_hours_and_lifts_by_itself() {
    with_env(|| {
        let b = Budget;
        let now = 1_000_000.0;
        assert!(b.daily(now, None).unwrap()["limit"].is_null() && b.daily_gate(now, None).unwrap().is_none());
        call(800, now - 100.0);
        call(5000, now - 90_000.0); // older than a day: not counted
        let d = b.daily(now, Some(1000)).unwrap();
        assert!(d["used"] == 800 && d["percent"] == 80.0);
        assert!(b.daily_gate(now, Some(1000)).unwrap().is_none());
        call(300, now - 10.0);
        assert!(b.daily_gate(now, Some(1000)).unwrap().unwrap().contains("daily token limit"));
        assert!(b.daily_gate(now + 86_400.0, Some(1000)).unwrap().is_none()); // the window moved on
    });
}

#[test]
fn overview_lists_features_with_their_status() {
    with_env(|| {
        let fid = DocRepo::new("features").unwrap().create(json!({"title": "Pay", "status": "approved"}), None).unwrap()["id"].as_str().unwrap().to_string();
        Budget.set(&fid, Some(900), None).unwrap();
        call(1000, 500.0);
        let ov = Budget.overview(500.0, Some(1000)).unwrap();
        assert!(ov["daily"]["used"] == 1000 && ov["daily"]["percent"] == 100.0);
        assert_eq!(ov["features"][0]["title"], "Pay");
        assert_eq!(ov["features"][0]["limit"], 900);
    });
}

#[test]
fn the_usage_window_blocks_at_98_percent_until_the_reset_and_schedules_one_resume() {
    with_env(|| {
        let b = Budget;
        b.set_window(50.0, Some(9000.0)).unwrap();
        assert!(b.window_gate(100.0).unwrap().is_none());
        b.set_window(99.0, Some(9000.0)).unwrap();
        b.set_window(99.5, Some(9000.0)).unwrap(); // updating the figure does not add a second reminder
        let msg = b.window_gate(100.0).unwrap().unwrap();
        assert!(msg.starts_with("usage window at 99.5% (stop at 98%); work resumes after the reset at "), "{msg}");
        assert!(b.window_gate(9000.0).unwrap().is_none()); // after the reset it lifts by itself
        let w = b.window(100.0).unwrap();
        assert!(w["percent"] == 99.5 && w["blocked"] == true);
        assert_eq!(ao_core::scheduler::Scheduler::new(80.0).list().unwrap().len(), 1);
    });
}

// ---------------------------------------------------------------- profiles

fn good() -> Value {
    json!({"max_steps": 5, "max_total_tokens": 1000, "max_output_tokens": 256, "compact_at_chars": 1000, "keep_recent": 2})
}

#[test]
fn three_profiles_ship_and_balanced_is_active_by_default() {
    with_env(|| {
        let p = Profiles;
        let names: Vec<String> = p.list().unwrap().iter().map(|x| x["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["lean", "balanced", "thorough"]);
        assert_eq!(p.active().unwrap()["name"], "balanced");
        let t = |n: &str| p.get(n).unwrap()["max_total_tokens"].as_i64().unwrap();
        assert!(t("lean") < t("balanced") && t("balanced") < t("thorough"));
        assert_eq!(p.get("balanced").unwrap()["builtin"], true);
    });
}

#[test]
fn switching_the_active_profile_changes_the_limits_runs_get() {
    with_env(|| {
        let p = Profiles;
        p.set_active("lean").unwrap();
        let lim = p.limits().unwrap();
        let lean = p.get("lean").unwrap();
        assert!(lim.max_steps == lean["max_steps"] && lim.compact_at_chars as i64 == lean["compact_at_chars"].as_i64().unwrap());
        assert_eq!(p.set_active("nope").unwrap_err().to_string(), "unknown profile nope");
    });
}

#[test]
fn a_custom_profile_can_be_added_edited_activated_and_deleted() {
    with_env(|| {
        let p = Profiles;
        let tight = json!({"max_steps": 8, "max_total_tokens": 50000, "max_output_tokens": 512, "compact_at_chars": 10000, "keep_recent": 3});
        p.save("tight", &tight).unwrap();
        let g = p.get("tight").unwrap();
        assert!(g["builtin"] == false && g["est_run_tokens"] == 50000);
        let mut t2 = tight.clone();
        t2["max_steps"] = json!(9);
        p.save("tight", &t2).unwrap();
        assert_eq!(p.get("tight").unwrap()["max_steps"], 9);
        p.set_active("tight").unwrap();
        assert_eq!(p.limits().unwrap().max_output_tokens, 512);
        p.delete("tight").unwrap();
        assert_eq!(p.active().unwrap()["name"], "balanced"); // deleting the active one falls back
    });
}

#[test]
fn built_in_profiles_cannot_be_changed_or_deleted_and_bad_values_are_refused() {
    with_env(|| {
        let p = Profiles;
        for n in ["lean", "balanced", "thorough"] {
            assert!(p.save(n, &good()).unwrap_err().to_string().contains("built-in"));
            assert!(p.delete(n).unwrap_err().to_string().contains("built-in"));
        }
        let mut zero = good();
        zero["max_steps"] = json!(0);
        assert!(p.save("x", &zero).is_err());
        let mut extra = good();
        extra["surprise"] = json!(1);
        assert!(p.save("x", &extra).is_err());
        let mut missing = good();
        missing.as_object_mut().unwrap().remove("keep_recent");
        assert!(p.save("x", &missing).is_err());
        let mut boolean = good();
        boolean["keep_recent"] = json!(true);
        assert!(p.save("x", &boolean).is_err());
        assert_eq!(p.save("  ", &good()).unwrap_err().to_string(), "a profile needs a name");
        assert_eq!(p.delete("ghost").unwrap_err().to_string(), "unknown profile ghost");
    });
}

// ---------------------------------------------------------------- agent registry

fn join(name: &str) -> Value {
    AgentRegistry.register(name, "https://agents.example/scout", &["code search", "web research"], "finds things", None).unwrap()
}

#[test]
fn a_new_agent_is_pending_gets_a_token_once_and_is_never_live_until_approved() {
    with_env(|| {
        let out = join("scout");
        assert!(out["agent"]["status"] == "pending" && out["token"].is_string() && out["agent"].get("token").is_none());
        let r = AgentRegistry;
        r.heartbeat("scout", out["token"].as_str().unwrap(), Some(100.0)).unwrap();
        let row = r.list(Some(101.0)).unwrap()[0].clone();
        assert!(row["name"] == "scout" && row["live"] == false && row["capabilities"] == json!(["code search", "web research"]));
        assert!(row.get("token_hash").is_none());
        r.approve("scout").unwrap();
        assert_eq!(r.list(Some(101.0)).unwrap()[0]["live"], true);
    });
}

#[test]
fn no_heartbeat_means_the_agent_leaves_the_live_list() {
    with_env(|| {
        let t = join("scout")["token"].as_str().unwrap().to_string();
        let r = AgentRegistry;
        r.approve("scout").unwrap();
        r.heartbeat("scout", &t, Some(1000.0)).unwrap();
        assert_eq!(r.live(Some(1000.0 + TTL)).unwrap().len(), 1);
        assert!(r.live(Some(1000.0 + TTL + 1.0)).unwrap().is_empty());
        r.heartbeat("scout", &t, Some(2000.0)).unwrap();
        assert!(!r.live(Some(2001.0)).unwrap().is_empty());
    });
}

#[test]
fn heartbeat_and_reregistration_need_the_agent_token() {
    with_env(|| {
        let t = join("scout")["token"].as_str().unwrap().to_string();
        let r = AgentRegistry;
        assert!(r.heartbeat("scout", "wrong", None).unwrap_err().to_string().contains("token"));
        assert!(r.heartbeat("ghost", &t, None).is_err());
        let bad = r.register("scout", "https://evil.example/x", &["everything"], "", Some("wrong")).unwrap_err();
        assert!(bad.to_string().contains("token"));
        let again = r.register("scout", "https://agents.example/v2", &["code search"], "v2", Some(&t)).unwrap();
        assert!(again["agent"]["endpoint"].as_str().unwrap().ends_with("/v2") && again["token"].is_null());
    });
}

#[test]
fn bad_registrations_are_refused() {
    with_env(|| {
        for (n, e) in [(" ", "https://x.example"), ("a", "ftp://x.example"), ("a", "file:///etc/passwd"), ("a", "not a url")] {
            assert!(AgentRegistry.register(n, e, &[], "", None).is_err(), "{n} {e}");
        }
    });
}

#[test]
fn a_rejected_agent_is_listed_as_rejected() {
    with_env(|| {
        join("scout");
        AgentRegistry.reject("scout").unwrap();
        assert_eq!(AgentRegistry.list(None).unwrap()[0]["status"], "rejected");
    });
}

#[test]
fn sending_a_task_needs_approval_a_live_agent_and_the_external_flag() {
    with_env(|| {
        let t = join("scout")["token"].as_str().unwrap().to_string();
        let sent = Mutex::new(vec![]);
        let post = |url: &str, payload: &Value| -> Result<String, String> {
            sent.lock().unwrap().push((url.to_string(), payload.clone()));
            Ok("found 3 files".into())
        };
        let r = AgentRegistry;
        assert!(r.send_task("scout", "look", &post, Some(10.0)).unwrap_err().to_string().contains("approved"));
        r.approve("scout").unwrap();
        r.heartbeat("scout", &t, Some(10.0)).unwrap();
        assert!(r.send_task("scout", "look", &post, Some(10.0)).unwrap_err().to_string().contains("external"));
        db::kv_set("settings", &json!({"feature_flags": {"external_providers": true}})).unwrap();
        assert_eq!(r.send_task("scout", "look", &post, Some(10.0)).unwrap()["reply"], "found 3 files");
        assert_eq!(sent.lock().unwrap()[0], ("https://agents.example/scout".to_string(), json!({"task": "look"})));
        assert!(r.send_task("scout", "look", &post, Some(10.0 + TTL + 5.0)).unwrap_err().to_string().contains("live"));
    });
}

#[test]
fn what_an_external_agent_replies_is_redacted_and_cut() {
    with_env(|| {
        let t = join("scout")["token"].as_str().unwrap().to_string();
        AgentRegistry.approve("scout").unwrap();
        AgentRegistry.heartbeat("scout", &t, Some(1.0)).unwrap();
        db::kv_set("settings", &json!({"feature_flags": {"external_providers": true}})).unwrap();
        let post = |_: &str, _: &Value| -> Result<String, String> { Ok(format!("key sk-abcdefghijklmnopqrstuvwxyz1234 {}", "x".repeat(5000))) };
        let out = AgentRegistry.send_task("scout", "t", &post, Some(1.0)).unwrap();
        let reply = out["reply"].as_str().unwrap();
        assert!(!reply.contains("sk-abcdef") && reply.chars().count() <= 4000);
    });
}

#[test]
fn the_registry_key_is_stable_and_checked_in_constant_time() {
    with_env(|| {
        let k = ao_core::agent_registry::registry_key().unwrap();
        assert_eq!(k, ao_core::agent_registry::registry_key().unwrap());
        assert!(ao_core::agent_registry::key_ok(Some(&k)) && !ao_core::agent_registry::key_ok(Some("nope")) && !ao_core::agent_registry::key_ok(None));
    });
}

// ---------------------------------------------------------------- BA helpers

#[test]
fn parse_json_handles_fences_and_prose() {
    assert_eq!(parse_json("```json\n{\"a\": 1}\n```").unwrap(), json!({"a": 1}));
    assert_eq!(parse_json("Sure! Here it is: {\"a\": [1, 2]} hope that helps").unwrap(), json!({"a": [1, 2]}));
    for bad in ["no json here", "{\"a\": ", ""] {
        assert_eq!(parse_json(bad).unwrap_err().to_string(), "the model did not return valid JSON");
    }
}

fn t(id: &str, files: &[&str], deps: &[&str]) -> Value {
    json!({"id": id, "title": id, "files": files, "depends_on": deps})
}

fn tasks_of(out: &[Value]) -> Vec<Value> {
    out.iter().flat_map(|p| p["tasks"].as_array().unwrap().clone()).collect()
}

fn by_id(out: &[Value], id: &str) -> Value {
    tasks_of(out).into_iter().find(|x| x["id"] == id).unwrap()
}

#[test]
fn independent_tasks_get_separate_lanes() {
    let out = lanes(&[json!({"id": "P1", "name": "x", "tasks": [t("a", &["f1"], &[]), t("b", &["f2"], &[]), t("c", &["f3"], &[])]})]).unwrap();
    let all = tasks_of(&out);
    let lane_set: std::collections::HashSet<String> = all.iter().map(|x| x["lane"].as_str().unwrap().to_string()).collect();
    assert_eq!(lane_set.len(), 3);
    assert!(all.iter().all(|x| x["stacked_on"].is_null()));
}

#[test]
fn tasks_sharing_a_file_share_a_lane_and_stack_in_order() {
    let out = lanes(&[json!({"id": "P1", "name": "x", "tasks": [t("a", &["server.py"], &[]), t("b", &["server.py", "x"], &[]), t("c", &["y"], &[])]})]).unwrap();
    let (a, b, c) = (by_id(&out, "a"), by_id(&out, "b"), by_id(&out, "c"));
    assert!(a["lane"] == b["lane"] && a["lane"] != c["lane"]);
    assert!(a["stacked_on"].is_null() && b["stacked_on"] == "a");
    assert_eq!(a["lane"], "L1"); // lanes are named in plan order
}

#[test]
fn shared_file_is_transitive_across_tasks() {
    let out = lanes(&[json!({"id": "P1", "name": "x", "tasks": [t("a", &["f1"], &[]), t("b", &["f2"], &[]), t("c", &["f1", "f2"], &[])]})]).unwrap();
    let lane_set: std::collections::HashSet<String> = tasks_of(&out).iter().map(|x| x["lane"].as_str().unwrap().to_string()).collect();
    assert_eq!(lane_set.len(), 1); // c bridges a and b
}

#[test]
fn dependencies_force_later_wave_and_bad_plans_are_rejected() {
    let out = lanes(&[json!({"id": "P1", "name": "x", "tasks": [t("a", &["f1"], &[]), t("b", &["f2"], &["a"])]})]).unwrap();
    assert!(by_id(&out, "a")["wave"] == 0 && by_id(&out, "b")["wave"] == 1);
    let cyc = lanes(&[json!({"id": "P", "name": "x", "tasks": [t("a", &[], &["b"]), t("b", &[], &["a"])]})]).unwrap_err().to_string();
    assert!(cyc.contains("dependency cycle through"), "{cyc}");
    let unk = lanes(&[json!({"id": "P", "name": "x", "tasks": [t("a", &[], &["zzz"])]})]).unwrap_err().to_string();
    assert_eq!(unk, "task a depends on unknown task zzz");
    let dup = lanes(&[json!({"id": "P", "name": "x", "tasks": [t("a", &[], &[]), t("a", &[], &[])]})]).unwrap_err().to_string();
    assert_eq!(dup, "duplicate task ids in plan");
}

fn feature_doc() -> Value {
    json!({
        "title": "Tax report", "text": "Add a tax report page",
        "analysis": {"restatement": "A page that lists taxes.", "why": "Finance needs it.",
                     "impact": {"modules": ["reports"], "data": ["invoices"], "security": ["role checks"], "risks": ["slow query"]}},
        "qa": [{"question": "DB?", "answer": "sqlite", "by": "human"},
               {"question": "Format?", "answer": "csv", "by": "accepted_default"},
               {"question": "Locale?", "answer": "en", "by": "auto_default"}],
        "plan": [{"id": "P1", "name": "Backend", "tasks": [t("a", &["api.py"], &[])]}],
    })
}

#[test]
fn srs_follows_the_template_with_decisions_assumptions_and_signoff() {
    let md = build_srs(&feature_doc());
    for h in ["# SRS: Tax report", "## 1. Purpose and background", "## 2. Decisions and defaults", "## 10. Development plan", "## 11. Sign-off"] {
        assert!(md.contains(h), "{h}");
    }
    assert!(md.contains("| D-1 | DB?: sqlite |"));
    assert!(md.contains("| A-1 | Format?: csv **[A]** (accepted default) |") && md.contains("| A-2 | Locale?: en **[A]** (auto default) |"));
    assert!(md.contains("Status: Draft") && md.contains("slow query") && md.contains("api.py") && md.contains("Approved by"));
    assert!(md.contains("Modules: reports\nData: invoices\nSecurity: role checks\n"));
    assert!(md.contains("| a: a | api.py | L1 |") || md.contains("| a: a | api.py |  |"));
}

#[test]
fn srs_with_no_questions_is_still_valid() {
    let mut f = feature_doc();
    f["qa"] = json!([]);
    let md = build_srs(&f);
    assert!(!md.contains("| D-1") && md.contains("No open decisions"));
    f["analysis"]["impact"] = json!({});
    let md = build_srs(&f);
    assert!(md.contains("Modules: none identified") && md.contains("| none identified | | |"));
}

// ---------------------------------------------------------------- features

const ANALYSIS: &str = r#"{"restatement": "A tax report page.", "why": "Finance needs it.",
 "impact": {"modules": ["reports"], "data": ["invoices"], "security": ["role checks"], "risks": ["slow query"]},
 "questions": [
  {"title": "DB?", "body": "Which database?", "options": ["sqlite", "mongo"], "default": "sqlite"},
  {"title": "Format?", "body": "Export format?", "options": ["csv", "pdf"], "default": "csv"}]}"#;
const PLAN: &str = r#"{"phases": [{"id": "P1", "name": "Backend", "tasks": [
  {"id": "t1", "title": "API", "files": ["api.py"], "depends_on": []},
  {"id": "t2", "title": "UI", "files": ["ui.tsx"], "depends_on": ["t1"]}]}]}"#;

#[derive(Clone)]
struct Fake {
    texts: Arc<Mutex<Vec<String>>>,
    seen: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl Fake {
    fn new(texts: &[&str]) -> Self {
        Fake { texts: Arc::new(Mutex::new(texts.iter().rev().map(|s| s.to_string()).collect())), seen: Default::default() }
    }
}

impl Provider for Fake {
    fn complete(&self, _m: &str, messages: &[Message], _t: Option<&[ToolSpec]>, _max: u32, _temp: Option<f64>) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(messages.to_vec());
        let text = self.texts.lock().unwrap().pop().expect("script ran out");
        Ok(Completion { text, tool_calls: vec![], usage: Usage { input_tokens: 5, output_tokens: 5 }, stop_reason: String::new(), model: "m".into() })
    }
    fn stream(&self, _m: &str, _msgs: &[Message], _t: Option<&[ToolSpec]>, _max: u32, _temp: Option<f64>) -> Result<Vec<String>, ProviderError> {
        Ok(vec![])
    }
}

fn features(f: &Fake, auto: bool) -> Features {
    Features::new(Box::new(f.clone()), "m", "", auto)
}

fn open_items() -> Vec<Value> {
    Inbox::new(false).list(Some("open")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

#[test]
fn intake_returns_the_analysis_and_asks_exactly_one_question() {
    with_env(|| {
        let f = features(&Fake::new(&[ANALYSIS]), false).intake("Tax report", "Add a tax report page").unwrap();
        assert!(f["status"] == "questioning" && f["analysis"]["restatement"] == "A tax report page.");
        let items = open_items();
        assert!(items.len() == 1 && items[0]["title"] == "DB?" && items[0]["default"] == "sqlite");
        assert_eq!(items[0]["feature_id"], f["id"]);
        assert_eq!(DocRepo::new("llm_calls").unwrap().list().unwrap().len(), 1); // the call was recorded
    });
}

#[test]
fn questions_come_one_at_a_time_and_answers_are_recorded() {
    with_env(|| {
        let ft = features(&Fake::new(&[ANALYSIS]), false);
        ft.intake("Tax report", "x").unwrap();
        let inbox = Inbox::new(false);
        let q1 = open_items()[0].clone();
        inbox.answer(s(&q1["id"]), "mongo").unwrap();
        ft.sync(&inbox.items.get(s(&q1["id"])).unwrap().unwrap()).unwrap();
        assert_eq!(open_items().iter().map(|i| s(&i["title"]).to_string()).collect::<Vec<_>>(), ["Format?"]); // next question only now
        inbox.accept_default(s(&open_items()[0]["id"])).unwrap();
        let last = inbox.items.list().unwrap().pop().unwrap();
        let f = ft.sync(&last).unwrap().unwrap();
        assert!(f["status"] == "ready" && open_items().is_empty());
        let qa: Vec<(String, String, String)> = f["qa"].as_array().unwrap().iter().map(|q| (s(&q["question"]).into(), s(&q["answer"]).into(), s(&q["by"]).into())).collect();
        assert_eq!(qa, [("DB?".into(), "mongo".into(), "human".into()), ("Format?".into(), "csv".into(), "accepted_default".into())]);
    });
}

#[test]
fn decide_with_defaults_runs_through_all_questions() {
    with_env(|| {
        let f = features(&Fake::new(&[ANALYSIS]), true).intake("Tax report", "x").unwrap();
        assert!(f["status"] == "ready" && open_items().is_empty());
        let by: Vec<&str> = f["qa"].as_array().unwrap().iter().map(|q| s(&q["by"])).collect();
        assert_eq!(by, ["auto_default", "auto_default"]);
        assert_eq!(Inbox::new(false).assumptions().unwrap().len(), 2);
    });
}

#[test]
fn finalize_builds_the_srs_with_lanes_and_requests_approval() {
    with_env(|| {
        let ft = features(&Fake::new(&[ANALYSIS, PLAN]), true);
        let f = ft.intake("Tax report", "x").unwrap();
        let f = ft.finalize(s(&f["id"])).unwrap();
        assert!(f["status"] == "srs_draft" && s(&f["srs_md"]).contains("# SRS: Tax report") && s(&f["srs_md"]).contains("[A]"));
        let tasks = f["plan"][0]["tasks"].as_array().unwrap();
        assert!(tasks[0]["wave"] == 0 && tasks[1]["wave"] == 1);
        let gates: Vec<Value> = open_items().into_iter().filter(|i| i["kind"] == "approval").collect();
        assert!(gates.len() == 1 && gates[0]["action"] == "srs_approval" && gates[0]["feature_id"] == f["id"]);
        assert_eq!(DocRepo::new("srs_versions").unwrap().list().unwrap().len(), 1);
    });
}

#[test]
fn finalize_before_the_questions_are_done_is_refused() {
    with_env(|| {
        let ft = features(&Fake::new(&[ANALYSIS]), false);
        let f = ft.intake("Tax report", "x").unwrap();
        assert!(ft.finalize(s(&f["id"])).unwrap_err().to_string().contains("questions"));
    });
}

#[test]
fn development_is_blocked_until_the_srs_is_approved_by_the_human() {
    with_env(|| {
        let ft = features(&Fake::new(&[ANALYSIS, PLAN]), true);
        let f = ft.intake("Tax report", "x").unwrap();
        let id = s(&f["id"]).to_string();
        ft.finalize(&id).unwrap();
        let blocked = require_approved(Some(&id)).unwrap_err().to_string();
        assert_eq!(blocked, "development needs a feature whose SRS the human approved");
        let inbox = Inbox::new(false);
        let gate = open_items().into_iter().find(|i| i["kind"] == "approval").unwrap();
        inbox.decide(s(&gate["id"]), true).unwrap();
        let f = ft.sync(&inbox.items.get(s(&gate["id"])).unwrap().unwrap()).unwrap().unwrap();
        assert_eq!(f["status"], "approved");
        require_approved(Some(&id)).unwrap();
        assert!(require_approved(Some("unknown-feature")).is_err() && require_approved(None).is_err());
    });
}

#[test]
fn rejecting_the_srs_keeps_development_blocked() {
    with_env(|| {
        let ft = features(&Fake::new(&[ANALYSIS, PLAN]), true);
        let f = ft.intake("T", "x").unwrap();
        ft.finalize(s(&f["id"])).unwrap();
        let inbox = Inbox::new(false);
        let gate = open_items().into_iter().find(|i| i["kind"] == "approval").unwrap();
        inbox.decide(s(&gate["id"]), false).unwrap();
        assert_eq!(ft.sync(&inbox.items.get(s(&gate["id"])).unwrap().unwrap()).unwrap().unwrap()["status"], "rejected");
        assert!(require_approved(Some(s(&f["id"]))).is_err());
    });
}

#[test]
fn decide_with_defaults_never_approves_the_srs() {
    with_env(|| {
        let ft = features(&Fake::new(&[ANALYSIS, PLAN]), true);
        let f = ft.intake("T", "x").unwrap();
        assert_eq!(ft.finalize(s(&f["id"])).unwrap()["status"], "srs_draft"); // still waiting for a human
    });
}

#[test]
fn bad_model_output_is_reported_not_stored() {
    with_env(|| {
        assert_eq!(features(&Fake::new(&["sorry, no json"]), false).intake("T", "x").unwrap_err().to_string(), "the model did not return valid JSON");
        assert!(DocRepo::new("features").unwrap().list().unwrap().is_empty());
        let e = features(&Fake::new(&[r#"{"restatement": "r"}"#]), false).intake("T", "x").unwrap_err().to_string();
        assert_eq!(e, "analysis is missing 'why'"); // missing required keys
    });
}

#[test]
fn at_most_five_questions_and_defaults_must_be_valid() {
    with_env(|| {
        let many: Vec<Value> = (0..9).map(|i| json!({"title": format!("Q{i}"), "body": "b", "options": ["a", "b"], "default": "a"})).collect();
        let a = json!({"restatement": "r", "why": "w", "impact": {}, "questions": many}).to_string();
        let f = features(&Fake::new(&[&a]), false).intake("T", "x").unwrap();
        assert_eq!(f["analysis"]["questions"].as_array().unwrap().len(), 5);
        let a = json!({"restatement": "r", "why": "w", "impact": {}, "questions": [{"title": "Q", "body": "b", "options": ["a", "b"], "default": "zzz"}]}).to_string();
        let f = features(&Fake::new(&[&a]), false).intake("T", "x").unwrap();
        assert_eq!(f["analysis"]["questions"][0]["default"], "a"); // an invalid default falls back to the first option
    });
}

#[test]
fn a_pasted_secret_in_the_request_is_not_sent_to_the_model() {
    with_env(|| {
        let fk = Fake::new(&[ANALYSIS]);
        features(&fk, false).intake("T", "use key sk-abcdefghijklmnopqrstuvwxyz1234 please").unwrap();
        assert!(fk.seen.lock().unwrap().iter().flatten().all(|m| !m.content.contains("sk-abcdef")));
    });
}

#[test]
fn sync_is_idempotent_and_never_marks_ready_while_a_question_is_open() {
    with_env(|| {
        let ft = features(&Fake::new(&[ANALYSIS]), false);
        ft.intake("T", "x").unwrap();
        let inbox = Inbox::new(false);
        let q1 = open_items()[0].clone();
        inbox.answer(s(&q1["id"]), "mongo").unwrap();
        let item = inbox.items.get(s(&q1["id"])).unwrap().unwrap();
        ft.sync(&item).unwrap();
        let f = ft.sync(&item).unwrap().unwrap(); // duplicate delivery (retry, double click)
        assert_eq!(f["status"], "questioning");
        assert_eq!(open_items().iter().map(|i| s(&i["title"]).to_string()).collect::<Vec<_>>(), ["Format?"]);
        assert_eq!(f["qa"].as_array().unwrap().len(), 1);
    });
}

// ---------------------------------------------------------------- settings

#[test]
fn settings_merge_stored_values_over_the_defaults_and_flags_key_by_key() {
    with_env(|| {
        let d = settings::get_settings().unwrap();
        assert!(d["feature_flags"]["external_providers"] == false && d["review_round_limit"] == 3 && d["main_branch"] == "main");
        assert!(!settings::external_enabled().unwrap());
        db::kv_set("settings", &json!({"review_round_limit": 5, "feature_flags": {"external_providers": true, "x": 1}})).unwrap();
        let s2 = settings::get_settings().unwrap();
        assert!(s2["review_round_limit"] == 5 && s2["integration_branch"] == "internal" && s2["feature_flags"]["x"] == 1);
        assert!(settings::external_enabled().unwrap());
    });
}
