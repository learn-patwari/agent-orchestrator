//! Health, auth, settings, providers, role models, prices, usage, secret scan, skills and roles.
use super::{py_list, role_dirs, settings, skill_registry};
use crate::{bad, ok, ApiError, AppState, Reply, Req, Routes, R, SESSION_COOKIE};
use ao_core::auth::{self, AuthError};
use ao_core::db::{kv_get, kv_set, DocRepo};
use ao_core::providers::{self, presets, pricing};
use ao_core::runtime::roles::{load_roles, validate};
use ao_core::runtime::tools::{ToolBox, Workspace};
use ao_core::secrets;
use axum::http::Method;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const ROLES: [&str; 9] = ["orchestrator", "architect", "developer_1", "developer_2", "tester", "cicd", "incident_reporter", "production_support", "technical_writer"];

fn session_reply() -> R {
    Ok(Reply {
        status: 200,
        body: json!({"ok": true}),
        cookies: vec![format!("{SESSION_COOKIE}={}; HttpOnly; Max-Age={}; Path=/; SameSite=strict", auth::make_token(), auth::SESSION_TTL)],
        raw: None,
    })
}

fn health(_: &AppState, _: &Req) -> R {
    ok(json!({"ok": true}))
}

fn auth_status(_: &AppState, _: &Req) -> R {
    ok(json!({"setup_done": auth::has_user()?}))
}

fn setup(_: &AppState, r: &Req) -> R {
    match auth::set_password(&r.field("password")?) {
        Ok(()) => session_reply(),
        Err(AuthError::TooShort) => bad(400, AuthError::TooShort.to_string()),
        Err(AuthError::AlreadySetUp) => bad(409, AuthError::AlreadySetUp.to_string()),
        Err(AuthError::Db(e)) => Err(e.into()),
    }
}

fn login(_: &AppState, r: &Req) -> R {
    match auth::verify_password(&r.field("password")?) {
        Ok(true) => session_reply(),
        Ok(false) => bad(401, "invalid credentials"),
        Err(e) => bad(500, e.to_string()),
    }
}

fn logout(_: &AppState, _: &Req) -> R {
    Ok(Reply { status: 200, body: json!({"ok": true}), cookies: vec![format!("{SESSION_COOKIE}=\"\"; Max-Age=0; Path=/; SameSite=lax")], raw: None })
}

fn get_settings(_: &AppState, _: &Req) -> R {
    ok(settings()?)
}

/// What people paste is not always a bare path: surrounding spaces and quotes go.
fn clean_path(v: &str) -> String {
    v.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string()
}

/// A comparable form of a path that may not exist yet: the nearest existing parent is resolved, the rest appended.
fn comparable(p: &str) -> String {
    let path = Path::new(p);
    let mut rest: Vec<std::ffi::OsString> = vec![];
    let mut cur = path.to_path_buf();
    while !cur.exists() {
        match (cur.file_name().map(|n| n.to_os_string()), cur.parent().map(|p| p.to_path_buf())) {
            (Some(n), Some(parent)) => {
                rest.push(n);
                cur = parent;
            }
            _ => break,
        }
    }
    let mut base = std::fs::canonicalize(&cur).unwrap_or(cur);
    for n in rest.into_iter().rev() {
        base.push(n);
    }
    base.to_string_lossy().replace('\\', "/").trim_start_matches("//?/").trim_end_matches('/').to_lowercase()
}

/// The development folder must be a folder, and task worktrees must live outside it (git refuses them inside).
fn check_dev_paths(patch: &Map<String, Value>, current: &Value) -> Result<(), ApiError> {
    let pick = |k: &str| patch.get(k).and_then(|v| v.as_str()).or_else(|| current[k].as_str()).unwrap_or("").to_string();
    let (repo, wt) = (pick("repo_path"), pick("worktree_root"));
    if patch.contains_key("repo_path") && !repo.is_empty() && !Path::new(&repo).is_dir() {
        return Err(ApiError::new(400, format!("repo_path is not a folder: {repo}")));
    }
    if patch.contains_key("worktree_root") && !wt.is_empty() && Path::new(&wt).is_file() {
        return Err(ApiError::new(400, format!("worktree_root is a file, not a folder: {wt}")));
    }
    if (patch.contains_key("repo_path") || patch.contains_key("worktree_root")) && !repo.is_empty() && !wt.is_empty() {
        let (r, w) = (comparable(&repo), comparable(&wt));
        if w == r || w.starts_with(&format!("{r}/")) {
            return Err(ApiError::new(400, "worktree_root must not be inside repo_path (git cannot nest a worktree in its own repository)"));
        }
    }
    Ok(())
}

