//! Plugins: a folder with plugin.json, roles/, skills/, workflows/, templates/ and optional mcp.json.
use crate::config::data_dir;
use crate::db::{DbError, DocRepo};
use std::path::PathBuf;

pub fn store() -> PathBuf {
    data_dir().join("plugins")
}

/// Roles and skills folders of the enabled plugins, to be added to what the app loads.
pub fn dirs() -> Result<(Vec<String>, Vec<String>), DbError> {
    let (mut roles, mut skills) = (vec![], vec![]);
    for p in DocRepo::new("plugins")?.list()? {
        if p["enabled"] == true {
            let base = store().join(p["name"].as_str().unwrap_or(""));
            if base.join("roles").is_dir() {
                roles.push(base.join("roles").to_string_lossy().into_owned());
            }
            if base.join("skills").is_dir() {
                skills.push(base.join("skills").to_string_lossy().into_owned());
            }
        }
    }
    Ok((roles, skills))
}
