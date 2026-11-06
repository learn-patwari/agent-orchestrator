import httpx

from app.orchestrator import Orchestrator, team_check, team_snapshot
from app.providers.base import Completion, Message, ProviderError, Usage
from app.runtime.roles import Role

ROLES = {
    "orchestrator": Role("orchestrator", "Only voice to the human", "body"),
    "developer": Role("developer", "Writes code in task worktrees", "body"),
    "tester": Role("tester", "Writes and runs tests", "body"),
}
MODELS = {"orchestrator": {"provider_id": "p1", "model": "sonnet"}, "developer": {"provider_id": "p1", "model": "opus"}}
PROVIDERS = {"p1": "Claude login"}
STATUS = [{"role": "orchestrator", "state": "idle", "task": None, "steps": 0, "error": None},
          {"role": "developer", "state": "running", "task": "add login", "steps": 4, "error": None},
          {"role": "tester", "state": "idle", "task": None, "steps": 0, "error": None}]


def test_snapshot_lists_every_role_with_purpose_model_and_state():
    s = team_snapshot(ROLES, MODELS, PROVIDERS, STATUS, open_items=2, features=[{"title": "Login", "status": "ready"}])
    for name in ROLES:
        assert name in s
    assert "Writes code in task worktrees" in s and "Claude login / opus" in s
    assert "running" in s and "add login" in s


def test_snapshot_flags_roles_without_a_model_and_counts_open_items():
    s = team_snapshot(ROLES, MODELS, PROVIDERS, STATUS, open_items=2, features=[])
    assert "tester" in s and "no model assigned" in s
    assert "2 open item" in s


def test_snapshot_never_claims_liveness_it_has_not_checked():
    s = team_snapshot(ROLES, MODELS, PROVIDERS, STATUS, open_items=0, features=[])
    assert "not verified live" in s.lower()


class Capture:
    def __init__(self):
        self.seen = None

    def complete(self, model, messages, tools=None, max_tokens=1024, temperature=None):
        self.seen = messages
        return Completion("ok", [], Usage(1, 1), "end_turn", model)


def test_chat_puts_the_team_snapshot_in_the_system_prompt(client):
    cap = Capture()
    o = Orchestrator(cap, "sonnet", "I am the orchestrator", "p1", context="TEAM: developer is running")
    o.chat("which agents are up?")
    system = next(m.content for m in cap.seen if m.role == "system")
    assert "I am the orchestrator" in system and "TEAM: developer is running" in system


class Pinger:
    def __init__(self, fail=()):
        self.calls, self.fail = [], set(fail)

    def test_connection(self, model):
        self.calls.append(model)
        if model in self.fail:
            raise ProviderError("down")
        return {"ok": True}


def test_check_pings_each_distinct_model_once_and_reports_per_role():
    p = Pinger(fail={"opus"})
    out = team_check(ROLES, MODELS, lambda pid: p)
    assert sorted(p.calls) == ["opus", "sonnet"]  # one call per distinct provider+model
    by = {r["role"]: r for r in out}
    assert by["orchestrator"]["responding"] is True
    assert by["developer"]["responding"] is False and "down" in by["developer"]["detail"]
    assert by["tester"]["responding"] is False and "no model" in by["tester"]["detail"]


def test_builtin_roles_are_loaded_by_default_and_match_the_role_ids(authed):
    rows = authed.get("/api/team").json()
    ids = {r["role"] for r in rows}
    assert {"orchestrator", "architect", "developer", "tester", "reviewer", "cicd",
            "incident_reporter", "production_support"} <= ids
    assert all(r["description"] for r in rows)
    assert all("model" in r and "state" in r for r in rows)


def test_team_endpoint_requires_login_and_check_reports_missing_models(client):
    assert client.get("/api/team").status_code == 401
    client.post("/api/auth/setup", json={"password": "correct-horse-1"})
    r = client.post("/api/team/check")
    assert r.status_code == 200
    assert all(x["responding"] is False for x in r.json())
