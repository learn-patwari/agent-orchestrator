//! A saved path to the `claude` program goes stale when the Claude desktop app updates itself (the version folder changes).
use ao_core::providers::claude_cli::resolve_exe;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn with_appdata<R>(f: impl FnOnce(&std::path::Path) -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let d = tempfile::tempdir().unwrap();
    std::env::set_var("APPDATA", d.path());
    f(d.path())
}

fn install(appdata: &std::path::Path, version: &str) -> String {
    let dir = appdata.join("Claude").join("claude-code").join(version).join("abc123");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("claude.exe");
    std::fs::write(&exe, b"x").unwrap();
    exe.to_string_lossy().into_owned()
}

#[test]
fn a_saved_path_that_still_exists_is_used_as_it_is() {
    with_appdata(|a| {
        let exe = install(a, "2.1.321");
        assert_eq!(resolve_exe(&format!("& \"{exe}\"")), exe);
    });
}

#[test]
fn a_saved_path_to_a_removed_version_falls_back_to_the_installed_one() {
    with_appdata(|a| {
        let now = install(a, "2.1.321");
        let stale = a.join("Claude").join("claude-code").join("2.1.317").join("a33aa5518497").join("claude.exe");
        assert_eq!(resolve_exe(&stale.to_string_lossy()), now);
    });
}

#[test]
fn a_bare_name_and_an_empty_value_are_left_to_the_path_search() {
    with_appdata(|a| {
        assert_eq!(resolve_exe("claude"), "claude");
        let exe = install(a, "2.1.321");
        assert_eq!(resolve_exe(""), exe);
    });
}
