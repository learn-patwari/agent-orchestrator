//! `ao-backend`: the Agent Orchestrator HTTP server (local only).
use ao_server::{app, AppState};

#[tokio::main]
async fn main() {
    let port = std::env::var("AO_PORT").ok().and_then(|p| p.parse::<u16>().ok()).unwrap_or(8000);
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("ao-backend: cannot listen on 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    let state = AppState::default();
    ao_server::background::start(state.clone(), std::time::Duration::from_secs(30));
    println!("ao-backend {} listening on http://127.0.0.1:{port}", env!("CARGO_PKG_VERSION"));
    if let Err(e) = axum::serve(listener, app(state)).await {
        eprintln!("ao-backend: {e}");
        std::process::exit(1);
    }
}
