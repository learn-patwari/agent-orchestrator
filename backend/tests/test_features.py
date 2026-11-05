import json

import pytest

from app import db
from app.ba import BAError
from app.features import Features, NotApproved, require_approved
from app.inbox import GateBlocked, Inbox, check_gate
from app.providers.base import Completion, Usage

ANALYSIS = {
    "restatement": "A tax report page.", "why": "Finance needs it.",
    "impact": {"modules": ["reports"], "data": ["invoices"], "security": ["role checks"], "risks": ["slow query"]},
    "questions": [
        {"title": "DB?", "body": "Which database?", "options": ["sqlite", "mongo"], "default": "sqlite"},
        {"title": "Format?", "body": "Export format?", "options": ["csv", "pdf"], "default": "csv"},
    ],
}
PLAN = {"phases": [{"id": "P1", "name": "Backend", "tasks": [
    {"id": "t1", "title": "API", "files": ["api.py"], "depends_on": []},
    {"id": "t2", "title": "UI", "files": ["ui.tsx"], "depends_on": ["t1"]}]}]}


class Fake:
    def __init__(self, *texts):
        self.texts, self.seen = list(texts), []

    def complete(self, model, messages, tools=None, max_tokens=0, temperature=None):
        self.seen.append(messages)
        t = self.texts.pop(0)
        return Completion(t if isinstance(t, str) else json.dumps(t), [], Usage(5, 5), "", "m")


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def open_items():
    return Inbox().list("open")


def test_intake_returns_analysis_and_asks_exactly_one_question():
    f = Features(Fake(ANALYSIS), "m").intake("Tax report", "Add a tax report page")
    assert f["status"] == "questioning" and f["analysis"]["restatement"] == "A tax report page."
    items = open_items()
    assert len(items) == 1 and items[0]["title"] == "DB?" and items[0]["default"] == "sqlite"
    assert items[0]["feature_id"] == f["id"]


def test_questions_come_one_at_a_time_and_answers_are_recorded():
    ft = Features(Fake(ANALYSIS), "m")
    f = ft.intake("Tax report", "x")
    q1 = open_items()[0]
    Inbox().answer(q1["id"], "mongo")
    f = ft.sync(Inbox().items.get(q1["id"]))
    assert [i["title"] for i in open_items()] == ["Format?"]  # next question only now
    Inbox().accept_default(open_items()[0]["id"])
    f = ft.sync(Inbox().items.list()[-1])
    assert f["status"] == "ready" and open_items() == []
    assert [(q["question"], q["answer"], q["by"]) for q in f["qa"]] == [
        ("DB?", "mongo", "human"), ("Format?", "csv", "accepted_default")]


def test_decide_with_defaults_runs_through_all_questions():
    ft = Features(Fake(ANALYSIS), "m", decide_with_defaults=True)
    f = ft.intake("Tax report", "x")
    assert f["status"] == "ready" and open_items() == []
    assert [q["by"] for q in f["qa"]] == ["auto_default", "auto_default"]
    assert len(Inbox().assumptions()) == 2


def test_finalize_builds_srs_with_lanes_and_requests_approval():
    ft = Features(Fake(ANALYSIS, PLAN), "m", decide_with_defaults=True)
    f = ft.intake("Tax report", "x")
    f = ft.finalize(f["id"])
    assert f["status"] == "srs_draft" and "# SRS: Tax report" in f["srs_md"] and "[A]" in f["srs_md"]
    tasks = f["plan"][0]["tasks"]
    assert {t["id"]: t["wave"] for t in tasks} == {"t1": 0, "t2": 1}
    gates = [i for i in open_items() if i["kind"] == "approval"]
    assert len(gates) == 1 and gates[0]["action"] == "srs_approval" and gates[0]["feature_id"] == f["id"]
    assert len(db.DocRepo("srs_versions").list()) == 1


def test_finalize_before_questions_are_done_is_refused():
    ft = Features(Fake(ANALYSIS), "m")
    f = ft.intake("Tax report", "x")
    with pytest.raises(ValueError, match="questions"):
        ft.finalize(f["id"])


def test_development_is_blocked_until_srs_approved_by_human():
    ft = Features(Fake(ANALYSIS, PLAN), "m", decide_with_defaults=True)
    f = ft.intake("Tax report", "x")
    ft.finalize(f["id"])
    with pytest.raises(NotApproved):
        require_approved(f["id"])
    gate = [i for i in open_items() if i["kind"] == "approval"][0]
    Inbox().decide(gate["id"], approve=True)
    f = ft.sync(Inbox().items.get(gate["id"]))
    assert f["status"] == "approved"
    require_approved(f["id"])  # no longer raises
    with pytest.raises(NotApproved):
        require_approved("unknown-feature")
    with pytest.raises(NotApproved):
        require_approved(None)


def test_rejecting_srs_keeps_development_blocked():
    ft = Features(Fake(ANALYSIS, PLAN), "m", decide_with_defaults=True)
    f = ft.intake("T", "x")
    ft.finalize(f["id"])
    gate = [i for i in open_items() if i["kind"] == "approval"][0]
    Inbox().decide(gate["id"], approve=False)
    assert ft.sync(Inbox().items.get(gate["id"]))["status"] == "rejected"
    with pytest.raises(NotApproved):
        require_approved(f["id"])


def test_decide_with_defaults_never_approves_the_srs():
    ft = Features(Fake(ANALYSIS, PLAN), "m", decide_with_defaults=True)
    f = ft.intake("T", "x")
    f = ft.finalize(f["id"])
    assert f["status"] == "srs_draft"  # still waiting for a human


def test_bad_model_output_is_reported_not_stored():
    with pytest.raises(BAError):
        Features(Fake("sorry, no json"), "m").intake("T", "x")
    assert db.DocRepo("features").list() == []
    with pytest.raises(BAError):
        Features(Fake({"restatement": "r"}), "m").intake("T", "x")  # missing required keys


def test_at_most_five_questions_and_defaults_must_be_valid():
    many = {**ANALYSIS, "questions": [{"title": f"Q{i}", "body": "b", "options": ["a", "b"], "default": "a"}
                                      for i in range(9)]}
    f = Features(Fake(many), "m").intake("T", "x")
    assert len(f["analysis"]["questions"]) == 5
    bad = {**ANALYSIS, "questions": [{"title": "Q", "body": "b", "options": ["a", "b"], "default": "zzz"}]}
    f = Features(Fake(bad), "m").intake("T", "x")
    assert f["analysis"]["questions"][0]["default"] == "a"  # invalid default falls back to the first option


def test_pasted_secret_in_request_is_not_sent_to_the_model():
    fk = Fake(ANALYSIS)
    Features(fk, "m").intake("T", "use key sk-abcdefghijklmnopqrstuvwxyz1234 please")
    assert all("sk-abcdef" not in m.content for msgs in fk.seen for m in msgs)


def test_sync_is_idempotent_and_never_marks_ready_while_a_question_is_open():
    ft = Features(Fake(ANALYSIS), "m")
    ft.intake("T", "x")
    q1 = open_items()[0]
    Inbox().answer(q1["id"], "mongo")
    ft.sync(Inbox().items.get(q1["id"]))
    f = ft.sync(Inbox().items.get(q1["id"]))  # duplicate delivery (retry, double click)
    assert f["status"] == "questioning" and [i["title"] for i in open_items()] == ["Format?"]
    assert len(f["qa"]) == 1
