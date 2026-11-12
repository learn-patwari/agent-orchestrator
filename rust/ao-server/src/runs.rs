//! Starting and restarting agent runs: the service behind `POST /api/runs`, the orchestrator's delegate tool, resume
//! and the scheduler (`_launch`, `_make_runner`, `_restart_*` in the Python app).
use crate::routes::{role_dirs, settings, skill_registry};
use crate::AppState;
use ao_core::budget::Budget;
use ao_core::config::data_dir;
use ao_core::db::{kv_get, now, DocRepo};
use ao_core::features::require_approved;
use ao_core::incidents::IncidentTool;
use ao_core::mcp::RoleMcp;
use ao_core::notify::Notifier;
use ao_core::orchestrator::{check_workspace, default_root};
use ao_core::pipeline::agent_tool::PipelineTool;
use ao_core::pipeline::flow::Flow;
use ao_core::pipeline::worktrees::WorktreeManager;
use ao_core::profiles::Profiles;
use ao_core::runtime::roles::{load_roles, Role, DEVELOPERS};
use ao_core::runtime::runner::Runner;
use ao_core::runtime::tools::{ToolBox, Workspace};
use ao_core::scheduler::budget_percent_used;
use serde_json::{json, Value};
use std::path::Path;

/// A launch that cannot go ahead, with the HTTP status the API answers with.
#[derive(Debug)]
pub struct LaunchError {
    pub status: u16,
    pub msg: String,
}

fn fail<T>(status: u16, msg: impl Into<String>) -> Result<T, LaunchError> {
    Err(LaunchError { status, msg: msg.into() })
}

impl<E: std::fmt::Display> From<E> for LaunchError {
    fn from(e: E) -> Self {
        LaunchError { status: 500, msg: e.to_string() }
    }
}

/// Agents report steps on existing tasks; creating worktrees stays a human/API action.
fn agent_flow() -> Result<Option<Flow>, LaunchError> {
    let st = settings()?;
    let wt = WorktreeManager::new(&default_root(), &data_dir().join("worktrees")).ok();
    Ok(wt.map(|wt| Flow::new(wt, st["integration_branch"].as_str().unwrap_or("internal"), st["review_round_limit"].as_i64().unwrap_or(3))))
}

/// Skills preloaded for a run: the feature's, then the task's, without repeats.
fn skill_names(feature_id: Option<&str>, task_id: Option<&str>) -> Result<Vec<String>, LaunchError> {
    let mut names: Vec<String> = vec![];
    for (table, id) in [("features", feature_id), ("tasks", task_id)] {
        let Some(id) = id else { continue };
        if let Some(doc) = DocRepo::new(table)?.get(id)? {
            for s in doc["skills"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
                if !names.iter().any(|n| n == s) {
                    names.push(s.to_string());
                }
            }
        }
    }
    Ok(names)
}

pub fn make_runner(st: &AppState, role: &Role, ws: &Path, feature_id: Option<&str>, task_id: Option<&str>) -> Result<Runner, LaunchError> {
    let mapping = kv_get("role_models", json!({}))?[role.name.as_str()].clone();
    if mapping.is_null() {
        return fail(400, format!("no model configured for role {}", role.name));
    }
    let Some(rec) = DocRepo::new("providers")?.get(mapping["provider_id"].as_str().unwrap_or(""))? else {
        return fail(400, "provider for this role no longer exists");
    };
    let provider = match st.build_provider(&rec) {
        Ok(p) => p,
        Err(e) => return fail(400, e.to_string()),
    };
    let names = skill_names(feature_id, task_id)?;
    let reg = skill_registry()?;
    let preload = reg.estimate(&names.iter().map(|s| s.as_str()).collect::<Vec<_>>()).bodies;
    let mut tb = ToolBox::new(Workspace::new(ws), Some(skill_registry()?)).incidents(IncidentTool::new(&role.name).into_hook()).mcp(Box::new(RoleMcp::new(&role.name)));
    if let Some(flow) = agent_flow()? {
        tb = tb.pipeline(PipelineTool::new(&role.name, Some(flow)).into_hook());
    }
    let fid = feature_id.map(String::from);
    let gate: ao_core::runtime::runner::Gate = Box::new(move || {
        let t = now();
        let daily = settings().ok().and_then(|s| s["daily_token_limit"].as_i64());
        let b = Budget;
        b.window_gate(t).ok().flatten().or_else(|| b.daily_gate(t, daily).ok().flatten()).or_else(|| fid.as_deref().and_then(|f| b.gate(Some(f)).ok().flatten()))
    });
    Ok(Runner::new(provider, mapping["model"].as_str().unwrap_or(""), role.clone(), tb).skills(reg).limits(Profiles.limits()?).provider_id(rec["id"].as_str().unwrap_or("")).gate(gate).preload(preload))
}

