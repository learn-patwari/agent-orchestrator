"""Architecture profiles for token use: a few ship, the person can add their own, the active one drives runs."""
import json

import pytest

from app import db
from app.profiles import ProfileError, Profiles


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def test_three_profiles_ship_and_balanced_is_active_by_default():
    p = Profiles()
    names = [x["name"] for x in p.list()]
    assert names == ["lean", "balanced", "thorough"]
    assert p.active()["name"] == "balanced"
    lean, thorough = p.get("lean"), p.get("thorough")
    assert lean["max_total_tokens"] < p.get("balanced")["max_total_tokens"] < thorough["max_total_tokens"]
    assert p.get("balanced")["builtin"] is True


def test_switching_the_active_profile_changes_the_limits_runs_get():
    p = Profiles()
    p.set_active("lean")
    lim = p.limits()
    assert lim.max_steps == p.get("lean")["max_steps"] and lim.compact_at_chars == p.get("lean")["compact_at_chars"]
    with pytest.raises(ProfileError):
        p.set_active("nope")


def test_a_custom_profile_can_be_added_edited_activated_and_deleted():
    p = Profiles()
    p.save("tight", {"max_steps": 8, "max_total_tokens": 50_000, "max_output_tokens": 512,
                     "compact_at_chars": 10_000, "keep_recent": 3})
    assert p.get("tight")["builtin"] is False and p.get("tight")["est_run_tokens"] == 50_000
    p.save("tight", {"max_steps": 9, "max_total_tokens": 60_000, "max_output_tokens": 512,
                     "compact_at_chars": 10_000, "keep_recent": 3})
    assert p.get("tight")["max_steps"] == 9
    p.set_active("tight")
    assert p.limits().max_output_tokens == 512
    p.delete("tight")
    assert p.active()["name"] == "balanced"  # deleting the active one falls back, never leaves nothing


def test_built_in_profiles_cannot_be_changed_or_deleted_and_bad_values_are_refused():
    p = Profiles()
    good = {"max_steps": 5, "max_total_tokens": 1000, "max_output_tokens": 256, "compact_at_chars": 1000, "keep_recent": 2}
    for name in ("lean", "balanced", "thorough"):
        with pytest.raises(ProfileError, match="built-in"):
            p.save(name, good)
        with pytest.raises(ProfileError, match="built-in"):
            p.delete(name)
    with pytest.raises(ProfileError):
        p.save("x", {**good, "max_steps": 0})
    with pytest.raises(ProfileError):
        p.save("x", {**good, "surprise": 1})
    with pytest.raises(ProfileError):
        p.save("x", {k: v for k, v in good.items() if k != "keep_recent"})
    with pytest.raises(ProfileError):
        p.save("  ", good)


def test_api_lists_activates_and_a_run_uses_the_active_profile(authed, tmp_path, monkeypatch):
    from tests.test_runs_api import completion, setup_role
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    got = authed.get("/api/profiles").json()
    assert got["active"] == "balanced" and [x["name"] for x in got["profiles"]] == ["lean", "balanced", "thorough"]
    assert authed.post("/api/profiles/active", json={"name": "ghost"}).status_code == 404
    custom = {"max_steps": 1, "max_total_tokens": 50_000, "max_output_tokens": 77, "compact_at_chars": 10_000, "keep_recent": 3}
    assert authed.put("/api/profiles/tiny", json=custom).status_code == 200
    assert authed.put("/api/profiles/lean", json=custom).status_code == 400
    assert authed.post("/api/profiles/active", json={"name": "tiny"}).json()["active"] == "tiny"
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    sent = []

    def handler(req):
        sent.append(json.loads(req.content))
        return completion("", [{"id": "t", "type": "function", "function": {"name": "list_dir", "arguments": "{}"}}])
    authed.upstream.handler = handler
    run = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path),
                                         "wait": True, "feature_id": fid}).json()
    assert run["status"] == "halted" and run["error"] == "max_steps"  # one step allowed by the tiny profile
    assert sent[0]["max_tokens"] == 77
    assert authed.delete("/api/profiles/tiny").json()["active"] == "balanced"
