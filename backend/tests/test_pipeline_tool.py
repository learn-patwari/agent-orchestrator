"""The `pipeline` tool: how the agents themselves move a task along, with authority enforced per role."""
import json

import pytest

from app import db
from app.pipeline.agent_tool import PipelineTool
from app.runtime.roles import DEFAULT_TOOLS
from app.runtime.tools import ToolBox, ToolError, Workspace
from tests.test_flow import H1, flow, review  # noqa: F401  (flow is a fixture)


def box(tmp_path, actor, flow_):
    return ToolBox(Workspace(tmp_path), pipeline=PipelineTool(actor, lambda: flow_))


def act(tb, action, **kw):
    return json.loads(tb.call(["pipeline"], "pipeline", {"action": action, **kw}))


def test_every_role_that_moves_work_has_the_tool_and_nobody_else_does():
    for r in ("developer_1", "developer_2", "architect", "tester", "cicd"):
        assert "pipeline" in DEFAULT_TOOLS[r], r
    for r in ("incident_reporter", "production_support", "orchestrator"):
        assert "pipeline" not in DEFAULT_TOOLS[r], r
    assert "pipeline" in ToolBox(Workspace(__import__("pathlib").Path(".")), None).specs


def test_a_developer_reports_work_done_and_the_other_developer_reviews_it(flow, tmp_path):
    t = flow.start(flow.approved_id, "t1", owner="developer_1")
    d1, d2 = box(tmp_path, "developer_1", flow), box(tmp_path, "developer_2", flow)
    with pytest.raises(ToolError, match="owner"):
        act(d2, "dev_done", task_id=t["id"], pr_number=5, head_sha=H1)
    assert act(d1, "dev_done", task_id=t["id"], pr_number=5, head_sha=H1)["status"] == "dev_done"
    assert act(d1, "tests", task_id=t["id"], passed=6, failed=0, area="x")["status"] == "tested"
    with pytest.raises(ToolError, match="peer"):
        act(d1, "review", task_id=t["id"], body=review("APPROVE", H1))  # nobody approves their own work
    assert act(d2, "review", task_id=t["id"], body=review("APPROVE", H1))["status"] == "approved"


def test_each_release_step_belongs_to_one_role(flow, tmp_path):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 4, 0, "x")
    flow.record_review(t["id"], review("APPROVE", H1))
    tid = t["id"]
    arch, ci, qc = box(tmp_path, "architect", flow), box(tmp_path, "cicd", flow), box(tmp_path, "tester", flow)
    dev = box(tmp_path, "developer_1", flow)
    with pytest.raises(ToolError, match="architect"):
        act(dev, "architect_review", task_id=tid, approve=True)
    assert act(arch, "architect_review", task_id=tid, approve=True)["release"] == "architect_approved"
    with pytest.raises(ToolError, match="cicd"):
        act(arch, "deploy_qc", task_id=tid)
    assert act(ci, "deploy_qc", task_id=tid)["release"] == "in_qc"
    with pytest.raises(ToolError, match="tester"):
        act(ci, "qc_result", task_id=tid, passed=3, failed=0)
    assert act(qc, "qc_result", task_id=tid, passed=3, failed=0)["release"] == "qc_passed"


def test_the_architect_signs_off_production_but_can_never_stand_in_for_the_human(flow, tmp_path):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 4, 0, "x")
    flow.record_review(t["id"], review("APPROVE", H1))
    from app.pipeline.release import Release
    r = Release()
    r.architect_review(t["id"], True)
    r.deploy_qc(t["id"])
    r.qc_result(t["id"], 3, 0)
    r.request_prod(t["id"])
    arch, ci = box(tmp_path, "architect", flow), box(tmp_path, "cicd", flow)
    out = act(arch, "approve_prod", task_id=t["id"])
    assert out["release"] == "awaiting_prod_approval" and out["approvals"] == ["architect"]  # the human still decides
    with pytest.raises(ToolError):
        act(ci, "deploy_prod", task_id=t["id"])  # not approved for production yet
    r.approve_prod(t["id"], "human")  # only the person in the UI can do this
    assert act(ci, "deploy_prod", task_id=t["id"])["release"] == "in_prod"
    assert "human" in db.DocRepo("tasks").get(t["id"])["approvals"]


