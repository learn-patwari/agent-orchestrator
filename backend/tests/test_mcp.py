"""MCP client: stdio and HTTP transports, per-role allow-lists, secrets by reference, test-connection."""
import json
import sys
from pathlib import Path

import httpx
import pytest

from app import db
from app.mcp import HttpTransport, McpClient, McpError, McpServers, McpTools, StdioTransport
from app.runtime.tools import ToolBox, ToolError, Workspace

FAKE = str(Path(__file__).parent / "fake_mcp_server.py")
CMD = [sys.executable, FAKE]


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def add(name="fake", roles=("developer",), **kw):
    return McpServers().save(name, {"transport": "stdio", "command": CMD, "roles": list(roles), **kw})


def test_stdio_client_lists_and_calls_tools():
    c = McpClient(StdioTransport(CMD))
    try:
        c.initialize()
        assert [t["name"] for t in c.list_tools()] == ["echo", "env_value"]
        assert c.call_tool("echo", {"text": "hi"}) == "echo: hi"
        with pytest.raises(McpError, match="unknown tool"):
            c.call_tool("nope", {})
    finally:
        c.close()


def test_a_server_that_cannot_start_is_a_clear_error_not_a_crash():
    with pytest.raises(McpError, match="could not start"):
        McpClient(StdioTransport(["definitely-not-a-real-program-xyz"])).initialize()


def test_http_transport_speaks_json_rpc():
    seen = []

    def handler(req):
        body = json.loads(req.content)
        seen.append(body["method"])
        if "id" not in body:  # a notification has no reply
            return httpx.Response(202)
        if body["method"] == "initialize":
            return httpx.Response(200, json={"jsonrpc": "2.0", "id": body["id"], "result": {"capabilities": {}}})
        if body["method"] == "tools/list":
            return httpx.Response(200, json={"jsonrpc": "2.0", "id": body["id"], "result": {"tools": [
                {"name": "ping", "description": "p", "inputSchema": {"type": "object"}}]}})
        return httpx.Response(200, json={"jsonrpc": "2.0", "id": body["id"],
                                         "result": {"content": [{"type": "text", "text": "pong"}]}})
    c = McpClient(HttpTransport("https://mcp.example/rpc", client=httpx.Client(transport=httpx.MockTransport(handler))))
    c.initialize()
    assert [t["name"] for t in c.list_tools()] == ["ping"] and c.call_tool("ping", {}) == "pong"
    assert seen[:3] == ["initialize", "notifications/initialized", "tools/list"]


def test_tools_show_only_for_the_roles_the_server_allows_and_calls_are_refused_for_others():
    add(roles=["developer"])
    dev, tester = McpTools("developer"), McpTools("tester")
    try:
        assert [s.name for s in dev.specs()] == ["mcp__fake__echo", "mcp__fake__env_value"]
        assert tester.specs() == []
        assert dev.call("mcp__fake__echo", {"text": "x"}) == "echo: x"
        with pytest.raises(ToolError, match="not allowed"):
            tester.call("mcp__fake__echo", {"text": "x"})
        with pytest.raises(ToolError, match="unknown"):
            dev.call("mcp__ghost__echo", {})
    finally:
        dev.close()
        tester.close()


def test_disabled_servers_and_a_dead_server_do_not_break_the_role():
    add("off", roles=["developer"], enabled=False)
    McpServers().save("dead", {"transport": "stdio", "command": ["definitely-not-a-real-program-xyz"],
                               "roles": ["developer"]})
    t = McpTools("developer")
    try:
        assert t.specs() == []  # one broken server must not take the agent's other tools away
        assert "dead" in t.problems
    finally:
        t.close()


def test_toolbox_offers_mcp_tools_alongside_its_own_and_keeps_the_role_check(tmp_path):
    add(roles=["developer"])
    mcp = McpTools("developer")
    try:
        tb = ToolBox(Workspace(tmp_path), mcp=mcp)
        names = [s.name for s in tb.specs_for(["read_file"])]
        assert names == ["read_file", "mcp__fake__echo", "mcp__fake__env_value"]
        assert tb.call(["read_file"], "mcp__fake__echo", {"text": "yo"}) == "echo: yo"
        with pytest.raises(ToolError, match="not allowed"):
            ToolBox(Workspace(tmp_path), mcp=McpTools("tester")).call(["read_file"], "mcp__fake__echo", {"text": "x"})
        with pytest.raises(ToolError, match="not available"):
            ToolBox(Workspace(tmp_path)).call(["read_file"], "mcp__fake__echo", {})
    finally:
        mcp.close()


def test_secrets_are_passed_by_reference_and_never_stored(monkeypatch):
    monkeypatch.setenv("MY_REAL_TOKEN", "s3cret-value-123")
    add(env_refs={"FAKE_SECRET": "MY_REAL_TOKEN"})
    assert "s3cret-value-123" not in json.dumps(db.DocRepo("mcp_servers").list())
    t = McpTools("developer")
    try:
        assert t.call("mcp__fake__env_value", {"name": "FAKE_SECRET"}) == "s3cret-value-123"
    finally:
        t.close()


def test_bad_server_configs_are_refused():
    for bad in ({"transport": "carrier-pigeon"}, {"transport": "stdio"}, {"transport": "stdio", "command": []},
                {"transport": "http"}, {"transport": "http", "url": "ftp://x"}):
        with pytest.raises(McpError):
            McpServers().save("x", {"roles": [], **bad})
    with pytest.raises(McpError):
        McpServers().save("bad name!", {"transport": "http", "url": "https://x.example", "roles": []})


def test_mcp_api_save_test_list_delete_and_an_agent_uses_the_tool(authed, tmp_path, monkeypatch):
    from tests.test_runs_api import completion, setup_role
    monkeypatch.setenv("AO_SYNC_RUNS", "1")
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    body = {"transport": "stdio", "command": CMD, "roles": ["developer"]}
    assert authed.put("/api/mcp/servers/fake", json=body).status_code == 200
    assert authed.put("/api/mcp/servers/bad", json={"transport": "nope"}).status_code == 400
    assert [s["name"] for s in authed.get("/api/mcp/servers").json()] == ["fake"]
    t = authed.post("/api/mcp/servers/fake/test").json()
    assert t["ok"] is True and [x["name"] for x in t["tools"]] == ["echo", "env_value"]
    authed.put("/api/mcp/servers/dead", json={"transport": "stdio", "command": ["no-such-program-xyz"], "roles": []})
    d = authed.post("/api/mcp/servers/dead/test").json()
    assert d["ok"] is False and "could not start" in d["error"]
    assert authed.post("/api/mcp/servers/ghost/test").status_code == 404
    fid = db.DocRepo("features").create({"title": "f", "status": "approved"})["id"]
    call = [{"id": "c1", "type": "function", "function": {"name": "mcp__fake__echo", "arguments": json.dumps({"text": "hello"})}}]
    replies = iter([completion("", call), completion("used the tool")])
    authed.upstream.handler = lambda req: next(replies)
    run = authed.post("/api/runs", json={"role": "developer", "task": "go", "workspace": str(tmp_path),
                                         "wait": True, "feature_id": fid}).json()
    assert run["status"] == "done"
    log = authed.get(f"/api/runs/{run['id']}").json()["log"]
    tool_msg = next(m for m in log if m["role"] == "tool")
    assert "echo: hello" in tool_msg["content"] and "<untrusted_data" in tool_msg["content"]
    assert authed.delete("/api/mcp/servers/fake").status_code == 200
    assert authed.delete("/api/mcp/servers/fake").status_code == 404
