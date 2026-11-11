"""Technical writer agent, common rules for every agent, and the guards: docs gate, merge gate, usage window, Vercel limits."""
import json
import time
from pathlib import Path

import pytest

from app import db
from app.budget import Budget
from app.config import BUILTIN
from app.orchestrator import RULES_FOR_ALL, build_brief, team_snapshot
from app.pipeline.docs_gate import DocsGate, check_docs
from app.pipeline.flow import FlowError, tasks
from app.pipeline.release import Release
from app.runtime.roles import DEFAULT_TOOLS, load_roles
from app.runtime.runner import build_context


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


# ---------------------------------------------------------------- the technical writer and the common rules

def test_the_technical_writer_ships_with_the_right_tools_and_skills():
    roles = load_roles([BUILTIN / "roles"])
    w = roles["technical_writer"]
    assert "people" in w.description.lower() or "document" in w.description.lower()
    assert "pipeline" not in w.tools and "write_file" in w.tools and "git_write" in w.tools
    assert {"writing-guidelines", "documentation-and-adrs", "engineering:documentation",
            "git-workflow-and-versioning", "browser-testing-with-devtools", "anthropic-skills:pdf"} <= set(w.skills)
    body = w.body.lower()
    for must in ("not verified", "never merge", "internal", "release notes", "changelog", "local run", "receptionist"):
        assert must in body, must
    assert DEFAULT_TOOLS["technical-writer"] == DEFAULT_TOOLS["technical_writer"]


def test_every_built_in_role_carries_the_common_rules_and_the_writer_is_in_the_team():
    roles = load_roles([BUILTIN / "roles"])
    assert {"orchestrator", "architect", "developer_1", "developer_2", "tester", "cicd", "incident_reporter",
            "production_support", "technical_writer"} <= set(roles)
    for name, role in roles.items():
        assert "RESPONSIBILITIES" in role.body or name == "orchestrator", name  # the pointer in the role file
        system = build_context(role, None, [])[0].content
        assert "incident_reporter" in system and "technical_writer" in system, name  # the rules are in code too


def test_the_common_rules_say_what_the_orchestrator_asked():
    for must in ("do not fix it", "incident_reporter", "technical_writer", "architect", "Stay in your lane",
                 "never the user's approval", "Not verified"):
        assert must in RULES_FOR_ALL, must


def test_every_brief_carries_the_common_rules():
    brief = build_brief("developer_1", "add login", "backend", ["tests pass"])
    assert RULES_FOR_ALL in brief


def test_the_snapshot_lists_the_writer_and_who_owns_what():
    roles = load_roles([BUILTIN / "roles"])
    s = team_snapshot(roles, {}, {}, [], 0, [])
    assert "technical_writer" in s and "WHO OWNS WHAT" in s
    assert "release notes" in s.lower() and "user guides" in s.lower()


# ---------------------------------------------------------------- docs gate: the checks

def repo(tmp_path, version="1.2.0", notes=None, changelog=None, package=None, backend=None, extra=None):
    (tmp_path / "docs" / "releases").mkdir(parents=True)
    (tmp_path / "docs" / "user-guides").mkdir(parents=True)
    (tmp_path / "docs" / "api").mkdir(parents=True)
    (tmp_path / "frontend").mkdir()
    (tmp_path / "backend").mkdir()
    (tmp_path / "frontend" / "package.json").write_text(json.dumps({"version": package or version}))
    (tmp_path / "backend" / "VERSION").write_text((backend or version) + "\n")
    (tmp_path / "docs" / "releases" / f"v{version}.md").write_text(
        notes if notes is not None else f"# v{version}\n\n## What is new\n- Booking\n\n## Needs your attention\n- None\n")
    (tmp_path / "CHANGELOG.md").write_text(
        changelog if changelog is not None else f"# Changelog\n\n## v{version}\n- Booking\n\n## v1.1.0\n- Old\n")
    for rel, text in (extra or {}).items():
        f = tmp_path / rel
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
    return tmp_path


def test_a_complete_release_passes_the_docs_check(tmp_path):
    r = check_docs(repo(tmp_path), "1.2.0")
    assert r == {"status": "PASS", "failed": []}


def test_missing_notes_and_version_mismatches_fail(tmp_path):
    r = repo(tmp_path, package="1.1.9")
    assert any("frontend/package.json" in x for x in check_docs(r, "1.2.0")["failed"])
    assert any("backend/VERSION" in x for x in check_docs(repo(tmp_path / "b", backend="0.9"), "1.2.0")["failed"])
    out = check_docs(tmp_path / "a", "9.9.9")
    assert out["status"] == "FAIL" and any("v9.9.9.md" in x for x in out["failed"])


