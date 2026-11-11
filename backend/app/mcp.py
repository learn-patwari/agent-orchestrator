"""MCP client (FR-44): stdio and HTTP transports, per-role allow-lists, secrets by reference.

A server is configured by name with the roles that may use it. A role sees a server's tools (named
`mcp__<server>__<tool>`) only if the server lists that role; the app works with zero servers, and one server that
fails to start never removes an agent's other tools. Secrets are never stored: `env_refs` maps a variable the server
needs to the name of an environment variable on this machine, resolved only when the server is started. Whatever a
tool returns is untrusted data (the run loop wraps it like every tool result).
"""
from __future__ import annotations

import json
import os
import queue
import re
import subprocess
import threading
from urllib.parse import urlparse

import httpx

from . import db
from .providers.base import ToolSpec
from .runtime.tools import ToolError

TIMEOUT = 20.0
NAME = re.compile(r"^[a-z0-9][a-z0-9_-]*$", re.I)
servers_repo = db.DocRepo("mcp_servers")


class McpError(Exception):
    pass


class StdioTransport:
    def __init__(self, command: list[str], env: dict | None = None):
        self.command, self.env = command, env or {}
        self.proc: subprocess.Popen | None = None
        self.lines: queue.Queue = queue.Queue()
        self._id = 0

    def _start(self) -> None:
        try:
            self.proc = subprocess.Popen(self.command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=subprocess.DEVNULL, text=True, encoding="utf-8", bufsize=1,
                                         env={**os.environ, **self.env})
        except (OSError, ValueError) as e:
            raise McpError(f"could not start the server: {e}")
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self) -> None:
        for line in self.proc.stdout:
            try:
                self.lines.put(json.loads(line))
            except ValueError:
                continue  # not JSON-RPC: ignore stray output
        self.lines.put(None)  # the server closed its output

    def _send(self, msg: dict) -> None:
        if self.proc is None:
            self._start()
        try:
            self.proc.stdin.write(json.dumps(msg) + "\n")
            self.proc.stdin.flush()
        except OSError as e:
            raise McpError(f"the server stopped: {e}")

    def notify(self, method: str, params: dict | None = None) -> None:
        self._send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def request(self, method: str, params: dict | None = None) -> dict:
        self._id += 1
        self._send({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params or {}})
        while True:
            try:
                msg = self.lines.get(timeout=TIMEOUT)
            except queue.Empty:
                raise McpError(f"the server did not answer {method} in {TIMEOUT:.0f} s")
            if msg is None:
                raise McpError("the server closed the connection")
            if msg.get("id") == self._id:
                if "error" in msg:
                    raise McpError(str(msg["error"].get("message", msg["error"])))
                return msg.get("result", {})

    def close(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait(timeout=5)


class HttpTransport:
    def __init__(self, url: str, client: httpx.Client | None = None, headers: dict | None = None):
        self.url, self.client, self.headers = url, client or httpx.Client(), headers or {}
        self._id = 0

    def notify(self, method: str, params: dict | None = None) -> None:
        try:
            self.client.post(self.url, json={"jsonrpc": "2.0", "method": method, "params": params or {}},
                             headers=self.headers, timeout=TIMEOUT)
        except httpx.HTTPError:
            pass

    def request(self, method: str, params: dict | None = None) -> dict:
        self._id += 1
        try:
            r = self.client.post(self.url, json={"jsonrpc": "2.0", "id": self._id, "method": method,
                                                 "params": params or {}}, headers=self.headers, timeout=TIMEOUT)
            r.raise_for_status()
            msg = r.json()
        except (httpx.HTTPError, ValueError) as e:
            raise McpError(f"the server did not answer {method}: {e}")
        if "error" in msg:
            raise McpError(str(msg["error"].get("message", msg["error"])))
        return msg.get("result", {})

    def close(self) -> None:
        pass


class McpClient:
    def __init__(self, transport):
        self.t = transport

    def initialize(self) -> dict:
        out = self.t.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                            "clientInfo": {"name": "agent-orchestrator", "version": "1"}})
        self.t.notify("notifications/initialized")
        return out

    def list_tools(self) -> list[dict]:
        return self.t.request("tools/list").get("tools", [])

    def call_tool(self, name: str, args: dict) -> str:
        res = self.t.request("tools/call", {"name": name, "arguments": args})
        text = "\n".join(b.get("text", "") for b in res.get("content", []) if b.get("type") == "text")
        if res.get("isError"):
            raise McpError(text or "the tool reported an error")
        return text

    def close(self) -> None:
        self.t.close()


