//! The route groups. Each module mounts its routes on `Routes`; `all()` puts them together.
mod core;

use crate::{ApiError, Routes};
use ao_core::runtime::skills::SkillRegistry;
use serde_json::Value;
use std::path::PathBuf;

pub fn all() -> Routes {
    core::mount(Routes::new())
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

pub(crate) fn skill_registry() -> Result<SkillRegistry, ApiError> {
    Ok(SkillRegistry::new(&paths(&settings()?["skill_paths"])))
}

/// Python's `repr` of a list of strings, as it shows in error text: `['a', 'b']`.
pub(crate) fn py_list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", "))
}
