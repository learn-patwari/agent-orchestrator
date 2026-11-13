//! Phase 5b: release stages, the docs gate, the Vercel limit guard and the agent `pipeline` tool.
//! Ported from backend/tests/test_release.py, test_pipeline_tool.py and the guards in test_writer_and_guards.py.
use ao_core::db::{self, DocRepo};
use ao_core::inbox::Inbox;
use ao_core::pipeline::agent_tool::PipelineTool;
use ao_core::pipeline::docs_gate::{check_docs, DocsGate};
use ao_core::pipeline::flow::{tasks, Flow, FlowError};
use ao_core::pipeline::release::Release;
use ao_core::pipeline::worktrees::WorktreeManager;
use ao_core::quota::QuotaGuard;
use ao_core::runtime::tools::{ToolBox, Workspace};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn h1() -> String {
    "1".repeat(40)
}

fn git(cwd: &Path, args: &[&str]) {
    assert!(Command::new("git").args(args).current_dir(cwd).output().unwrap().status.success(), "git {args:?}");
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_string()
}

struct F {
    flow: Flow,
    feature: String,
}

/// A fresh database and an "internal" repo; the flow starts tasks from an approved feature.
fn with_flow<R>(f: impl FnOnce(&F, &Path) -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (data, base) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "internal"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("a"), "a").unwrap();
    git(&repo, &["add", "a"]);
    git(&repo, &["commit", "-q", "-m", "i"]);
    let feature = id(&DocRepo::new("features").unwrap().create(json!({"title": "f", "status": "approved"}), None).unwrap());
    let flow = Flow::new(WorktreeManager::new(&repo, &base.path().join("wt")).unwrap(), "internal", 2);
    f(&F { flow, feature }, base.path())
}

fn with_db<R>(f: impl FnOnce(&Path) -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (data, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    f(work.path())
}

fn review(verdict: &str, head: &str) -> String {
    format!("Verdict: {verdict}\nHead: {head}\n")
}

fn handoffs() -> Vec<(String, String)> {
    let mut h = DocRepo::new("handoffs").unwrap().list().unwrap();
    h.sort_by(|a, b| a["at"].as_f64().partial_cmp(&b["at"].as_f64()).unwrap());
    h.iter().map(|x| (x["from"].as_str().unwrap().into(), x["to"].as_str().unwrap().into())).collect()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.into(), b.into())
}

fn approved_task(f: &F, owner: &str, name: &str) -> String {
    let t = id(&f.flow.start(&f.feature, name, None, owner).unwrap());
    f.flow.dev_done(&t, 5, &h1()).unwrap();
    f.flow.record_tests(&t, 4, 0, "x").unwrap();
    f.flow.record_review(&t, &review("APPROVE", &h1())).unwrap();
    t
}

fn pass_docs(tid: &str) {
    tasks().unwrap().update(tid, json!({"docs_gate": {"status": "PASS", "failed": []}})).unwrap();
}

fn msg(r: Result<Value, FlowError>) -> String {
    r.unwrap_err().to_string()
}

// ---------------------------------------------------------------- release stages

