import subprocess

import pytest
from fastapi.testclient import TestClient

from app import db
from app.main import create_app

H1 = "1" * 40


def git(cwd, *a):
    subprocess.run(["git", *a], cwd=cwd, check=True, capture_output=True)


class FakeGh:
    def __init__(self, tests_lookup):
        self.tests_lookup, self.calls = tests_lookup, []
        self.prs = {5: {"number": 5, "branch": "task/t1", "base": "internal", "head_sha": H1, "ci": "success",
                        "mergeable": True, "changed_files": ["a.py"],
                        "reviews": [{"verdict": "APPROVE", "head_sha": H1, "at": 1, "findings": []}]}}

    def facts(self, n):
        return {**self.prs[n], "tests": self.tests_lookup(n)}

    def retarget(self, n, base):
        self.calls.append(("retarget", n, base))

    def merge(self, cmd):
        self.calls.append(("merge", cmd))


@pytest.fixture
def api(env, tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    git(repo, "init", "-q", "-b", "internal")
    git(repo, "config", "user.email", "t@t")
    git(repo, "config", "user.name", "t")
    (repo / "a").write_text("a")
    git(repo, "add", "a")
    git(repo, "commit", "-q", "-m", "i")
    holder = {}

    def factory(repo_slug, tests_lookup):
        holder["gh"] = FakeGh(tests_lookup)
        return holder["gh"]

    with TestClient(create_app(gh_factory=factory)) as c:
        c.post("/api/auth/setup", json={"password": "correct-horse-1"})
        c.repo, c.holder, c.tmp = repo, holder, tmp_path
        db.init_db()
        c.fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
        yield c


def configure(api, **extra):
    api.put("/api/settings", json={"repo_path": str(api.repo), "worktree_root": str(api.tmp / "wt"),
                                   "github_repo": "o/r", **extra})


def test_requires_auth(env):
    with TestClient(create_app()) as c:
        assert c.get("/api/pipeline/tasks").status_code == 401
        assert c.post("/api/pipeline/merge", json={"numbers": [1]}).status_code == 401


def test_start_needs_repo_configured_and_approved_feature(api):
    body = {"feature_id": api.fid, "name": "t1"}
    assert api.post("/api/pipeline/tasks", json=body).status_code == 400
    configure(api)
    draft = db.DocRepo("features").create({"title": "d", "status": "srs_draft"})["id"]
    assert api.post("/api/pipeline/tasks", json={**body, "feature_id": draft}).status_code == 409
    r = api.post("/api/pipeline/tasks", json=body)
    assert r.status_code == 200 and r.json()["branch"] == "task/t1"
    assert [t["name"] for t in api.get("/api/pipeline/tasks").json()] == ["t1"]


def test_full_task_lifecycle_over_api(api):
    configure(api)
    t = api.post("/api/pipeline/tasks", json={"feature_id": api.fid, "name": "t1"}).json()
    base = f"/api/pipeline/tasks/{t['id']}"
    assert api.post(f"{base}/dev-done", json={"pr_number": 5, "head_sha": H1}).json()["status"] == "dev_done"
    assert api.post(f"{base}/tests", json={"passed": 4, "failed": 0, "area": "x"}).json()["status"] == "tested"
    assert api.post(f"{base}/review", json={"body": "looks fine"}).status_code == 409
    r = api.post(f"{base}/review", json={"body": f"Verdict: APPROVE\nHead: {H1}"})
    assert r.json()["status"] == "approved"


def test_merge_is_off_by_default_then_merges_and_marks_tasks(api):
    configure(api)
    t = api.post("/api/pipeline/tasks", json={"feature_id": api.fid, "name": "t1"}).json()
    base = f"/api/pipeline/tasks/{t['id']}"
    api.post(f"{base}/dev-done", json={"pr_number": 5, "head_sha": H1})
    api.post(f"{base}/tests", json={"passed": 4, "failed": 0, "area": "x"})
    api.post(f"{base}/review", json={"body": f"Verdict: APPROVE\nHead: {H1}"})
    assert api.post("/api/pipeline/merge", json={"numbers": [5]}).status_code == 403  # authority off
    api.put("/api/settings", json={"reviewer_can_merge": True})
    out = api.post("/api/pipeline/merge", json={"numbers": [5]}).json()
    assert [m["number"] for m in out["merged"]] == [5] and out["refused"] == []
    cmd = api.holder["gh"].calls[0][1]
    assert cmd[-2:] == ["--match-head-commit", H1] and "--merge" in cmd
    assert api.get("/api/pipeline/tasks").json()[0]["status"] == "merged"


def test_merge_never_targets_main_even_with_authority_on(api):
    configure(api, reviewer_can_merge=True, integration_branch="main")
    assert api.post("/api/pipeline/merge", json={"numbers": [5]}).status_code == 403


def test_merge_without_recorded_tests_is_refused_by_the_gate(api):
    configure(api, reviewer_can_merge=True)
    out = api.post("/api/pipeline/merge", json={"numbers": [5]}).json()  # no task/tests recorded for PR 5
    assert out["merged"] == [] and any("tests" in r for r in out["refused"][0]["reasons"])


def _to_approved(api, owner=None):
    configure(api)
    body = {"feature_id": api.fid, "name": "t1", **({"owner": owner} if owner else {})}
    t = api.post("/api/pipeline/tasks", json=body).json()
    base = f"/api/pipeline/tasks/{t['id']}"
    api.post(f"{base}/dev-done", json={"pr_number": 5, "head_sha": H1})
    api.post(f"{base}/tests", json={"passed": 4, "failed": 0, "area": "x"})
    api.post(f"{base}/review", json={"body": f"Verdict: APPROVE\nHead: {H1}"})
    return base


def test_release_over_api_shows_in_the_office_feed_and_waits_for_the_human(api):
    base = _to_approved(api, owner="developer_2")
    assert api.post(f"{base}/request-prod").status_code == 409  # cannot skip QC
    assert api.post(f"{base}/architect", json={"approve": True}).json()["release"] == "architect_approved"
    assert api.post(f"{base}/deploy-qc").json()["release"] == "in_qc"
    assert api.get("/api/office").json()["environments"]["qc"]["version"] == H1[:7]
    from app.pipeline.flow import tasks
    tasks.update(base.rsplit("/", 1)[1], {"docs_gate": {"status": "PASS", "failed": []}})
    assert api.post(f"{base}/qc", json={"passed": 6, "failed": 0}).json()["release"] == "qc_passed"
    assert api.post(f"{base}/request-prod").json()["release"] == "awaiting_prod_approval"
    feed = api.get("/api/office").json()
    assert feed["pending_prod"] == [{"id": base.rsplit("/", 1)[1], "name": "t1", "approvals": []}]
    assert api.post(f"{base}/approve-prod", json={"by": "human"}).json()["release"] == "awaiting_prod_approval"
    assert api.post(f"{base}/deploy-prod").status_code == 409  # still needs the architect
    assert api.post(f"{base}/approve-prod", json={"by": "architect"}).json()["release"] == "approved_for_prod"
    assert api.post(f"{base}/deploy-prod").json()["release"] == "in_prod"
    feed = api.get("/api/office").json()
    assert feed["environments"]["prod"]["name"] == "t1" and feed["pending_prod"] == []
    assert ("developer_2", "developer_1") in [(h["from"], h["to"]) for h in feed["handoffs"]]
    assert feed["handoffs"][-1]["to"] == "cicd"


def test_release_endpoints_need_auth(env):
    with TestClient(create_app()) as c:
        assert c.post("/api/pipeline/tasks/x/architect", json={"approve": True}).status_code == 401
        assert c.post("/api/pipeline/tasks/x/approve-prod", json={"by": "human"}).status_code == 401
