//! Runtime configuration. Everything is local-first; data lives under AO_DATA_DIR.
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let d = std::env::var_os("AO_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Roles and skills that ship with the app (`.development`). `AO_BUILTIN_DIR` overrides it (packaged builds);
/// otherwise it is the repository's own folder.
pub fn builtin_dir() -> PathBuf {
    std::env::var_os("AO_BUILTIN_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.development"))
}

/// Plugins that ship with the app, next to the built-in folder unless `AO_PLUGINS_DIR` says otherwise.
pub fn shipped_plugins_dir() -> PathBuf {
    std::env::var_os("AO_PLUGINS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins"))
}

pub fn db_path() -> PathBuf {
    data_dir().join("orchestrator.db")
}
