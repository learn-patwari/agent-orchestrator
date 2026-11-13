//! Phase 5a: worktrees, the dev/review flow, the merge gate, the gh client and the batch merge executor.
//! Ported from backend/tests/test_worktrees.py, test_flow.py, test_merge_gate.py, test_merge_exec.py.
use ao_core::db::{self, DocRepo};
use ao_core::inbox::Inbox;
use ao_core::pipeline::flow::{Flow, FlowError};
use ao_core::pipeline::github::{parse_review, pr_facts_from_gh, GhClient};
use ao_core::pipeline::merge_exec::{merge_batch, Gh, MergeError};
use ao_core::pipeline::merge_gate::{evaluate, merge_command, parse_verdict, plan_batch, GateConfig};
use ao_core::pipeline::worktrees::{WorktreeError, WorktreeManager};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

static ENV: Mutex<()> = Mutex::new(());

fn h1() -> String {
    "1".repeat(40)
}
fn h2() -> String {
    "2".repeat(40)
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let o = Command::new("git").args(args).current_dir(cwd).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A fresh database plus a git repo whose integration branch is "internal".
fn with_repo<R>(f: impl FnOnce(&Path, &Path) -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
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
    f(&repo, base.path())
}

// ---------------------------------------------------------------- worktrees

fn mgr(repo: &Path, base: &Path) -> WorktreeManager {
    WorktreeManager::new(repo, &base.join("worktrees")).unwrap()
}

#[test]
fn create_makes_an_isolated_worktree_on_a_new_branch_from_the_base() {
    with_repo(|repo, base| {
        let wt = mgr(repo, base).create("task-1", "internal").unwrap();
        assert!(wt.path.is_dir() && std::fs::read_to_string(wt.path.join("a")).unwrap() == "a");
        assert_eq!(git(&wt.path, &["rev-parse", "--abbrev-ref", "HEAD"]), "task/task-1");
        assert_eq!(wt.branch, "task/task-1");
        assert_eq!(git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "internal"); // main checkout untouched
        std::fs::write(wt.path.join("b.txt"), "b").unwrap();
        assert!(!repo.join("b.txt").exists());
    });
}

#[test]
fn two_tasks_share_no_files_or_branches_and_stacking_branches_from_a_task() {
    with_repo(|repo, base| {
        let m = mgr(repo, base);
        let (a, b) = (m.create("t1", "internal").unwrap(), m.create("t2", "internal").unwrap());
        assert!(a.path != b.path && a.branch != b.branch);
        std::fs::write(a.path.join("x"), "1").unwrap();
        assert!(!b.path.join("x").exists());
        std::fs::write(a.path.join("f.txt"), "f").unwrap();
        git(&a.path, &["add", "f.txt"]);
        git(&a.path, &["commit", "-q", "-m", "work"]);
        let c = m.create("next-task", &a.branch).unwrap();
        assert_eq!(std::fs::read_to_string(c.path.join("f.txt")).unwrap(), "f");
    });
}

#[test]
fn bad_names_existing_tasks_and_unknown_bases_are_refused() {
    with_repo(|repo, base| {
        let m = mgr(repo, base);
        for bad in ["../evil", "a b", "", "x/../y", "-rf"] {
            assert!(m.create(bad, "internal").is_err(), "{bad:?}");
        }
        m.create("ok", "internal").unwrap();
        assert!(m.create("ok", "internal").unwrap_err().to_string().contains("exists"));
        assert!(m.create("other", "no-such-branch").unwrap_err().to_string().contains("base"));
    });
}

#[test]
fn list_and_remove_refuse_uncommitted_work_unless_told_to_discard() {
    with_repo(|repo, base| {
        let m = mgr(repo, base);
        let wt = m.create("t1", "internal").unwrap();
        assert_eq!(m.list().unwrap().iter().map(|w| w.task.clone()).collect::<Vec<_>>(), ["t1"]);
        std::fs::write(wt.path.join("dirty.txt"), "unsaved").unwrap();
        assert!(m.remove("t1", false).unwrap_err().to_string().contains("uncommitted"));
        assert!(wt.path.exists());
        m.remove("t1", true).unwrap();
        assert!(!wt.path.exists() && m.list().unwrap().is_empty());
        assert!(matches!(m.remove("t1", false), Err(WorktreeError::Git(_))));
    });
}

#[test]
fn the_worktree_root_must_not_be_inside_the_repo() {
    with_repo(|repo, _| {
        assert!(WorktreeManager::new(repo, &repo.join("wt")).unwrap_err().to_string().contains("inside"));
    });
}

// ---------------------------------------------------------------- flow

struct F {
    flow: Flow,
    approved: String,
    draft: String,
}

fn with_flow<R>(f: impl FnOnce(&F) -> R) -> R {
    with_repo(|repo, base| {
        let features = DocRepo::new("features").unwrap();
        let approved = features.create(json!({"title": "f", "status": "approved"}), None).unwrap();
        let draft = features.create(json!({"title": "d", "status": "srs_draft"}), None).unwrap();
        let flow = Flow::new(mgr(repo, base), "internal", 2);
        f(&F { flow, approved: id(&approved), draft: id(&draft) })
    })
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_string()
}

fn review(verdict: &str, head: &str) -> String {
    format!("Verdict: {verdict}\nHead: {head}\n")
}

fn err(r: Result<Value, FlowError>) -> String {
    r.unwrap_err().to_string()
}

#[test]
fn a_task_needs_an_approved_feature_and_gets_its_own_worktree() {
    with_flow(|f| {
        assert!(err(f.flow.start(&f.draft, "t1", None, "developer_1")).contains("approved"));
        let t = f.flow.start(&f.approved, "t1", None, "developer_1").unwrap();
        assert_eq!((t["status"].clone(), t["branch"].clone(), t["rounds"].clone(), t["base"].clone()), (json!("developing"), json!("task/t1"), json!(0), json!("internal")));
        assert!(t["worktree"].as_str().unwrap().len() > 3);
        assert!(err(f.flow.start(&f.approved, "t2", None, "tester")).contains("developer"));
    });
}

#[test]
fn a_stacked_task_branches_from_its_base_task() {
    with_flow(|f| {
        let a = f.flow.start(&f.approved, "a", None, "developer_1").unwrap();
        let b = f.flow.start(&f.approved, "b", Some(&id(&a)), "developer_1").unwrap();
        assert_eq!(b["base"], a["branch"]);
    });
}

#[test]
fn the_task_document_keeps_the_python_key_order() {
    with_flow(|f| {
        let t = f.flow.start(&f.approved, "t1", None, "developer_2").unwrap();
        let keys: Vec<_> = t.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["feature_id", "name", "branch", "worktree", "base", "status", "rounds", "tests", "pr_number", "head_sha", "created_at", "owner", "reviewer", "release", "id"]);
        assert_eq!(t["reviewer"], "developer_1"); // the other developer reviews
    });
}

