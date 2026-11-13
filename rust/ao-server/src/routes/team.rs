//! Features, the agent registry, team and office views, orchestrator chat, status, notifications and schedules.
use super::core::ROLES;
use super::extras::{berr, feature_or_404};
use super::{role_dirs, settings};
use crate::runs::launch;
use crate::{bad, ok, ApiError, AppState, Reply, Req, Routes, R};
use ao_core::agent_registry::{key_ok, registry_key, AgentRegistry, RegistryError};
use ao_core::budget::Budget;
use ao_core::db::{kv_get, now, DocRepo};
use ao_core::features::{FeatureError, Features};
use ao_core::inbox::Inbox;
use ao_core::incidents::Incidents;
use ao_core::notify::Notifier;
use ao_core::orchestrator::{default_root, status_table, team_check, team_snapshot, DelegationError, Delegator, Orchestrator, OrchestratorError};
use ao_core::pipeline::docs_gate;
use ao_core::pipeline::flow::{tasks, FlowError};
use ao_core::pipeline::release::Release;
use ao_core::providers::base::{HttpRequest, Provider};
use ao_core::providers::http::default_transport;
use ao_core::runtime::roles::{load_roles, Role};
use ao_core::scheduler::Scheduler;
use ao_core::secrets::redact;
use axum::http::Method;
use serde_json::{json, Value};
use std::collections::BTreeMap;

// ---- features (the architect's intake) ----

/// The architect's model, from the role-model settings.
fn architect(st: &AppState) -> Result<Features, ApiError> {
    let mapping = kv_get("role_models", json!({}))?["architect"].clone();
    let rec = match mapping["provider_id"].as_str() {
        Some(pid) => DocRepo::new("providers")?.get(pid)?,
        None => None,
    };
    let Some(rec) = rec else { return bad_err(400, "no model configured for the architect role") };
    let provider = st.build_provider(&rec).map_err(|e| ApiError::new(400, e.to_string()))?;
    let auto = settings()?["decide_with_defaults"].as_bool().unwrap_or(false);
    Ok(Features::new(provider, mapping["model"].as_str().unwrap_or(""), rec["id"].as_str().unwrap_or(""), auto))
}

fn bad_err<T>(status: u16, msg: &str) -> Result<T, ApiError> {
    Err(ApiError::new(status, msg))
}

fn feature_err(e: FeatureError, conflict: bool) -> ApiError {
    match e {
        FeatureError::Msg(m) if conflict => ApiError::new(409, m),
        FeatureError::BA(_) | FeatureError::Provider(_) => ApiError::new(502, redact(&format!("architect failed: {e}"))),
        other => ApiError::new(500, other.to_string()),
    }
}

fn create_feature(st: &AppState, r: &Req) -> R {
    let (title, text) = (r.field("title")?, r.field("text")?);
    ok(architect(st)?.intake(&title, &text).map_err(|e| feature_err(e, false))?)
}

fn list_features(_: &AppState, _: &Req) -> R {
    let rows: Vec<Value> = DocRepo::new("features")?
        .list()?
        .into_iter()
        .map(|mut f| {
            if let Some(o) = f.as_object_mut() {
                o.remove("srs_md");
            }
            f
        })
        .collect();
    ok(Value::Array(rows))
}

fn get_feature(_: &AppState, r: &Req) -> R {
    ok(feature_or_404(r.param("fid"))?)
}

fn get_srs(_: &AppState, r: &Req) -> R {
    let f = feature_or_404(r.param("fid"))?;
    match f["srs_md"].as_str().filter(|s| !s.is_empty()) {
        Some(md) => Ok(Reply { status: 200, body: Value::Null, cookies: vec![], raw: Some(("text/plain; charset=utf-8".into(), md.as_bytes().to_vec())) }),
        None => bad(404, "no SRS yet"),
    }
}

fn finalize_feature(st: &AppState, r: &Req) -> R {
    let fid = r.param("fid");
    feature_or_404(fid)?;
    ok(architect(st)?.finalize(fid).map_err(|e| feature_err(e, true))?)
}

// ---- the agent registry ----

fn rerr(e: RegistryError, status: u16) -> ApiError {
    match e {
        RegistryError::Msg(m) => ApiError::new(status, m),
        RegistryError::Db(d) => d.into(),
    }
}

fn get_registry_key(_: &AppState, _: &Req) -> R {
    ok(json!({"key": registry_key()?}))
}

