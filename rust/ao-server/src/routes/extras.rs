//! Plugins, MCP servers, budget, profiles and the skill pickers (feature and task).
use super::{role_dirs, skill_registry};
use crate::{bad, ok, ApiError, AppState, Reply, Req, Routes, R};
use ao_core::budget::{Budget, BudgetError};
use ao_core::db::DocRepo;
use ao_core::mcp::{self, McpError, McpServers};
use ao_core::plugins::{PluginError, Plugins};
use ao_core::profiles::{ProfileError, Profiles};
use ao_core::runtime::roles::load_roles;
use ao_core::runtime::tools::{ToolBox, Workspace};
use ao_core::secrets;
use axum::http::Method;
use serde_json::{json, Value};
use std::path::Path;

/// The names of the roles, tools and skills that exist, for validating plugins (`_known` in Python).
fn known() -> Result<(Vec<String>, Vec<String>, Vec<String>), ApiError> {
    let roles: Vec<String> = load_roles(&role_dirs()?).keys().cloned().collect();
    let tools: Vec<String> = ToolBox::new(Workspace::new(Path::new(".")), None).names().into_iter().map(String::from).collect();
    let skills: Vec<String> = skill_registry()?.scan().into_iter().map(|s| s.name).collect();
    Ok((roles, tools, skills))
}

fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(|s| s.as_str()).collect()
}

/// A plugin error as an API error; `msg_status` is what a validation message maps to (400 or 409).
fn perr(e: PluginError, msg_status: u16) -> ApiError {
    match e {
        PluginError::Msg(m) => ApiError::new(msg_status, m),
        PluginError::NotFound(_) => ApiError::new(404, "not found"),
        PluginError::Db(d) => d.into(),
    }
}

fn plugins_list(_: &AppState, _: &Req) -> R {
    ok(json!({"installed": Plugins.list().map_err(|e| perr(e, 400))?, "available": Plugins.available().map_err(|e| perr(e, 400))?}))
}

fn plugins_install(_: &AppState, r: &Req) -> R {
    let path = r.field("path")?;
    let (roles, tools, skills) = known()?;
    ok(Plugins.install(&path, &refs(&roles), &refs(&tools), &refs(&skills)).map_err(|e| perr(e, 400))?)
}

fn agents_build(_: &AppState, r: &Req) -> R {
    let (roles, tools, skills) = known()?;
    ok(Plugins.build_agent(&r.body, &refs(&roles), &refs(&tools), &refs(&skills)).map_err(|e| perr(e, 400))?)
}

fn plugins_export(_: &AppState, r: &Req) -> R {
    let name = r.param("name");
    let data = Plugins.export_zip(name).map_err(|e| perr(e, 400))?;
    Ok(Reply { status: 200, body: Value::Null, cookies: vec![], raw: Some(("application/zip".into(), data)) })
}

fn plugins_enable(_: &AppState, r: &Req) -> R {
    let (roles, tools, skills) = known()?;
    ok(Plugins.enable(r.param("name"), &refs(&roles), &refs(&tools), &refs(&skills)).map_err(|e| perr(e, 409))?)
}

fn plugins_disable(_: &AppState, r: &Req) -> R {
    ok(Plugins.disable(r.param("name")).map_err(|e| perr(e, 400))?)
}

fn plugins_remove(_: &AppState, r: &Req) -> R {
    if !Plugins.remove(r.param("name")).map_err(|e| perr(e, 400))? {
        return bad(404, "not found");
    }
    ok(json!({"ok": true}))
}

fn merr(e: McpError) -> ApiError {
    match e {
        McpError::Msg(m) => ApiError::new(400, m),
        McpError::Db(d) => d.into(),
    }
}

fn mcp_list(_: &AppState, _: &Req) -> R {
    ok(Value::Array(McpServers.list().map_err(merr)?))
}

