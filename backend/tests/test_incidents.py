"""Incidents: reported from production test failures, handled by production support L1 to L4."""
import json

import pytest

from app import db
from app.incidents import IncidentError, Incidents
from app.inbox import Inbox
from app.runtime.roles import DEFAULT_TOOLS
from app.runtime.tools import ToolBox, ToolError, Workspace


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def handoffs():
    return [(h["from"], h["to"]) for h in sorted(db.DocRepo("handoffs").list(), key=lambda h: h["at"])]


def make_task(owner="developer_2"):
    return db.DocRepo("tasks").create({"name": "login", "owner": owner, "reviewer": "developer_1", "status": "approved",
                                       "release": "in_prod"})["id"]


def test_a_reported_incident_starts_at_l1_and_goes_to_production_support():
    inc = Incidents().report("Login 500", "high", "POST /login returns 500", source="prod")
    assert inc["level"] == 1 and inc["status"] == "open" and inc["severity"] == "high"
    assert handoffs() == [("incident_reporter", "production_support")]


def test_the_same_open_incident_is_not_reported_twice():
    i = Incidents()
    a = i.report("Login 500", "high", "e")
    b = i.report("login 500 ", "high", "e again")
    assert a["id"] == b["id"] and len(i.list()) == 1 and len(handoffs()) == 1
    i.resolve(a["id"])
    assert i.report("Login 500", "high", "back")["id"] != a["id"]  # a resolved one can come back


def test_escalation_goes_l1_to_l4_then_asks_the_human_and_stops():
    i = Incidents()
    inc = i.report("Slow", "medium", "e")
    assert i.escalate(inc["id"], "needs a code change")["level"] == 2
    assert i.escalate(inc["id"], "design issue")["level"] == 3
    assert handoffs()[-1] == ("production_support", "architect")  # L3 is the architect's call
    assert i.escalate(inc["id"], "outage")["level"] == 4
    assert [x["title"] for x in Inbox().list("open")] == ["Incident needs you: Slow"]
    with pytest.raises(IncidentError, match="L4"):
        i.escalate(inc["id"], "more")
    assert [e["level"] for e in i.get(inc["id"])["history"]] == [2, 3, 4]


def test_a_fix_goes_to_the_developer_who_owns_the_feature_then_resolves():
    i = Incidents()
    inc = i.report("Login 500", "high", "e", task_id=make_task("developer_2"))
    out = i.hand_to_developer(inc["id"])
    assert out["status"] == "fixing" and out["fix_owner"] == "developer_2"
    assert handoffs()[-1] == ("production_support", "developer_2")
    assert i.resolve(inc["id"])["status"] == "resolved"
    with pytest.raises(IncidentError):
        i.resolve(inc["id"])


def test_a_fix_needs_to_know_the_task_that_owns_the_feature():
    inc = Incidents().report("Mystery", "low", "e")
    with pytest.raises(IncidentError, match="task"):
        Incidents().hand_to_developer(inc["id"])


def test_a_failing_production_test_report_raises_an_incident_once():
    i = Incidents()
    tid = make_task()
    assert i.production_tests(tid, passed=10, failed=0, area="smoke") is None
    inc = i.production_tests(tid, passed=7, failed=3, area="smoke")
    assert inc["source"] == "prod" and inc["task_id"] == tid and "3 failing" in inc["evidence"]
    assert i.production_tests(tid, passed=7, failed=3, area="smoke")["id"] == inc["id"]
    assert len(i.list()) == 1


def test_only_the_right_roles_have_the_incident_tool_and_authority_is_enforced(tmp_path):
    assert "incident" in DEFAULT_TOOLS["incident_reporter"] and "incident" in DEFAULT_TOOLS["production_support"]
    assert "incident" not in DEFAULT_TOOLS["developer_1"]
    from app.incidents import IncidentTool

    def box(actor):
        return ToolBox(Workspace(tmp_path), incidents=IncidentTool(actor))

    def act(tb, **kw):
        return json.loads(tb.call(["incident"], "incident", kw))

    rep, sup = box("incident_reporter"), box("production_support")
    inc = act(rep, action="report", title="Down", severity="high", evidence="502s")
    with pytest.raises(ToolError, match="production_support"):
        act(rep, action="escalate", incident_id=inc["id"], reason="x")
    with pytest.raises(ToolError, match="incident_reporter"):
        act(sup, action="report", title="x", severity="low", evidence="x")
    assert act(sup, action="escalate", incident_id=inc["id"], reason="needs code")["level"] == 2
    assert [r["id"] for r in act(sup, action="list")] == [inc["id"]]
    with pytest.raises(ToolError):
        act(sup, action="escalate", incident_id="nope", reason="x")
    with pytest.raises(ToolError, match="severity"):
        act(rep, action="report", title="y", severity="catastrophic", evidence="x")


def test_incident_api_prod_tests_trigger_and_office_counts(authed):
    tid = make_task()
    assert authed.post("/api/pipeline/prod-tests", json={"task_id": tid, "passed": 4, "failed": 0, "area": "a"}).json() == {"incident": None}
    r = authed.post("/api/pipeline/prod-tests", json={"task_id": tid, "passed": 1, "failed": 2, "area": "a"}).json()
    assert r["incident"]["level"] == 1
    assert [x["id"] for x in authed.get("/api/incidents").json()] == [r["incident"]["id"]]
    assert authed.get("/api/office").json()["open_incidents"] == 1
    assert authed.post("/api/pipeline/prod-tests", json={"task_id": "zzz", "passed": 1, "failed": 1, "area": "a"}).status_code == 404
