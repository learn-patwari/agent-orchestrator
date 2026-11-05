"""Anthropic-compatible adapter: POST {base_url}/v1/messages."""
from .base import Completion, LLMProvider, ToolCall, Usage


class AnthropicCompatProvider(LLMProvider):
    version = "2023-06-01"

    def _request(self, model, messages, tools, max_tokens, temperature, stream):
        system = "\n\n".join(m.content for m in messages if m.role == "system")
        msgs: list[dict] = []
        for m in messages:
            if m.role == "system":
                continue
            if m.role == "tool":
                block = {"type": "tool_result", "tool_use_id": m.tool_call_id, "content": m.content}
                if msgs and msgs[-1]["role"] == "user" and isinstance(msgs[-1]["content"], list):
                    msgs[-1]["content"].append(block)  # merge consecutive tool results
                else:
                    msgs.append({"role": "user", "content": [block]})
            elif m.role == "assistant" and m.tool_calls:
                blocks = [{"type": "text", "text": m.content}] if m.content else []
                blocks += [{"type": "tool_use", "id": t.id, "name": t.name, "input": t.arguments}
                           for t in m.tool_calls]
                msgs.append({"role": "assistant", "content": blocks})
            else:
                msgs.append({"role": m.role, "content": m.content})
        body = {"model": model, "messages": msgs, "max_tokens": max_tokens}
        if system:
            body["system"] = system
        if temperature is not None:
            body["temperature"] = temperature
        if tools:
            body["tools"] = [{"name": t.name, "description": t.description,
                              "input_schema": t.parameters} for t in tools]
        if stream:
            body["stream"] = True
        headers = {"x-api-key": self.api_key, "anthropic-version": self.version,
                   "Content-Type": "application/json"}
        return f"{self.base_url}/v1/messages", headers, body

    def _parse(self, data):
        text, calls = [], []
        for b in data.get("content", []):
            if b["type"] == "text":
                text.append(b["text"])
            elif b["type"] == "tool_use":
                calls.append(ToolCall(b["id"], b["name"], b.get("input") or {}))
        u = data.get("usage") or {}
        return Completion("".join(text), calls,
                          Usage(u.get("input_tokens", 0), u.get("output_tokens", 0)),
                          data.get("stop_reason") or "", data.get("model", ""))

    def _parse_stream_line(self, event, data):
        if data.get("type") == "content_block_delta":
            return data.get("delta", {}).get("text")
        return None