class McpServers:
    def list(self) -> list[dict]:
        return servers_repo.list()

    def get(self, name: str) -> dict | None:
        return servers_repo.get(name)

    def save(self, name: str, cfg: dict) -> dict:
        if not NAME.match(name or "") or "__" in name:
            raise McpError("a server name uses letters, digits, - and _ (no double underscore)")
        transport = cfg.get("transport")
        if transport == "stdio":
            cmd = cfg.get("command")
            if not isinstance(cmd, list) or not cmd or not all(isinstance(c, str) and c for c in cmd):
                raise McpError("stdio needs a command: a non-empty list of strings")
        elif transport == "http":
            u = urlparse(cfg.get("url") or "")
            if u.scheme not in ("http", "https") or not u.netloc:
                raise McpError("http needs an http or https url")
        else:
            raise McpError("transport must be stdio or http")
        doc = {"name": name, "transport": transport, "command": cfg.get("command"), "url": cfg.get("url"),
               "roles": [str(r) for r in cfg.get("roles", [])], "enabled": cfg.get("enabled", True),
               "env_refs": {str(k): str(v) for k, v in (cfg.get("env_refs") or {}).items()}}
        if servers_repo.get(name):
            return servers_repo.update(name, doc)
        return servers_repo.create(doc, id=name)

    def delete(self, name: str) -> bool:
        if not servers_repo.get(name):
            return False
        servers_repo.delete(name)
        return True


def connect(server: dict, client: httpx.Client | None = None) -> McpClient:
    """Start a configured server and return an initialised client."""
    if server["transport"] == "stdio":
        env = {}
        for var, ref in server.get("env_refs", {}).items():
            if ref not in os.environ:
                raise McpError(f"environment variable {ref} is not set on this machine")
            env[var] = os.environ[ref]
        t = StdioTransport(server["command"], env)
    else:
        t = HttpTransport(server["url"], client)
    c = McpClient(t)
    try:
        c.initialize()
    except McpError:
        c.close()
        raise
    return c


class McpTools:
    """The MCP tools one role may use in one run. Connects lazily; `close()` stops the servers it started."""

    def __init__(self, role: str):
        self.role = role
        self.clients: dict[str, McpClient] = {}
        self.problems: dict[str, str] = {}
        self._specs: list[ToolSpec] | None = None

    def _allowed(self) -> list[dict]:
        return [s for s in servers_repo.list() if s.get("enabled", True) and self.role in s.get("roles", [])]

    def _client(self, server: dict) -> McpClient:
        if server["name"] not in self.clients:
            self.clients[server["name"]] = connect(server)
        return self.clients[server["name"]]

    def specs(self) -> list[ToolSpec]:
        if self._specs is None:
            out = []
            for s in self._allowed():
                try:
                    for t in self._client(s).list_tools():
                        out.append(ToolSpec(f"mcp__{s['name']}__{t['name']}", t.get("description", ""),
                                            t.get("inputSchema") or {"type": "object", "properties": {}}))
                except McpError as e:
                    self.problems[s["name"]] = str(e)
            self._specs = out
        return self._specs

    def call(self, name: str, args: dict) -> str:
        parts = name.split("__", 2)
        if len(parts) != 3:
            raise ToolError(f"unknown MCP tool {name}")
        server = servers_repo.get(parts[1])
        if server is None:
            raise ToolError(f"unknown MCP server {parts[1]}")
        if not server.get("enabled", True) or self.role not in server.get("roles", []):
            raise ToolError(f"MCP server {parts[1]} is not allowed for the {self.role} role")
        try:
            return self._client(server).call_tool(parts[2], args)
        except McpError as e:
            raise ToolError(f"MCP tool failed: {e}")

    def close(self) -> None:
        for c in self.clients.values():
            c.close()
        self.clients.clear()
