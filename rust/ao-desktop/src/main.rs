//! Agent Orchestrator for Windows: starts the backend in this process, then opens a window on it.
//! `--smoke` starts the backend, checks it answers, and exits (used by the installer smoke test).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use ao_desktop::{bundled_dirs, data_dir, pick_port, start_backend, ui_dir, DEFAULT_PORT};
use std::io::{Read, Write};
use std::net::TcpStream;
use tauri::webview::NewWindowResponse;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// A bare HTTP GET status, enough for the smoke check.
fn get_status(port: u16, path: &str) -> Option<u16> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    out.split_whitespace().nth(1)?.parse().ok()
}

fn boot() -> Result<u16, String> {
    let appdata = env("APPDATA").map(std::path::PathBuf::from);
    let dir = data_dir(appdata.as_deref(), env("AO_DATA_DIR").as_deref());
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    std::env::set_var("AO_DATA_DIR", &dir);
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()));
    let (builtin, plugins) = bundled_dirs(exe_dir.as_deref());
    if let Some(b) = builtin {
        std::env::set_var("AO_BUILTIN_DIR", b);
    }
    if let Some(p) = plugins {
        std::env::set_var("AO_PLUGINS_DIR", p);
    }
    let ui = ui_dir(env("AO_UI_DIR").as_deref(), exe_dir.as_deref());
    if ui.is_none() {
        return Err("The UI files were not found. Build them once with build-frontend-static.cmd, or set AO_UI_DIR.".into());
    }
    let port = pick_port(env("AO_DESKTOP_PORT").and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT));
    start_backend(port, ui)?;
    Ok(port)
}

fn main() {
    if std::env::args().any(|a| a == "--smoke") {
        match boot() {
            Ok(port) => {
                let (api, ui) = (get_status(port, "/api/health"), get_status(port, "/"));
                println!("SMOKE api={api:?} ui={ui:?}");
                std::process::exit(if api == Some(200) && ui == Some(200) { 0 } else { 1 });
            }
            Err(e) => {
                eprintln!("SMOKE FAILED: {e}");
                std::process::exit(1);
            }
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            let port = boot().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            let url: tauri::Url = format!("http://127.0.0.1:{port}/").parse()?;
            let handle = app.handle().clone();
            let opened = std::sync::atomic::AtomicUsize::new(0);
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Agent Orchestrator")
                .inner_size(1280.0, 860.0)
                .maximized(true)
                // the Agents page's Detach button: our own pages may open in their own window, anything else stays out
                .on_new_window(move |url, features| {
                    if !ao_desktop::is_detach_url(&url) {
                        return NewWindowResponse::Deny;
                    }
                    let n = opened.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    match WebviewWindowBuilder::new(&handle, format!("detached-{n}"), WebviewUrl::External(url))
                        .window_features(features)
                        .title("Agents")
                        .inner_size(1280.0, 820.0)
                        .build()
                    {
                        Ok(window) => NewWindowResponse::Create { window },
                        Err(_) => NewWindowResponse::Deny,
                    }
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the desktop shell failed to start");
}

