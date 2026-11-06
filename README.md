# Agent Orchestrator

Standalone web app that runs a team of AI agents for software development. See `docs/srs/orchestrator-app.md` (requirements and plan) and `docs/PROGRESS.md` (current state).

## Run locally
Needs Python 3.12 and Node 20+.

```bash
cd backend && pip install -r requirements-dev.txt && python -m pytest -q
cd backend && python -m uvicorn app.main:app --port 8000
cd frontend && npm ci && npm run build && npm start
```

Open http://localhost:3000, set a password on first run, then add an LLM provider (base URL + API key) and use Test connection. Data is stored in `backend/data/` (SQLite, encrypted API keys); override with `AO_DATA_DIR`. Set `AO_SECRET_KEY` (a Fernet key) to keep the encryption key outside the data folder.

## Layout
`backend/` FastAPI + SQLite repository layer, auth, settings, LLM provider adapters. `frontend/` Next.js UI (proxies `/api` to the backend, `AO_BACKEND_URL`). `docs/` SRS, plans, progress. `reference/` read-only source material from the BookMeDoc workflow.

## Windows desktop app (test build)
Double-click `run-desktop.cmd` (or run it in a terminal). First run installs Python and Node packages and builds the UI, then opens the app window. Needs Python 3.12 and Node 20+ on PATH (or set `AO_PYTHON`). The app uses ports 8765 (backend) and 3765 (UI); data and logs are under `%APPDATA%\agent-orchestrator-desktop\`. `cd desktop && npm run smoke` starts everything, checks it, and exits. This is a runnable shell, not yet an installer: bundling Python and producing a `.exe` installer is still to do.