#[test]
fn the_happy_path_reaches_approved() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        let d = f.flow.dev_done(&t, 5, &h1()).unwrap();
        assert_eq!((d["status"].clone(), d["pr_number"].clone()), (json!("dev_done"), json!(5)));
        assert_eq!(f.flow.record_tests(&t, 10, 0, "inbox").unwrap()["status"], "tested");
        let a = f.flow.record_review(&t, &review("APPROVE", &h1())).unwrap();
        assert_eq!((a["status"].clone(), a["rounds"].clone()), (json!("approved"), json!(0)));
    });
}

#[test]
fn failed_tests_send_the_task_back_to_the_developer() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        f.flow.dev_done(&t, 5, &h1()).unwrap();
        let r = f.flow.record_tests(&t, 8, 2, "x").unwrap();
        assert_eq!((r["status"].clone(), r["tests"]["failed"].clone()), (json!("developing"), json!(2)));
    });
}

#[test]
fn request_changes_goes_back_to_the_author_and_counts_rounds() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        f.flow.dev_done(&t, 5, &h1()).unwrap();
        f.flow.record_tests(&t, 3, 0, "x").unwrap();
        let r = f.flow.record_review(&t, &review("REQUEST_CHANGES", &h1())).unwrap();
        assert_eq!((r["status"].clone(), r["rounds"].clone()), (json!("changes_requested"), json!(1)));
        let d = f.flow.dev_done(&t, 5, &h2()).unwrap(); // the author pushed fixes
        assert_eq!((d["status"].clone(), d["head_sha"].clone()), (json!("dev_done"), json!(h2())));
        f.flow.record_tests(&t, 3, 0, "x").unwrap();
        assert_eq!(f.flow.record_review(&t, &review("APPROVE", &h2())).unwrap()["status"], "approved");
    });
}