fn registry_register(_: &AppState, r: &Req) -> R {
    let given = r.headers.get("x-registry-key").and_then(|v| v.to_str().ok());
    if !key_ok(given) {
        return bad(401, "missing or wrong registry key");
    }
    let (name, endpoint) = (r.field("name")?, r.field("endpoint")?);
    let caps: Vec<String> = r.body.get("capabilities").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let cap_refs: Vec<&str> = caps.iter().map(|s| s.as_str()).collect();
    let description = r.opt_str("description").unwrap_or_default();
    let out = AgentRegistry.register(&name, &endpoint, &cap_refs, &description, r.opt_str("token").as_deref()).map_err(|e| rerr(e, 400))?;
    if out["token"].as_str().is_some_and(|t| !t.is_empty()) {
        // a first registration: the human decides whether this agent may join
        let a = &out["agent"];
        let can = a["capabilities"].as_array().map(|c| c.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).filter(|s| !s.is_empty()).unwrap_or_else(|| "not stated".into());
        Inbox::new(false)
            .post(
                "status",
                &format!("External agent wants to join: {}", a["name"].as_str().unwrap_or("")),
                &format!("Endpoint {}. Can: {can}. Approve or reject it on the Agents page.", a["endpoint"].as_str().unwrap_or("")),
                json!({}),
            )
            .map_err(|e| ApiError::new(500, e.to_string()))?;
    }
    ok(json!({"status": out["agent"]["status"], "token": out["token"]}))
}

fn registry_heartbeat(_: &AppState, r: &Req) -> R {
    let (name, token) = (r.field("name")?, r.field("token")?);
    match AgentRegistry.heartbeat(&name, &token, None) {
        Ok(()) => ok(json!({"ok": true})),
        Err(RegistryError::Msg(m)) => bad(if m.contains("token") { 401 } else { 404 }, m),
        Err(RegistryError::Db(d)) => Err(d.into()),
    }
}

fn registry_list(_: &AppState, _: &Req) -> R {
    ok(Value::Array(AgentRegistry.list(None).map_err(|e| rerr(e, 404))?))
}

fn registry_approve(_: &AppState, r: &Req) -> R {
    ok(AgentRegistry.approve(r.param("name")).map_err(|e| rerr(e, 404))?)
}

fn registry_reject(_: &AppState, r: &Req) -> R {
    ok(AgentRegistry.reject(r.param("name")).map_err(|e| rerr(e, 404))?)
}

/// POSTs a task to an agent's endpoint and returns its reply text. An `Err` is already phrased for the person.
fn post_to_agent(st: &AppState, url: &str, payload: &Value) -> Result<String, String> {
    let transport = st.http.clone().unwrap_or_else(|| default_transport(30));
    let down = |why: String| redact(&format!("the agent did not answer: {why}"));
    let resp = transport(&HttpRequest { url: url.to_string(), headers: vec![], body: payload.clone() }).map_err(down)?;
    if !(200..300).contains(&resp.status) {
        return Err(down(format!("HTTP {}", resp.status)));
    }
    Ok(match serde_json::from_str::<Value>(&resp.body) {
        Ok(Value::Object(o)) => o.get("reply").map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or(resp.body),
        _ => resp.body,
    })
}

fn registry_task(st: &AppState, r: &Req) -> R {
    let task = r.field("task")?;
    let post = |url: &str, p: &Value| post_to_agent(st, url, p);
    match AgentRegistry.send_task(r.param("name"), &task, &post, None) {
        Ok(v) => ok(v),
        Err(RegistryError::Msg(m)) if m.starts_with("the agent did not answer") => bad(502, m),
        Err(RegistryError::Msg(m)) => bad(if m.contains("unknown") { 404 } else { 409 }, m),
        Err(RegistryError::Db(d)) => Err(d.into()),
    }
}

// ---- team and office ----