#[test]
fn full_release_to_production_needs_architect_qc_the_human_and_the_architect() {
    with_flow(|f, _| {
        let t = approved_task(f, "developer_1", "t1");
        let r = Release;
        assert_eq!(r.architect_review(&t, true).unwrap()["release"], "architect_approved");
        assert_eq!(r.deploy_qc(&t).unwrap()["release"], "in_qc");
        let env = r.environments().unwrap();
        assert!(env["qc"]["task_id"] == json!(t) && env["prod"].is_null());
        pass_docs(&t); // cicd ran the docs check
        assert_eq!(r.qc_result(&t, 12, 0).unwrap()["release"], "qc_passed");
        assert_eq!(r.request_prod(&t).unwrap()["release"], "awaiting_prod_approval");
        let kinds: Vec<_> = Inbox::new(false).list(Some("open")).unwrap().iter().map(|i| i["kind"].clone()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == json!("approval")).count(), 1); // the human is asked once
        assert_eq!(r.approve_prod(&t, "human").unwrap()["release"], "awaiting_prod_approval"); // still needs the architect
        assert_eq!(r.approve_prod(&t, "architect").unwrap()["release"], "approved_for_prod");
        assert!(r.environments().unwrap()["prod"].is_null()); // approved is not deployed
        assert_eq!(r.deployed_prod(&t).unwrap()["release"], "in_prod");
        assert_eq!(r.environments().unwrap()["prod"]["task_id"], json!(t));
        assert_eq!(r.environments().unwrap()["prod"]["version"], "1111111");
        let tail: Vec<_> = handoffs()[2..].to_vec();
        assert_eq!(tail, [pair("architect", "cicd"), pair("cicd", "tester"), pair("tester", "orchestrator"), pair("orchestrator", "architect"), pair("architect", "cicd")]);
    });
}

#[test]
fn the_other_developer_reviews_and_the_hand_offs_are_recorded() {
    with_flow(|f, _| {
        let t = f.flow.start(&f.feature, "t1", None, "developer_2").unwrap();
        assert_eq!((t["owner"].clone(), t["reviewer"].clone()), (json!("developer_2"), json!("developer_1")));
        approved_task(f, "developer_1", "t2");
        let h = handoffs();
        assert_eq!(h, [pair("developer_1", "developer_2"), pair("developer_2", "architect")]);
    });
}

#[test]
fn stages_cannot_be_skipped() {
    with_flow(|f, _| {
        let t = id(&f.flow.start(&f.feature, "t1", None, "developer_1").unwrap());
        let r = Release;
        assert!(r.architect_review(&t, true).is_err()); // not peer approved yet
        let t = approved_task(f, "developer_1", "t2");
        assert!(r.deploy_qc(&t).is_err());
        assert!(r.qc_result(&t, 1, 0).is_err());
        assert!(r.request_prod(&t).is_err());
        assert!(r.approve_prod(&t, "human").is_err());
        assert!(r.deployed_prod(&t).is_err());
        assert!(msg(r.deploy_qc(&t)).contains("release stage is None, expected architect_approved"));
    });
}

#[test]
fn only_the_human_and_the_architect_can_approve_production() {
    with_flow(|f, _| {
        let t = approved_task(f, "developer_1", "t1");
        let r = Release;
        r.architect_review(&t, true).unwrap();
        r.deploy_qc(&t).unwrap();
        pass_docs(&t);
        r.qc_result(&t, 3, 0).unwrap();
        r.request_prod(&t).unwrap();
        assert!(msg(r.approve_prod(&t, "orchestrator")).contains("human or the architect"));
        for who in ["developer_1", "tester", "technical_writer", "cicd"] {
            assert!(r.approve_prod(&t, who).is_err(), "{who}");
        }
    });
}

#[test]
fn failed_qc_sends_the_work_back_to_the_owning_developer_and_the_docs_gate_never_blocks_it() {
    with_flow(|f, _| {
        let t = approved_task(f, "developer_2", "t1");
        let r = Release;
        r.architect_review(&t, true).unwrap();
        r.deploy_qc(&t).unwrap();
        let out = r.qc_result(&t, 9, 2).unwrap(); // no docs gate has run, and none is needed to fail
        assert_eq!((out["status"].clone(), out["release"].clone()), (json!("developing"), Value::Null));
        assert_eq!(handoffs().last().unwrap(), &pair("tester", "developer_2"));
    });
}

#[test]
fn the_architect_can_send_code_back() {
    with_flow(|f, _| {
        let t = approved_task(f, "developer_1", "t1");
        let out = Release.architect_review(&t, false).unwrap();
        assert_eq!((out["status"].clone(), out["release"].clone()), (json!("developing"), Value::Null));
        assert_eq!(handoffs().last().unwrap(), &pair("architect", "developer_1"));
    });
}