#[test]
fn the_review_round_limit_escalates_to_the_human() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        let mut last = Value::Null;
        for head in [h1(), h2()] {
            f.flow.dev_done(&t, 5, &head).unwrap();
            f.flow.record_tests(&t, 3, 0, "x").unwrap();
            last = f.flow.record_review(&t, &review("REQUEST_CHANGES", &head)).unwrap();
        }
        assert_eq!((last["status"].clone(), last["rounds"].clone()), (json!("escalated"), json!(2)));
        let kinds: Vec<_> = Inbox::new(false).list(Some("open")).unwrap().iter().map(|i| i["kind"].clone()).collect();
        assert_eq!(kinds, [json!("status")]);
        assert!(f.flow.dev_done(&t, 5, &"3".repeat(40)).is_err()); // no more automatic rounds
    });
}

#[test]
fn hold_stops_the_task() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        f.flow.dev_done(&t, 5, &h1()).unwrap();
        f.flow.record_tests(&t, 3, 0, "x").unwrap();
        assert_eq!(f.flow.record_review(&t, &review("HOLD", &h1())).unwrap()["status"], "held");
        assert_eq!(Inbox::new(false).list(Some("open")).unwrap().len(), 1);
    });
}

#[test]
fn invalid_review_text_and_illegal_transitions_are_rejected() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        assert!(err(f.flow.record_review(&t, &review("APPROVE", &h1()))).contains("state")); // not tested yet
        f.flow.dev_done(&t, 5, &h1()).unwrap();
        f.flow.record_tests(&t, 3, 0, "x").unwrap();
        assert!(err(f.flow.record_review(&t, "looks fine to me")).contains("verdict"));
        let t2 = id(&f.flow.start(&f.approved, "t2", None, "developer_1").unwrap());
        assert!(err(f.flow.record_tests(&t2, 1, 0, "x")).contains("state")); // no PR yet
        assert!(f.flow.dev_done("nope", 1, &h1()).is_err());
        assert_eq!(err(f.flow.dev_done("nope", 1, &h1())), "unknown task nope");
    });
}

#[test]
fn a_review_of_a_stale_head_does_not_count_as_approval() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        f.flow.dev_done(&t, 5, &h2()).unwrap();
        f.flow.record_tests(&t, 3, 0, "x").unwrap();
        assert!(err(f.flow.record_review(&t, &review("APPROVE", &h1()))).contains("head"));
    });
}

#[test]
fn merged_marks_the_task_and_the_tests_lookup_works() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_1").unwrap());
        f.flow.dev_done(&t, 7, &h1()).unwrap();
        f.flow.record_tests(&t, 3, 0, "x").unwrap();
        f.flow.record_review(&t, &review("APPROVE", &h1())).unwrap();
        assert_eq!(f.flow.tests_for_pr(7).unwrap(), Some(json!({"passed": 3, "failed": 0, "area": "x"})));
        assert_eq!(f.flow.tests_for_pr(99).unwrap(), None);
        assert_eq!(f.flow.mark_merged(7).unwrap().unwrap()["status"], "merged");
    });
}

#[test]
fn hand_offs_are_recorded_for_real_transitions_only() {
    with_flow(|f| {
        let t = id(&f.flow.start(&f.approved, "t1", None, "developer_2").unwrap());
        f.flow.dev_done(&t, 5, &h1()).unwrap();
        f.flow.record_tests(&t, 4, 0, "x").unwrap();
        f.flow.record_review(&t, &review("APPROVE", &h1())).unwrap();
        let mut hs = DocRepo::new("handoffs").unwrap().list().unwrap();
        hs.sort_by(|a, b| a["at"].as_f64().partial_cmp(&b["at"].as_f64()).unwrap());
        let pairs: Vec<_> = hs.iter().map(|h| (h["from"].as_str().unwrap(), h["to"].as_str().unwrap())).collect();
        assert_eq!(pairs, [("developer_2", "developer_1"), ("developer_1", "architect")]);
    });
}

// ---------------------------------------------------------------- merge gate (pure)

fn head() -> String {
    "a".repeat(40)
}

fn pr() -> Value {
    json!({"number": 7, "branch": "task/x", "base": "internal", "head_sha": head(), "ci": "success", "mergeable": true,
           "tests": {"passed": 12, "failed": 0, "area": "inbox"},
           "changed_files": ["backend/app/inbox.py", "backend/tests/test_inbox.py"],
           "reviews": [{"verdict": "APPROVE", "head_sha": head(), "at": 2, "findings": []}]})
}

