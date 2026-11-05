import pytest

from app import db
from app.orchestrator import Orchestrator, build_brief, check_workspace, status_table
from app.providers.base import Completion, Usage


class Fake:
    def __init__(self, text="Noted. Next step: ask the architect."):
        self.text, self.seen = text, []

    def complete(self, model, messages, tools=None, max_tokens=0, temperature=None):
        self.seen.append(messages)
        return Completion(self.text, [], Usage(3, 4), "", "m")


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def test_chat_stores_both_sides_and_replies():
    f = Fake()
    o = Orchestrator(f, "m", "You are the orchestrator.")
    out = o.chat("Add a tax report page")
    assert out["reply"] == f.text and out["warning"] is None
    log = db.DocRepo("messages").list()
    assert [(m["role"], m["content"]) for m in log if m.get("chat")] == [
        ("user", "Add a tax report page"), ("assistant", f.text)]
    assert f.seen[0][0].role == "system" and "orchestrator" in f.seen[0][0].content.lower()
    assert f.seen[0][-1].content == "Add a tax report page"


def test_chat_history_is_included_in_later_turns():
    f = Fake()
    o = Orchestrator(f, "m", "sys")
    o.chat("first")
    o.chat("second")
    roles = [m.role for m in f.seen[1]]
    assert roles == ["system", "user", "assistant", "user"]


def test_pasted_secret_is_warned_redacted_and_never_sent_to_the_model():
    f = Fake()
    o = Orchestrator(f, "m", "sys")
    out = o.chat("use this key sk-abcdefghijklmnopqrstuvwxyz1234 for the API")
    assert out["warning"] and "rotate" in out["warning"].lower()
    stored = [m["content"] for m in db.DocRepo("messages").list() if m.get("chat")]
    assert all("sk-abcdef" not in c for c in stored)
    assert all("sk-abcdef" not in m.content for msgs in f.seen for m in msgs)
    assert "[REDACTED]" in stored[0]


def test_system_prompt_states_orchestrator_rules_and_untrusted_policy():
    f = Fake()
    Orchestrator(f, "m", "ROLE BODY").chat("hi")
    sys = f.seen[0][0].content
    assert "ROLE BODY" in sys and "only voice" in sys and "untrusted" in sys.lower()


def test_brief_is_self_contained_and_needs_done_criteria():
    b = build_brief("developer", "Add GET /health", "backend/app/main.py", ["test passes", "PR opened"],
                    context="Single-file API")
    for needle in ("developer", "Add GET /health", "backend/app/main.py", "test passes", "PR opened", "Single-file API"):
        assert needle in b
    with pytest.raises(ValueError):
        build_brief("developer", "x", "area", [])
    with pytest.raises(ValueError):
        build_brief("developer", "", "area", ["done"])


def test_status_table_has_a_row_per_role_with_last_run():
    db.DocRepo("runs").create({"role": "developer", "task": "t1", "status": "done", "steps": 3, "error": None})
    db.DocRepo("runs").create({"role": "developer", "task": "t2", "status": "running", "steps": 1, "error": None})
    rows = {r["role"]: r for r in status_table(["developer", "tester"])}
    assert rows["developer"]["state"] == "running" and rows["developer"]["task"] == "t2"
    assert rows["tester"]["state"] == "idle" and rows["tester"]["task"] is None


def test_workspace_must_be_inside_allowed_roots(tmp_path):
    root = tmp_path / "ws"
    (root / "task1").mkdir(parents=True)
    assert check_workspace(str(root / "task1"), [str(root)]) == (root / "task1").resolve()
    with pytest.raises(ValueError):
        check_workspace(str(tmp_path), [str(root)])
    with pytest.raises(ValueError):
        check_workspace(str(root / ".." / "ws2"), [str(root)])
    with pytest.raises(ValueError):
        check_workspace(str(root / "missing"), [str(root)])