def test_list_and_bad_calls_fail_as_tool_errors(flow, tmp_path):
    t = flow.start(flow.approved_id, "t1")
    tb = box(tmp_path, "developer_1", flow)
    rows = json.loads(tb.call(["pipeline"], "pipeline", {"action": "list"}))
    assert rows[0]["id"] == t["id"] and rows[0]["owner"] == "developer_1"
    for bad in ({"action": "nope"}, {"action": "dev_done"}, {"action": "dev_done", "task_id": "zzz", "pr_number": 1, "head_sha": H1}):
        with pytest.raises(ToolError):
            tb.call(["pipeline"], "pipeline", bad)
    with pytest.raises(ToolError, match="not allowed"):
        tb.call(["read_file"], "pipeline", {"action": "list"})
    with pytest.raises(ToolError, match="not available"):
        ToolBox(Workspace(tmp_path)).call(["pipeline"], "pipeline", {"action": "list"})


def _task(status="developing", release=None, **kw):
    return db.DocRepo("tasks").create({"name": "t", "status": status, "owner": "developer_1", "reviewer": "developer_2",
                                       "release": release, "rounds": 0, "tests": None, "pr_number": None,
                                       "head_sha": None, **kw})


def test_a_real_agent_run_moves_the_task_and_the_hand_off_shows_up_in_the_office_feed(authed, tmp_path):
    from tests.test_runs_api import completion, setup_role
    pid = setup_role(authed, tmp_path)
    (tmp_path / "roles" / "developer_1.md").write_text("---\nname: developer_1\ndescription: d\n---\nYou develop.")
    authed.put("/api/role-models/developer_1", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    t = _task()
    call = [{"id": "c1", "type": "function", "function": {"name": "pipeline", "arguments": json.dumps(
        {"action": "dev_done", "task_id": t["id"], "pr_number": 9, "head_sha": H1})}}]
    replies = iter([completion("", call), completion("reported")])
    authed.upstream.handler = lambda req: next(replies)
    run = authed.post("/api/runs", json={"role": "developer_1", "task": "go", "workspace": str(tmp_path),
                                         "wait": True, "feature_id": fid}).json()
    assert run["status"] == "done"
    assert db.DocRepo("tasks").get(t["id"])["status"] == "dev_done"
    hs = authed.get("/api/office").json()["handoffs"]
    assert [(h["from"], h["to"]) for h in hs if h["label"] == "peer review"] == [("developer_1", "developer_2")]


def test_the_orchestrator_asks_the_human_for_production_but_only_after_qc(authed, tmp_path, monkeypatch):
    from tests.test_runs_api import completion, setup_role
    pid = setup_role(authed, tmp_path)
    (tmp_path / "roles" / "orchestrator.md").write_text("---\nname: orchestrator\ndescription: boss\n---\nLead.")
    authed.put("/api/role-models/orchestrator", json={"provider_id": pid, "model": "m"})
    early, ready = _task(status="approved"), _task(status="approved", release="qc_passed")

    def call(tid):
        return [{"id": "c", "type": "function", "function": {"name": "request_production",
                                                              "arguments": json.dumps({"task_id": tid})}}]
    replies = iter([completion("", call(early["id"])), completion("not yet"), completion("", call(ready["id"])),
                    completion("asked")])
    authed.upstream.handler = lambda req: next(replies)
    assert authed.post("/api/chat", json={"text": "ship it"}).status_code == 200
    assert db.DocRepo("tasks").get(early["id"])["release"] is None  # refused: QC has not signed off
    assert authed.post("/api/chat", json={"text": "and the other"}).status_code == 200
    assert db.DocRepo("tasks").get(ready["id"])["release"] == "awaiting_prod_approval"
    assert [i["kind"] for i in db.DocRepo("inbox").list()].count("approval") == 1


def test_the_tester_reports_production_tests_and_a_failure_raises_an_incident(flow, tmp_path):
    t = flow.start(flow.approved_id, "t1")
    tester, dev = box(tmp_path, "tester", flow), box(tmp_path, "developer_1", flow)
    with pytest.raises(ToolError, match="tester"):
        act(dev, "prod_tests", task_id=t["id"], passed=1, failed=1, area="smoke")
    act(tester, "prod_tests", task_id=t["id"], passed=5, failed=0, area="smoke")
    assert db.DocRepo("incidents").list() == []
    act(tester, "prod_tests", task_id=t["id"], passed=3, failed=2, area="smoke")
    assert [i["level"] for i in db.DocRepo("incidents").list()] == [1]