fn with(mut p: Value, k: &str, v: Value) -> Value {
    p[k] = v;
    p
}

fn reasons(p: &Value) -> Vec<String> {
    evaluate(p, &GateConfig::default())["reasons"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect()
}

fn ok(p: &Value) -> bool {
    evaluate(p, &GateConfig::default())["ok"] == true
}

fn rv(verdict: &str, h: &str, at: i64) -> Value {
    json!({"verdict": verdict, "head_sha": h, "at": at, "findings": []})
}

#[test]
fn all_conditions_met_is_ok() {
    assert_eq!(evaluate(&pr(), &GateConfig::default()), json!({"ok": true, "reasons": []}));
}

#[test]
fn the_verdict_is_read_only_from_the_first_line() {
    assert_eq!(parse_verdict("Verdict: APPROVE\nlooks good").as_deref(), Some("APPROVE"));
    assert_eq!(parse_verdict("verdict: request_changes\nfix x").as_deref(), Some("REQUEST_CHANGES"));
    assert_eq!(parse_verdict("Verdict: HOLD").as_deref(), Some("HOLD"));
    assert_eq!(parse_verdict("Looks good.\nVerdict: APPROVE"), None); // a later line must not count
    assert_eq!(parse_verdict("Verdict: MAYBE"), None);
    assert_eq!(parse_verdict(""), None);
}

#[test]
fn the_base_must_be_the_integration_branch() {
    assert!(reasons(&with(pr(), "base", json!("main"))).iter().any(|r| r.contains("base")));
    assert!(reasons(&with(pr(), "base", json!("task/other"))).iter().any(|r| r.contains("base")));
}

#[test]
fn approval_must_be_on_the_current_head_and_the_latest_review_wins() {
    let stale = with(pr(), "reviews", json!([rv("APPROVE", &"b".repeat(40), 2)]));
    assert!(reasons(&stale).iter().any(|r| r.contains("head")));
    assert!(reasons(&with(pr(), "reviews", json!([]))).iter().any(|r| r.contains("review")));
    let mut rs = vec![rv("APPROVE", &head(), 1), rv("REQUEST_CHANGES", &head(), 2)];
    assert!(reasons(&with(pr(), "reviews", json!(rs))).iter().any(|r| r.contains("APPROVE")));
    rs.push(rv("APPROVE", &head(), 3));
    assert!(ok(&with(pr(), "reviews", json!(rs))));
}

#[test]
fn unresolved_medium_or_higher_findings_block_but_low_and_resolved_do_not() {
    let f = |sev: &str, res: bool| json!({"severity": sev, "resolved": res, "title": "x"});
    let review = |findings: Vec<Value>| json!([{"verdict": "APPROVE", "head_sha": head(), "at": 2, "findings": findings}]);
    assert!(reasons(&with(pr(), "reviews", review(vec![f("medium", false)]))).iter().any(|r| r.contains("finding")));
    assert!(ok(&with(pr(), "reviews", review(vec![f("low", false), f("high", true)]))));
}

#[test]
fn ci_must_be_green_and_the_pr_mergeable() {
    for (ci, mergeable) in [(json!("failure"), true), (json!("pending"), true), (Value::Null, true), (json!("success"), false)] {
        assert!(!ok(&with(with(pr(), "ci", ci), "mergeable", json!(mergeable))));
    }
}

#[test]
fn tests_must_have_run_with_counts_and_none_failing() {
    for t in [Value::Null, json!({"passed": 0, "failed": 0, "area": "x"}), json!({"passed": 5, "failed": 1, "area": "x"})] {
        assert!(reasons(&with(pr(), "tests", t)).iter().any(|r| r.contains("tests")));
    }
}

#[test]
fn secret_and_production_config_changes_block_the_merge() {
    for path in [".env", "backend/.env.local", "keys/server.pem", "backend/seed_super_admin.py", "vercel.json", "frontend/vercel.json", ".github/workflows/deploy.yml"] {
        let r = reasons(&with(pr(), "changed_files", json!(["a.py", path])));
        assert!(r.iter().any(|x| x.contains("secret") || x.contains("production")), "{path}");
    }
}

#[test]
fn a_pr_changing_reviewer_permissions_is_never_auto_merged() {
    for path in ["plugins/software-delivery/roles/reviewer.md", "plugins/x/policies.md", ".development/agents/reviewer.md"] {
        assert!(reasons(&with(pr(), "changed_files", json!([path]))).iter().any(|r| r.contains("permission")), "{path}");
    }
}

#[test]
fn too_many_review_rounds_escalate_and_all_failures_are_reported_together() {
    let mut rs: Vec<Value> = (1..4).map(|i| rv("REQUEST_CHANGES", &format!("{i}").repeat(40), i)).collect();
    rs.push(rv("APPROVE", &head(), 9));
    let out = evaluate(&with(pr(), "reviews", json!(rs)), &GateConfig { review_round_limit: 3, ..GateConfig::default() });
    assert!(out["ok"] == false && out["reasons"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains("rounds")));
    let bad = with(with(with(with(pr(), "base", json!("main")), "ci", json!("failure")), "tests", Value::Null), "reviews", json!([]));
    let many = evaluate(&bad, &GateConfig::default());
    assert!(many["reasons"].as_array().unwrap().len() >= 4);
}

#[test]
fn the_merge_command_pins_the_head_uses_the_style_and_never_admin() {
    let cmd = merge_command(&pr(), "merge").unwrap();
    assert_eq!(cmd, ["gh", "pr", "merge", "7", "--merge", "--match-head-commit", head().as_str()]);
    assert!(merge_command(&pr(), "squash").unwrap().contains(&"--squash".to_string()));
    assert!(merge_command(&pr(), "rebase").unwrap().contains(&"--rebase".to_string()));
    assert!(!cmd.contains(&"--admin".to_string()));
    assert!(merge_command(&pr(), "fast-forward").is_err());
}

fn node(number: i64, branch: &str, base: &str) -> Value {
    with(with(with(pr(), "number", json!(number)), "branch", json!(branch)), "base", json!(base))
}

#[test]
fn a_batch_orders_stacked_prs_base_first_and_skips_orphans() {
    let (a, b, c) = (node(1, "task/a", "internal"), node(2, "task/b", "task/a"), node(3, "task/c", "task/gone"));
    let out = plan_batch(&[b, c, a], "internal");
    let nums: Vec<_> = out["order"].as_array().unwrap().iter().map(|p| p["number"].clone()).collect();
    assert_eq!(nums, [json!(1), json!(2)]);
    assert_eq!(out["skipped"][0]["number"], 3);
    assert!(out["skipped"][0]["reason"].as_str().unwrap().contains("base"));
}

#[test]
fn a_batch_detects_cycles() {
    let out = plan_batch(&[node(1, "task/a", "task/b"), node(2, "task/b", "task/a")], "internal");
    assert!(out["order"].as_array().unwrap().is_empty() && out["skipped"].as_array().unwrap().len() == 2);
    assert_eq!(out["skipped"][0]["reason"], "stack cycle or broken chain");
}

// ---------------------------------------------------------------- github

#[test]
fn parse_review_reads_verdict_head_and_findings() {
    let body = format!("Verdict: APPROVE\nHead: {}\n- [low] naming (resolved)\n- [high] sql injection\nnotes", h1());
    let r = parse_review(&body, 5.0).unwrap();
    assert_eq!((r["verdict"].clone(), r["head_sha"].clone(), r["at"].clone()), (json!("APPROVE"), json!(h1()), json!(5.0)));
    let fs: Vec<_> = r["findings"].as_array().unwrap().iter().map(|f| (f["severity"].clone(), f["resolved"].clone(), f["title"].clone())).collect();
    assert_eq!(fs, [(json!("low"), json!(true), json!("naming")), (json!("high"), json!(false), json!("sql injection"))]);
    assert!(parse_review("no verdict here", 1.0).is_none());
    assert_eq!(parse_review("Verdict: APPROVE\nno head line", 1.0).unwrap()["head_sha"], Value::Null); // cannot match any head
}

#[test]
fn pr_facts_from_gh_json_maps_checks_files_and_reviews() {
    let mut raw = json!({"number": 9, "baseRefName": "internal", "headRefName": "task/x", "headRefOid": h1(), "mergeable": "MERGEABLE",
        "statusCheckRollup": [{"conclusion": "SUCCESS"}, {"conclusion": "SUCCESS"}], "files": [{"path": "a.py"}],
        "comments": [{"body": format!("Verdict: APPROVE\nHead: {}", h1()), "createdAt": "2026-01-01T00:00:01Z"}]});
    let f = pr_facts_from_gh(&raw, Some(json!({"passed": 2, "failed": 0, "area": "x"})));
    assert_eq!((f["ci"].clone(), f["mergeable"].clone(), f["changed_files"].clone(), f["base"].clone()), (json!("success"), json!(true), json!(["a.py"]), json!("internal")));
    assert_eq!(f["reviews"][0]["verdict"], "APPROVE");
    assert_eq!(f["reviews"][0]["at"], 1767225601.0); // 2026-01-01T00:00:01Z
    raw["statusCheckRollup"][1]["conclusion"] = json!("FAILURE");
    assert_eq!(pr_facts_from_gh(&raw, None)["ci"], "failure");
    raw["statusCheckRollup"] = json!([{"conclusion": null, "status": "IN_PROGRESS"}]);
    assert_eq!(pr_facts_from_gh(&raw, None)["ci"], "pending");
    raw["statusCheckRollup"] = json!([]);
    assert_eq!(pr_facts_from_gh(&raw, None)["ci"], Value::Null); // no checks is not green
}

#[test]
fn the_gh_client_builds_commands_and_never_uses_admin() {
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Default::default();
    let s = seen.clone();
    let c = GhClient::new("owner/repo", Some(Box::new(move |argv: &[String]| {
        s.lock().unwrap().push(argv.to_vec());
        Ok("{}".to_string())
    })), None);
    c.retarget(4, "internal").unwrap();
    let cmd: Vec<String> = ["gh", "pr", "merge", "4", "--merge", "--match-head-commit", &h1()].iter().map(|s| s.to_string()).collect();
    c.merge(&cmd).unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0][..4] == ["gh", "pr", "edit", "4"] && seen[0].contains(&"--base".to_string()) && seen[0].contains(&"internal".to_string()));
    assert!(seen[1].contains(&"--repo".to_string()) && seen[1].contains(&"owner/repo".to_string()) && !seen[1].contains(&"--admin".to_string()));
    let mut bad = cmd.clone();
    bad.push("--admin".into());
    assert!(c.merge(&bad).is_err());
}