fn role_models() -> Result<BTreeMap<String, Value>, ApiError> {
    Ok(kv_get("role_models", json!({}))?.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
}

fn provider_names() -> Result<BTreeMap<String, String>, ApiError> {
    Ok(DocRepo::new("providers")?.list()?.iter().map(|p| (p["id"].as_str().unwrap_or("").to_string(), p["name"].as_str().unwrap_or("").to_string())).collect())
}

fn role_list(roles: &BTreeMap<String, Role>) -> Vec<String> {
    if roles.is_empty() {
        ROLES.iter().map(|s| s.to_string()).collect()
    } else {
        roles.keys().cloned().collect()
    }
}

fn team_rows() -> Result<Vec<Value>, ApiError> {
    let roles = load_roles(&role_dirs()?);
    let (models, names) = (role_models()?, provider_names()?);
    let list = role_list(&roles);
    let refs: Vec<&str> = list.iter().map(|s| s.as_str()).collect();
    let state = status_table(&refs)?;
    Ok(list
        .iter()
        .map(|name| {
            let s = state.iter().find(|x| x["role"] == name.as_str()).cloned().unwrap_or(json!({}));
            let model = models.get(name).map(|m| format!("{} / {}", names.get(m["provider_id"].as_str().unwrap_or("")).map_or("unknown provider", |s| s.as_str()), ao_core::runtime::py_str(&m["model"])));
            let role = roles.get(name);
            json!({"role": name, "description": role.map_or("", |r| r.description.as_str()), "tools": role.map_or(vec![], |r| r.tools.clone()),
                   "model": model, "state": s["state"], "task": s["task"], "steps": s["steps"]})
        })
        .collect())
}

fn team(_: &AppState, _: &Req) -> R {
    ok(Value::Array(team_rows()?))
}

fn office(_: &AppState, _: &Req) -> R {
    let mut hs: Vec<Value> = DocRepo::new("handoffs")?.list()?.iter().map(|h| json!({"id": h["id"], "from": h["from"], "to": h["to"], "label": h["label"], "at": h["at"]})).collect();
    for r in DocRepo::new("runs")?.list()? {
        let status = r["status"].as_str().unwrap_or("");
        if (status == "done" || status == "failed") && r["finished_at"].as_f64().is_some_and(|t| t != 0.0) {
            hs.push(json!({"id": format!("report-{}", r["id"].as_str().unwrap_or("")), "from": r["role"], "to": "orchestrator", "label": status, "at": r["finished_at"]}));
        }
    }
    hs.sort_by(|a, b| a["at"].as_f64().unwrap_or(0.0).total_cmp(&b["at"].as_f64().unwrap_or(0.0)));
    let skip = hs.len().saturating_sub(40);
    let all_tasks = tasks()?.list()?;
    let pending: Vec<Value> = all_tasks.iter().filter(|x| x["release"] == "awaiting_prod_approval").map(|x| json!({"id": x["id"], "name": x["name"], "approvals": x.get("approvals").cloned().unwrap_or(json!([]))})).collect();
    let docs: Vec<Value> = all_tasks.iter().filter(|x| x.get("docs_gate").is_some_and(|g| !g.is_null())).map(|x| json!({"id": x["id"], "name": x["name"], "status": docs_gate::state(x)})).collect();
    let open = Incidents.list(false).map_err(|e| ApiError::new(500, e.to_string()))?.len();
    ok(json!({"team": team_rows()?, "handoffs": hs.split_off(skip), "environments": Release.environments().map_err(|e| ApiError::new(500, e.to_string()))?,
              "pending_prod": pending, "docs_gates": docs, "usage_window": Budget.window(now()).map_err(berr)?, "open_incidents": open}))
}

fn team_check_endpoint(st: &AppState, _: &Req) -> R {
    let mut roles = load_roles(&role_dirs()?);
    if roles.is_empty() {
        roles = ROLES.iter().map(|n| (n.to_string(), Role::new(n, "", "", &[], &[]))).collect();
    }
    let build = |pid: &str| -> Result<Box<dyn Provider>, String> {
        let rec = DocRepo::new("providers").map_err(|e| e.to_string())?.get(pid).map_err(|e| e.to_string())?;
        let Some(rec) = rec else { return Err("ProviderError: provider was deleted".into()) };
        st.build_provider(&rec).map_err(|e| e.to_string())
    };
    ok(Value::Array(team_check(&roles, &role_models()?, &build)))
}

// ---- orchestrator chat ----

/// What the orchestrator's delegate, report and list tools call. Same rules as `POST /api/runs`.
struct RunDelegator {
    st: AppState,
    roles: BTreeMap<String, Role>,
}

impl Delegator for RunDelegator {
    fn ask_agent(&self, agent: &str, task: &str) -> Result<Value, DelegationError> {
        let post = |url: &str, p: &Value| post_to_agent(&self.st, url, p);
        AgentRegistry.send_task(agent, task, &post, None).map_err(|e| DelegationError(e.to_string()))
    }

    fn delegate(&self, role: &str, task: &str, _area: &str, _done: &[String], _context: &str, feature_id: Option<&str>, task_id: Option<&str>) -> Result<Value, DelegationError> {
        let fail = |m: String| DelegationError(m);
        let label: String = task.lines().find_map(|l| l.strip_prefix("Task: ")).unwrap_or(task).chars().take(60).collect();
        if role == "orchestrator" {
            return Err(fail("the orchestrator cannot delegate to itself".into()));
        }
        if !self.roles.contains_key(role) {
            return Err(fail(format!("unknown role {role}; roles are: {}", self.roles.keys().cloned().collect::<Vec<_>>().join(", "))));
        }
        let s = settings().map_err(|e| fail(e.to_string()))?;
        let ws = s["workspace_roots"].as_array().and_then(|a| a.first()).and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| default_root().to_string_lossy().into_owned());
        let sent_at = now(); // taken before the run starts, so the report can never be dated earlier
        let wait = std::env::var("AO_SYNC_RUNS").as_deref() == Ok("1");
        let run = launch(&self.st, role, task, &ws, wait, feature_id, task_id).map_err(|e| fail(e.msg))?;
        let store = |e: ao_core::db::DbError| fail(e.to_string());
        DocRepo::new("handoffs").map_err(store)?.create(json!({"from": "orchestrator", "to": role, "label": label, "at": sent_at, "run_id": run["id"]}), None).map_err(store)?;
        Ok(json!({"run_id": run["id"], "role": role, "status": run["status"]}))
    }

    fn request_prod(&self, task_id: &str) -> Result<Value, DelegationError> {
        let t = Release.request_prod(task_id).map_err(|e: FlowError| DelegationError(e.to_string()))?;
        Ok(json!({"task_id": task_id, "release": t["release"], "note": "the human was asked in the inbox"}))
    }

    fn report(&self, run_id: &str) -> Result<Value, DelegationError> {
        let run = DocRepo::new("runs").and_then(|r| r.get(run_id)).map_err(|e| DelegationError(e.to_string()))?;
        let Some(run) = run else { return Err(DelegationError(format!("no run {run_id}"))) };
        Ok(json!({"run_id": run_id, "role": run["role"], "task": run["task"], "status": run["status"], "steps": run["steps"], "error": run["error"], "result": run.get("result").cloned().unwrap_or(Value::Null)}))
    }

    fn runs(&self) -> Result<Value, DelegationError> {
        let all = DocRepo::new("runs").and_then(|r| r.list()).map_err(|e| DelegationError(e.to_string()))?;
        let rows: Vec<Value> = all.iter().map(|r| json!({"run_id": r["id"], "role": r["role"], "task": r["task"].as_str().unwrap_or("").chars().take(120).collect::<String>(), "status": r["status"]})).collect();
        Ok(Value::Array(rows[rows.len().saturating_sub(30)..].to_vec()))
    }
}