def test_the_changelog_entry_must_be_on_top(tmp_path):
    bad = "# Changelog\n\n## v1.1.0\n- Old\n\n## v1.2.0\n- Booking\n"
    assert any("CHANGELOG" in x for x in check_docs(repo(tmp_path, changelog=bad), "1.2.0")["failed"])


def test_every_flagged_pr_needs_its_guide_updated_or_a_gap_with_an_owner(tmp_path):
    r = repo(tmp_path, notes="# v1.2.0\n\n## Guides updated\n- docs/user-guides/admin.md\n\n## Needs your attention\n- None\n",
             extra={"docs/user-guides/admin.md": "# Admin\n"})
    flagged = [{"pr": 12, "guide": "docs/user-guides/admin.md"}, {"pr": 13, "guide": "docs/user-guides/doctor.md"}]
    out = check_docs(r, "1.2.0", flagged=flagged)
    assert out["status"] == "FAIL" and len(out["failed"]) == 1 and "#13" in out["failed"][0]
    ok = check_docs(r, "1.2.0", flagged=flagged, gaps=[{"pr": 13, "owner": "technical_writer"}])
    assert ok["status"] == "PASS"
    no_owner = check_docs(r, "1.2.0", flagged=flagged, gaps=[{"pr": 13, "owner": ""}])
    assert no_owner["status"] == "FAIL"


def test_changed_endpoints_must_appear_in_the_api_docs(tmp_path):
    r = repo(tmp_path, extra={"docs/api/bookings.md": "POST /api/v1/bookings creates one.\n"})
    assert check_docs(r, "1.2.0", endpoints=["/api/v1/bookings"])["status"] == "PASS"
    out = check_docs(r, "1.2.0", endpoints=["/api/v1/bookings", "/api/v1/refunds"])
    assert out["status"] == "FAIL" and "/api/v1/refunds" in out["failed"][0]


def test_broken_links_and_image_paths_fail(tmp_path):
    guide = "# Admin\n![add doctor](images/admin-add-doctor.png)\nSee [the FAQ](faq.md) and [web](https://x.example).\n"
    r = repo(tmp_path, extra={"docs/user-guides/admin.md": guide})
    out = check_docs(r, "1.2.0")
    assert out["status"] == "FAIL"
    assert any("admin-add-doctor.png" in x for x in out["failed"]) and any("faq.md" in x for x in out["failed"])
    assert not any("x.example" in x for x in out["failed"])
    (r / "docs" / "user-guides" / "images").mkdir()
    (r / "docs" / "user-guides" / "images" / "admin-add-doctor.png").write_bytes(b"png")
    (r / "docs" / "user-guides" / "faq.md").write_text("# FAQ\n")
    assert check_docs(r, "1.2.0")["status"] == "PASS"


def test_secrets_in_documents_fail_and_are_never_echoed_back(tmp_path):
    secret = "sk-live-abcdefghijklmnopqrstuvwx"
    r = repo(tmp_path, extra={"docs/user-guides/admin.md": f"# Admin\nlogin key {secret}\n"})
    out = check_docs(r, "1.2.0")
    assert out["status"] == "FAIL" and any("secret" in x.lower() for x in out["failed"])
    assert secret not in json.dumps(out)


def test_not_verified_items_must_be_repeated_under_needs_your_attention(tmp_path):
    bad = "# v1.2.0\n\n## What is new\n- Refunds. Not verified: the email template.\n\n## Needs your attention\n- None\n"
    good = bad.replace("- None", "- Not verified: the email template.")
    assert any("Not verified" in x for x in check_docs(repo(tmp_path / "a", notes=bad), "1.2.0")["failed"])
    assert check_docs(repo(tmp_path / "b", notes=good), "1.2.0")["status"] == "PASS"


# ---------------------------------------------------------------- docs gate: recording and enforcement

def approved_in_qc(tmp_path, release="in_qc"):
    return tasks.create({"name": "t", "status": "approved", "release": release, "owner": "developer_1",
                         "reviewer": "developer_2", "worktree": str(tmp_path), "head_sha": "abcdef1234"})["id"]