// ---------------------------------------------------------------- batch merge executor

fn facts(number: i64, branch: &str, base: &str, h: &str) -> Value {
    json!({"number": number, "branch": branch, "base": base, "head_sha": h, "ci": "success", "mergeable": true,
           "tests": {"passed": 3, "failed": 0, "area": "x"}, "changed_files": ["a.py"],
           "reviews": [{"verdict": "APPROVE", "head_sha": h, "at": 1, "findings": []}]})
}

#[derive(Default)]
struct FakeGh {
    prs: Mutex<Vec<Value>>,
    calls: Mutex<Vec<String>>,
    moved_head: Option<String>, // after the first `facts` call, report this head (someone pushed)
    seen: Mutex<usize>,
}

impl FakeGh {
    fn new(prs: Vec<Value>) -> Self {
        FakeGh { prs: Mutex::new(prs), ..Default::default() }
    }
}

impl Gh for FakeGh {
    fn facts(&self, n: i64) -> Result<Value, String> {
        let mut seen = self.seen.lock().unwrap();
        *seen += 1;
        let mut f = self.prs.lock().unwrap().iter().find(|p| p["number"] == n).cloned().ok_or("no such pr")?;
        if let (Some(h), true) = (&self.moved_head, *seen > 1) {
            f["head_sha"] = json!(h);
        }
        Ok(f)
    }
    fn retarget(&self, n: i64, base: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("retarget {n} {base}"));
        for p in self.prs.lock().unwrap().iter_mut().filter(|p| p["number"] == n) {
            p["base"] = json!(base);
        }
        Ok(())
    }
    fn merge(&self, cmd: &[String]) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("merge {}", cmd.join(" ")));
        Ok(())
    }
}