// ---------------------------------------------------------------- Vercel limit guard

fn in_stage(work: &Path, release: &str, name: &str, head: &str) -> String {
    id(&tasks().unwrap().create(
        json!({"name": name, "status": "approved", "release": release, "owner": "developer_1", "reviewer": "developer_2", "worktree": work.to_string_lossy(), "head_sha": head}),
        None,
    ).unwrap())
}

#[test]
fn deploys_stop_at_70_percent_of_the_daily_vercel_limit_and_are_counted() {
    with_db(|work| {
        let g = QuotaGuard::new("vercel_hobby", 70.0, None).unwrap();
        for _ in 0..69 {
            g.record("deployments_per_day", None).unwrap();
        }
        let tid = in_stage(work, "architect_approved", "t", "abcdef1234");
        assert_eq!(Release.deploy_qc(&tid).unwrap()["release"], "in_qc"); // the 70th deployment is allowed and counted
        assert_eq!(g.usage(None).unwrap()["deployments_per_day"].used, 70);
        let t2 = in_stage(work, "architect_approved", "t2", "abc1234");
        assert!(msg(Release.deploy_qc(&t2)).contains("70%")); // ci-cd stops at 70 and tells the orchestrator
        assert_eq!(tasks().unwrap().get(&t2).unwrap().unwrap()["release"], "architect_approved");
        assert_eq!(g.usage(None).unwrap()["deployments_per_day"].used, 70); // a refused deploy is not counted
        assert!(DocRepo::new("notifications").unwrap().list().unwrap().iter().any(|n| n["kind"] == "quota_block"));
    });
}

#[test]
fn production_deploys_are_counted_too() {
    with_db(|work| {
        let tid = in_stage(work, "approved_for_prod", "p", "abc1234");
        assert_eq!(Release.deployed_prod(&tid).unwrap()["release"], "in_prod");
        assert_eq!(QuotaGuard::new("vercel_hobby", 70.0, None).unwrap().usage(None).unwrap()["deployments_per_day"].used, 1);
    });
}

// ---------------------------------------------------------------- docs gate: the checks

