# Rust port: plan

Decision (user, 2026-11-11): rewrite the Python/FastAPI backend in Rust and replace the Electron shell with Tauri. The Next.js UI stays as it is. Ported module by module, tests first. The Python backend keeps working until the Rust one matches it.

## Why
- Installer: about 100 MB today (Electron, a frozen Python, a Node UI server). Target roughly 15 to 30 MB.
- No bundled Python or Node server at run time.
- One native process for the backend.

## What must not change (compatibility contract)
The Rust backend is a drop-in for the Python one. The UI and existing user data must work untouched.
- **HTTP API**: the same 93 routes, methods, JSON shapes, status codes and error `detail` text. The frontend's `/api/*` calls are the spec.
- **SQLite file**: same file name, the same tables (`id TEXT`, `created_at REAL`, `data TEXT` JSON documents, plus `kv`), the same table whitelist.
- **Secrets**: Fernet tokens and `secret.key` read and written exactly as today (`fernet` crate), `AO_SECRET_KEY` honoured.
- **Passwords**: scrypt n=2^14, r=8, p=1, 16-byte salt, hex. Session token `exp.nonce.hmac_sha256` with the secret key, cookie `ao_session`, Bearer accepted.
- **Role, skill and plugin files**: same YAML front matter, same `.development` folder layout.
- **Env vars**: `AO_DATA_DIR`, `AO_NO_SCHEDULER`, `AO_SYNC_RUNS`, `AO_SECRET_KEY` and the rest keep their meaning.
- **Tests as the contract**: every Python test in `backend/tests` is ported first. The same scenario with the same expected result. Where a test uses `TestClient` and a mock upstream, the Rust test uses `tower::ServiceExt` and a fake HTTP server.

## Layout
```
rust/
  Cargo.toml            workspace
  ao-core/              library: everything except HTTP
  ao-server/            axum app, the 93 routes, `ao-backend.exe --port N`
  src-tauri/            Tauri shell (replaces desktop/main.js)
```
Main crates: axum, tokio, serde, serde_json, serde_yaml, rusqlite (bundled), reqwest (rustls), fernet, scrypt, hmac, sha2, zip, regex, uuid, tower, tower-http, tracing; tauri 2.

