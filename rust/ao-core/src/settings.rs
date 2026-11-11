//! Settings: the stored values over the defaults, with the enabled plugins' folders added after the configured ones.
use crate::config::builtin_dir;
use crate::db::{kv_get, DbError};
use serde_json::{json, Map, Value};

fn home() -> String {
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default()
}

pub fn defaults() -> Map<String, Value> {
    let b = builtin_dir();
    let skills = b.join("skills").to_string_lossy().into_owned();
    let personal = std::path::Path::new(&home()).join(".claude").join("skills").to_string_lossy().into_owned();
    let v = json!({
        "feature_flags": {"external_providers": false},
        "merge_style": "merge", // merge | squash | rebase
        "integration_branch": "internal",
        "stage_branch": "stage",
        "main_branch": "main",
        "attribution_trailer": "", // empty = off
        "review_round_limit": 3,
        "decide_with_defaults": false,
        "skill_paths": [skills, personal], // precedence: first wins
        "role_dirs": [b.join("roles").to_string_lossy()],
        "workspace_roots": [], // empty = only <data>/workspaces
        "daily_budget_usd": null, // null = no cap; the scheduler pauses at 80%, new runs stop at 100%
        "daily_token_limit": null, // null = no cap; tokens in the last 24 h across all work
        "pause_at_percent": 80,
        "repo_path": "",
        "worktree_root": "", // empty = <data>/worktrees
        "github_repo": "", // owner/name for gh
        "reviewer_can_merge": false,
        "run_limits": {"max_steps": 30, "max_total_tokens": 500000},
    });
    match v {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

pub fn get_settings() -> Result<Value, DbError> {
    let stored = kv_get("settings", json!({}))?;
    let mut merged = defaults();
    let mut flags = merged["feature_flags"].as_object().cloned().unwrap_or_default();
    if let Some(f) = stored["feature_flags"].as_object() {
        flags.extend(f.clone());
    }
    if let Some(s) = stored.as_object() {
        merged.extend(s.clone());
    }
    merged.insert("feature_flags".into(), Value::Object(flags));
    let (roles, skills) = crate::plugins::dirs()?;
    for (key, extra) in [("role_dirs", roles), ("skill_paths", skills)] {
        let mut all = merged.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        all.extend(extra.into_iter().map(Value::String));
        merged.insert(key.into(), Value::Array(all));
    }
    Ok(Value::Object(merged))
}

fn truthy(v: &Value) -> bool {
    !(v.is_null() || *v == json!(false) || *v == json!(0) || *v == json!("") || *v == json!([]) || *v == json!({}))
}

pub fn external_enabled() -> Result<bool, DbError> {
    Ok(truthy(&get_settings()?["feature_flags"]["external_providers"]))
}