/// Starts one agent run. Shared by `POST /api/runs` and the orchestrator's delegate tool.
pub fn launch(st: &AppState, role_name: &str, task: &str, workspace: &str, wait: bool, feature_id: Option<&str>, task_id: Option<&str>) -> Result<Value, LaunchError> {
    if DEVELOPERS.contains(&role_name) {
        // no development before the human approved the SRS
        if let Err(e) = require_approved(feature_id) {
            return fail(409, e.to_string());
        }
    }
    let roles = load_roles(&role_dirs()?);
    let Some(role) = roles.get(role_name) else { return fail(404, format!("unknown role {role_name}")) };
    let s = settings()?;
    let roots: Vec<String> = s["workspace_roots"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
    let roots = if roots.is_empty() { vec![default_root().to_string_lossy().into_owned()] } else { roots };
    let ws = match check_workspace(workspace, &roots) {
        Ok(p) => p,
        Err(e) => return fail(400, e),
    };
    if let Some(pct) = budget_percent_used(s["daily_budget_usd"].as_f64(), None)? {
        if pct >= 100.0 {
            Notifier::new().send("budget_pause", "Daily LLM budget reached", &format!("Used {pct:.0}% of the daily cap; new runs are blocked."), Some("budget_pause"))?;
            return fail(429, "daily LLM budget reached; new runs are blocked");
        }
    }
    let t = now();
    let b = Budget;
    if let Some(why) = b.window_gate(t)?.or(b.daily_gate(t, s["daily_token_limit"].as_i64())?) {
        return fail(429, why);
    }
    if let Some(held) = b.gate(feature_id)? {
        return fail(429, format!("this feature is {held}; press Resume once your window has cleared"));
    }
    let runner = make_runner(st, role, &ws, feature_id, task_id)?;
    let run = runner.start(task, &ws.to_string_lossy(), feature_id, task_id)?;
    let id = run["id"].as_str().unwrap_or("").to_string();
    if wait {
        return Ok(runner.run(&id)?);
    }
    std::thread::spawn(move || {
        let _ = runner.run(&id);
    });
    Ok(run)
}

/// Held runs that match continue from where they stopped.
fn restart_runs(st: &AppState, matches: &dyn Fn(&Value) -> bool) -> Result<Vec<String>, LaunchError> {
    let roles = load_roles(&role_dirs()?);
    let runs = DocRepo::new("runs")?;
    let mut started = vec![];
    for r in runs.list()? {
        let Some(role) = r["role"].as_str().and_then(|n| roles.get(n)) else { continue };
        if r["status"] != "held" || !matches(&r) {
            continue;
        }
        let ws = r["workspace"].as_str().unwrap_or("");
        let Ok(runner) = make_runner(st, role, Path::new(ws), r["feature_id"].as_str(), r["task_id"].as_str()) else {
            continue; // stays held; the person sees it on the runs page
        };
        let id = r["id"].as_str().unwrap_or("").to_string();
        runs.update(&id, json!({"status": "running", "error": null, "finished_at": null}))?;
        if std::env::var("AO_SYNC_RUNS").as_deref() == Ok("1") {
            let _ = runner.run(&id);
        } else {
            let rid = id.clone();
            std::thread::spawn(move || {
                let _ = runner.run(&rid);
            });
        }
        started.push(id);
    }
    Ok(started)
}

/// After a resume: runs held for this feature continue.
pub fn restart_held(st: &AppState, fid: &str) -> Result<Vec<String>, LaunchError> {
    restart_runs(st, &|r| r["feature_id"].as_str() == Some(fid))
}

/// Runs held by the daily limit or the usage window continue once that guard has room again.
pub fn restart_daily(st: &AppState) -> Result<Vec<String>, LaunchError> {
    let (t, b) = (now(), Budget);
    let daily_ok = b.daily_gate(t, settings()?["daily_token_limit"].as_i64())?.is_none();
    let window_ok = b.window_gate(t)?.is_none();
    restart_runs(st, &|r| {
        let e = r["error"].as_str().unwrap_or("");
        (e.starts_with("daily token limit") && daily_ok && window_ok) || (e.starts_with("usage window") && window_ok && daily_ok)
    })
}
