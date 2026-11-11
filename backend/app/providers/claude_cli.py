"""Claude via the local `claude` program (Claude Code) using the user's own login, so no API key is needed.

Each call runs `claude -p` once with the CLI's own tools turned off. Our tool calls go through a small JSON
protocol in the prompt: the model answers with {"tool_calls": [...]} and we parse it; only tools offered in
this call are accepted. Meant for testing and personal use under a subscription; slower than the API.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import uuid
from pathlib import Path
from typing import Callable, Iterator

from .base import (AuthError, Completion, LLMProvider, Message, NetworkError, ProviderError, RateLimited,
                   ServerError, ToolCall, ToolSpec, Usage)

FENCE = re.compile(r"```(?:json)?\s*(\{.*?\})\s*```", re.S)
TOOL_RULES = (
    "You have NO built-in tools in this session; never try to call one, a native call fails. The tools below are "
    "not native: you call one by writing, as your whole reply, a single JSON object and nothing else: "
    '{"tool_calls": [{"name": "<tool>", "arguments": {...}}]}. Do this whenever the task needs one of them. '
    "If no tool is needed, answer in plain text.\nTools:\n")
REMINDER = ("\n\n[Reminder] Reply with the JSON tool_calls object if you need a listed tool; "
            "no native tools exist.")  # read last; without it sonnet and haiku tried native tools and gave up


def _default_runner(argv: list[str], stdin: str, env: dict, timeout: float) -> tuple[int, str, str]:
    r = subprocess.run(argv, input=stdin, capture_output=True, text=True, encoding="utf-8", env=env,
                       timeout=timeout)
    return r.returncode, r.stdout, r.stderr


def normalise_exe(value: str) -> str:
    """What people paste is not always a path: a PowerShell call such as & "C:/.../claude.exe", or a quoted path.
    Returns the plain path, or "" when nothing usable was typed (the caller then auto-detects)."""
    s = (value or "").strip()
    if s.startswith("&"):
        s = s[1:].strip()
    return s.strip("\"'").strip()


def _resolve(value: str) -> str:
    """A cleaned path; a folder that holds the program stands for the program."""
    s = normalise_exe(value)
    if s and Path(s).is_dir():
        for name in ("claude.exe", "claude"):
            if (Path(s) / name).is_file():
                return str(Path(s) / name)
    return s


def find_claude() -> str:
    """The program on PATH, else the newest copy bundled with the Claude desktop app (Windows)."""
    appdata = os.environ.get("APPDATA")
    if appdata:
        found = sorted(Path(appdata, "Claude", "claude-code").glob("*/*/claude.exe"),
                       key=lambda p: p.stat().st_mtime, reverse=True)
        if found:
            return str(found[0])
    return "claude"


def _flatten(messages: list[Message]) -> str:
    parts = []
    for m in messages:
        if m.role == "user":
            parts.append(f"[User]\n{m.content}")
        elif m.role == "assistant":
            calls = "".join(f"\n(called tool {t.name} with {json.dumps(t.arguments)})" for t in m.tool_calls)
            parts.append(f"[Assistant]\n{m.content}{calls}")
        elif m.role == "tool":
            parts.append(f"[Tool result]\n{m.content}")
    return "\n\n".join(parts)


def _classify(text: str) -> ProviderError:
    low = text.lower()
    if "/login" in low or "not logged in" in low or "invalid api key" in low or "authenticat" in low:
        return AuthError(f"Claude login needed: run `claude` once and sign in. ({text[:200]})")
    if "limit" in low or "rate" in low or "overloaded" in low:
        return RateLimited(text[:300])
    return ServerError(text[:300])


class ClaudeCliProvider(LLMProvider):
    def __init__(self, base_url: str = "", api_key: str = "", *, runner: Callable | None = None,
                 timeout: float = 300.0, **_):
        self.exe = _resolve(base_url) or find_claude()
        self.run, self.timeout = runner or _default_runner, timeout
        self.base_url, self.api_key, self.max_retries, self.backoff = self.exe, "", 0, 0

    # the HTTP template methods do not apply
    def _request(self, *a, **k):  # pragma: no cover
        raise NotImplementedError

    def _parse(self, data):  # pragma: no cover
        raise NotImplementedError

    def _parse_stream_line(self, event, data):  # pragma: no cover
        return None

    def complete(self, model: str, messages: list[Message], tools: list[ToolSpec] | None = None,
                 max_tokens: int = 1024, temperature: float | None = None) -> Completion:
        system = "\n\n".join(m.content for m in messages if m.role == "system")
        if tools:
            system = (system + "\n\n" if system else "") + TOOL_RULES + "\n".join(
                f"- {t.name}: {t.description} Parameters: {json.dumps(t.parameters)}" for t in tools)
        argv = [self.exe, "-p", "--output-format", "json", "--tools", "", "--model", model,
                "--system-prompt", system, "--no-session-persistence"]
        env = {k: v for k, v in os.environ.items() if k != "ANTHROPIC_API_KEY"}
        try:
            code, out, err = self.run(argv, _flatten(messages) + (REMINDER if tools else ""), env, self.timeout)
        except (TimeoutError, subprocess.TimeoutExpired) as e:
            raise NetworkError(f"claude did not answer in time: {e}") from e
        except FileNotFoundError as e:
            raise NetworkError(f"Program not found: {self.exe}. Leave this empty to auto-detect, "
                               "or paste the full path to claude.exe.") from e
        except OSError as e:
            raise NetworkError(f"could not run claude ({self.exe}): {e}") from e
        try:
            data = json.loads(out)
        except ValueError:
            raise _classify(err or out or "claude returned no output")
        if code != 0 or data.get("is_error"):
            raise _classify(str(data.get("result") or err))
        text = str(data.get("result") or "")
        calls: list[ToolCall] = []
        if tools:
            text, calls = self._extract_calls(text, {t.name for t in tools})
        u = data.get("usage") or {}
        return Completion(text, calls, Usage(u.get("input_tokens", 0), u.get("output_tokens", 0)),
                          "tool_use" if calls else "end_turn", model)

    @staticmethod
    def _extract_calls(text: str, allowed: set[str]) -> tuple[str, list[ToolCall]]:
        m = FENCE.search(text)
        blob = m.group(1) if m else text.strip()
        try:
            obj = json.loads(blob)
            raw = obj["tool_calls"]
        except (ValueError, KeyError, TypeError):
            return text, []
        calls = [ToolCall(f"call_{uuid.uuid4().hex[:12]}", c["name"], c.get("arguments") or {})
                 for c in raw if isinstance(c, dict) and c.get("name") in allowed]
        if not calls:
            return text, []
        rest = (text.replace(m.group(0), "") if m else "").strip()
        return rest, calls

    def stream(self, model, messages, tools=None, max_tokens=1024, temperature=None) -> Iterator[str]:
        yield self.complete(model, messages, tools, max_tokens, temperature).text
