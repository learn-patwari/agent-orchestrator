//! Per-feature setup: the feature's own code folder (with its own knowledge graph), its own model, and skill uploads.
use super::extras::feature_or_404;
use crate::{bad, ok, ApiError, AppState, Req, Routes, R};
use ao_core::db::DocRepo;
use axum::http::Method;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The folder a feature works in: its own, else the one in Settings. None when neither is set.
pub(crate) fn feature_repo(fid: Option<&str>) -> Option<PathBuf> {
    let own = fid.and_then(|f| DocRepo::new("features").ok()?.get(f).ok()?).and_then(|f| f["repo_path"].as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty());
    let from = own.or_else(|| super::settings().ok()?["repo_path"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))?;
    Some(PathBuf::from(from))
}

fn view(f: &Value) -> Value {
    json!({"repo_path": f["repo_path"].as_str().unwrap_or(""), "model": f.get("model").cloned().unwrap_or(Value::Null)})
}

fn get_setup(_: &AppState, r: &Req) -> R {
    ok(view(&feature_or_404(r.param("fid"))?))
}

fn put_setup(_: &AppState, r: &Req) -> R {
    let fid = r.param("fid");
    feature_or_404(fid)?;
    let mut patch = json!({});
    if let Some(p) = r.body.get("repo_path") {
        let p = p.as_str().unwrap_or("").trim().to_string();
        if !p.is_empty() && !Path::new(&p).is_dir() {
            return bad(400, format!("repo_path is not a folder: {p}"));
        }
        patch["repo_path"] = json!(p);
    }
    if let Some(m) = r.body.get("model") {
        if m.is_null() {
            patch["model"] = Value::Null;
        } else {
            let (pid, model) = (m["provider_id"].as_str().unwrap_or(""), m["model"].as_str().unwrap_or("").trim());
            if model.is_empty() || DocRepo::new("providers")?.get(pid)?.is_none() {
                return bad(400, "pick an existing provider and a model name");
            }
            patch["model"] = json!({"provider_id": pid, "model": model});
        }
    }
    ok(view(&DocRepo::new("features")?.update(fid, patch)?.unwrap_or(Value::Null)))
}

fn graph_reply(fid: &str) -> R {
    let used = ao_core::graph::usage();
    let repo = feature_repo(Some(fid));
    let mut out = json!({"built": false, "repo": repo.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), "queries": used["queries"], "tokens_saved": used["tokens_saved"]});
    let Some(repo) = repo else {
        out["reason"] = json!("set a folder for this feature (or repo_path in settings) first");
        return ok(out);
    };
    match ao_core::graph::Graph::load(&ao_core::graph::graph_path(&repo)) {
        Some(g) => {
            let s = g.stats();
            for k in ["files", "symbols", "import_links", "test_files", "source_bytes", "by_language", "built_at"] {
                out[k] = s[k].clone();
            }
            out["built"] = json!(true);
        }
        None => out["reason"] = json!("not built yet; press Build"),
    }
    ok(out)
}

fn get_graph(_: &AppState, r: &Req) -> R {
    feature_or_404(r.param("fid"))?;
    graph_reply(r.param("fid"))
}

fn build_graph(_: &AppState, r: &Req) -> R {
    let fid = r.param("fid");
    feature_or_404(fid)?;
    let Some(repo) = feature_repo(Some(fid)) else { return bad(400, "set a folder for this feature (or repo_path in settings) first") };
    if !repo.is_dir() {
        return bad(400, format!("not a folder: {}", repo.display()));
    }
    ao_core::graph::ensure(&repo, true);
    graph_reply(fid)
}

fn safe_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && !n.starts_with('-')
}

/// Add a skill the user wrote or downloaded: it lands in `<data>/skills/<name>/SKILL.md` and joins the catalog.
fn upload_skill(_: &AppState, r: &Req) -> R {
    let (name, desc, body) = (r.field("name")?, r.field("description")?, r.field("body")?);
    let (name, desc) = (name.trim().to_string(), desc.trim().replace('\n', " "));
    if !safe_name(&name) {
        return bad(400, "a skill name uses letters, digits, '-' and '_' only");
    }
    if body.trim().is_empty() {
        return bad(400, "the skill has no instructions");
    }
    if super::skill_registry()?.scan().iter().any(|s| s.name == name) {
        return bad(409, format!("a skill named {name} already exists"));
    }
    let dir = ao_core::config::data_dir().join("skills").join(&name);
    std::fs::create_dir_all(&dir).map_err(|e| ApiError::new(500, e.to_string()))?;
    let text = format!("---\nname: {name}\ndescription: {}\n---\n\n{}\n", desc.replace(':', " -"), body.trim());
    std::fs::write(dir.join("SKILL.md"), text).map_err(|e| ApiError::new(500, e.to_string()))?;
    ok(json!({"name": name, "description": desc}))
}

pub fn mount(r: Routes) -> Routes {
    r.guarded(Method::GET, "/api/features/:fid/setup", get_setup)
        .guarded(Method::PUT, "/api/features/:fid/setup", put_setup)
        .guarded(Method::GET, "/api/features/:fid/graph", get_graph)
        .guarded(Method::POST, "/api/features/:fid/graph/build", build_graph)
        .guarded(Method::POST, "/api/skills/upload", upload_skill)
}
