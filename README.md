# Agent Orchestrator

Windows desktop app that runs a team of AI agents for software development. See `docs/srs/orchestrator-app.md` (requirements and plan), `docs/design/rust-port.md` (the Rust design) and `docs/PROGRESS.md` (current state).

## Build and test
Needs Rust (stable) and, for the UI and installer, Node 20+.

```bash
cd rust && cargo test                 # ao-core and ao-server
cd rust && cargo test -p ao-desktop   # the desktop shell (slower to build)
cd rust && cargo run -p ao-server     # backend only, API on http://127.0.0.1:8000
```

Data is stored in `AO_DATA_DIR` (SQLite, encrypted API keys); set `AO_SECRET_KEY` (a Fernet key) to keep the encryption key outside the data folder.

## Installer
`build-installer-rust.cmd` exports the UI (`build-frontend-static.cmd`) and builds a per-user NSIS installer with the Tauri bundler. The app runs the backend in process and serves the UI and API from one port (default 8765). Data lives in `%APPDATA%\agent-orchestrator-desktop\data`.

## Layout
`rust/` Cargo workspace: `ao-core` (domain logic), `ao-server` (HTTP API, 93 routes), `ao-desktop` (Tauri shell). `frontend/` Next.js UI, built as a static export. `plugins/` and `.development/` ship inside the installer. `docs/` SRS, plans, progress. `reference/` read-only source material from the BookMeDoc workflow. The earlier Python backend and Electron shell were removed in the cut-over commit; they remain in git history.
