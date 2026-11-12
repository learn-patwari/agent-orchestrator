//! Inbox, assumptions, audit, and the dev/test/review pipeline: tasks, release stages, incidents, usage window, merge.
use super::extras::berr;
use super::settings;
use crate::{bad, ok, ApiError, AppState, Req, Routes, R};
use ao_core::db::{now, DocRepo};
use ao_core::features::Features;
use ao_core::inbox::{Inbox, InboxError};
use ao_core::incidents::{IncidentError, Incidents};
use ao_core::pipeline::docs_gate::DocsGate;
use ao_core::pipeline::flow::{tasks, Flow, FlowError};
use ao_core::pipeline::github::GhClient;
use ao_core::pipeline::merge_exec::{merge_batch, MergeError};
use ao_core::pipeline::merge_gate::GateConfig;
use ao_core::pipeline::release::Release;
use ao_core::pipeline::worktrees::WorktreeManager;
use ao_core::providers::base::{Completion, Kind, Message, Provider, ProviderError, ToolSpec};
use axum::http::Method;
use serde_json::{json, Value};
use std::path::Path;

fn ierr(e: InboxError) -> ApiError {
    match e {
        InboxError::NotFound(_) => ApiError::new(404, "not found"),
        InboxError::Conflict(m) => ApiError::new(409, m),
        InboxError::Invalid(m) => ApiError::new(400, m),
        InboxError::Db(d) => d.into(),
    }
}

fn decide_with_defaults() -> Result<bool, ApiError> {
    Ok(settings()?["decide_with_defaults"].as_bool().unwrap_or(false))
}

/// A provider for code paths that never call the model (the feature sync after an inbox answer).
struct NoModel;

impl Provider for NoModel {
    fn complete(&self, _: &str, _: &[Message], _: Option<&[ToolSpec]>, _: u32, _: Option<f64>) -> Result<Completion, ProviderError> {
        Err(ProviderError::new(Kind::BadRequest, "no model is configured for this step"))
    }
    fn stream(&self, _: &str, _: &[Message], _: Option<&[ToolSpec]>, _: u32, _: Option<f64>) -> Result<Vec<String>, ProviderError> {
        Err(ProviderError::new(Kind::BadRequest, "no model is configured for this step"))
    }
}

fn synced(item: Value) -> R {
    Features::new(Box::new(NoModel), "", "", decide_with_defaults()?).sync(&item).map_err(|e| ApiError::new(500, e.to_string()))?;
    ok(item)
}

fn list_inbox(_: &AppState, r: &Req) -> R {
    ok(Value::Array(Inbox::new(false).list(r.query.get("status").map(|s| s.as_str())).map_err(ierr)?))
}

fn answer_item(_: &AppState, r: &Req) -> R {
    synced(Inbox::new(decide_with_defaults()?).answer(r.param("iid"), &r.field("answer")?).map_err(ierr)?)
}

fn accept_default(_: &AppState, r: &Req) -> R {
    synced(Inbox::new(decide_with_defaults()?).accept_default(r.param("iid")).map_err(ierr)?)
}

fn decide(_: &AppState, r: &Req) -> R {
    let approve = boolean(r, "approve")?;
    synced(Inbox::new(decide_with_defaults()?).decide(r.param("iid"), approve).map_err(ierr)?)
}

fn list_assumptions(_: &AppState, _: &Req) -> R {
    ok(Value::Array(Inbox::new(false).assumptions().map_err(ierr)?))
}

fn overrule(_: &AppState, r: &Req) -> R {
    ok(Inbox::new(false).overrule(r.param("aid"), &r.field("value")?).map_err(ierr)?)
}

fn audit(_: &AppState, _: &Req) -> R {
    ok(Value::Array(DocRepo::new("audit")?.list()?))
}

// ---- body fields (a missing or mistyped one is a 422, like a pydantic model) ----

fn int(r: &Req, k: &str) -> Result<i64, ApiError> {
    r.body.get(k).and_then(|v| v.as_i64()).ok_or_else(|| ApiError::new(422, format!("field required: {k} (integer)")))
}

fn boolean(r: &Req, k: &str) -> Result<bool, ApiError> {
    r.body.get(k).and_then(|v| v.as_bool()).ok_or_else(|| ApiError::new(422, format!("field required: {k} (boolean)")))
}