fn run(gh: &FakeGh, numbers: &[i64]) -> Value {
    merge_batch(gh, numbers, &GateConfig::default(), "merge", true).unwrap()
}

fn nums(v: &Value) -> Vec<i64> {
    v.as_array().unwrap().iter().map(|m| m["number"].as_i64().unwrap()).collect()
}

fn events() -> Vec<String> {
    DocRepo::new("audit").unwrap().list().unwrap().iter().map(|e| e["event"].as_str().unwrap().to_string()).collect()
}

fn with_db<R>(f: impl FnOnce() -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    f()
}

#[test]
fn merge_authority_is_a_revocable_setting() {
    with_db(|| {
        let gh = FakeGh::new(vec![facts(1, "task/a", "internal", &h1())]);
        let e = merge_batch(&gh, &[1], &GateConfig::default(), "merge", false).unwrap_err();
        assert!(matches!(&e, MergeError::Refused(m) if m.contains("authority")));
        assert!(gh.calls.lock().unwrap().is_empty());
    });
}

#[test]
fn merging_into_main_or_the_stage_branches_is_refused() {
    with_db(|| {
        let gh = FakeGh::new(vec![facts(1, "task/a", "internal", &h1())]);
        for bad in ["main", "master", "stage"] {
            let cfg = GateConfig { integration_branch: bad.into(), ..GateConfig::default() };
            assert!(matches!(merge_batch(&gh, &[1], &cfg, "merge", true), Err(MergeError::Refused(m)) if m.contains("human")));
        }
    });
}

