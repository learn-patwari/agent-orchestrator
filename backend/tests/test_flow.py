import subprocess

import pytest

from app import db
from app.inbox import Inbox
from app.pipeline.flow import FlowError, Flow
from app.pipeline.worktrees import WorktreeManager

H1, H2 = "1" * 40, "2" * 40


def git(cwd, *a):
    subprocess.run(["git", *a], cwd=cwd, check=True, capture_output=True)


@pytest.fixture
def flow(env, tmp_path):
    db.init_db()
    repo = tmp_path / "repo"
    repo.mkdir()
    git(repo, "init", "-q", "-b", "internal")
    git(repo, "config", "user.email", "t@t")
    git(repo, "config", "user.name", "t")
    (repo / "a").write_text("a")
    git(repo, "add", "a")
    git(repo, "commit", "-q", "-m", "i")
    approved = db.DocRepo("features").create({"title": "f", "status": "approved"})
    draft = db.DocRepo("features").create({"title": "d", "status": "srs_draft"})
    f = Flow(WorktreeManager(repo, tmp_path / "wt"), integration="internal", review_round_limit=2)
    f.approved_id, f.draft_id = approved["id"], draft["id"]
    return f


def review(verdict, head):
    return f"Verdict: {verdict}\nHead: {head}\n"


def test_task_needs_an_approved_feature_and_gets_its_own_worktree(flow):
    with pytest.raises(FlowError, match="approved"):
        flow.start(flow.draft_id, "t1")
    t = flow.start(flow.approved_id, "t1")
    assert t["status"] == "developing" and t["branch"] == "task/t1" and t["rounds"] == 0
    assert t["worktree"] and t["base"] == "internal"


def test_stacked_task_branches_from_its_base_task(flow):
    a = flow.start(flow.approved_id, "a")
    b = flow.start(flow.approved_id, "b", stacked_on=a["id"])
    assert b["base"] == a["branch"]


def test_happy_path_to_approved(flow):
    t = flow.start(flow.approved_id, "t1")
    t = flow.dev_done(t["id"], pr_number=5, head_sha=H1)
    assert t["status"] == "dev_done" and t["pr_number"] == 5
    t = flow.record_tests(t["id"], passed=10, failed=0, area="inbox")
    assert t["status"] == "tested"
    t = flow.record_review(t["id"], review("APPROVE", H1))
    assert t["status"] == "approved" and t["rounds"] == 0


def test_failed_tests_send_the_task_back_to_the_developer(flow):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 5, H1)
    t = flow.record_tests(t["id"], passed=8, failed=2, area="x")
    assert t["status"] == "developing" and t["tests"]["failed"] == 2


def test_request_changes_goes_back_to_the_pr_author_and_counts_rounds(flow):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 3, 0, "x")
    t = flow.record_review(t["id"], review("REQUEST_CHANGES", H1))
    assert t["status"] == "changes_requested" and t["rounds"] == 1
    t = flow.dev_done(t["id"], 5, H2)  # author pushed fixes
    assert t["status"] == "dev_done" and t["head_sha"] == H2
    flow.record_tests(t["id"], 3, 0, "x")
    t = flow.record_review(t["id"], review("APPROVE", H2))
    assert t["status"] == "approved"


def test_review_round_limit_escalates_to_the_human(flow):
    t = flow.start(flow.approved_id, "t1")
    for head in (H1, H2):
        flow.dev_done(t["id"], 5, head)
        flow.record_tests(t["id"], 3, 0, "x")
        t = flow.record_review(t["id"], review("REQUEST_CHANGES", head))
    assert t["status"] == "escalated" and t["rounds"] == 2
    assert [i["kind"] for i in Inbox().list("open")] == ["status"]
    with pytest.raises(FlowError):
        flow.dev_done(t["id"], 5, "3" * 40)  # no more automatic rounds


def test_hold_stops_the_task_and_notifies(flow):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 3, 0, "x")
    t = flow.record_review(t["id"], review("HOLD", H1))
    assert t["status"] == "held"


def test_invalid_review_text_and_illegal_transitions_are_rejected(flow):
    t = flow.start(flow.approved_id, "t1")
    with pytest.raises(FlowError, match="state"):
        flow.record_review(t["id"], review("APPROVE", H1))  # not tested yet
    flow.dev_done(t["id"], 5, H1)
    flow.record_tests(t["id"], 3, 0, "x")
    with pytest.raises(FlowError, match="verdict"):
        flow.record_review(t["id"], "looks fine to me")
    t2 = flow.start(flow.approved_id, "t2")
    with pytest.raises(FlowError, match="state"):
        flow.record_tests(t2["id"], 1, 0, "x")  # no PR yet
    with pytest.raises(FlowError):
        flow.dev_done("nope", 1, H1)


def test_review_on_a_stale_head_does_not_count_as_approval(flow):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 5, H2)
    flow.record_tests(t["id"], 3, 0, "x")
    with pytest.raises(FlowError, match="head"):
        flow.record_review(t["id"], review("APPROVE", H1))  # review of an older commit


def test_merged_marks_task_done_and_tests_lookup_is_available(flow):
    t = flow.start(flow.approved_id, "t1")
    flow.dev_done(t["id"], 7, H1)
    flow.record_tests(t["id"], 3, 0, "x")
    flow.record_review(t["id"], review("APPROVE", H1))
    assert flow.tests_for_pr(7) == {"passed": 3, "failed": 0, "area": "x"}
    assert flow.tests_for_pr(99) is None
    assert flow.mark_merged(7)["status"] == "merged"
