import json

import pytest

from app.providers import registry
from app.providers.base import AuthError, Message, NetworkError, RateLimited, ServerError, ToolCall, ToolSpec
from app.providers.claude_cli import ClaudeCliProvider

TOOL = ToolSpec("read_file", "read a file", {"type": "object", "properties": {"path": {"type": "string"}}})


def fake(result="hello", code=0, stderr="", usage=None, is_error=False, seen=None):
    def run(argv, stdin, env, timeout):
        if seen is not None:
            seen.update(argv=argv, stdin=stdin, env=env)
        out = json.dumps({"type": "result", "is_error": is_error, "result": result,
                          "usage": usage or {"input_tokens": 7, "output_tokens": 3}})
        return code, out, stderr
    return run


def mk(runner, exe="C:/x/claude.exe"):
    return ClaudeCliProvider(exe, "", runner=runner)


def test_plain_completion_returns_text_and_usage():
    c = mk(fake("hi there")).complete("sonnet", [Message("user", "hello")])
    assert c.text == "hi there" and c.tool_calls == []
    assert (c.usage.input_tokens, c.usage.output_tokens) == (7, 3)


def test_command_disables_tools_sets_model_system_and_sends_prompt_on_stdin():
    seen = {}
    mk(fake(seen=seen)).complete("sonnet", [Message("system", "be brief"), Message("user", "hello")])
    a = seen["argv"]
    assert a[0] == "C:/x/claude.exe" and "-p" in a
    assert a[a.index("--model") + 1] == "sonnet"
    assert a[a.index("--system-prompt") + 1] == "be brief"
    assert a[a.index("--tools") + 1] == ""  # the CLI's own tools stay off; ours go through the tool protocol
    assert a[a.index("--output-format") + 1] == "json"
    assert "hello" in seen["stdin"] and "hello" not in " ".join(a)


def test_conversation_with_tool_result_is_flattened_in_order():
    seen = {}
    msgs = [Message("user", "read a.txt"),
            Message("assistant", "", [ToolCall("c1", "read_file", {"path": "a.txt"})]),
            Message("tool", "file body", tool_call_id="c1"), Message("user", "summarise")]
    mk(fake(seen=seen)).complete("sonnet", msgs, tools=[TOOL])
    s = seen["stdin"]
    assert s.index("read a.txt") < s.index("file body") < s.index("summarise")
    assert "read_file" in s  # tool list is described to the model


def test_tool_call_reply_is_parsed_from_fenced_json():
    reply = 'Reading it.\n```json\n{"tool_calls": [{"name": "read_file", "arguments": {"path": "a.txt"}}]}\n```'
    c = mk(fake(reply)).complete("sonnet", [Message("user", "go")], tools=[TOOL])
    assert len(c.tool_calls) == 1 and c.tool_calls[0].name == "read_file"
    assert c.tool_calls[0].arguments == {"path": "a.txt"} and c.tool_calls[0].id
    assert "tool_calls" not in c.text


def test_unknown_tool_in_reply_is_ignored_not_executed():
    reply = '{"tool_calls": [{"name": "rm_rf", "arguments": {}}]}'
    c = mk(fake(reply)).complete("sonnet", [Message("user", "go")], tools=[TOOL])
    assert c.tool_calls == []


def test_json_in_reply_without_tools_offered_stays_text():
    reply = '{"tool_calls": [{"name": "read_file", "arguments": {}}]}'
    c = mk(fake(reply)).complete("sonnet", [Message("user", "go")])
    assert c.tool_calls == [] and "tool_calls" in c.text


def test_not_logged_in_is_an_auth_error():
    with pytest.raises(AuthError):
        mk(fake("Invalid API key · Please run /login", is_error=True, code=1)).complete("m", [Message("user", "x")])


def test_rate_limit_and_other_failures_are_classified():
    with pytest.raises(RateLimited):
        mk(fake("Claude usage limit reached", is_error=True, code=1)).complete("m", [Message("user", "x")])
    with pytest.raises(ServerError):
        mk(fake("boom", is_error=True, code=1)).complete("m", [Message("user", "x")])


def test_timeout_and_missing_program_are_network_errors():
    def slow(argv, stdin, env, timeout):
        raise TimeoutError("took too long")

    def missing(argv, stdin, env, timeout):
        raise FileNotFoundError("claude")
    with pytest.raises(NetworkError):
        mk(slow).complete("m", [Message("user", "x")])
    with pytest.raises(NetworkError):
        mk(missing).complete("m", [Message("user", "x")])


def test_api_key_variable_is_removed_so_the_subscription_login_is_used():
    seen = {}
    p = ClaudeCliProvider("claude", "", runner=fake(seen=seen))
    import os
    os.environ["ANTHROPIC_API_KEY"] = "sk-ant-should-not-leak"
    try:
        p.complete("m", [Message("user", "x")])
    finally:
        del os.environ["ANTHROPIC_API_KEY"]
    assert "ANTHROPIC_API_KEY" not in seen["env"]


def test_registry_offers_and_builds_the_type_without_a_key(client):
    from app import secrets_store
    assert "claude_cli" in registry.allowed_types()
    rec = {"type": "claude_cli", "base_url": "", "api_key_enc": secrets_store.encrypt("")}
    assert isinstance(registry.build(rec), ClaudeCliProvider)


def test_api_accepts_the_provider_with_no_key_or_url(authed):
    r = authed.post("/api/providers", json={"name": "Claude login", "type": "claude_cli"})
    assert r.status_code == 200, r.text
    assert r.json()["type"] == "claude_cli"
