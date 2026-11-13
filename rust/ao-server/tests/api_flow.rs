//! Inbox, assumptions, audit and the pipeline routes. Ported from backend/tests/{test_inbox_api,test_pipeline_api,test_incidents}.py.
mod common;
use ao_core::db::DocRepo;
use ao_core::inbox::{request_gate, Inbox};
use ao_core::pipeline::flow::tasks;
use ao_server::AppState;
use common::{env, env_with, Client};
use serde_json::{json, Value};
use std::process::Command;
use std::sync::{Arc, Mutex};

const H1: &str = "1111111111111111111111111111111111111111";

#[tokio::test]
async fn inbox_requires_auth() {
    let e = env();
    for p in ["/api/inbox", "/api/assumptions", "/api/audit"] {
        assert_eq!(e.c.get(p).await.0, 401, "{p}");
    }
}

#[tokio::test]
async fn question_flow_over_the_api() {
    let e = env();
    let c = e.c.authed().await;
    let q = Inbox::new(false).ask("DB?", "Which?", &["sqlite", "mongo"], Some("sqlite"), None).unwrap();
    let qid = q["id"].as_str().unwrap();
    let open = c.get("/api/inbox?status=open").await.1;
    assert_eq!(open.as_array().unwrap().iter().map(|i| i["id"].clone()).collect::<Vec<_>>(), vec![json!(qid)]);
    let (st, r) = c.act(&format!("/api/inbox/{qid}/accept-default")).await;
    assert!(st == 200 && r["answer"] == "sqlite", "{r}");
    let a = c.get("/api/assumptions").await.1;
    assert_eq!(a[0]["assumed"], "sqlite");
    let o = c.post(&format!("/api/assumptions/{}/overrule", a[0]["id"].as_str().unwrap()), json!({"value": "mongo"})).await.1;
    assert!(o["status"] == "overruled" && o["overruled_to"] == "mongo", "{o}");
    assert_eq!(c.post(&format!("/api/inbox/{qid}/answer"), json!({"answer": "x"})).await.0, 409); // already resolved
    assert_eq!(c.post("/api/inbox/nope/answer", json!({"answer": "x"})).await.0, 404);
    let no_default = Inbox::new(false).ask("n", "n", &[], None, None).unwrap();
    assert_eq!(c.act(&format!("/api/inbox/{}/accept-default", no_default["id"].as_str().unwrap())).await.0, 400);
}

#[tokio::test]
async fn gate_decision_over_the_api_and_the_audit_trail() {
    let e = env();
    let c = e.c.authed().await;
    let g = request_gate("merge_main", "merge PR 5", Some("r1")).unwrap();
    let gid = g["id"].as_str().unwrap();
    assert_eq!(c.post(&format!("/api/inbox/{gid}/answer"), json!({"answer": "yes"})).await.0, 400); // approvals use /decide
    let (st, r) = c.post(&format!("/api/inbox/{gid}/decide"), json!({"approve": true})).await;
    assert!(st == 200 && r["status"] == "approved", "{r}");
    let events: Vec<Value> = c.get("/api/audit").await.1.as_array().unwrap().iter().map(|e| e["event"].clone()).collect();
    assert_eq!(events, vec![json!("gate_requested"), json!("gate_approved")]);
    assert_eq!(c.post(&format!("/api/inbox/{gid}/decide"), json!({})).await.0, 422);
}