/// Where agents write code: the development folder, whether it is a git repository, and where task worktrees go.
fn dev_folder(_: &AppState, _: &Req) -> R {
    let s = settings()?;
    let repo = s["repo_path"].as_str().unwrap_or("").to_string();
    let custom = s["worktree_root"].as_str().unwrap_or("").to_string();
    let effective = if custom.is_empty() { ao_core::config::data_dir().join("worktrees").to_string_lossy().into_owned() } else { custom.clone() };
    let exists = !repo.is_empty() && Path::new(&repo).is_dir();
    ok(json!({
        "repo_path": repo,
        "exists": exists,
        "is_git": exists && Path::new(&repo).join(".git").exists(),
        "worktree_root": effective,
        "worktree_root_custom": !custom.is_empty(),
    }))
}

fn put_settings(_: &AppState, r: &Req) -> R {
    let Some(patch) = r.body.as_object() else { return bad(422, "the body must be a JSON object") };
    let cur = kv_get("settings", json!({}))?;
    let known = settings()?;
    let mut unknown: Vec<String> = patch.keys().filter(|k| known.get(k.as_str()).is_none()).cloned().collect();
    if !unknown.is_empty() {
        unknown.sort();
        return bad(400, format!("unknown settings: {}", py_list(&unknown)));
    }
    let mut patch = patch.clone();
    for key in ["repo_path", "worktree_root"] {
        if let Some(v) = patch.get(key).and_then(|v| v.as_str()) {
            patch.insert(key.into(), json!(clean_path(v)));
        }
    }
    check_dev_paths(&patch, &known)?;
    let mut merged: Map<String, Value> = cur.as_object().cloned().unwrap_or_default();
    for (k, v) in &patch {
        if k == "feature_flags" {
            let mut flags = cur["feature_flags"].as_object().cloned().unwrap_or_default();
            flags.extend(v.as_object().cloned().unwrap_or_default());
            merged.insert(k.clone(), Value::Object(flags));
        } else {
            merged.insert(k.clone(), v.clone());
        }
    }
    kv_set("settings", &Value::Object(merged))?;
    ok(settings()?)
}

fn provider_presets(_: &AppState, _: &Req) -> R {
    ok(json!({"presets": presets::as_json()}))
}

fn allowed_types() -> Result<Vec<&'static str>, ApiError> {
    Ok(providers::allowed_types(ao_core::settings::external_enabled()?))
}

fn provider_types(_: &AppState, _: &Req) -> R {
    ok(json!({"types": allowed_types()?}))
}

fn public(rec: &Value) -> Value {
    let mut out: Map<String, Value> = rec.as_object().cloned().unwrap_or_default();
    out.remove("api_key_enc");
    let plain = secrets::decrypt(rec["api_key_enc"].as_str().unwrap_or("")).unwrap_or_default();
    out.insert("api_key".into(), json!(secrets::mask(&plain)));
    Value::Object(out)
}

fn list_providers(_: &AppState, _: &Req) -> R {
    ok(Value::Array(DocRepo::new("providers")?.list()?.iter().map(public).collect()))
}

fn add_provider(_: &AppState, r: &Req) -> R {
    let (name, kind) = (r.field("name")?, r.field("type")?);
    if !allowed_types()?.contains(&kind.as_str()) {
        return bad(400, format!("provider type not available: {kind}"));
    }
    let base_url = r.opt_str("base_url").unwrap_or_default();
    let key = r.opt_str("api_key").unwrap_or_default();
    let rec = DocRepo::new("providers")?.create(
        json!({"name": name, "type": kind, "base_url": base_url.trim_end_matches('/'), "api_key_enc": secrets::encrypt(&key)}),
        None,
    )?;
    ok(public(&rec))
}

fn delete_provider(_: &AppState, r: &Req) -> R {
    if !DocRepo::new("providers")?.delete(r.param("pid"))? {
        return bad(404, "not found");
    }
    ok(json!({"ok": true}))
}

fn test_provider(st: &AppState, r: &Req) -> R {
    let model = r.field("model")?;
    let Some(rec) = DocRepo::new("providers")?.get(r.param("pid"))? else { return bad(404, "not found") };
    let adapter = match st.build_provider(&rec) {
        Ok(a) => a,
        Err(e) => return bad(400, e.to_string()),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| adapter.test_connection(&model)));
    match outcome {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => ok(json!({"ok": false, "error_kind": e.kind.as_str(), "error": secrets::redact(&e.to_string())})),
        // never let an adapter crash become a silent 500 in the UI
        Err(p) => {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            ok(json!({"ok": false, "error_kind": "unknown", "error": secrets::redact(&format!("Panic: {msg}"))}))
        }
    }
}

