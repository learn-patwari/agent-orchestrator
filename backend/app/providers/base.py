"""LLM provider interface (FR-6). Adapters map this neutral shape to a vendor API."""
from __future__ import annotations

import json as _json
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Iterator

import httpx


class ProviderError(Exception):
    kind = "unknown"


class AuthError(ProviderError):
    kind = "auth"


class ModelNotFound(ProviderError):
    kind = "model"


class RateLimited(ProviderError):
    kind = "rate_limit"


class NetworkError(ProviderError):
    kind = "network"


class BadRequest(ProviderError):
    kind = "bad_request"


class ServerError(ProviderError):
    kind = "server"


@dataclass
class ToolSpec:
    name: str
    description: str
    parameters: dict  # JSON schema


@dataclass
class ToolCall:
    id: str
    name: str
    arguments: dict


@dataclass
class Message:
    role: str  # system | user | assistant | tool
    content: str = ""
    tool_calls: list[ToolCall] = field(default_factory=list)  # on assistant
    tool_call_id: str | None = None  # on tool


@dataclass
class Usage:
    input_tokens: int = 0
    output_tokens: int = 0


@dataclass
class Completion:
    text: str
    tool_calls: list[ToolCall]
    usage: Usage
    stop_reason: str = ""
    model: str = ""


def classify(status: int, body: str) -> ProviderError:
    msg = f"HTTP {status}: {body[:300]}"
    if status in (401, 403):
        return AuthError(msg)
    if status == 404:
        return ModelNotFound(msg)
    if status == 429:
        return RateLimited(msg)
    if status >= 500:
        return ServerError(msg)
    if "model" in body.lower() and status in (400, 422):
        return ModelNotFound(msg)
    return BadRequest(msg)


class LLMProvider(ABC):
    def __init__(self, base_url: str, api_key: str, *, client: httpx.Client | None = None,
                 max_retries: int = 3, backoff: float = 0.5, timeout: float = 120.0):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.client = client or httpx.Client(timeout=timeout)
        self.max_retries = max_retries
        self.backoff = backoff

    @abstractmethod
    def _request(self, model: str, messages: list[Message], tools: list[ToolSpec] | None,
                 max_tokens: int, temperature: float | None, stream: bool) -> tuple[str, dict, dict]:
        """Return (url, headers, json_body)."""

    @abstractmethod
    def _parse(self, data: dict) -> Completion: ...

    @abstractmethod
    def _parse_stream_line(self, event: str | None, data: dict) -> str | None:
        """Return a text delta or None."""

    def _post(self, url: str, headers: dict, body: dict) -> httpx.Response:
        last: ProviderError | None = None
        for attempt in range(self.max_retries + 1):
            try:
                r = self.client.post(url, headers=headers, json=body)
            except httpx.HTTPError as e:
                last = NetworkError(str(e))
            else:
                if r.status_code < 400:
                    return r
                err = classify(r.status_code, r.text)
                if not isinstance(err, (RateLimited, ServerError)):
                    raise err  # auth/model/bad request: retrying will not help
                last = err
            if attempt < self.max_retries:
                time.sleep(self.backoff * (2 ** attempt))
        assert last is not None
        raise last

    def complete(self, model: str, messages: list[Message], tools: list[ToolSpec] | None = None,
                 max_tokens: int = 1024, temperature: float | None = None) -> Completion:
        url, headers, body = self._request(model, messages, tools, max_tokens, temperature, False)
        return self._parse(self._post(url, headers, body).json())

    def stream(self, model: str, messages: list[Message], tools: list[ToolSpec] | None = None,
               max_tokens: int = 1024, temperature: float | None = None) -> Iterator[str]:
        """Yield text deltas. Tool calls are not streamed in the MVP; use complete()."""
        url, headers, body = self._request(model, messages, tools, max_tokens, temperature, True)
        try:
            with self.client.stream("POST", url, headers=headers, json=body) as r:
                if r.status_code >= 400:
                    r.read()
                    raise classify(r.status_code, r.text)
                event = None
                for line in r.iter_lines():
                    if line.startswith("event:"):
                        event = line[6:].strip()
                    elif line.startswith("data:"):
                        raw = line[5:].strip()
                        if raw == "[DONE]":
                            return
                        try:
                            delta = self._parse_stream_line(event, _json.loads(raw))
                        except ValueError:
                            continue
                        if delta:
                            yield delta
        except httpx.HTTPError as e:
            raise NetworkError(str(e)) from e

    def test_connection(self, model: str) -> dict:
        c = self.complete(model, [Message("user", "Reply with the word ok.")], max_tokens=8)
        return {"ok": True, "model": c.model or model, "usage": vars(c.usage)}
