//! The route groups. Each module mounts its routes on `Routes`; `all()` puts them together.
mod core;
mod extras;
mod flow;
mod runs;
mod team;

use crate::{ApiError, Routes};
use ao_core::runtime::skills::SkillRegistry;
use serde_json::Value;
use std::path::PathBuf;

pub fn all() -> Routes {
    team::mount(flow::mount(runs::mount(extras::mount(core::mount(Routes::new())))))
}

pub(crate) fn settings() -> Result<Value, ApiError> {
    Ok(ao_core::settings::get_settings()?)
}

fn paths(v: &Value) -> Vec<PathBuf> {
    v.as_array().into_iter().flatten().filter_map(|p| p.as_str()).map(PathBuf::from).collect()
}

pub(crate) fn role_dirs() -> Result<Vec<PathBuf>, ApiError> {
    Ok(paths(&settings()?["role_dirs"]))
}

/// Role names to list in the shared snapshot: the configured roles, or the built-in team when none are configured.
pub(crate) fn configured_roles() -> Vec<String> {
    let roles = ao_core::runtime::roles::load_roles(&role_dirs().unwrap_or_default());
    if roles.is_empty() {
        core::ROLES.iter().map(|s| s.to_string()).collect()
    } else {
        roles.keys().cloned().collect()
    }
}

pub(crate) fn skill_registry() -> Result<SkillRegistry, ApiError> {
    Ok(SkillRegistry::new(&paths(&settings()?["skill_paths"])))
}

/// Python's `repr` of a list of strings, as it shows in error text: `['a', 'b']`.
pub(crate) fn py_list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", "))
}