fn git(cwd: &std::path::Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(cwd).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

struct Api {
    e: common::Env,
    tmp: tempfile::TempDir,
    fid: String,
    gh_calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Api {
    fn c(&self) -> &Client {
        &self.e.c
    }
}

/// A repo with one commit on `internal`, an approved feature, and a fake `gh` that knows PR 5.
async fn api() -> Api {
    let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let seen = calls.clone();
    let runner: ao_server::GhRunnerFn = Arc::new(move |argv: &[String]| {
        if argv.get(2).map(|s| s.as_str()) == Some("view") {
            return Ok(json!({"number": 5, "headRefName": "task/t1", "baseRefName": "internal", "headRefOid": H1, "mergeable": "MERGEABLE",
                             "statusCheckRollup": [{"conclusion": "SUCCESS"}], "files": [{"path": "a.py"}],
                             "comments": [{"body": format!("Verdict: APPROVE\nHead: {H1}"), "createdAt": "2026-01-01T00:00:01Z"}], "reviews": []})
            .to_string());
        }
        seen.lock().unwrap().push(argv.to_vec());
        Ok(String::new())
    });
    let e = env_with(move |st: &mut AppState| st.gh_runner = Some(runner));
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "internal"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("a"), "a").unwrap();
    git(&repo, &["add", "a"]);
    git(&repo, &["commit", "-q", "-m", "i"]);
    e.c.authed().await;
    let fid = DocRepo::new("features").unwrap().create(json!({"title": "f", "status": "approved"}), None).unwrap()["id"].as_str().unwrap().to_string();
    Api { e, tmp, fid, gh_calls: calls }
}

async fn configure(a: &Api, extra: Value) {
    let mut s = json!({"repo_path": a.tmp.path().join("repo").to_string_lossy(), "worktree_root": a.tmp.path().join("wt").to_string_lossy(), "github_repo": "o/r"});
    s.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    assert_eq!(a.c().put("/api/settings", s).await.0, 200);
}

#[tokio::test]
async fn pipeline_routes_need_auth() {
    let e = env();
    assert_eq!(e.c.get("/api/pipeline/tasks").await.0, 401);
    assert_eq!(e.c.post("/api/pipeline/merge", json!({"numbers": [1]})).await.0, 401);
    assert_eq!(e.c.post("/api/pipeline/tasks/x/architect", json!({"approve": true})).await.0, 401);
    assert_eq!(e.c.post("/api/pipeline/tasks/x/approve-prod", json!({"by": "human"})).await.0, 401);
}

#[tokio::test]
async fn start_needs_a_repo_and_an_approved_feature() {
    let a = api().await;
    let c = a.c();
    let body = json!({"feature_id": a.fid, "name": "t1"});
    assert_eq!(c.post("/api/pipeline/tasks", body.clone()).await.0, 400);
    configure(&a, json!({})).await;
    let draft = DocRepo::new("features").unwrap().create(json!({"title": "d", "status": "srs_draft"}), None).unwrap()["id"].clone();
    assert_eq!(c.post("/api/pipeline/tasks", json!({"feature_id": draft, "name": "t1"})).await.0, 409);
    let (st, r) = c.post("/api/pipeline/tasks", body).await;
    assert!(st == 200 && r["branch"] == "task/t1", "{r}");
    let listed = c.get("/api/pipeline/tasks").await.1;
    assert_eq!(listed.as_array().unwrap().iter().map(|t| t["name"].clone()).collect::<Vec<_>>(), vec![json!("t1")]);
}

async fn to_approved(a: &Api, owner: Option<&str>) -> String {
    configure(a, json!({})).await;
    let mut body = json!({"feature_id": a.fid, "name": "t1"});
    if let Some(o) = owner {
        body["owner"] = json!(o);
    }
    let t = a.c().post("/api/pipeline/tasks", body).await.1;
    let base = format!("/api/pipeline/tasks/{}", t["id"].as_str().unwrap());
    a.c().post(&format!("{base}/dev-done"), json!({"pr_number": 5, "head_sha": H1})).await;
    a.c().post(&format!("{base}/tests"), json!({"passed": 4, "failed": 0, "area": "x"})).await;
    a.c().post(&format!("{base}/review"), json!({"body": format!("Verdict: APPROVE\nHead: {H1}")})).await;
    base
}

#[tokio::test]
async fn a_full_task_lifecycle_over_the_api() {
    let a = api().await;
    let c = a.c();
    configure(&a, json!({})).await;
    let t = c.post("/api/pipeline/tasks", json!({"feature_id": a.fid, "name": "t1"})).await.1;
    let base = format!("/api/pipeline/tasks/{}", t["id"].as_str().unwrap());
    assert_eq!(c.post(&format!("{base}/dev-done"), json!({"pr_number": 5, "head_sha": H1})).await.1["status"], "dev_done");
    assert_eq!(c.post(&format!("{base}/tests"), json!({"passed": 4, "failed": 0, "area": "x"})).await.1["status"], "tested");
    assert_eq!(c.post(&format!("{base}/review"), json!({"body": "looks fine"})).await.0, 409);
    assert_eq!(c.post(&format!("{base}/review"), json!({"body": format!("Verdict: APPROVE\nHead: {H1}")})).await.1["status"], "approved");
    assert_eq!(c.post(&format!("{base}/dev-done"), json!({"pr_number": "five"})).await.0, 422);
}

#[tokio::test]
async fn merge_is_off_by_default_then_merges_and_marks_tasks() {
    let a = api().await;
    let c = a.c();
    to_approved(&a, None).await;
    assert_eq!(c.post("/api/pipeline/merge", json!({"numbers": [5]})).await.0, 403); // authority off
    c.put("/api/settings", json!({"reviewer_can_merge": true})).await;
    let (st, out) = c.post("/api/pipeline/merge", json!({"numbers": [5]})).await;
    assert!(st == 200 && out["merged"].as_array().unwrap().iter().map(|m| m["number"].clone()).collect::<Vec<_>>() == vec![json!(5)] && out["refused"] == json!([]), "{out}");
    let cmd = a.gh_calls.lock().unwrap()[0].clone();
    assert!(cmd.ends_with(&["--match-head-commit".to_string(), H1.to_string(), "--repo".to_string(), "o/r".to_string()]) || cmd.windows(2).any(|w| w == ["--match-head-commit", H1]), "{cmd:?}");
    assert!(cmd.contains(&"--merge".to_string()));
    assert_eq!(c.get("/api/pipeline/tasks").await.1[0]["status"], "merged");
}

#[tokio::test]
async fn merge_never_targets_main_even_with_authority_on() {
    let a = api().await;
    configure(&a, json!({"reviewer_can_merge": true, "integration_branch": "main"})).await;
    assert_eq!(a.c().post("/api/pipeline/merge", json!({"numbers": [5]})).await.0, 403);
}

#[tokio::test]
async fn merge_without_recorded_tests_is_refused_by_the_gate() {
    let a = api().await;
    configure(&a, json!({"reviewer_can_merge": true})).await;
    let out = a.c().post("/api/pipeline/merge", json!({"numbers": [5]})).await.1; // no task or tests recorded for PR 5
    assert!(out["merged"] == json!([]) && out["refused"][0]["reasons"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains("tests")), "{out}");
}

#[tokio::test]
async fn release_over_the_api_waits_for_the_human() {
    let a = api().await;
    let c = a.c();
    let base = to_approved(&a, Some("developer_2")).await;
    let tid = base.rsplit('/').next().unwrap().to_string();
    assert_eq!(c.act(&format!("{base}/request-prod")).await.0, 409); // cannot skip QC
    assert_eq!(c.post(&format!("{base}/architect"), json!({"approve": true})).await.1["release"], "architect_approved");
    assert_eq!(c.act(&format!("{base}/deploy-qc")).await.1["release"], "in_qc");
    tasks().unwrap().update(&tid, json!({"docs_gate": {"status": "PASS", "failed": []}})).unwrap();
    assert_eq!(c.post(&format!("{base}/qc"), json!({"passed": 6, "failed": 0})).await.1["release"], "qc_passed");
    assert_eq!(c.act(&format!("{base}/request-prod")).await.1["release"], "awaiting_prod_approval");
    assert_eq!(c.post(&format!("{base}/approve-prod"), json!({"by": "human"})).await.1["release"], "awaiting_prod_approval");
    assert_eq!(c.act(&format!("{base}/deploy-prod")).await.0, 409); // still needs the architect
    assert_eq!(c.post(&format!("{base}/approve-prod"), json!({"by": "architect"})).await.1["release"], "approved_for_prod");
    assert_eq!(c.act(&format!("{base}/deploy-prod")).await.1["release"], "in_prod");
}

#[tokio::test]
async fn docs_gate_routes_check_the_task_and_who_asks() {
    let e = env();
    let c = e.c.authed().await;
    let tid = tasks().unwrap().create(json!({"name": "t1", "status": "approved"}), None).unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(c.post("/api/pipeline/tasks/ghost/docs-check", json!({"version": "1"})).await.0, 404);
    assert_eq!(c.post("/api/pipeline/tasks/ghost/docs-exception", json!({"reason": "r"})).await.0, 404);
    let (st, r) = c.post(&format!("/api/pipeline/tasks/{tid}/docs-exception"), json!({"reason": "r", "by": "architect"})).await;
    assert!(st == 403 && r["detail"] == "only the human can ship with a documented exception", "{r}");
    assert_eq!(c.post(&format!("/api/pipeline/tasks/{tid}/docs-exception"), json!({"reason": "legacy", "by": "human"})).await.0, 200);
}

#[tokio::test]
async fn the_usage_window_is_validated_and_reported() {
    let e = env();
    let c = e.c.authed().await;
    assert_eq!(c.put("/api/usage-window", json!({"percent": 120})).await.0, 400);
    assert_eq!(c.put("/api/usage-window", json!({})).await.0, 422);
    let r = c.put("/api/usage-window", json!({"percent": 99.0, "resets_at": 4102444800.0})).await.1;
    assert_eq!(r["percent"], 99.0);
    let g = c.get("/api/usage-window").await.1;
    assert!(g["blocked"] == true && g["percent"] == 99.0, "{g}");
}

#[tokio::test]
async fn production_test_reports_raise_one_incident() {
    let e = env();
    let c = e.c.authed().await;
    let tid = tasks().unwrap().create(json!({"name": "t1", "status": "merged"}), None).unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(c.post("/api/pipeline/prod-tests", json!({"task_id": tid, "passed": 4, "failed": 0, "area": "a"})).await.1, json!({"incident": null}));
    let r = c.post("/api/pipeline/prod-tests", json!({"task_id": tid, "passed": 1, "failed": 2, "area": "a"})).await.1;
    assert_eq!(r["incident"]["level"], 1);
    let listed = c.get("/api/incidents").await.1;
    assert_eq!(listed.as_array().unwrap().iter().map(|x| x["id"].clone()).collect::<Vec<_>>(), vec![r["incident"]["id"].clone()]);
    assert_eq!(c.post("/api/pipeline/prod-tests", json!({"task_id": "zzz", "passed": 1, "failed": 1, "area": "a"})).await.0, 404);
}