fn team_context(roles: &BTreeMap<String, Role>) -> Result<String, ApiError> {
    let list = role_list(roles);
    let refs: Vec<&str> = list.iter().map(|s| s.as_str()).collect();
    let mut feats = vec![];
    for f in DocRepo::new("features")?.list()? {
        let b = Budget.status(f["id"].as_str().unwrap_or("")).map_err(berr)?;
        feats.push(json!({"title": f["title"], "status": f["status"], "limit": b["limit"], "used": b["used"], "held": b["held"]}));
    }
    let agents = if ao_core::settings::external_enabled()? { Some(ao_core::agent_registry::AgentRegistry.live(None).map_err(|e| rerr(e, 500))?) } else { None };
    let open = Inbox::new(false).list(Some("open")).map_err(|e| ApiError::new(500, e.to_string()))?.len();
    Ok(team_snapshot(roles, &role_models()?, &provider_names()?, &status_table(&refs)?, open, &feats, agents.as_deref()))
}

fn chat(st: &AppState, r: &Req) -> R {
    let text = r.field("text")?;
    let mapping = role_models()?.get("orchestrator").cloned();
    let rec = match mapping.as_ref().and_then(|m| m["provider_id"].as_str()) {
        Some(pid) => DocRepo::new("providers")?.get(pid)?,
        None => None,
    };
    let (Some(mapping), Some(rec)) = (mapping, rec) else { return bad(400, "no model configured for the orchestrator role") };
    let roles = load_roles(&role_dirs()?);
    let provider = st.build_provider(&rec).map_err(|e| ApiError::new(400, e.to_string()))?;
    let body = roles.get("orchestrator").map_or("", |r| r.body.as_str()).to_string();
    let o = Orchestrator::new(provider, mapping["model"].as_str().unwrap_or(""), &body, rec["id"].as_str().unwrap_or(""), &team_context(&roles)?).roles(role_list(&roles)).delegator(Box::new(RunDelegator { st: st.clone(), roles }));
    match o.chat(&text) {
        Ok(v) => ok(v),
        Err(OrchestratorError::Provider(e)) => bad(502, redact(&format!("{}: {}", e.kind.as_str(), e.message))),
        Err(OrchestratorError::Db(d)) => Err(d.into()),
    }
}

