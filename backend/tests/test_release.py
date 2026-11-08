"""Release stages after peer review: architect review, QC, human production gate, environments and hand-offs."""
import pytest

from app import db
from app.inbox import Inbox
from app.pipeline.flow import Flow, FlowError
from app.pipeline.release import Release
from tests.test_flow import H1, flow, review  # noqa: F401  (flow is a fixture)


def handoffs():
    return [(h["from"], h["to"]) for h in sorted(db.DocRepo("handoffs").list(), key=lambda h: h["at"])]


def approved_task(flow, owner="developer_1", name="t1"):
    t = flow.start(flow.approved_id, name, owner=owner)
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 4, 0, "x")
    return flow.record_review(t["id"], review("APPROVE", H1))


def test_the_other_developer_is_the_peer_reviewer_and_hand_offs_are_recorded(flow):
    t = flow.start(flow.approved_id, "t1", owner="developer_2")
    assert t["owner"] == "developer_2" and t["reviewer"] == "developer_1"
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 4, 0, "x")
    flow.record_review(t["id"], review("APPROVE", H1))
    assert handoffs() == [("developer_2", "developer_1"), ("developer_1", "architect")]


def test_request_changes_hands_the_work_back_to_the_owner(flow):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 4, 0, "x")
    flow.record_review(t["id"], review("REQUEST_CHANGES", H1))
    assert handoffs()[-1] == ("developer_2", "developer_1")


def test_owner_must_be_a_developer(flow):
    with pytest.raises(FlowError, match="developer"):
        flow.start(flow.approved_id, "t1", owner="tester")


def test_full_release_to_production_needs_architect_qc_human_and_architect_sign_off(flow):
    t = approved_task(flow)
    r = Release()
    assert r.architect_review(t["id"], True)["release"] == "architect_approved"
    assert r.deploy_qc(t["id"])["release"] == "in_qc"
    assert r.environments()["qc"]["task_id"] == t["id"] and r.environments()["prod"] is None
    assert r.qc_result(t["id"], passed=12, failed=0)["release"] == "qc_passed"
    t = r.request_prod(t["id"])
    assert t["release"] == "awaiting_prod_approval"
    assert [i["kind"] for i in Inbox().list("open")].count("approval") == 1  # the human is asked once
    assert r.approve_prod(t["id"], "human")["release"] == "awaiting_prod_approval"  # still needs the architect
    assert r.approve_prod(t["id"], "architect")["release"] == "approved_for_prod"
    assert r.environments()["prod"] is None  # approved is not deployed
    assert r.deployed_prod(t["id"])["release"] == "in_prod"
    assert r.environments()["prod"]["task_id"] == t["id"]
    assert handoffs()[2:] == [("architect", "cicd"), ("cicd", "tester"), ("tester", "orchestrator"),
                              ("orchestrator", "architect"), ("architect", "cicd")]


def test_stages_cannot_be_skipped(flow):
    t = flow.start(flow.approved_id, "t1")
    r = Release()
    with pytest.raises(FlowError):
        r.architect_review(t["id"], True)  # not peer approved yet
    t = approved_task(flow, name="t2")
    for step in (lambda: r.deploy_qc(t["id"]), lambda: r.qc_result(t["id"], 1, 0),
                 lambda: r.request_prod(t["id"]), lambda: r.approve_prod(t["id"], "human"),
                 lambda: r.deployed_prod(t["id"])):
        with pytest.raises(FlowError):
            step()


def test_only_the_human_and_the_architect_can_approve_production(flow):
    t = approved_task(flow)
    r = Release()
    r.architect_review(t["id"], True)
    r.deploy_qc(t["id"])
    r.qc_result(t["id"], 3, 0)
    r.request_prod(t["id"])
    with pytest.raises(FlowError, match="human or the architect"):
        r.approve_prod(t["id"], "orchestrator")


def test_failed_qc_sends_the_work_back_to_the_owning_developer(flow):
    t = approved_task(flow, owner="developer_2")
    r = Release()
    r.architect_review(t["id"], True)
    r.deploy_qc(t["id"])
    out = r.qc_result(t["id"], passed=9, failed=2)
    assert out["status"] == "developing" and out["release"] is None
    assert handoffs()[-1] == ("tester", "developer_2")


def test_architect_can_send_code_back(flow):
    t = approved_task(flow)
    out = Release().architect_review(t["id"], False)
    assert out["status"] == "developing" and out["release"] is None
    assert handoffs()[-1] == ("architect", "developer_1")