fn mcp_save(_: &AppState, r: &Req) -> R {
    ok(McpServers.save(r.param("name"), &r.body).map_err(merr)?)
}

fn mcp_delete(_: &AppState, r: &Req) -> R {
    if !McpServers.delete(r.param("name")).map_err(merr)? {
        return bad(404, "not found");
    }
    ok(json!({"ok": true}))
}

fn mcp_test(st: &AppState, r: &Req) -> R {
    let Some(server) = McpServers.get(r.param("name")).map_err(merr)? else { return bad(404, "not found") };
    let failed = |e: McpError| ok(json!({"ok": false, "error": secrets::redact(&e.to_string()), "tools": []}));
    let client = match mcp::connect(&server, st.http.clone()) {
        Ok(c) => c,
        Err(e) => return failed(e),
    };
    let listed = client.list_tools();
    client.close();
    match listed {
        Ok(tools) => {
            let tools: Vec<Value> = tools.iter().map(|t| json!({"name": t["name"], "description": t["description"].as_str().unwrap_or("")})).collect();
            ok(json!({"ok": true, "error": null, "tools": tools}))
        }
        Err(e) => failed(e),
    }
}

pub(crate) fn berr(e: BudgetError) -> ApiError {
    match e {
        BudgetError::Msg(m) => ApiError::new(400, m),
        BudgetError::Db(d) => d.into(),
    }
}

pub(crate) fn feature_or_404(fid: &str) -> Result<Value, ApiError> {
    DocRepo::new("features")?.get(fid)?.ok_or_else(|| ApiError::new(404, "not found"))
}

fn budget_overview(_: &AppState, _: &Req) -> R {
    let daily = super::settings()?["daily_token_limit"].as_i64();
    ok(Budget.overview(ao_core::db::now(), daily).map_err(berr)?)
}

fn get_budget(_: &AppState, r: &Req) -> R {
    feature_or_404(r.param("fid"))?;
    ok(Budget.status(r.param("fid")).map_err(berr)?)
}

fn set_budget(_: &AppState, r: &Req) -> R {
    let fid = r.param("fid");
    feature_or_404(fid)?;
    let limit = r.body.get("limit").filter(|v| !v.is_null());
    let resume = r.body.get("resume_at").filter(|v| !v.is_null());
    if limit.is_some_and(|v| v.as_i64().is_none()) || resume.is_some_and(|v| v.as_f64().is_none()) {
        return bad(422, "value is not a valid number");
    }
    Budget.set(fid, limit.and_then(|v| v.as_i64()), resume.and_then(|v| v.as_f64())).map_err(berr)?;
    ok(Budget.status(fid).map_err(berr)?)
}

fn perr2(e: ProfileError, status: u16) -> ApiError {
    match e {
        ProfileError::Msg(m) => ApiError::new(status, m),
        ProfileError::Db(d) => d.into(),
    }
}

fn profiles_view() -> R {
    let active = Profiles.active().map_err(|e| perr2(e, 400))?;
    ok(json!({"active": active["name"], "profiles": Profiles.list().map_err(|e| perr2(e, 400))?}))
}

fn get_profiles(_: &AppState, _: &Req) -> R {
    profiles_view()
}

fn save_profile(_: &AppState, r: &Req) -> R {
    Profiles.save(r.param("name"), &r.body).map_err(|e| perr2(e, 400))?;
    profiles_view()
}

fn delete_profile(_: &AppState, r: &Req) -> R {
    let name = r.param("name");
    let builtin = ["lean", "balanced", "thorough"].contains(&name);
    Profiles.delete(name).map_err(|e| perr2(e, if builtin { 400 } else { 404 }))?;
    profiles_view()
}

fn set_active_profile(_: &AppState, r: &Req) -> R {
    Profiles.set_active(&r.field("name")?).map_err(|e| perr2(e, 404))?;
    profiles_view()
}

