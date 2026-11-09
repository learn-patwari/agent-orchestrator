"""Skills chosen for a feature are preloaded into the agent's prompt, with a token estimate shown first."""
import json

import pytest

from app import db
from app.runtime.roles import Role
from app.runtime.runner import build_context
from app.runtime.skills import SkillRegistry


def make_skill(root, name, body, desc="d"):
    d = root / "skills" / name
    d.mkdir(parents=True)
    (d / "SKILL.md").write_text(f"---\nname: {name}\ndescription: {desc}\n---\n{body}")


def test_preloaded_skills_are_in_the_system_prompt_and_other_text_is_untouched():
    role = Role("developer", "d", "You develop.")
    ctx = build_context(role, None, [], preload=[("tdd", "WRITE THE TEST FIRST"), ("sql", "USE PARAMS")])
    sys = ctx[0].content
    assert "You develop." in sys and "WRITE THE TEST FIRST" in sys and "USE PARAMS" in sys
    assert sys.index("tdd") < sys.index("WRITE THE TEST FIRST")
    assert "WRITE THE TEST FIRST" not in build_context(role, None, [])[0].content


def test_the_registry_estimates_tokens_and_reports_unknown_names(tmp_path):
    make_skill(tmp_path, "tdd", "x" * 400)
    reg = SkillRegistry([tmp_path / "skills"])
    est = reg.estimate(["tdd", "ghost"])
    assert est["tokens"] == 100 and est["unknown"] == ["ghost"] and est["bodies"] == [("tdd", "x" * 400)]


def test_feature_skills_api_validates_estimates_and_lists_what_is_available(authed, tmp_path):
    from tests.test_runs_api import setup_role
    setup_role(authed, tmp_path)
    make_skill(tmp_path, "tdd", "y" * 800, desc="test first")
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    got = authed.get(f"/api/features/{fid}/skills").json()
    assert got["selected"] == [] and got["tokens"] == 0
    assert [(s["name"], s["tokens"]) for s in got["available"]] == [("tdd", 200)]
    assert authed.put(f"/api/features/{fid}/skills", json={"skills": ["ghost"]}).status_code == 400
    assert authed.put("/api/features/zzz/skills", json={"skills": []}).status_code == 404
    ok = authed.put(f"/api/features/{fid}/skills", json={"skills": ["tdd", "tdd"]}).json()
    assert ok["selected"] == ["tdd"] and ok["tokens"] == 200
    assert authed.put(f"/api/features/{fid}/skills", json={"skills": []}).json()["selected"] == []


def test_a_run_for_the_feature_starts_with_the_skill_text_in_context(authed, tmp_path, monkeypatch):
    from tests.test_runs_api import completion, setup_role
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    make_skill(tmp_path, "tdd", "ALWAYS WRITE THE FAILING TEST FIRST")
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    authed.put(f"/api/features/{fid}/skills", json={"skills": ["tdd"]})
    sent = []

    def handler(req):
        sent.append(json.loads(req.content))
        return completion("done")
    authed.upstream.handler = handler
    authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path), "wait": True,
                                   "feature_id": fid})
    system = sent[0]["messages"][0]["content"]
    assert "ALWAYS WRITE THE FAILING TEST FIRST" in system
    other = db.DocRepo("features").create({"title": "g", "status": "approved"})["id"]
    authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path), "wait": True,
                                   "feature_id": other})
    assert "ALWAYS WRITE" not in sent[1]["messages"][0]["content"]