def test_the_tester_cannot_sign_off_before_the_docs_gate_is_pass(tmp_path):
    tid = approved_in_qc(tmp_path)
    with pytest.raises(FlowError, match="waiting for docs gate"):
        Release().qc_result(tid, passed=5, failed=0)
    assert tasks.get(tid)["release"] == "in_qc"
    DocsGate().run(tid, "1.2.0")  # the notes are missing in this folder, so it fails
    assert tasks.get(tid)["docs_gate"]["status"] == "FAIL"
    with pytest.raises(FlowError, match="waiting for docs gate"):
        Release().qc_result(tid, passed=5, failed=0)
    repo(tmp_path)
    assert DocsGate().run(tid, "1.2.0")["status"] == "PASS"
    assert Release().qc_result(tid, passed=5, failed=0)["release"] == "qc_passed"


def test_a_failing_qc_report_is_never_blocked_by_the_docs_gate(tmp_path):
    tid = approved_in_qc(tmp_path)
    assert Release().qc_result(tid, passed=3, failed=2)["status"] == "developing"


def test_only_the_human_can_ship_with_a_documented_exception(tmp_path):
    tid = approved_in_qc(tmp_path)
    DocsGate().run(tid, "1.2.0")
    with pytest.raises(FlowError, match="human"):
        DocsGate().exception(tid, "ship it", by="architect")
    with pytest.raises(FlowError, match="reason"):
        DocsGate().exception(tid, "  ", by="human")
    DocsGate().exception(tid, "hotfix, docs follow in 1.2.1", by="human")
    assert Release().qc_result(tid, passed=5, failed=0)["release"] == "qc_passed"
    assert tasks.get(tid)["docs_gate"]["exception"]["reason"].startswith("hotfix")


def test_ci_cd_runs_the_check_and_the_tester_can_not(tmp_path):
    from app.pipeline.agent_tool import PipelineTool
    from app.runtime.tools import ToolBox, ToolError, Workspace
    tid = approved_in_qc(tmp_path)
    repo(tmp_path)

    def act(actor, **kw):
        tb = ToolBox(Workspace(tmp_path), pipeline=PipelineTool(actor, lambda: None))
        return json.loads(tb.call(["pipeline"], "pipeline", kw))
    with pytest.raises(ToolError, match="cicd"):
        act("tester", action="docs_check", task_id=tid, version="1.2.0")
    with pytest.raises(ToolError, match="waiting for docs gate"):
        act("tester", action="qc_result", task_id=tid, passed=5, failed=0)
    assert act("cicd", action="docs_check", task_id=tid, version="1.2.0")["docs_gate"] == "PASS"
    assert act("tester", action="qc_result", task_id=tid, passed=5, failed=0)["release"] == "qc_passed"


def test_docs_gate_api_and_office_show_the_state(authed, tmp_path):
    tid = approved_in_qc(tmp_path)
    r = authed.post(f"/api/pipeline/tasks/{tid}/docs-check", json={"version": "1.2.0"}).json()
    assert r["status"] == "FAIL" and r["failed"]
    assert authed.get("/api/office").json()["docs_gates"] == [{"id": tid, "name": "t", "status": "FAIL"}]
    assert authed.post(f"/api/pipeline/tasks/{tid}/docs-exception", json={"reason": "ok", "by": "architect"}).status_code == 403
    assert authed.post(f"/api/pipeline/tasks/{tid}/docs-exception", json={"reason": "hotfix", "by": "human"}).status_code == 200
    assert authed.get("/api/office").json()["docs_gates"][0]["status"] == "EXCEPTION"
    assert authed.post("/api/pipeline/tasks/zzz/docs-check", json={"version": "1"}).status_code == 404


# ---------------------------------------------------------------- merge gate and approvals

def test_reviewers_merge_only_into_the_integration_branch_never_stage_or_main():
    from app.pipeline.merge_exec import MergeRefused, merge_batch
    from app.pipeline.merge_gate import GateConfig
    for base in ("stage", "main"):
        with pytest.raises(MergeRefused, match="human"):
            merge_batch(None, [1], GateConfig(integration_branch=base), "merge", True)
    from app.pipeline.merge_gate import evaluate
    pr = {"base": "stage", "number": 1, "branch": "x", "changed_files": [], "checks": "success", "reviews": []}
    assert any("not the integration branch" in w for w in evaluate(pr, GateConfig())["reasons"])


def test_nobody_but_the_human_or_the_architect_can_approve_production_and_the_orchestrator_cannot():
    tid = approved_in_qc(Path("."), release="awaiting_prod_approval")
    for who in ("orchestrator", "developer_1", "tester", "technical_writer", "cicd"):
        with pytest.raises(FlowError):
            Release().approve_prod(tid, who)


# ---------------------------------------------------------------- usage-window guard

