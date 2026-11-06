"""OpenAI-compatible adapter: POST {base_url}/chat/completions (base_url includes /v1)."""
import json

from .base import Completion, LLMProvider, ToolCall, Usage


class OpenAICompatProvider(LLMProvider):
    def _request(self, model, messages, tools, max_tokens, temperature, stream):
        msgs = []
        for m in messages:
            if m.role == "tool":
                msgs.append({"role": "tool", "tool_call_id": m.tool_call_id, "content": m.content})
            elif m.role == "assistant" and m.tool_calls:
                msgs.append({
                    "role": "assistant", "content": m.content or None,
                    "tool_calls": [{"id": t.id, "type": "function",
                                    "function": {"name": t.name, "arguments": json.dumps(t.arguments)}}
                                   for t in m.tool_calls],
                })
            else:
                msgs.append({"role": m.role, "content": m.content})
        body = {"model": model, "messages": msgs, "max_tokens": max_tokens}
        if temperature is not None:
            body["temperature"] = temperature
        if tools:
            body["tools"] = [{"type": "function", "function": {
                "name": t.name, "description": t.description, "parameters": t.parameters}} for t in tools]
        if stream:
            body["stream"] = True
        headers = {"Authorization": f"Bearer {self.api_key}", "Content-Type": "application/json"}
        return f"{self.base_url}/chat/completions", headers, body

    def _parse(self, data):
        choice = data["choices"][0]
        msg = choice["message"]
        calls = []
        for tc in msg.get("tool_calls") or []:
            raw = tc["function"].get("arguments") or "{}"
            try:
                args = json.loads(raw)
            except ValueError:
                args = {"_raw": raw}
            calls.append(ToolCall(tc["id"], tc["function"]["name"], args))
        u = data.get("usage") or {}
        return Completion(msg.get("content") or "", calls,
                          Usage(u.get("prompt_tokens", 0), u.get("completion_tokens", 0)),
                          choice.get("finish_reason") or "", data.get("model", ""))

    def _parse_stream_line(self, event, data):
        ch = data.get("choices") or []
        return (ch[0].get("delta") or {}).get("content") if ch else None
