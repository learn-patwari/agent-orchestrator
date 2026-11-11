"""Token limit per feature: work holds when the limit is reached and resumes when the user says the window cleared."""
import pytest

from app import db
from app.budget import Budget


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def feature():
    return db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]


def spend(fid, tokens):
    db.DocRepo("runs").create({"feature_id": fid, "tokens": tokens, "status": "done"})


def test_no_limit_never_holds():
    fid = feature()
    spend(fid, 10**9)
    b = Budget()
    assert b.gate(fid) is None
    assert b.status(fid)["limit"] is None and b.status(fid)["held"] is False


def test_the_feature_holds_once_its_runs_reach_the_limit():
    fid = feature()
    b = Budget()
    b.set(fid, 1000)
    spend(fid, 400)
    assert b.gate(fid) is None
    assert b.status(fid)["used"] == 400 and b.status(fid)["remaining"] == 600
    spend(fid, 600)
    assert "token limit" in b.gate(fid)
    assert b.status(fid)["held"] is True
    assert "held" in b.gate(fid) or "token limit" in b.gate(fid)  # stays held on later checks


def test_other_features_are_not_affected():
    a, c = feature(), feature()
    b = Budget()
    b.set(a, 100)
    spend(a, 500)
    spend(c, 500)
    assert b.gate(a) and b.gate(c) is None


def test_resume_gives_a_fresh_allowance_and_clears_the_hold():
    fid = feature()
    b = Budget()
    b.set(fid, 1000)
    spend(fid, 1000)
    assert b.gate(fid)
    b.resume(fid)
    assert b.gate(fid) is None and b.status(fid)["used"] == 0 and b.status(fid)["held"] is False
    spend(fid, 1000)
    assert b.gate(fid)  # the new window has its own limit


def test_tick_resumes_only_after_the_resume_time():
    fid = feature()
    b = Budget()
    b.set(fid, 100, resume_at=5000.0)
    spend(fid, 100)
    assert b.gate(fid)
    assert b.tick(4999.0) == [] and b.status(fid)["held"] is True
    assert b.tick(5000.0) == [fid] and b.status(fid)["held"] is False
    assert b.gate(fid) is None


def test_a_hold_without_a_resume_time_waits_for_a_manual_resume():
    fid = feature()
    b = Budget()
    b.set(fid, 100)
    spend(fid, 100)
    b.gate(fid)
    assert b.tick(10**12) == [] and b.status(fid)["held"] is True


def test_bad_limits_are_refused():
    fid = feature()
    for bad in (0, -5):
        with pytest.raises(ValueError):
            Budget().set(fid, bad)
    with pytest.raises(ValueError):
        Budget().set("nope", 100)
    Budget().set(fid, None)  # removing the limit is allowed


# ---- through the runner and the HTTP API ----

def test_a_run_halts_as_held_at_the_limit_and_continues_after_resume(authed, tmp_path, monkeypatch):
    import json
    from tests.test_runs_api import completion, setup_role
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    assert authed.put(f"/api/features/{fid}/budget", json={"limit": 10}).status_code == 200

    def tool(i):
        return [{"id": f"t{i}", "type": "function", "function": {"name": "write_file", "arguments": json.dumps(
            {"path": f"f{i}.txt", "content": "x"})}}]
    replies = iter([completion("", tool(1)), completion("", tool(2)), completion("all done")])
    authed.upstream.handler = lambda req: next(replies)
    run = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path),
                                         "wait": True, "feature_id": fid}).json()
    assert run["status"] == "held" and "token limit" in run["error"]  # 7 tokens per call, limit 10: stops after 2 calls
    b = authed.get(f"/api/features/{fid}/budget").json()
    assert b["held"] is True and b["used"] >= 10
    again = authed.post("/api/runs", json={"role": "developer", "task": "more", "workspace": str(tmp_path),
                                           "feature_id": fid})
    assert again.status_code == 429 and "held" in again.json()["detail"]
    assert authed.post(f"/api/features/{fid}/resume").json()["held"] is False
    finished = authed.get(f"/api/runs/{run['id']}").json()
    assert finished["status"] == "done" and finished["result"] == "all done"


def test_budget_api_validates_and_reports(authed):
    fid = feature()
    assert authed.get(f"/api/features/{fid}/budget").json()["limit"] is None
    assert authed.put(f"/api/features/{fid}/budget", json={"limit": 0}).status_code == 400
    assert authed.put("/api/features/zzz/budget", json={"limit": 5}).status_code == 404
    ok = authed.put(f"/api/features/{fid}/budget", json={"limit": 5000, "resume_at": 123.0}).json()
    assert ok["limit"] == 5000 and ok["resume_at"] == 123.0
    assert authed.post("/api/features/zzz/resume").status_code == 404


# ---- per-day token limit and the overview ----

def call(tokens, at):
    db.DocRepo("llm_calls").create({"role": "developer", "input_tokens": tokens, "output_tokens": 0, "at": at})


def test_daily_limit_counts_only_the_last_24_hours_and_lifts_by_itself():
    b = Budget()
    now = 1_000_000.0
    assert b.daily(now, None)["limit"] is None and b.daily_gate(now, None) is None
    call(800, now - 100)
    call(5000, now - 90_000)  # older than a day: not counted
    d = b.daily(now, 1000)
    assert d["used"] == 800 and d["percent"] == 80.0
    assert b.daily_gate(now, 1000) is None
    call(300, now - 10)
    assert "daily token limit" in b.daily_gate(now, 1000)
    assert b.daily_gate(now + 86_400, 1000) is None  # the window moved on, no manual resume needed


def test_overview_and_the_daily_limit_block_new_runs(authed, tmp_path):
    import time
    from tests.test_runs_api import setup_role
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "Pay", "status": "approved"})["id"]
    authed.put(f"/api/features/{fid}/budget", json={"limit": 900})
    assert authed.put("/api/settings", json={"daily_token_limit": 1000}).status_code == 200
    call(1000, time.time())
    ov = authed.get("/api/budget").json()
    assert ov["daily"]["limit"] == 1000 and ov["daily"]["used"] == 1000 and ov["daily"]["percent"] == 100.0
    assert [(f["title"], f["limit"]) for f in ov["features"]] == [("Pay", 900)]
    r = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path),
                                       "feature_id": fid})
    assert r.status_code == 429 and "daily token limit" in r.json()["detail"]


def test_a_run_held_by_the_daily_limit_continues_when_the_window_has_room(authed, tmp_path, monkeypatch):
    import json
    from tests.test_runs_api import completion, setup_role
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    authed.put("/api/settings", json={"daily_token_limit": 10})
    step = [{"id": "t1", "type": "function", "function": {"name": "list_dir", "arguments": "{}"}}]
    replies = iter([completion("", step), completion("", step), completion("finished")])
    authed.upstream.handler = lambda req: next(replies)
    run = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path),
                                         "wait": True, "feature_id": fid}).json()
    assert run["status"] == "held" and run["error"].startswith("daily token limit")
    assert authed.app.state.restart_daily() == []  # still over the limit: nothing moves
    authed.put("/api/settings", json={"daily_token_limit": None})
    assert authed.app.state.restart_daily() == [run["id"]]
    assert authed.get(f"/api/runs/{run['id']}").json()["status"] == "done"