fn list_of(r: &Req, k: &str) -> Vec<Value> {
    r.body.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default()
}

fn flow() -> Result<Flow, ApiError> {
    let st = settings()?;
    let repo = st["repo_path"].as_str().unwrap_or("");
    if repo.is_empty() {
        return Err(ApiError::new(400, "set repo_path in settings first"));
    }
    let root = match st["worktree_root"].as_str().filter(|s| !s.is_empty()) {
        Some(r) => std::path::PathBuf::from(r),
        None => ao_core::config::data_dir().join("worktrees"),
    };
    let wt = WorktreeManager::new(Path::new(repo), &root).map_err(|e| ApiError::new(400, e.to_string()))?;
    Ok(Flow::new(wt, st["integration_branch"].as_str().unwrap_or("internal"), st["review_round_limit"].as_i64().unwrap_or(3)))
}

fn flow_err(e: FlowError) -> ApiError {
    match e {
        FlowError::Db(d) => d.into(),
        other => ApiError::new(409, other.to_string()),
    }
}

fn list_tasks(_: &AppState, _: &Req) -> R {
    ok(Value::Array(tasks()?.list()?))
}

fn start_task(_: &AppState, r: &Req) -> R {
    let (fid, name) = (r.field("feature_id")?, r.field("name")?);
    let owner = r.opt_str("owner").unwrap_or_else(|| "developer_1".into());
    ok(flow()?.start(&fid, &name, r.opt_str("stacked_on").as_deref(), &owner).map_err(flow_err)?)
}

fn dev_done(_: &AppState, r: &Req) -> R {
    ok(flow()?.dev_done(r.param("tid"), int(r, "pr_number")?, &r.field("head_sha")?).map_err(flow_err)?)
}

fn record_tests(_: &AppState, r: &Req) -> R {
    ok(flow()?.record_tests(r.param("tid"), int(r, "passed")?, int(r, "failed")?, &r.field("area")?).map_err(flow_err)?)
}

fn record_review(_: &AppState, r: &Req) -> R {
    ok(flow()?.record_review(r.param("tid"), &r.field("body")?).map_err(flow_err)?)
}

fn architect(_: &AppState, r: &Req) -> R {
    ok(Release.architect_review(r.param("tid"), boolean(r, "approve")?).map_err(flow_err)?)
}

fn deploy_qc(_: &AppState, r: &Req) -> R {
    ok(Release.deploy_qc(r.param("tid")).map_err(flow_err)?)
}

fn qc(_: &AppState, r: &Req) -> R {
    ok(Release.qc_result(r.param("tid"), int(r, "passed")?, int(r, "failed")?).map_err(flow_err)?)
}

fn known_task(tid: &str) -> Result<(), ApiError> {
    if tasks()?.get(tid)?.is_none() {
        return Err(ApiError::new(404, "unknown task"));
    }
    Ok(())
}

fn docs_check(_: &AppState, r: &Req) -> R {
    let tid = r.param("tid");
    known_task(tid)?;
    let version = r.field("version")?;
    let endpoints: Vec<String> = list_of(r, "endpoints").iter().filter_map(|v| v.as_str().map(String::from)).collect();
    let eps: Vec<&str> = endpoints.iter().map(|s| s.as_str()).collect();
    ok(DocsGate.run(tid, &version, &eps, &list_of(r, "flagged"), &list_of(r, "gaps")).map_err(flow_err)?)
}

fn docs_exception(_: &AppState, r: &Req) -> R {
    let tid = r.param("tid");
    known_task(tid)?;
    let reason = r.field("reason")?;
    let by = r.opt_str("by").unwrap_or_else(|| "human".into());
    if by != "human" {
        return bad(403, "only the human can ship with a documented exception");
    }
    ok(DocsGate.exception(tid, &reason, &by).map_err(flow_err)?)
}

fn get_window(_: &AppState, _: &Req) -> R {
    ok(ao_core::budget::Budget.window(now()).map_err(berr)?)
}

fn put_window(_: &AppState, r: &Req) -> R {
    let Some(percent) = r.body.get("percent").and_then(|v| v.as_f64()) else { return bad(422, "field required: percent (number)") };
    if !(0.0..=100.0).contains(&percent) {
        return bad(400, "percent must be between 0 and 100");
    }
    ok(ao_core::budget::Budget.set_window(percent, r.body.get("resets_at").and_then(|v| v.as_f64())).map_err(berr)?)
}

