//! Runtime configuration. Everything is local-first; data lives under AO_DATA_DIR.
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let d = std::env::var_os("AO_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"));
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn db_path() -> PathBuf {
    data_dir().join("orchestrator.db")
}
