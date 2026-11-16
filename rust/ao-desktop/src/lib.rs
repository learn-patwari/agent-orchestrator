//! The desktop shell's logic, kept apart from the window code so it can be tested: where data lives, which port and
//! UI folder to use, and running the backend in process.
use ao_server::{app_with_ui, AppState};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The Agents page's Detach button opens `/agents-window/` on our own local server; nothing else may open a window.
pub fn is_detach_url(url: &tauri::Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost")) && url.path().trim_end_matches('/') == "/agents-window"
}

/// The port baked into the previous (Electron) build, so bookmarks and the UI stay the same.
pub const DEFAULT_PORT: u16 = 8765;

/// Per-user data folder. The same place the Electron app used (`%APPDATA%\agent-orchestrator-desktop\data`), so an
/// existing database, keys and plugins carry over. `AO_DATA_DIR` overrides it.
pub fn data_dir(appdata: Option<&Path>, over: Option<&str>) -> PathBuf {
    if let Some(o) = over.filter(|o| !o.is_empty()) {
        return PathBuf::from(o);
    }
    appdata.map(|a| a.join("agent-orchestrator-desktop").join("data")).unwrap_or_else(|| PathBuf::from("data"))
}

/// The exported UI: `AO_UI_DIR`, else a `ui` folder next to the program, else the repository's `frontend/out`.
pub fn ui_dir(over: Option<&str>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(o) = over.filter(|o| !o.is_empty()) {
        return Some(PathBuf::from(o));
    }
    let mut candidates: Vec<PathBuf> = vec![];
    if let Some(d) = exe_dir {
        candidates.push(d.join("ui"));
        candidates.push(d.join("resources").join("ui"));
        // running from `target/<profile>` inside the repository
        candidates.extend(d.ancestors().skip(1).take(4).map(|a| a.join("frontend").join("out")));
        candidates.extend(d.ancestors().skip(1).take(4).map(|a| a.join("..").join("frontend").join("out")));
    }
    candidates.into_iter().find(|c| c.join("index.html").is_file())
}

/// A free local port, preferring `preferred`.
pub fn pick_port(preferred: u16) -> u16 {
    if preferred != 0 && TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return preferred;
    }
    TcpListener::bind(("127.0.0.1", 0)).ok().and_then(|l| l.local_addr().ok()).map_or(preferred, |a| a.port())
}

/// Runs the backend (API plus UI) on its own thread and returns once it accepts connections.
pub fn start_backend(port: u16, ui: Option<PathBuf>) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => return drop(tx.send(Err(e.to_string()))),
        };
        rt.block_on(async move {
            let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                Ok(l) => l,
                Err(e) => return drop(tx.send(Err(format!("cannot listen on 127.0.0.1:{port}: {e}")))),
            };
            let state = AppState::default();
            ao_server::background::start(state.clone(), Duration::from_secs(30));
            ao_server::background::publish_snapshots();
            let _ = tx.send(Ok(()));
            let _ = axum_serve(listener, app_with_ui(state, ui)).await;
        });
    });
    rx.recv_timeout(Duration::from_secs(20)).map_err(|_| "the backend did not start in time".to_string())?
}

async fn axum_serve(listener: tokio::net::TcpListener, app: ao_server::Router) -> std::io::Result<()> {
    ao_server::serve(listener, app).await
}

/// The folders the installer ships next to the program: built-in roles and skills, and the shipped plugins.
/// Looks directly beside the program and under `resources`, whichever the bundler used.
pub fn bundled_dirs(exe_dir: Option<&Path>) -> (Option<PathBuf>, Option<PathBuf>) {
    let find = |name: &str, marker: &str| {
        let d = exe_dir?;
        [d.join(name), d.join("resources").join(name)].into_iter().find(|c| c.join(marker).exists())
    };
    (find("builtin", "roles"), find("plugins", "software-delivery"))
}

const DIGITS: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

/// The Windows taskbar badge for `waiting` items: a 32x32 red disc with the number in white (RGBA pixels).
/// None when nothing waits. Numbers above 99 show 99.
pub fn badge_icon(waiting: usize) -> Option<(Vec<u8>, u32, u32)> {
    if waiting == 0 {
        return None;
    }
    const N: usize = 32;
    let mut px = vec![0u8; N * N * 4];
    let mut put = |x: usize, y: usize, c: [u8; 4]| px[(y * N + x) * 4..(y * N + x) * 4 + 4].copy_from_slice(&c);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 + 0.5 - 16.0, y as f32 + 0.5 - 16.0);
            if dx * dx + dy * dy <= 16.0 * 16.0 {
                put(x, y, [217, 48, 37, 255]);
            }
        }
    }
    let text = waiting.min(99).to_string();
    let scale = if text.len() == 1 { 3 } else { 2 };
    let (gw, gh) = (5 * scale, 7 * scale);
    let total = text.len() * gw + (text.len() - 1) * scale;
    let (x0, y0) = ((N - total) / 2, (N - gh) / 2);
    for (i, ch) in text.chars().enumerate() {
        let glyph = DIGITS[ch.to_digit(10).unwrap() as usize];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            put(x0 + i * (gw + scale) + col * scale + sx, y0 + row * scale + sy, [255, 255, 255, 255]);
                        }
                    }
                }
            }
        }
    }
    Some((px, N as u32, N as u32))
}