#[test]
fn a_batch_merges_ready_prs_pinned_to_the_head_and_audits() {
    with_db(|| {
        let gh = FakeGh::new(vec![facts(1, "task/a", "internal", &h1()), facts(2, "task/b", "internal", &h2())]);
        let out = run(&gh, &[2, 1]);
        assert_eq!(nums(&out["merged"]), [1, 2]);
        let calls = gh.calls.lock().unwrap().clone();
        assert!(calls[0].ends_with(&h1()) && calls[1].ends_with(&h2()) && calls[0].contains("--match-head-commit"));
        assert_eq!(events(), ["merge_done", "merge_done"]);
    });
}

#[test]
fn a_stacked_pr_is_retargeted_after_its_base_merges_then_merged() {
    with_db(|| {
        let gh = FakeGh::new(vec![facts(1, "task/a", "internal", &h1()), facts(2, "task/b", "task/a", &h1())]);
        let out = run(&gh, &[2, 1]);
        assert_eq!(nums(&out["merged"]), [1, 2]);
        let calls = gh.calls.lock().unwrap().clone();
        let kinds: Vec<_> = calls.iter().map(|c| c.split(' ').next().unwrap()).collect();
        assert_eq!(kinds, ["merge", "retarget", "merge"]);
        assert_eq!(calls[1], "retarget 2 internal");
    });
}

#[test]
fn a_not_ready_pr_is_refused_with_reasons_and_its_dependents_are_skipped() {
    with_db(|| {
        let mut a = facts(1, "task/a", "internal", &h1());
        a["ci"] = json!("failure");
        let gh = FakeGh::new(vec![a, facts(2, "task/b", "task/a", &h1()), facts(3, "task/c", "internal", &h1())]);
        let out = run(&gh, &[1, 2, 3]);
        assert_eq!(nums(&out["merged"]), [3]);
        let reasons_of = |n: i64| out["refused"].as_array().unwrap().iter().find(|r| r["number"] == n).unwrap()["reasons"].to_string();
        assert!(reasons_of(1).contains("CI") && reasons_of(2).contains("depends"));
        assert!(gh.calls.lock().unwrap().iter().all(|c| c.starts_with("merge pr merge 3") || c.starts_with("merge gh pr merge 3")));
        let ev = events();
        assert_eq!((ev.iter().filter(|e| *e == "merge_refused").count(), ev.iter().filter(|e| *e == "merge_done").count()), (2, 1));
    });
}

#[test]
fn a_stale_head_after_review_is_refused_even_when_everything_else_is_green() {
    with_db(|| {
        let mut f = facts(1, "task/a", "internal", &h2());
        f["reviews"] = json!([{"verdict": "APPROVE", "head_sha": h1(), "at": 1, "findings": []}]);
        let out = run(&FakeGh::new(vec![f]), &[1]);
        assert!(out["merged"].as_array().unwrap().is_empty() && out["refused"][0]["reasons"].to_string().contains("head"));
    });
}

#[test]
fn the_head_is_rechecked_just_before_the_merge() {
    with_db(|| {
        let gh = FakeGh { moved_head: Some(h2()), ..FakeGh::new(vec![facts(1, "task/a", "internal", &h1())]) };
        let out = run(&gh, &[1]);
        assert!(out["merged"].as_array().unwrap().is_empty() && out["refused"][0]["reasons"].to_string().contains("changed"));
        assert!(gh.calls.lock().unwrap().is_empty());
    });
}