def test_the_usage_window_guard_stops_at_98_percent_and_lifts_at_the_reset():
    b = Budget()
    now = 1_000_000.0
    assert b.window_gate(now) is None
    b.set_window(97.0, now + 3600)
    assert b.window_gate(now) is None
    b.set_window(98.0, now + 3600)
    why = b.window_gate(now)
    assert "usage window" in why and "98" in why
    assert b.window_gate(now + 3601) is None  # the window has reset
    b.set_window(10.0, now + 3600)
    assert b.window_gate(now) is None


def test_reaching_the_limit_schedules_one_resume_after_the_reset_and_only_one():
    b = Budget()
    now = 1_000_000.0
    b.set_window(99.0, now + 3600)
    b.set_window(99.5, now + 3600)  # same window again: no second job
    jobs = db.DocRepo("schedules").list()
    assert len(jobs) == 1 and jobs[0]["run_at"] == now + 3600 and "usage window" in jobs[0]["name"].lower()
    b.set_window(50.0, now + 9999)  # below the limit: nothing scheduled
    assert len(db.DocRepo("schedules").list()) == 1


def test_a_full_window_blocks_new_runs_and_held_runs_continue_after_it_clears(authed, tmp_path, monkeypatch):
    from tests.test_runs_api import completion, setup_role
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    reset = time.time() + 3600
    assert authed.put("/api/usage-window", json={"percent": 99, "resets_at": reset}).json()["blocked"] is True
    r = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path), "feature_id": fid})
    assert r.status_code == 429 and "usage window" in r.json()["detail"]
    authed.put("/api/usage-window", json={"percent": 20, "resets_at": reset})
    authed.upstream.handler = lambda req: completion("done")
    run = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path),
                                         "wait": True, "feature_id": fid}).json()
    assert run["status"] == "done"
    assert authed.get("/api/usage-window").json()["percent"] == 20


def test_a_run_held_by_the_window_resumes_when_it_clears(authed, tmp_path, monkeypatch):
    from tests.test_runs_api import completion, setup_role
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    step = [{"id": "t1", "type": "function", "function": {"name": "list_dir", "arguments": "{}"}}]
    calls = {"n": 0}

    def handler(req):
        calls["n"] += 1
        if calls["n"] == 1:  # the window fills up while the run is working
            authed.put("/api/usage-window", json={"percent": 99, "resets_at": time.time() + 3600})
            return completion("", step)
        return completion("finished")
    authed.upstream.handler = handler
    run = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path),
                                         "wait": True, "feature_id": fid}).json()
    assert run["status"] == "held" and run["error"].startswith("usage window")
    assert authed.app.state.restart_daily() == []
    authed.put("/api/usage-window", json={"percent": 5, "resets_at": time.time() + 3600})
    assert authed.app.state.restart_daily() == [run["id"]]
    assert authed.get(f"/api/runs/{run['id']}").json()["status"] == "done"


# ---------------------------------------------------------------- Vercel limits guard

def test_deploys_stop_at_70_percent_of_the_daily_vercel_limit_and_are_counted(tmp_path):
    from app.quota import QuotaGuard
    g = QuotaGuard("vercel_hobby")
    for _ in range(69):
        g.record("deployments_per_day")
    tid = approved_in_qc(tmp_path, release="architect_approved")
    assert Release().deploy_qc(tid)["release"] == "in_qc"  # the 70th deployment is allowed and counted
    assert g.usage()["deployments_per_day"]["used"] == 70
    t2 = tasks.create({"name": "t2", "status": "approved", "release": "architect_approved", "owner": "developer_1",
                       "reviewer": "developer_2", "head_sha": "abc1234"})["id"]
    with pytest.raises(FlowError, match="70%"):
        Release().deploy_qc(t2)  # ci-cd stops at 70 and tells the orchestrator
    assert tasks.get(t2)["release"] == "architect_approved"
    assert g.usage()["deployments_per_day"]["used"] == 70  # a refused deploy is not counted
    assert [n for n in db.DocRepo("notifications").list() if n["kind"] == "quota_block"]


def test_production_deploys_are_counted_too(tmp_path):
    from app.quota import QuotaGuard
    tid = tasks.create({"name": "p", "status": "approved", "release": "approved_for_prod", "owner": "developer_1",
                        "reviewer": "developer_2", "approvals": ["human", "architect"], "head_sha": "abc1234"})["id"]
    assert Release().deployed_prod(tid)["release"] == "in_prod"
    assert QuotaGuard("vercel_hobby").usage()["deployments_per_day"]["used"] == 1