/// Built-in roles plus any role file the user added, so every role can be given a model.
fn role_names() -> Result<Vec<String>, ApiError> {
    let mut names: Vec<String> = ROLES.iter().map(|s| s.to_string()).collect();
    for n in load_roles(&role_dirs()?).keys() {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    Ok(names)
}

fn get_role_models(_: &AppState, _: &Req) -> R {
    ok(json!({"roles": role_names()?, "models": kv_get("role_models", json!({}))?}))
}

fn set_role_model(_: &AppState, r: &Req) -> R {
    let role = r.param("role");
    if !role_names()?.iter().any(|n| n == role) {
        return bad(404, "unknown role");
    }
    let pid = r.body["provider_id"].as_str().unwrap_or("");
    let model = r.body["model"].as_str().unwrap_or("");
    if DocRepo::new("providers")?.get(pid)?.is_none() || model.is_empty() {
        return bad(400, "need a known provider_id and a model");
    }
    let mut m = kv_get("role_models", json!({}))?;
    m[role] = json!({"provider_id": pid, "model": model});
    kv_set("role_models", &m)?;
    ok(m)
}

fn get_prices(_: &AppState, _: &Req) -> R {
    ok(pricing::get_table()?)
}

fn put_prices(_: &AppState, r: &Req) -> R {
    if let Err(e) = pricing::set_table(&r.body) {
        return bad(400, e);
    }
    ok(pricing::get_table()?)
}

fn usage(_: &AppState, _: &Req) -> R {
    let mut by_role = Map::new();
    for c in DocRepo::new("llm_calls")?.list()? {
        let role = c["role"].as_str().unwrap_or("").to_string();
        let row = by_role.entry(role).or_insert_with(|| json!({"calls": 0, "input_tokens": 0, "output_tokens": 0, "cost_usd": 0.0}));
        row["calls"] = json!(row["calls"].as_i64().unwrap_or(0) + 1);
        row["input_tokens"] = json!(row["input_tokens"].as_i64().unwrap_or(0) + c["input_tokens"].as_i64().unwrap_or(0));
        row["output_tokens"] = json!(row["output_tokens"].as_i64().unwrap_or(0) + c["output_tokens"].as_i64().unwrap_or(0));
        row["cost_usd"] = json!(row["cost_usd"].as_f64().unwrap_or(0.0) + c["cost_usd"].as_f64().unwrap_or(0.0));
    }
    ok(json!({"by_role": by_role}))
}

fn scan(_: &AppState, r: &Req) -> R {
    ok(json!({"looks_like_secret": secrets::looks_like_secret(&r.field("text")?)}))
}

fn list_skills(_: &AppState, _: &Req) -> R {
    ok(Value::Array(skill_registry()?.catalog(None)))
}

fn list_roles(_: &AppState, _: &Req) -> R {
    let skills: Vec<String> = skill_registry()?.scan().into_iter().map(|s| s.name).collect();
    let known_skills: Vec<&str> = skills.iter().map(|s| s.as_str()).collect();
    let tools = ToolBox::new(Workspace::new(Path::new(".")), None).names();
    let rows: Vec<Value> = load_roles(&role_dirs()?)
        .values()
        .map(|r| json!({"name": r.name, "description": r.description, "tools": r.tools, "skills": r.skills, "model": r.model, "source": r.source,
                        "problems": validate(r, &tools, &known_skills)}))
        .collect();
    ok(Value::Array(rows))
}

pub fn mount(r: Routes) -> Routes {
    r.open(Method::GET, "/api/health", health)
        .open(Method::GET, "/api/auth/status", auth_status)
        .open(Method::POST, "/api/auth/setup", setup)
        .open(Method::POST, "/api/auth/login", login)
        .open(Method::POST, "/api/auth/logout", logout)
        .guarded(Method::GET, "/api/settings", get_settings)
        .guarded(Method::PUT, "/api/settings", put_settings)
        .guarded(Method::GET, "/api/dev-folder", dev_folder)
        .guarded(Method::GET, "/api/providers/presets", provider_presets)
        .guarded(Method::GET, "/api/providers/types", provider_types)
        .guarded(Method::GET, "/api/providers", list_providers)
        .guarded(Method::POST, "/api/providers", add_provider)
        .guarded(Method::DELETE, "/api/providers/:pid", delete_provider)
        .guarded(Method::POST, "/api/providers/:pid/test", test_provider)
        .guarded(Method::GET, "/api/role-models", get_role_models)
        .guarded(Method::PUT, "/api/role-models/:role", set_role_model)
        .guarded(Method::GET, "/api/prices", get_prices)
        .guarded(Method::PUT, "/api/prices", put_prices)
        .guarded(Method::GET, "/api/usage", usage)
        .guarded(Method::POST, "/api/secrets/scan", scan)
        .guarded(Method::GET, "/api/skills", list_skills)
        .guarded(Method::GET, "/api/roles", list_roles)
}