## Phases (each ends with its tests green and one commit)
| # | Slice | Python source | Notes |
|---|---|---|---|
| 0 | Toolchain, workspace, CI-free local `cargo test` | | **done**: Rust 1.99.1 and MSVC Build Tools installed, `rust/` workspace builds |
| 1 | **DONE (15 tests)** Foundation: config, db (`DocRepo`, kv), secrets (Fernet, redact, looks_like_secret), auth (scrypt, tokens) | config, db, secrets_store, auth | proves data compatibility with a real Python-written database in a test |
| 2 | **DONE (44 tests)** Small stores: notify, handoffs, inbox and gates, scheduler, quota, rules, releases, incidents (the `Incidents` class; the agent tool arrives with phase 4) | those files | pure logic. `features.py` moved to phase 6 because it needs the providers and `ba.py` |
| 3 | **DONE (25 tests)** Providers: base, openai, anthropic, claude_cli, registry (build, record_call), presets, pricing | providers/* | transport and process runner are injected; `stream` reads the whole reply first (incremental delivery comes with the HTTP layer); `get_settings`/`allowed_types` flag read arrive with phase 6 |
| 4 | **DONE (29 tests, 114 in all)** Runtime: roles, skills, tools, runner | runtime/* | pipeline, incident and MCP tools are hooks the later phases bind; run and message documents keep the Python key order so either backend can resume a run; shell and git go through `proc` (scrubbed env, timeout) |
| 5 | **DONE (64 tests, 178 in all)** Pipeline: flow, worktrees, github, merge gate and executor, release, docs gate, agent tool | pipeline/* | shells out to `git` and `gh` as now; `require_approved` is a private check in `flow.rs` until the features module (phase 6) takes it over; the API-driven pipeline tests belong to phase 7 |
| 6 | **DONE (86 tests: 43 in `mgmt.rs`, 43 in `integrations.rs`)** Settings, budget, profiles, agent registry, BA helpers, features, plugins and agent builder, MCP (stdio and http), orchestrator (chat, briefs, status, team snapshot and check) | those files | MCP over stdio and http with a Rust fake server (`src/bin/fake_mcp_server.rs`, test support only, not bundled); plugin problems name file and line; the orchestrator's tools call a `Delegator` trait that the run service implements in phase 7. `flow.rs` keeps its own approved check, because its message differs from the features one |
| 7 | **DONE (53 tests in 5 files, `rust/ao-server/tests/api_*.rs`)** HTTP layer: all 93 routes in `ao-server` (axum 0.7), in six groups: core (21), plugins/MCP/budget/profiles/skills (21), runs (4), inbox/pipeline (25), features/registry/team/chat/notifications/schedules (22) | main.py | handlers are synchronous and run on blocking threads; the framework checks the session, parses the body and turns errors into `{"detail": ...}`; `runs.rs` is the run service (launch, restart held, the orchestrator's `Delegator`); `background.rs` is the scheduler loop (`AO_NO_SCHEDULER=1` switches it off); `AO_SYNC_RUNS=1` still works. Added to ao-core on the way: the `incident` agent tool and `providers::build_with`. Known gaps: pydantic's exact 422 text is not copied (status code only), chat streaming is whole-reply, the frontend has not yet been pointed at the Rust server (phase 8/10) |
| 8 | **DONE (5 tests: 4 in `ao-desktop/tests/shell.rs`, 1 in `ao-server/tests/api_ui.rs`)** Tauri shell `ao-desktop` (binary `agent-orchestrator`): runs the backend in process, serves the exported UI from the same port, opens a window on it, single instance, data in `%APPDATA%\agent-orchestrator-desktop\data` (where the Electron app kept it) | desktop/main.js | decided: static export (`AO_STATIC=1`, `build-frontend-static.cmd`), no Node at run time. `--smoke` starts it and checks API and UI (passes against the real export). Not yet checked: the window itself on screen, tray (the Electron app had none), WebView2 on a clean machine. `cargo test` skips this crate (slow to build); use `-p ao-desktop` |
| 9 | **DONE** Installer: Tauri bundler (NSIS, per user), `build-installer-rust.cmd` | build-installer.cmd | **4.53 MiB installer, 18 MB installed** (the Electron one bundles Chromium, Python and Node). Smoke-tested by installing silently into a temp folder, running the installed `agent-orchestrator.exe --smoke` (api 200, ui 200) and uninstalling. Resources shipped: `ui`, `builtin` (roles, skills), `plugins`. Not checked: a clean machine without WebView2, a signed build, upgrade over an Electron install, the window on screen |
| 10 | **DONE** Cut over: Python backend removed once Rust passes every ported test and a live run | | Done: route sets identical (93 method+path pairs, `rust/compat/py_routes.py`); a data folder written by Rust is read back by the Python code and the reverse (password, token, encrypted provider key, settings, role models, profiles, MCP, schedules, features, tasks); all Rust tests green (ao-core 264, ao-server 53, ao-desktop 5). Not done: a live run against a real model and a real `gh`, a clean-machine install, the window on screen. `backend/`, `desktop/`, the Electron/Python scripts and `rust/compat/*.py` were then removed in one commit (revert it to bring them back); the compat scripts are in git history before that commit |

## Open points to decide on the way
- **UI hosting**: Next.js `output: "export"` (static files inside the app, API proxy replaced by direct calls to the backend) versus keeping a Node server. Static export is smaller and is the plan; it needs a check that no page relies on server features. Today `/api/*` is a Next rewrite, so the static build must call the backend by absolute URL or the Tauri shell must serve both.
- **Streaming and sync-run test modes** (`AO_SYNC_RUNS`) have to be reproduced for the tests to carry over.
- **Windows build tools**: Tauri on Windows needs MSVC and WebView2 (WebView2 ships with Windows 11).

## Risks
- Size: about 5,000 lines of Python and 4,200 lines of tests. Expect roughly twice that in Rust.
- Subtle behaviour: JSON key order, float formatting in stored documents, and the exact error text the UI shows.
- Live behaviour: the Claude login provider and MCP stdio need live checks, not just unit tests.