fn repo(root: &Path, version: &str, notes: Option<&str>, changelog: Option<&str>, package: Option<&str>, backend: Option<&str>, extra: &[(&str, &str)]) -> std::path::PathBuf {
    for d in ["docs/releases", "docs/user-guides", "docs/api", "frontend", "backend"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(root.join("frontend/package.json"), json!({"version": package.unwrap_or(version)}).to_string()).unwrap();
    std::fs::write(root.join("backend/VERSION"), format!("{}\n", backend.unwrap_or(version))).unwrap();
    let default_notes = format!("# v{version}\n\n## What is new\n- Booking\n\n## Needs your attention\n- None\n");
    std::fs::write(root.join(format!("docs/releases/v{version}.md")), notes.unwrap_or(&default_notes)).unwrap();
    let default_log = format!("# Changelog\n\n## v{version}\n- Booking\n\n## v1.1.0\n- Old\n");
    std::fs::write(root.join("CHANGELOG.md"), changelog.unwrap_or(&default_log)).unwrap();
    for (rel, text) in extra {
        let f = root.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    root.to_path_buf()
}

fn simple(root: &Path) -> std::path::PathBuf {
    repo(root, "1.2.0", None, None, None, None, &[])
}

fn failed(v: &Value) -> Vec<String> {
    v["failed"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn sub(root: &Path, name: &str) -> std::path::PathBuf {
    let p = root.join(name);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn a_complete_release_passes_the_docs_check() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(check_docs(&simple(t.path()), "1.2.0", &[], &[], &[]), json!({"status": "PASS", "failed": []}));
}

#[test]
fn missing_notes_and_version_mismatches_fail() {
    let t = tempfile::tempdir().unwrap();
    let a = repo(&sub(t.path(), "a"), "1.2.0", None, None, Some("1.1.9"), None, &[]);
    assert!(failed(&check_docs(&a, "1.2.0", &[], &[], &[])).iter().any(|x| x.contains("frontend/package.json says 1.1.9")));
    let b = repo(&sub(t.path(), "b"), "1.2.0", None, None, None, Some("0.9"), &[]);
    assert!(failed(&check_docs(&b, "1.2.0", &[], &[], &[])).iter().any(|x| x.contains("backend/VERSION")));
    let out = check_docs(&a, "9.9.9", &[], &[], &[]);
    assert!(out["status"] == "FAIL" && failed(&out).iter().any(|x| x.contains("v9.9.9.md")));
    assert!(failed(&check_docs(&sub(t.path(), "empty"), "1.0.0", &[], &[], &[])).contains(&"CHANGELOG.md is missing".to_string()));
}

#[test]
fn the_changelog_entry_must_be_on_top() {
    let t = tempfile::tempdir().unwrap();
    let bad = "# Changelog\n\n## v1.1.0\n- Old\n\n## v1.2.0\n- Booking\n";
    let r = repo(t.path(), "1.2.0", None, Some(bad), None, None, &[]);
    assert!(failed(&check_docs(&r, "1.2.0", &[], &[], &[])).iter().any(|x| x.contains("CHANGELOG")));
}

#[test]
fn every_flagged_pr_needs_its_guide_updated_or_a_gap_with_an_owner() {
    let t = tempfile::tempdir().unwrap();
    let notes = "# v1.2.0\n\n## Guides updated\n- docs/user-guides/admin.md\n\n## Needs your attention\n- None\n";
    let r = repo(t.path(), "1.2.0", Some(notes), None, None, None, &[("docs/user-guides/admin.md", "# Admin\n")]);
    let flagged = [json!({"pr": 12, "guide": "docs/user-guides/admin.md"}), json!({"pr": 13, "guide": "docs/user-guides/doctor.md"})];
    let out = check_docs(&r, "1.2.0", &flagged, &[], &[]);
    assert!(out["status"] == "FAIL" && failed(&out).len() == 1 && failed(&out)[0].contains("#13"));
    assert_eq!(check_docs(&r, "1.2.0", &flagged, &[json!({"pr": 13, "owner": "technical_writer"})], &[])["status"], "PASS");
    assert_eq!(check_docs(&r, "1.2.0", &flagged, &[json!({"pr": 13, "owner": ""})], &[])["status"], "FAIL");
}

#[test]
fn changed_endpoints_must_appear_in_the_api_docs() {
    let t = tempfile::tempdir().unwrap();
    let r = repo(t.path(), "1.2.0", None, None, None, None, &[("docs/api/bookings.md", "POST /api/v1/bookings creates one.\n")]);
    assert_eq!(check_docs(&r, "1.2.0", &[], &[], &["/api/v1/bookings"])["status"], "PASS");
    let out = check_docs(&r, "1.2.0", &[], &[], &["/api/v1/bookings", "/api/v1/refunds"]);
    assert!(out["status"] == "FAIL" && failed(&out)[0].contains("/api/v1/refunds"));
}

#[test]
fn broken_links_and_image_paths_fail() {
    let t = tempfile::tempdir().unwrap();
    let guide = "# Admin\n![add doctor](images/admin-add-doctor.png)\nSee [the FAQ](faq.md) and [web](https://x.example).\n";
    let r = repo(t.path(), "1.2.0", None, None, None, None, &[("docs/user-guides/admin.md", guide)]);
    let out = check_docs(&r, "1.2.0", &[], &[], &[]);
    assert_eq!(out["status"], "FAIL");
    let f = failed(&out);
    assert!(f.iter().any(|x| x.contains("admin-add-doctor.png")) && f.iter().any(|x| x.contains("faq.md")) && !f.iter().any(|x| x.contains("x.example")));
    std::fs::create_dir_all(r.join("docs/user-guides/images")).unwrap();
    std::fs::write(r.join("docs/user-guides/images/admin-add-doctor.png"), b"png").unwrap();
    std::fs::write(r.join("docs/user-guides/faq.md"), "# FAQ\n").unwrap();
    assert_eq!(check_docs(&r, "1.2.0", &[], &[], &[])["status"], "PASS");
}

#[test]
fn secrets_in_documents_fail_and_are_never_echoed_back() {
    let t = tempfile::tempdir().unwrap();
    let secret = "sk-live-abcdefghijklmnopqrstuvwx";
    let r = repo(t.path(), "1.2.0", None, None, None, None, &[("docs/user-guides/admin.md", &format!("# Admin\nlogin key {secret}\n"))]);
    let out = check_docs(&r, "1.2.0", &[], &[], &[]);
    assert!(out["status"] == "FAIL" && failed(&out).iter().any(|x| x.to_lowercase().contains("secret")));
    assert!(!out.to_string().contains(secret));
    assert!(failed(&out)[0].starts_with("docs/user-guides/admin.md:2: looks like a secret"));
}

#[test]
fn not_verified_items_must_be_repeated_under_needs_your_attention() {
    let t = tempfile::tempdir().unwrap();
    let bad = "# v1.2.0\n\n## What is new\n- Refunds. Not verified: the email template.\n\n## Needs your attention\n- None\n";
    let good = bad.replace("- None", "- Not verified: the email template.");
    let a = repo(&sub(t.path(), "a"), "1.2.0", Some(bad), None, None, None, &[]);
    assert!(failed(&check_docs(&a, "1.2.0", &[], &[], &[])).iter().any(|x| x.contains("Not verified")));
    let b = repo(&sub(t.path(), "b"), "1.2.0", Some(&good), None, None, None, &[]);
    assert_eq!(check_docs(&b, "1.2.0", &[], &[], &[])["status"], "PASS");
}

// ---------------------------------------------------------------- docs gate: recording and enforcement

#[test]
fn the_tester_cannot_sign_off_before_the_docs_gate_is_pass() {
    with_db(|work| {
        let tid = in_stage(work, "in_qc", "t", "abcdef1234");
        assert!(msg(Release.qc_result(&tid, 5, 0)).contains("waiting for docs gate"));
        assert_eq!(tasks().unwrap().get(&tid).unwrap().unwrap()["release"], "in_qc");
        DocsGate.run(&tid, "1.2.0", &[], &[], &[]).unwrap(); // the notes are missing in this folder, so it fails
        assert_eq!(tasks().unwrap().get(&tid).unwrap().unwrap()["docs_gate"]["status"], "FAIL");
        assert!(msg(Release.qc_result(&tid, 5, 0)).contains("waiting for docs gate"));
        simple(work);
        assert_eq!(DocsGate.run(&tid, "1.2.0", &[], &[], &[]).unwrap()["status"], "PASS");
        assert_eq!(Release.qc_result(&tid, 5, 0).unwrap()["release"], "qc_passed");
    });
}

#[test]
fn only_the_human_can_ship_with_a_documented_exception() {
    with_db(|work| {
        let tid = in_stage(work, "in_qc", "t", "abcdef1234");
        DocsGate.run(&tid, "1.2.0", &[], &[], &[]).unwrap();
        assert!(msg(DocsGate.exception(&tid, "ship it", "architect")).contains("human"));
        assert!(msg(DocsGate.exception(&tid, "  ", "human")).contains("reason"));
        DocsGate.exception(&tid, "hotfix, docs follow in 1.2.1", "human").unwrap();
        assert_eq!(Release.qc_result(&tid, 5, 0).unwrap()["release"], "qc_passed");
        assert!(tasks().unwrap().get(&tid).unwrap().unwrap()["docs_gate"]["exception"]["reason"].as_str().unwrap().starts_with("hotfix"));
    });
}

// ---------------------------------------------------------------- the agent tool

fn box_for(root: &Path, actor: &str, flow: Option<&Flow>) -> ToolBox {
    ToolBox::new(Workspace::new(root), None).pipeline(PipelineTool::new(actor, flow.cloned()).into_hook())
}

fn act(tb: &ToolBox, action: &str, mut args: Value) -> Result<Value, String> {
    args["action"] = json!(action);
    tb.call(&["pipeline"], "pipeline", &args).map(|s| serde_json::from_str(&s).unwrap()).map_err(|e| e.to_string())
}

#[test]
fn a_developer_reports_work_done_and_the_other_developer_reviews_it() {
    with_flow(|f, root| {
        let t = id(&f.flow.start(&f.feature, "t1", None, "developer_1").unwrap());
        let (d1, d2) = (box_for(root, "developer_1", Some(&f.flow)), box_for(root, "developer_2", Some(&f.flow)));
        assert!(act(&d2, "dev_done", json!({"task_id": t, "pr_number": 5, "head_sha": h1()})).unwrap_err().contains("owner"));
        assert_eq!(act(&d1, "dev_done", json!({"task_id": t, "pr_number": 5, "head_sha": h1()})).unwrap()["status"], "dev_done");
        assert_eq!(act(&d1, "tests", json!({"task_id": t, "passed": 6, "failed": 0, "area": "x"})).unwrap()["status"], "tested");
        assert!(act(&d1, "review", json!({"task_id": t, "body": review("APPROVE", &h1())})).unwrap_err().contains("peer")); // nobody approves their own work
        assert_eq!(act(&d2, "review", json!({"task_id": t, "body": review("APPROVE", &h1())})).unwrap()["status"], "approved");
    });
}

#[test]
fn each_release_step_belongs_to_one_role() {
    with_flow(|f, root| {
        let t = approved_task(f, "developer_1", "t1");
        let (arch, ci, qc, dev) = (box_for(root, "architect", Some(&f.flow)), box_for(root, "cicd", Some(&f.flow)), box_for(root, "tester", Some(&f.flow)), box_for(root, "developer_1", Some(&f.flow)));
        assert!(act(&dev, "architect_review", json!({"task_id": t, "approve": true})).unwrap_err().contains("architect"));
        assert_eq!(act(&arch, "architect_review", json!({"task_id": t, "approve": true})).unwrap()["release"], "architect_approved");
        assert!(act(&arch, "deploy_qc", json!({"task_id": t})).unwrap_err().contains("cicd"));
        assert_eq!(act(&ci, "deploy_qc", json!({"task_id": t})).unwrap()["release"], "in_qc");
        assert!(act(&ci, "qc_result", json!({"task_id": t, "passed": 3, "failed": 0})).unwrap_err().contains("tester"));
        pass_docs(&t);
        assert_eq!(act(&qc, "qc_result", json!({"task_id": t, "passed": 3, "failed": 0})).unwrap()["release"], "qc_passed");
    });
}

#[test]
fn the_architect_signs_off_production_but_never_stands_in_for_the_human() {
    with_flow(|f, root| {
        let t = approved_task(f, "developer_1", "t1");
        let r = Release;
        r.architect_review(&t, true).unwrap();
        r.deploy_qc(&t).unwrap();
        pass_docs(&t);
        r.qc_result(&t, 3, 0).unwrap();
        r.request_prod(&t).unwrap();
        let (arch, ci) = (box_for(root, "architect", Some(&f.flow)), box_for(root, "cicd", Some(&f.flow)));
        let out = act(&arch, "approve_prod", json!({"task_id": t})).unwrap();
        assert_eq!((out["release"].clone(), out["approvals"].clone()), (json!("awaiting_prod_approval"), json!(["architect"]))); // the human still decides
        assert!(act(&ci, "deploy_prod", json!({"task_id": t})).is_err()); // not approved for production yet
        r.approve_prod(&t, "human").unwrap(); // only the person in the UI can do this
        assert_eq!(act(&ci, "deploy_prod", json!({"task_id": t})).unwrap()["release"], "in_prod");
        assert!(tasks().unwrap().get(&t).unwrap().unwrap()["approvals"].to_string().contains("human"));
    });
}

#[test]
fn list_and_bad_calls_fail_as_tool_errors() {
    with_flow(|f, root| {
        let t = id(&f.flow.start(&f.feature, "t1", None, "developer_1").unwrap());
        let tb = box_for(root, "developer_1", Some(&f.flow));
        let rows = act(&tb, "list", json!({})).unwrap();
        assert_eq!((rows[0]["id"].clone(), rows[0]["owner"].clone(), rows[0]["docs_gate"].clone()), (json!(t), json!("developer_1"), Value::Null));
        assert!(act(&tb, "nope", json!({})).unwrap_err().contains("unknown pipeline action nope"));
        assert!(act(&tb, "dev_done", json!({})).unwrap_err().contains("task_id is required"));
        assert!(act(&tb, "dev_done", json!({"task_id": "zzz", "pr_number": 1, "head_sha": h1()})).unwrap_err().contains("unknown task zzz"));
        assert!(act(&tb, "dev_done", json!({"task_id": t})).unwrap_err().contains("pr_number and head_sha are required"));
        assert!(tb.call(&["read_file"], "pipeline", &json!({"action": "list"})).unwrap_err().to_string().contains("not allowed"));
    });
}

#[test]
fn ci_cd_runs_the_docs_check_and_the_tester_cannot() {
    with_db(|work| {
        let tid = in_stage(work, "in_qc", "t", "abcdef1234");
        simple(work);
        let (tester, cicd) = (box_for(work, "tester", None), box_for(work, "cicd", None));
        assert!(act(&tester, "docs_check", json!({"task_id": tid, "version": "1.2.0"})).unwrap_err().contains("cicd"));
        assert!(act(&tester, "qc_result", json!({"task_id": tid, "passed": 5, "failed": 0})).unwrap_err().contains("waiting for docs gate"));
        assert!(act(&cicd, "docs_check", json!({"task_id": tid})).unwrap_err().contains("version is required"));
        assert_eq!(act(&cicd, "docs_check", json!({"task_id": tid, "version": "1.2.0"})).unwrap()["docs_gate"], "PASS");
        assert_eq!(act(&tester, "qc_result", json!({"task_id": tid, "passed": 5, "failed": 0})).unwrap()["release"], "qc_passed");
    });
}

#[test]
fn the_tester_reports_production_tests_and_a_failure_raises_an_incident() {
    with_flow(|f, root| {
        let t = id(&f.flow.start(&f.feature, "t1", None, "developer_1").unwrap());
        let (tester, dev) = (box_for(root, "tester", Some(&f.flow)), box_for(root, "developer_1", Some(&f.flow)));
        assert!(act(&dev, "prod_tests", json!({"task_id": t, "passed": 1, "failed": 1, "area": "smoke"})).unwrap_err().contains("tester"));
        act(&tester, "prod_tests", json!({"task_id": t, "passed": 5, "failed": 0, "area": "smoke"})).unwrap();
        assert!(DocRepo::new("incidents").unwrap().list().unwrap().is_empty());
        act(&tester, "prod_tests", json!({"task_id": t, "passed": 3, "failed": 2, "area": "smoke"})).unwrap();
        let levels: Vec<_> = DocRepo::new("incidents").unwrap().list().unwrap().iter().map(|i| i["level"].clone()).collect();
        assert_eq!(levels, [json!(1)]);
    });
}
