import json

import httpx
import pytest

from app.providers.anthropic_compat import AnthropicCompatProvider
from app.providers.base import (AuthError, Message, ModelNotFound, NetworkError, RateLimited,
                                ToolCall, ToolSpec)
from app.providers.openai_compat import OpenAICompatProvider
from app.providers.pricing import cost

TOOL = ToolSpec("read_file", "read a file", {"type": "object", "properties": {"path": {"type": "string"}}})


def mk(cls, handler, **kw):
    return cls("https://llm.example/v1" if cls is OpenAICompatProvider else "https://llm.example",
               "sk-test-123456789012345678", client=httpx.Client(transport=httpx.MockTransport(handler)),
               backoff=0, **kw)


def test_openai_complete_with_tools_and_usage():
    seen = {}

    def h(req):
        seen["url"], seen["auth"], seen["body"] = str(req.url), req.headers["authorization"], json.loads(req.content)
        return httpx.Response(200, json={
            "model": "m1", "usage": {"prompt_tokens": 10, "completion_tokens": 5},
            "choices": [{"finish_reason": "tool_calls", "message": {
                "content": None, "tool_calls": [{"id": "c1", "type": "function", "function": {
                    "name": "read_file", "arguments": "{\"path\": \"a.txt\"}"}}]}}]})

    p = mk(OpenAICompatProvider, h)
    c = p.complete("m1", [Message("system", "s"), Message("user", "hi")], tools=[TOOL])
    assert seen["url"] == "https://llm.example/v1/chat/completions"
    assert seen["auth"].startswith("Bearer sk-test")
    assert seen["body"]["tools"][0]["function"]["name"] == "read_file"
    assert c.tool_calls == [ToolCall("c1", "read_file", {"path": "a.txt"})]
    assert (c.usage.input_tokens, c.usage.output_tokens) == (10, 5)


def test_openai_tool_result_roundtrip_mapping():
    body = {}

    def h(req):
        body.update(json.loads(req.content))
        return httpx.Response(200, json={"choices": [{"message": {"content": "done"}}]})

    p = mk(OpenAICompatProvider, h)
    p.complete("m", [Message("user", "go"),
                     Message("assistant", "", [ToolCall("c1", "read_file", {"path": "a"})]),
                     Message("tool", "contents", tool_call_id="c1")])
    assert body["messages"][1]["tool_calls"][0]["function"]["arguments"] == "{\"path\": \"a\"}"
    assert body["messages"][2] == {"role": "tool", "tool_call_id": "c1", "content": "contents"}


def test_anthropic_complete_and_mapping():
    seen = {}

    def h(req):
        seen["url"], seen["key"], seen["body"] = str(req.url), req.headers["x-api-key"], json.loads(req.content)
        return httpx.Response(200, json={
            "model": "claude-x", "stop_reason": "tool_use", "usage": {"input_tokens": 7, "output_tokens": 3},
            "content": [{"type": "text", "text": "ok "}, {"type": "tool_use", "id": "t1", "name": "read_file",
                                                          "input": {"path": "b"}}]})

    p = mk(AnthropicCompatProvider, h)
    c = p.complete("claude-x", [Message("system", "be brief"), Message("user", "hi"),
                                Message("assistant", "", [ToolCall("t0", "read_file", {"path": "a"})]),
                                Message("tool", "r1", tool_call_id="t0"),
                                Message("tool", "r2", tool_call_id="t0b")], tools=[TOOL])
    b = seen["body"]
    assert seen["url"] == "https://llm.example/v1/messages" and seen["key"].startswith("sk-test")
    assert b["system"] == "be brief" and b["tools"][0]["input_schema"]["type"] == "object"
    assert [m["role"] for m in b["messages"]] == ["user", "assistant", "user"]
    assert len(b["messages"][2]["content"]) == 2  # tool results merged into one user turn
    assert c.text == "ok " and c.tool_calls[0].arguments == {"path": "b"}
    assert (c.usage.input_tokens, c.usage.output_tokens) == (7, 3)


@pytest.mark.parametrize("status,exc", [(401, AuthError), (404, ModelNotFound)])
def test_error_taxonomy_no_retry(status, exc):
    calls = []

    def h(req):
        calls.append(1)
        return httpx.Response(status, text="nope")

    with pytest.raises(exc):
        mk(OpenAICompatProvider, h).complete("m", [Message("user", "x")])
    assert len(calls) == 1


def test_retry_on_rate_limit_then_success_and_exhaustion():
    n = {"i": 0}

    def flaky(req):
        n["i"] += 1
        if n["i"] < 3:
            return httpx.Response(429, text="slow down")
        return httpx.Response(200, json={"choices": [{"message": {"content": "hi"}}]})

    assert mk(OpenAICompatProvider, flaky).complete("m", [Message("user", "x")]).text == "hi"
    assert n["i"] == 3
    with pytest.raises(RateLimited):
        mk(OpenAICompatProvider, lambda r: httpx.Response(429), max_retries=1).complete("m", [Message("user", "x")])


def test_network_error():
    def boom(req):
        raise httpx.ConnectError("refused")

    with pytest.raises(NetworkError):
        mk(OpenAICompatProvider, boom, max_retries=0).complete("m", [Message("user", "x")])


def test_streaming_both_styles():
    def oa(req):
        sse = "".join(f"data: {json.dumps({'choices': [{'delta': {'content': t}}]})}\n\n" for t in ["Hel", "lo"])
        return httpx.Response(200, text=sse + "data: [DONE]\n\n")

    assert "".join(mk(OpenAICompatProvider, oa).stream("m", [Message("user", "x")])) == "Hello"

    def an(req):
        ev = [("content_block_delta", {"type": "content_block_delta", "delta": {"type": "text_delta", "text": t}})
              for t in ["Wor", "ld"]] + [("message_stop", {"type": "message_stop"})]
        return httpx.Response(200, text="".join(f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in ev))

    assert "".join(mk(AnthropicCompatProvider, an).stream("m", [Message("user", "x")])) == "World"


def test_cost_from_price_table():
    t = {"m": {"input": 3.0, "output": 15.0}}
    assert cost("m", 1_000_000, 100_000, t) == 4.5
    assert cost("unknown", 10, 10, t) is None
