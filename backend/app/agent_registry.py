"""Registry of external agents (FR-59), like a service registry.

An external agent registers itself (name, endpoint, what it can do) with the registry key the human gave it, and
keeps sending heartbeats with its own secret token. It is `pending` until the human approves it; an agent with no
heartbeat inside TTL seconds is not live. Tasks go only to an approved, live agent and only while the
external-providers feature flag is on (D-2). What an external agent replies is untrusted data.
"""
from __future__ import annotations

import hashlib
import hmac
import re
import secrets
import time
from typing import Callable
from urllib.parse import urlparse

from . import db, secrets_store
from .providers import registry as providers

TTL = 60.0  # seconds without a heartbeat before an agent is no longer live
MAX_REPLY = 4000
agents = db.DocRepo("ext_agents")


class RegistryError(Exception):
    pass


def _hash(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()


def _slug(name: str) -> str:
    return re.sub(r"[^a-z0-9_-]+", "-", name.strip().lower()).strip("-")


def registry_key() -> str:
    key = db.kv_get("registry_key", None)
    if not key:
        key = secrets.token_urlsafe(24)
        db.kv_set("registry_key", key)
    return key


def key_ok(candidate: str | None) -> bool:
    return bool(candidate) and hmac.compare_digest(candidate, registry_key())


def _public(a: dict, now: float) -> dict:
    live = a["status"] == "approved" and a.get("last_seen") is not None and now - a["last_seen"] <= TTL
    return {k: a.get(k) for k in ("name", "endpoint", "capabilities", "description", "status", "last_seen")} | {"live": live}


class AgentRegistry:
    def _get(self, name: str) -> dict:
        a = agents.get(_slug(name))
        if a is None:
            raise RegistryError(f"unknown agent {name}")
        return a

    def register(self, name: str, endpoint: str, capabilities: list[str], description: str = "",
                 token: str | None = None) -> dict:
        slug = _slug(name)
        if not slug:
            raise RegistryError("an agent needs a name")
        u = urlparse(endpoint or "")
        if u.scheme not in ("http", "https") or not u.netloc:
            raise RegistryError("the endpoint must be an http or https URL")
        caps = [str(c)[:80] for c in (capabilities or [])][:20]
        fields = {"name": name.strip(), "endpoint": endpoint, "capabilities": caps, "description": description[:300]}
        existing = agents.get(slug)
        if existing:
            if not token or not hmac.compare_digest(existing["token_hash"], _hash(token)):
                raise RegistryError("that name is taken; send the agent's token to update it")
            doc = agents.update(slug, fields)
            return {"agent": _public(doc, time.time()), "token": None}
        secret = secrets.token_urlsafe(24)
        doc = agents.create({**fields, "status": "pending", "token_hash": _hash(secret), "last_seen": None}, id=slug)
        return {"agent": _public(doc, time.time()), "token": secret}

    def heartbeat(self, name: str, token: str, now: float | None = None) -> None:
        a = self._get(name)
        if not hmac.compare_digest(a["token_hash"], _hash(token or "")):
            raise RegistryError("wrong token")
        agents.update(a["id"], {"last_seen": now if now is not None else time.time()})

    def approve(self, name: str) -> dict:
        a = self._get(name)
        return _public(agents.update(a["id"], {"status": "approved"}), time.time())

    def reject(self, name: str) -> dict:
        a = self._get(name)
        return _public(agents.update(a["id"], {"status": "rejected"}), time.time())

    def list(self, now: float | None = None) -> list[dict]:
        now = now if now is not None else time.time()
        return [_public(a, now) for a in agents.list()]

    def live(self, now: float | None = None) -> list[dict]:
        return [a for a in self.list(now) if a["live"]]

    def send_task(self, name: str, task: str, post: Callable[[str, dict], str], now: float | None = None) -> dict:
        a = self._get(name)
        if a["status"] != "approved":
            raise RegistryError(f"{a['name']} is not approved by the human")
        if not providers.external_enabled():
            raise RegistryError("sending work to external agents is off: turn on the external providers flag")
        if not any(x["name"] == a["name"] for x in self.live(now)):
            raise RegistryError(f"{a['name']} is not live (no recent heartbeat)")
        reply = post(a["endpoint"], {"task": task})
        return {"agent": a["name"], "reply": secrets_store.redact(str(reply))[:MAX_REPLY]}
