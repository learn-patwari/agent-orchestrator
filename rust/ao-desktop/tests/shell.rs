//! The shell's logic: where data lives, which UI folder and port, and the backend running in process.
use ao_desktop::{data_dir, pick_port, start_backend, ui_dir};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

#[test]
fn data_lives_where_the_electron_app_kept_it() {
    assert_eq!(data_dir(Some(Path::new("C:/Users/x/AppData/Roaming")), None), Path::new("C:/Users/x/AppData/Roaming/agent-orchestrator-desktop/data"));
    assert_eq!(data_dir(Some(Path::new("C:/a")), Some("D:/mine")), Path::new("D:/mine")); // AO_DATA_DIR wins
    assert_eq!(data_dir(Some(Path::new("C:/a")), Some("")), Path::new("C:/a/agent-orchestrator-desktop/data"));
}

#[test]
fn the_ui_folder_is_found_next_to_the_program_or_in_the_repo() {
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join("app");
    std::fs::create_dir_all(exe.join("ui")).unwrap();
    assert_eq!(ui_dir(None, Some(&exe)), None); // a ui folder without index.html is not a UI
    std::fs::write(exe.join("ui/index.html"), "x").unwrap();
    assert_eq!(ui_dir(None, Some(&exe)), Some(exe.join("ui")));
    assert_eq!(ui_dir(Some("E:/ui"), Some(&exe)), Some("E:/ui".into()));
    let repo_out = t.path().join("frontend/out");
    std::fs::create_dir_all(&repo_out).unwrap();
    std::fs::write(repo_out.join("index.html"), "x").unwrap();
    let dbg = t.path().join("rust/target/debug");
    std::fs::create_dir_all(&dbg).unwrap();
    assert_eq!(ui_dir(None, Some(&dbg)).map(|p| p.canonicalize().unwrap()), Some(repo_out.canonicalize().unwrap()));
}

#[test]
fn a_busy_port_falls_back_to_a_free_one() {
    let hold = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let busy = hold.local_addr().unwrap().port();
    let got = pick_port(busy);
    assert_ne!(got, busy);
    assert!(std::net::TcpListener::bind(("127.0.0.1", got)).is_ok());
}

fn status(port: u16, path: &str) -> u16 {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn the_backend_runs_in_process_and_serves_api_and_ui() {
    let data = tempfile::tempdir().unwrap();
    let ui = tempfile::tempdir().unwrap();
    std::fs::write(ui.path().join("index.html"), "<h1>hi</h1>").unwrap();
    std::env::set_var("AO_DATA_DIR", data.path());
    std::env::set_var("AO_NO_SCHEDULER", "1");
    let port = pick_port(0);
    start_backend(port, Some(ui.path().to_path_buf())).unwrap();
    assert_eq!(status(port, "/api/health"), 200);
    assert_eq!(status(port, "/api/settings"), 401);
    assert_eq!(status(port, "/"), 200);
}

#[test]
fn installed_builtin_roles_and_plugins_are_found_beside_the_program() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(ao_desktop::bundled_dirs(Some(t.path())), (None, None));
    std::fs::create_dir_all(t.path().join("builtin/roles")).unwrap();
    std::fs::create_dir_all(t.path().join("resources/plugins/software-delivery")).unwrap();
    let (b, p) = ao_desktop::bundled_dirs(Some(t.path()));
    assert_eq!(b, Some(t.path().join("builtin")));
    assert_eq!(p, Some(t.path().join("resources/plugins")));
}
