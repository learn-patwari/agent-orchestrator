//! Runs: start, list, read; and resuming a held feature.
use super::extras::feature_or_404;
use crate::runs::{launch, restart_held};
use crate::{bad, ok, AppState, Req, Routes, R};
use ao_core::budget::Budget;
use ao_core::db::DocRepo;
use ao_core::runtime::runner::load_log;
use axum::http::Method;
use serde_json::Value;

fn start_run(st: &AppState, r: &Req) -> R {
    let (role, task, ws) = (r.field("role")?, r.field("task")?, r.field("workspace")?);
    let wait = r.body.get("wait").and_then(|w| w.as_bool()).unwrap_or(false);
    match launch(st, &role, &task, &ws, wait, r.opt_str("feature_id").as_deref(), r.opt_str("task_id").as_deref()) {
        Ok(run) => ok(run),
        Err(e) => bad(e.status, e.msg),
    }
}

fn list_runs(_: &AppState, _: &Req) -> R {
    ok(Value::Array(DocRepo::new("runs")?.list()?))
}

fn get_run(_: &AppState, r: &Req) -> R {
    let Some(mut run) = DocRepo::new("runs")?.get(r.param("rid"))? else { return bad(404, "not found") };
    run["log"] = Value::Array(load_log(r.param("rid")).map_err(|e| crate::ApiError::new(500, e.to_string()))?);
    ok(run)
}

fn resume_feature(st: &AppState, r: &Req) -> R {
    let fid = r.param("fid");
    feature_or_404(fid)?;
    let status = Budget.resume(fid).map_err(|e| crate::ApiError::new(400, e.to_string()))?;
    restart_held(st, fid).map_err(|e| crate::ApiError::new(e.status, e.msg))?;
    ok(status)
}

pub fn mount(r: Routes) -> Routes {
    r.guarded(Method::POST, "/api/runs", start_run)
        .guarded(Method::GET, "/api/runs", list_runs)
        .guarded(Method::GET, "/api/runs/:rid", get_run)
        .guarded(Method::POST, "/api/features/:fid/resume", resume_feature)
}
