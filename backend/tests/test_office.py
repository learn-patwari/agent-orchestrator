"""Team redesign: developer 1 and 2 (peer review), shipped role files, and the feed the office view animates."""
from pathlib import Path

import pytest

from app import db
from app.runtime.roles import DEVELOPERS, load_roles
from tests.test_runs_api import completion, setup_role

ROLE_DIR = Path(__file__).resolve().parents[2] / ".development" / "roles"


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def test_shipped_team_has_two_developers_and_no_separate_reviewer():
    roles = load_roles([ROLE_DIR])
    assert {"orchestrator", "architect", "developer_1", "developer_2", "tester", "cicd",
            "incident_reporter", "production_support"} <= set(roles)
    assert "reviewer" not in roles and "developer" not in roles


def test_developers_review_each_other_and_the_architect_reviews_after_them():
    roles = load_roles([ROLE_DIR])
    assert "developer_2" in roles["developer_1"].body and "developer_1" in roles["developer_2"].body
    assert "Verdict" in roles["developer_1"].body
    assert "architect" in roles["architect"].name and "review" in roles["architect"].body.lower()
    assert set(roles["developer_1"].tools) == set(roles["developer_2"].tools)
    assert "git_write" in roles["developer_1"].tools


def test_production_support_fixes_go_to_the_owning_developer_and_production_needs_the_human():
    roles = load_roles([ROLE_DIR])
    assert "developer" in roles["production_support"].body.lower()
    assert "human" in roles["orchestrator"].body.lower() and "production" in roles["orchestrator"].body.lower()
    assert "QC" in roles["tester"].body and "unit" in roles["tester"].body.lower()


def test_every_developer_role_needs_an_approved_srs(authed, tmp_path):
    assert set(DEVELOPERS) == {"developer", "developer_1", "developer_2"}
    pid = setup_role(authed, tmp_path)
    roles = tmp_path / "roles"
    for n in ("developer_1", "developer_2"):
        (roles / f"{n}.md").write_text(f"---\nname: {n}\ndescription: d\n---\nYou develop.")
        authed.put(f"/api/role-models/{n}", json={"provider_id": pid, "model": "m"})
        r = authed.post("/api/runs", json={"role": n, "task": "t", "workspace": str(tmp_path), "wait": True})
        assert r.status_code == 409, n  # no approved feature, no development


def test_office_feed_lists_the_team_and_every_hand_off(authed, tmp_path, monkeypatch):
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    (tmp_path / "roles" / "orchestrator.md").write_text("---\nname: orchestrator\ndescription: boss\n---\nLead.")
    for n in ("developer", "orchestrator"):
        authed.put(f"/api/role-models/{n}", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    import json
    call = [{"id": "t1", "type": "function", "function": {"name": "delegate", "arguments": json.dumps(
        {"role": "developer", "task": "Create hello.txt", "area": "root", "done_criteria": ["exists"],
         "feature_id": fid})}}]
    replies = iter([completion("", call), completion("hello done"), completion("Started.")])
    authed.upstream.handler = lambda req: next(replies)
    assert authed.post("/api/chat", json={"text": "go"}).status_code == 200
    feed = authed.get("/api/office").json()
    assert {t["role"] for t in feed["team"]} >= {"developer", "orchestrator"}
    hs = feed["handoffs"]
    assert [(h["from"], h["to"]) for h in hs] == [("orchestrator", "developer"), ("developer", "orchestrator")]
    assert hs[0]["label"].startswith("Create hello.txt") and hs[1]["label"] == "done"
    assert len({h["id"] for h in hs}) == 2 and hs[0]["at"] <= hs[1]["at"]


def test_office_feed_is_empty_before_any_work(authed, tmp_path):
    setup_role(authed, tmp_path)
    assert authed.get("/api/office").json()["handoffs"] == []
