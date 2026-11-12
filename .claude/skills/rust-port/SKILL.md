---
name: rust-port
description: Work on the Rust port of the Agent Orchestrator backend (rust/ workspace, ao-core and ao-server) with few tokens. Use when porting a Python module to Rust, adding or fixing Rust tests, running cargo, or continuing a phase from docs/design/rust-port.md.
---

# Rust port: token-lean workflow

The plan is `docs/design/rust-port.md` (phases 0-10, status per row). Read only that table row for the phase you are on, not the whole doc.

## Loop for one module (tests first)

1. Read the Python source once (`backend/app/<m>.py`) and its test (`backend/tests/test_<m>.py`). Skip the API-driven tests: they belong to phase 7.
2. Write the Rust tests in `rust/ao-core/tests/<group>.rs` mirroring the Python ones. Keep Python error text, JSON key order and status strings identical.
3. Implement in `rust/ao-core/src/<m>.rs`, add `pub mod` to `lib.rs`.
4. Run only that test file, then the whole workspace once at the end.
5. Update the phase row in the plan doc, commit, push `rust-port`.

## Save tokens

- Do not re-read files you just wrote or edited; Edit/Write already confirm.
- Do not read big test files in full; `Grep` for the helper you need (`fn with_env`, `struct Script`) or `Read` with `offset`/`limit`.
- Pipe cargo through a filter: `cargo test -p ao-core --test NAME 2>&1 | grep -E "^test result|FAILED|panicked|error(\[|:)" | head -40`. Add `-A5` only for a failure you are fixing.
- Never print full compiler output; use `cargo check -p ao-core 2>&1 | grep -E "^(error|warning: unused)" -A6 | head -60`.
- Pipeline tests run git and take 40-210 s. Run the full workspace with `run_in_background: true`, read the output file when notified, and do not poll or start a second copy.
- Run a single test with `cargo test -p ao-core --test NAME TESTNAME`.
- Port by reading the Python once and writing complete code; avoid trial-and-error compiles. Most modules compile first time if error types and signatures are decided up front.
- For shared helpers, grep `pub fn` in the target module (`py_str`, `py_dumps`, `resolve_path`, `Inbox::ask_with`, `DocRepo`) instead of reading files.

## Conventions

- Env for cargo in Git Bash: `export PATH="$PATH:$USERPROFILE/.cargo/bin"`; run from `rust/`.
- Storage: `DocRepo::new(table)`; `create(doc, Some(id))` appends `id` last; `update` merges top-level keys. Same SQLite file and tables as Python.
- Python-compatible JSON text: `providers::base::py_dumps`. Keep `serde_json` `preserve_order`.
- Tests that touch the db use a shared `static ENV: Mutex<()>` and a temp `AO_DATA_DIR` (copy `with_env` from `tests/mgmt.rs`).
- Providers, HTTP (`Transport`) and process runners are injected so tests need no network. Fake MCP server: `src/bin/fake_mcp_server.rs` (test support only, never bundled).
- Errors: one `thiserror` enum per module, messages equal to the Python text; `ToolError(String)` for agent tools.
- Phase 7 (axum): the routes call these modules and map errors to the Python status codes and `{"detail": ...}` bodies.

## Windows gotchas

- Use the Write tool instead of multi-heredocs. CRLF warnings from git are harmless.
- Python edits to CRLF files can silently fail; use Edit.
- Compare shell env keys in upper case. Strip `\\?\` from canonicalised paths before giving them to git (`resolve_path`).

## Not yet verified (check before cut-over, phase 10)

Python reading Rust-written data, Python resuming a Rust-started run, real `gh` and network calls, the installer on a clean machine.
