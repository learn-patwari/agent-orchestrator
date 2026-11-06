import time

import httpx

from tests.test_runs_api import completion, setup_role


def provider(authed):
    pid = authed.post("/api/providers", json={"name": "p", "type": "openai", "base_url": "https://x/v1",
                                              "api_key": "sk-test-abcdefghijkl"}).json()["id"]
    return pid


def test_new_endpoints_require_auth(client):
    for path in ("/api/chat", "/api/status", "/api/notifications", "/api/schedules"):
        assert client.get(path).status_code == 401


def test_chat_needs_orchestrator_model_then_works_and_warns(authed):
    assert authed.post("/api/chat", json={"text": "hi"}).status_code == 400
    pid = provider(authed)
    authed.put("/api/role-models/orchestrator", json={"provider_id": pid, "model": "m"})
    authed.upstream.handler = lambda req: completion("On it.")
    r = authed.post("/api/chat", json={"text": "build a thing"}).json()
    assert r["reply"] == "On it." and r["warning"] is None
    r = authed.post("/api/chat", json={"text": "my token sk-abcdefghijklmnopqrstuvwxyz1234"}).json()
    assert r["warning"]
    hist = authed.get("/api/chat").json()
    assert [m["role"] for m in hist] == ["user", "assistant", "user", "assistant"]
    assert "sk-abcdef" not in str(hist)


def test_status_table_endpoint_lists_roles(authed, tmp_path):
    setup_role(authed, tmp_path)
    rows = authed.get("/api/status").json()
    assert {"role": "developer", "state": "idle"}.items() <= rows[0].items()


def test_notifications_list_and_read(authed):
    from app.inbox import Inbox
    Inbox().ask("DB?", "Which?", default="a")
    n = authed.get("/api/notifications").json()
    assert len(n) == 1 and n[0]["read"] is False
    assert authed.post(f"/api/notifications/{n[0]['id']}/read").json()["read"] is True
    assert authed.get("/api/notifications?unread=true").json() == []


def test_schedules_add_and_list(authed):
    r = authed.post("/api/schedules", json={"name": "check-in", "payload": {"kind": "reminder", "text": "continue"},
                                            "run_at": time.time() + 3600})
    assert r.status_code == 200 and r.json()["status"] == "scheduled"
    assert authed.post("/api/schedules", json={"name": "x", "payload": {"kind": "nope"}, "run_at": 1}).status_code == 400
    assert [j["name"] for j in authed.get("/api/schedules").json()] == ["check-in"]


def test_run_workspace_must_be_inside_allowed_roots(authed, tmp_path):
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    allowed = tmp_path / "allowed"
    (allowed / "t1").mkdir(parents=True)
    outside = tmp_path / "outside"
    outside.mkdir()
    authed.put("/api/settings", json={"workspace_roots": [str(allowed)]})
    authed.upstream.handler = lambda req: completion("done")
    body = {"role": "developer", "task": "t", "wait": True}
    assert authed.post("/api/runs", json={**body, "workspace": str(outside)}).status_code == 400
    assert authed.post("/api/runs", json={**body, "workspace": str(allowed / "t1")}).json()["status"] == "done"


def test_default_workspace_root_is_inside_data_dir(authed, tmp_path):
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    authed.put("/api/settings", json={"workspace_roots": []})  # setup_role allowed all of tmp_path
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    r = authed.post("/api/runs", json={"role": "developer", "task": "t", "workspace": str(elsewhere), "wait": True})
    assert r.status_code == 400  # nothing configured: only <data>/workspaces is allowed


def test_daily_budget_cap_blocks_new_runs(authed, tmp_path):
    from app.db import DocRepo
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    ws = tmp_path / "ws"
    ws.mkdir()
    authed.put("/api/settings", json={"workspace_roots": [str(ws)], "daily_budget_usd": 1.0})
    DocRepo("llm_calls").create({"role": "x", "cost_usd": 1.5, "at": time.time()})
    r = authed.post("/api/runs", json={"role": "developer", "task": "t", "workspace": str(ws), "wait": True})
    assert r.status_code == 429 and "budget" in r.json()["detail"].lower()
    assert [n["kind"] for n in authed.get("/api/notifications").json()] == ["budget_pause"]