fn request_prod(_: &AppState, r: &Req) -> R {
    ok(Release.request_prod(r.param("tid")).map_err(flow_err)?)
}

fn approve_prod(_: &AppState, r: &Req) -> R {
    ok(Release.approve_prod(r.param("tid"), &r.field("by")?).map_err(flow_err)?)
}

fn deploy_prod(_: &AppState, r: &Req) -> R {
    ok(Release.deployed_prod(r.param("tid")).map_err(flow_err)?)
}

fn incident_err(e: IncidentError) -> ApiError {
    match e {
        IncidentError::Db(d) => d.into(),
        other => ApiError::new(404, other.to_string()),
    }
}

/// A production test report. A failing one raises an incident for production support, once.
fn prod_tests(_: &AppState, r: &Req) -> R {
    let inc = Incidents.production_tests(&r.field("task_id")?, int(r, "passed")?, int(r, "failed")?, &r.field("area")?).map_err(incident_err)?;
    ok(json!({"incident": inc}))
}

fn list_incidents(_: &AppState, _: &Req) -> R {
    ok(Value::Array(Incidents.list(true).map_err(incident_err)?))
}

fn merge(st: &AppState, r: &Req) -> R {
    let s = settings()?;
    let numbers: Vec<i64> = match r.body.get("numbers").and_then(|v| v.as_array()) {
        Some(l) if l.iter().all(|n| n.is_i64()) => l.iter().filter_map(|n| n.as_i64()).collect(),
        _ => return bad(422, "field required: numbers (list of integers)"),
    };
    let flow = flow()?;
    let lookup_flow = flow.clone();
    let runner = st.gh_runner.clone().map(|f| -> ao_core::pipeline::github::GhRunner { Box::new(move |a| f(a)) });
    let gh = GhClient::new(s["github_repo"].as_str().unwrap_or(""), runner, Some(Box::new(move |n| lookup_flow.tests_for_pr(n).ok().flatten())));
    let cfg = GateConfig {
        integration_branch: s["integration_branch"].as_str().unwrap_or("internal").into(),
        review_round_limit: s["review_round_limit"].as_i64().unwrap_or(3),
        ..GateConfig::default()
    };
    let out = match merge_batch(&gh, &numbers, &cfg, s["merge_style"].as_str().unwrap_or("merge"), s["reviewer_can_merge"].as_bool().unwrap_or(false)) {
        Ok(o) => o,
        Err(MergeError::Refused(m)) => return bad(403, m),
        Err(e) => return bad(500, e.to_string()),
    };
    for m in out["merged"].as_array().into_iter().flatten() {
        if let Some(n) = m["number"].as_i64() {
            flow.mark_merged(n).map_err(flow_err)?;
        }
    }
    ok(out)
}

pub fn mount(r: Routes) -> Routes {
    r.guarded(Method::GET, "/api/inbox", list_inbox)
        .guarded(Method::POST, "/api/inbox/:iid/answer", answer_item)
        .guarded(Method::POST, "/api/inbox/:iid/accept-default", accept_default)
        .guarded(Method::POST, "/api/inbox/:iid/decide", decide)
        .guarded(Method::GET, "/api/assumptions", list_assumptions)
        .guarded(Method::POST, "/api/assumptions/:aid/overrule", overrule)
        .guarded(Method::GET, "/api/audit", audit)
        .guarded(Method::GET, "/api/pipeline/tasks", list_tasks)
        .guarded(Method::POST, "/api/pipeline/tasks", start_task)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/dev-done", dev_done)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/tests", record_tests)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/review", record_review)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/architect", architect)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/deploy-qc", deploy_qc)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/qc", qc)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/docs-check", docs_check)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/docs-exception", docs_exception)
        .guarded(Method::GET, "/api/usage-window", get_window)
        .guarded(Method::PUT, "/api/usage-window", put_window)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/request-prod", request_prod)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/approve-prod", approve_prod)
        .guarded(Method::POST, "/api/pipeline/tasks/:tid/deploy-prod", deploy_prod)
        .guarded(Method::POST, "/api/pipeline/prod-tests", prod_tests)
        .guarded(Method::GET, "/api/incidents", list_incidents)
        .guarded(Method::POST, "/api/pipeline/merge", merge)
}
