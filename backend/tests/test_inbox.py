import pytest

from app import db
from app.inbox import GATED, GateBlocked, Inbox, check_gate, request_gate

OPTS = ["sqlite", "mongo"]


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def test_question_answered_explicitly_records_no_assumption():
    ib = Inbox()
    q = ib.ask("DB?", "Which database?", options=OPTS, default="sqlite")
    assert q["status"] == "open"
    done = ib.answer(q["id"], "mongo")
    assert done["status"] == "resolved" and done["answer"] == "mongo" and done["resolved_by"] == "human"
    assert ib.assumptions() == []


def test_accept_default_records_assumption():
    ib = Inbox()
    q = ib.ask("DB?", "Which database?", options=OPTS, default="sqlite")
    ib.accept_default(q["id"])
    a = ib.assumptions()
    assert len(a) == 1 and a[0]["assumed"] == "sqlite" and a[0]["status"] == "assumed" and a[0]["item_id"] == q["id"]


def test_decide_with_defaults_mode_auto_resolves_questions_but_not_gates():
    ib = Inbox(decide_with_defaults=True)
    q = ib.ask("DB?", "Which database?", options=OPTS, default="sqlite")
    assert q["status"] == "resolved" and q["resolved_by"] == "auto_default"
    assert ib.assumptions()[0]["assumed"] == "sqlite"
    no_default = ib.ask("Name?", "Product name?")
    assert no_default["status"] == "open"  # nothing to assume
    gate = request_gate("merge_main", "merge PR 5 into main", decide_with_defaults=True)
    assert gate["status"] == "open"  # gates always wait for a human


def test_overrule_assumption_later():
    ib = Inbox()
    q = ib.ask("DB?", "Which?", options=OPTS, default="sqlite")
    ib.accept_default(q["id"])
    a = ib.assumptions()[0]
    out = ib.overrule(a["id"], "mongo")
    assert out["status"] == "overruled" and out["assumed"] == "sqlite" and out["overruled_to"] == "mongo"


def test_cannot_resolve_twice_or_answer_unknown():
    ib = Inbox()
    q = ib.ask("x", "x", default="a")
    ib.answer(q["id"], "b")
    with pytest.raises(ValueError):
        ib.answer(q["id"], "c")
    with pytest.raises(KeyError):
        ib.answer("nope", "c")
    with pytest.raises(ValueError):
        ib.accept_default(ib.ask("y", "y")["id"])  # no default to accept


def test_gate_blocks_until_approved_and_is_single_use():
    g = request_gate("merge_main", "merge PR 5 into main", run_id="r1")
    with pytest.raises(GateBlocked):
        check_gate("merge_main", g["id"])  # still open
    with pytest.raises(GateBlocked):
        check_gate("merge_main", None)  # no approval at all
    Inbox().decide(g["id"], approve=True)
    check_gate("merge_main", g["id"])  # passes once
    with pytest.raises(GateBlocked):
        check_gate("merge_main", g["id"])  # consumed


def test_gate_rejected_or_wrong_action_stays_blocked():
    g = request_gate("production_config", "change env")
    Inbox().decide(g["id"], approve=False)
    with pytest.raises(GateBlocked):
        check_gate("production_config", g["id"])
    g2 = request_gate("credentials", "need token")
    Inbox().decide(g2["id"], approve=True)
    with pytest.raises(GateBlocked):
        check_gate("merge_main", g2["id"])  # approval was for a different action


def test_unknown_gate_action_rejected():
    with pytest.raises(ValueError):
        request_gate("make_coffee", "x")
    assert {"merge_main", "production_config", "credentials", "destructive_git", "delete_data"} <= set(GATED)


def test_audit_trail_records_requests_decisions_and_blocks():
    g = request_gate("merge_main", "merge", run_id="r9")
    with pytest.raises(GateBlocked):
        check_gate("merge_main", g["id"])
    Inbox().decide(g["id"], approve=True)
    check_gate("merge_main", g["id"])
    events = [e["event"] for e in db.DocRepo("audit").list()]
    assert events == ["gate_requested", "gate_blocked", "gate_approved", "gate_passed"]