/// The `skills` list of a request body, de-duplicated in order (422 when it is not a list of strings).
fn skill_names(r: &Req) -> Result<Vec<String>, ApiError> {
    let Some(list) = r.body.get("skills").and_then(|v| v.as_array()) else { return Err(ApiError::new(422, "field required: skills")) };
    let mut out: Vec<String> = Vec::new();
    for v in list {
        let Some(s) = v.as_str() else { return Err(ApiError::new(422, "skills must be strings")) };
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    }
    Ok(out)
}

fn check_known(names: &[String]) -> Result<usize, ApiError> {
    let est = skill_registry()?.estimate(&refs(names));
    if !est.unknown.is_empty() {
        return Err(ApiError::new(400, format!("unknown skill(s): {}", est.unknown.join(", "))));
    }
    Ok(est.tokens)
}

fn skills_view(fid: &str) -> R {
    let reg = skill_registry()?;
    let selected: Vec<String> = feature_or_404(fid)?["skills"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect();
    let available: Vec<Value> = reg
        .catalog(None)
        .into_iter()
        .map(|mut s| {
            let tokens = reg.estimate(&[s["name"].as_str().unwrap_or("")]).tokens;
            s["tokens"] = json!(tokens);
            s
        })
        .collect();
    ok(json!({"selected": selected, "tokens": reg.estimate(&refs(&selected)).tokens, "available": available}))
}

fn set_task_skills(_: &AppState, r: &Req) -> R {
    let tid = r.param("tid");
    let tasks = DocRepo::new("tasks")?;
    if tasks.get(tid)?.is_none() {
        return bad(404, "not found");
    }
    let names = skill_names(r)?;
    let tokens = check_known(&names)?;
    tasks.update(tid, json!({"skills": names}))?;
    ok(json!({"selected": names, "tokens": tokens}))
}

fn get_feature_skills(_: &AppState, r: &Req) -> R {
    skills_view(r.param("fid"))
}

fn set_feature_skills(_: &AppState, r: &Req) -> R {
    let fid = r.param("fid");
    feature_or_404(fid)?;
    check_known(&skill_names(r)?)?;
    DocRepo::new("features")?.update(fid, json!({"skills": skill_names(r)?}))?;
    skills_view(fid)
}

pub fn mount(r: Routes) -> Routes {
    r.guarded(Method::GET, "/api/plugins", plugins_list)
        .guarded(Method::POST, "/api/plugins/install", plugins_install)
        .guarded(Method::POST, "/api/agents/build", agents_build)
        .guarded(Method::GET, "/api/plugins/:name/export", plugins_export)
        .guarded(Method::POST, "/api/plugins/:name/enable", plugins_enable)
        .guarded(Method::POST, "/api/plugins/:name/disable", plugins_disable)
        .guarded(Method::DELETE, "/api/plugins/:name", plugins_remove)
        .guarded(Method::GET, "/api/mcp/servers", mcp_list)
        .guarded(Method::PUT, "/api/mcp/servers/:name", mcp_save)
        .guarded(Method::DELETE, "/api/mcp/servers/:name", mcp_delete)
        .guarded(Method::POST, "/api/mcp/servers/:name/test", mcp_test)
        .guarded(Method::GET, "/api/budget", budget_overview)
        .guarded(Method::GET, "/api/features/:fid/budget", get_budget)
        .guarded(Method::PUT, "/api/features/:fid/budget", set_budget)
        .guarded(Method::GET, "/api/profiles", get_profiles)
        .guarded(Method::PUT, "/api/profiles/:name", save_profile)
        .guarded(Method::DELETE, "/api/profiles/:name", delete_profile)
        .guarded(Method::POST, "/api/profiles/active", set_active_profile)
        .guarded(Method::PUT, "/api/pipeline/tasks/:tid/skills", set_task_skills)
        .guarded(Method::GET, "/api/features/:fid/skills", get_feature_skills)
        .guarded(Method::PUT, "/api/features/:fid/skills", set_feature_skills)
}