fn chat_history(_: &AppState, _: &Req) -> R {
    let rows: Vec<Value> = DocRepo::new("messages")?.list()?.iter().filter(|m| m["chat"].as_bool().unwrap_or(false) || m["chat"].as_i64().is_some_and(|n| n != 0)).map(|m| json!({"role": m["role"], "content": m["content"], "at": m["at"]})).collect();
    ok(Value::Array(rows))
}

fn status(_: &AppState, _: &Req) -> R {
    let roles = load_roles(&role_dirs()?);
    let list = role_list(&roles);
    let refs: Vec<&str> = list.iter().map(|s| s.as_str()).collect();
    ok(Value::Array(status_table(&refs)?))
}

// ---- notifications and schedules ----

fn notifications(_: &AppState, r: &Req) -> R {
    let unread = matches!(r.query.get("unread").map(|s| s.to_lowercase()).as_deref(), Some("true" | "1" | "yes" | "on"));
    ok(Value::Array(if unread { Notifier::new().unread()? } else { DocRepo::new("notifications")?.list()? }))
}

fn read_notification(_: &AppState, r: &Req) -> R {
    match Notifier::new().mark_read(r.param("nid"))? {
        Some(n) => ok(n),
        None => bad(404, "not found"),
    }
}

const JOB_KINDS: [&str; 1] = ["reminder"];

fn list_schedules(_: &AppState, _: &Req) -> R {
    ok(Value::Array(Scheduler::new(98.0).list()?))
}

fn add_schedule(_: &AppState, r: &Req) -> R {
    let name = r.field("name")?;
    let Some(payload) = r.body.get("payload").filter(|p| p.is_object()) else { return bad(422, "field required: payload (object)") };
    let Some(run_at) = r.body.get("run_at").and_then(|v| v.as_f64()) else { return bad(422, "field required: run_at (number)") };
    let every = r.body.get("every_s").filter(|v| !v.is_null()).and_then(|v| v.as_f64());
    if !payload["kind"].as_str().is_some_and(|k| JOB_KINDS.contains(&k)) {
        return bad(400, format!("payload.kind must be one of {}", super::py_list(&JOB_KINDS.iter().map(|s| s.to_string()).collect::<Vec<_>>())));
    }
    ok(Scheduler::new(98.0).add(&name, payload.clone(), run_at, every)?)
}


pub fn mount(r: Routes) -> Routes {
    r.guarded(Method::POST, "/api/features", create_feature)
        .guarded(Method::GET, "/api/features", list_features)
        .guarded(Method::GET, "/api/features/:fid", get_feature)
        .guarded(Method::GET, "/api/features/:fid/srs", get_srs)
        .guarded(Method::POST, "/api/features/:fid/finalize", finalize_feature)
        .guarded(Method::GET, "/api/registry/key", get_registry_key)
        .open(Method::POST, "/api/registry/register", registry_register)
        .open(Method::POST, "/api/registry/heartbeat", registry_heartbeat)
        .guarded(Method::GET, "/api/registry", registry_list)
        .guarded(Method::POST, "/api/registry/:name/approve", registry_approve)
        .guarded(Method::POST, "/api/registry/:name/reject", registry_reject)
        .guarded(Method::POST, "/api/registry/:name/task", registry_task)
        .guarded(Method::GET, "/api/team", team)
        .guarded(Method::GET, "/api/office", office)
        .guarded(Method::POST, "/api/team/check", team_check_endpoint)
        .guarded(Method::POST, "/api/chat", chat)
        .guarded(Method::GET, "/api/chat", chat_history)
        .guarded(Method::GET, "/api/status", status)
        .guarded(Method::GET, "/api/notifications", notifications)
        .guarded(Method::POST, "/api/notifications/:nid/read", read_notification)
        .guarded(Method::GET, "/api/schedules", list_schedules)
        .guarded(Method::POST, "/api/schedules", add_schedule)
}
