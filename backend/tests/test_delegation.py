"""The Orchestrator talks to the other agents: it delegates with briefs, reads their reports, never runs code itself."""
import pytest

from app import db
from app.orchestrator import DelegationError, Orchestrator
from app.providers.base import Completion, ToolCall, Usage
from tests.test_runs_api import completion, setup_role


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def call(name, **args):
    return ToolCall(f"c-{name}", name, args)


class Script:
    """Provider that replays scripted completions and records what it was offered and sent."""

    def __init__(self, *steps):
        self.steps, self.seen, self.tools = list(steps), [], []

    def complete(self, model, messages, tools=None, max_tokens=0, temperature=None):
        self.seen.append(list(messages))
        self.tools.append([t.name for t in tools] if tools else [])
        text, calls = self.steps.pop(0) if self.steps else ("done", [])
        return Completion(text, calls, Usage(1, 1), "", "m")


class FakeDelegator:
    def __init__(self, fail=None):
        self.started, self.fail = [], fail

    def delegate(self, role, task, area, done_criteria, context, feature_id):
        if self.fail:
            raise DelegationError(self.fail)
        self.started.append({"role": role, "task": task, "area": area, "done": done_criteria,
                             "context": context, "feature_id": feature_id})
        return {"run_id": "r1", "role": role, "status": "running"}

    def report(self, run_id):
        return {"run_id": run_id, "role": "developer", "status": "done", "steps": 3, "error": None,
                "result": "created hello.txt"}

    def runs(self):
        return [{"run_id": "r1", "role": "developer", "status": "done", "task": "make a file"}]


def test_orchestrator_is_offered_the_delegation_tools_only_when_it_can_delegate():
    s = Script(("hi", []))
    Orchestrator(s, "m", "sys").chat("hello")
    assert s.tools[0] == []
    s = Script(("hi", []))
    Orchestrator(s, "m", "sys", delegator=FakeDelegator()).chat("hello")
    assert s.tools[0] == ["delegate", "get_report", "request_production", "list_runs"]


def test_delegate_sends_a_self_contained_brief_and_the_model_sees_the_run_id():
    s = Script(("", [call("delegate", role="developer", task="Add hello.txt", area="repo root",
                          done_criteria=["file exists", "test passes"], feature_id="f1")]),
               ("Started the developer on it (run r1).", []))
    d = FakeDelegator()
    out = Orchestrator(s, "m", "sys", delegator=d).chat("make a hello file")
    assert out["reply"] == "Started the developer on it (run r1)."
    st = d.started[0]
    assert st["role"] == "developer" and st["feature_id"] == "f1"
    assert "Add hello.txt" in st["task"] and "file exists" in st["task"] and "repo root" in st["task"]
    tool_msg = next(m for m in s.seen[1] if m.role == "tool")
    assert "r1" in tool_msg.content and "<untrusted_data" in tool_msg.content
    assert tool_msg.tool_call_id == "c-delegate"


def test_a_refused_delegation_is_fed_back_to_the_model_not_raised():
    s = Script(("", [call("delegate", role="developer", task="t", area="a", done_criteria=["x"])]),
               ("The developer cannot start until the SRS is approved.", []))
    out = Orchestrator(s, "m", "sys", delegator=FakeDelegator(fail="SRS not approved")).chat("build it")
    tool_msg = next(m for m in s.seen[1] if m.role == "tool")
    assert "ERROR" in tool_msg.content and "SRS not approved" in tool_msg.content
    assert "SRS" in out["reply"]


def test_bad_brief_is_an_error_for_the_model_to_fix():
    s = Script(("", [call("delegate", role="developer", task="t", area="a", done_criteria=[])]), ("ok", []))
    d = FakeDelegator()
    Orchestrator(s, "m", "sys", delegator=d).chat("go")
    tool_msg = next(m for m in s.seen[1] if m.role == "tool")
    assert "done criteria" in tool_msg.content and d.started == []


def test_report_and_list_runs_return_what_the_agents_produced():
    s = Script(("", [call("list_runs"), call("get_report", run_id="r1")]), ("The developer finished.", []))
    Orchestrator(s, "m", "sys", delegator=FakeDelegator()).chat("how is it going?")
    tools = [m for m in s.seen[1] if m.role == "tool"]
    assert len(tools) == 2
    assert "make a file" in tools[0].content and "created hello.txt" in tools[1].content


def test_unknown_tool_is_refused_and_chat_history_keeps_only_the_conversation():
    s = Script(("", [call("run_shell", command="rm -rf /")]), ("I cannot do that.", []))
    Orchestrator(s, "m", "sys", delegator=FakeDelegator()).chat("do something")
    tool_msg = next(m for m in s.seen[1] if m.role == "tool")
    assert "ERROR" in tool_msg.content
    chat = [(m["role"], m["content"]) for m in db.DocRepo("messages").list() if m.get("chat")]
    assert chat == [("user", "do something"), ("assistant", "I cannot do that.")]


def test_a_runaway_tool_loop_stops_with_a_plain_message():
    s = Script(*[("", [call("list_runs")])] * 20)
    out = Orchestrator(s, "m", "sys", delegator=FakeDelegator(), max_rounds=3).chat("loop")
    assert len(s.seen) == 3 and "too many" in out["reply"].lower()


def test_prompt_tells_the_orchestrator_to_delegate_and_verify():
    s = Script(("hi", []))
    Orchestrator(s, "m", "sys", delegator=FakeDelegator()).chat("hello")
    system = s.seen[0][0].content
    assert "delegate" in system.lower() and "get_report" in system


# ---- through the HTTP API with a real run of a real agent ----

def tool_call(name, args):
    import json
    return [{"id": "t1", "type": "function", "function": {"name": name, "arguments": json.dumps(args)}}]


def test_chat_delegates_to_the_developer_agent_end_to_end(authed, tmp_path, monkeypatch):
    monkeypatch.setenv("AO_SYNC_RUNS", "1")  # run the agent inline so the test is deterministic
    pid = setup_role(authed, tmp_path)
    (tmp_path / "roles" / "orchestrator.md").write_text("---\nname: orchestrator\ndescription: boss\n---\nYou lead.")
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    authed.put("/api/role-models/orchestrator", json={"provider_id": pid, "model": "m"})
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    replies = iter([
        # orchestrator decides to delegate
        completion("", tool_call("delegate", {"role": "developer", "task": "Create hello.txt", "area": "root",
                                              "done_criteria": ["hello.txt exists"], "feature_id": fid})),
        # the developer agent's own run (thread) shares this upstream; it writes the file, then finishes
        completion("", tool_call("write_file", {"path": "hello.txt", "content": "hi"})),
        completion("created hello.txt"),
        # orchestrator's final answer
        completion("Developer started."),
    ])
    authed.upstream.handler = lambda req: next(replies)
    r = authed.post("/api/chat", json={"text": "make hello.txt"}).json()
    assert r["reply"] == "Developer started."
    runs = authed.get("/api/runs").json()
    assert len(runs) == 1 and runs[0]["role"] == "developer" and runs[0]["status"] == "done"
    assert (tmp_path / "hello.txt").read_text() == "hi"
