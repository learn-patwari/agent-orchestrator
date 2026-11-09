"""Architecture profiles for token use (FR-61).

A profile sets how much an agent run may spend: steps, total tokens, output size per call, and how early the
context is compacted. Three ship (lean, balanced, thorough, read-only); the person can add their own and pick the
active one. The active profile gives every new run its limits. Nothing here changes a run already in flight.
"""
from __future__ import annotations

from . import db
from .runtime.runner import Limits

FIELDS = ("max_steps", "max_total_tokens", "max_output_tokens", "compact_at_chars", "keep_recent")
BUILTIN = {
    "lean": {"max_steps": 15, "max_total_tokens": 150_000, "max_output_tokens": 1024,
             "compact_at_chars": 20_000, "keep_recent": 4},
    "balanced": {"max_steps": 30, "max_total_tokens": 500_000, "max_output_tokens": 2048,
                 "compact_at_chars": 60_000, "keep_recent": 6},
    "thorough": {"max_steps": 60, "max_total_tokens": 1_500_000, "max_output_tokens": 4096,
                 "compact_at_chars": 120_000, "keep_recent": 10},
}
DEFAULT = "balanced"
KEY = "profiles"


class ProfileError(Exception):
    pass


class Profiles:
    def _state(self) -> dict:
        return db.kv_get(KEY, {"active": DEFAULT, "custom": {}})

    def _all(self) -> dict:
        return {**BUILTIN, **self._state()["custom"]}

    def get(self, name: str) -> dict:
        p = self._all().get(name)
        if p is None:
            raise ProfileError(f"unknown profile {name}")
        return {"name": name, **p, "builtin": name in BUILTIN, "est_run_tokens": p["max_total_tokens"]}

    def list(self) -> list[dict]:
        return [self.get(n) for n in self._all()]

    def active(self) -> dict:
        name = self._state()["active"]
        return self.get(name if name in self._all() else DEFAULT)

    def set_active(self, name: str) -> dict:
        self.get(name)
        s = self._state()
        db.kv_set(KEY, {**s, "active": name})
        return self.active()

    def save(self, name: str, values: dict) -> dict:
        name = (name or "").strip()
        if not name:
            raise ProfileError("a profile needs a name")
        if name in BUILTIN:
            raise ProfileError(f"{name} is a built-in profile; save your own under another name")
        if set(values) != set(FIELDS):
            raise ProfileError(f"a profile sets exactly: {', '.join(FIELDS)}")
        for k in FIELDS:
            if not isinstance(values[k], int) or isinstance(values[k], bool) or values[k] <= 0:
                raise ProfileError(f"{k} must be a whole number above zero")
        s = self._state()
        db.kv_set(KEY, {**s, "custom": {**s["custom"], name: {k: values[k] for k in FIELDS}}})
        return self.get(name)

    def delete(self, name: str) -> dict:
        if name in BUILTIN:
            raise ProfileError(f"{name} is a built-in profile and cannot be deleted")
        s = self._state()
        if name not in s["custom"]:
            raise ProfileError(f"unknown profile {name}")
        custom = {k: v for k, v in s["custom"].items() if k != name}
        db.kv_set(KEY, {"active": DEFAULT if s["active"] == name else s["active"], "custom": custom})
        return self.active()

    def limits(self) -> Limits:
        a = self.active()
        return Limits(max_steps=a["max_steps"], max_total_tokens=a["max_total_tokens"],
                      max_output_tokens=a["max_output_tokens"], compact_at_chars=a["compact_at_chars"],
                      keep_recent=a["keep_recent"])
