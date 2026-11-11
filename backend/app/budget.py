"""Token limit per feature (FR-60).

A feature may carry a token limit. Its runs are counted against it; when the limit is reached the feature is held:
running work stops at its next step and new runs for it are refused. The app cannot read a provider's quota window
(for example a 5-hour Claude window), so the human says when it clears: set `resume_at`, or press Resume. Resuming
starts a fresh allowance (the same limit again, counted from now) and the held runs continue where they stopped.
"""
from __future__ import annotations

import time

from . import db
from .notify import Notifier
from .scheduler import Scheduler

WINDOW_STOP = 98.0  # percent of the 5-hour window at which new work stops

features = db.DocRepo("features")
runs = db.DocRepo("runs")


class Budget:
    def _feature(self, fid: str) -> dict:
        f = features.get(fid)
        if f is None:
            raise ValueError(f"unknown feature {fid}")
        return f

    def _total(self, fid: str) -> int:
        return sum(r.get("tokens") or 0 for r in runs.list() if r.get("feature_id") == fid)

    def _used(self, f: dict) -> int:
        return max(0, self._total(f["id"]) - (f.get("token_baseline") or 0))

    def set(self, fid: str, limit: int | None, resume_at: float | None = None) -> dict:
        f = self._feature(fid)
        if limit is not None and limit <= 0:
            raise ValueError("the token limit must be above zero")
        patch = {"token_limit": limit, "resume_at": resume_at}
        if limit is None or self._used(f) < limit:
            patch["held"] = False  # a raised or removed limit lifts the hold
        return features.update(fid, patch)

    def status(self, fid: str) -> dict:
        f = self._feature(fid)
        limit, used = f.get("token_limit"), self._used(f)
        return {"limit": limit, "used": used, "remaining": None if limit is None else max(0, limit - used),
                "held": bool(f.get("held")), "resume_at": f.get("resume_at")}

    def gate(self, fid: str | None) -> str | None:
        """None when work may go on; otherwise the reason it is held (and the feature is marked held)."""
        f = features.get(fid) if fid else None
        if f is None or not f.get("token_limit"):
            return None
        used, limit = self._used(f), f["token_limit"]
        if not f.get("held") and used < limit:
            return None
        if not f.get("held"):
            features.update(fid, {"held": True, "held_at": time.time()})
            Notifier().send("budget_hold", f"Feature held: {f.get('title', fid)}",
                            f"Token limit reached ({used} of {limit}). Work resumes when you press Resume"
                            + (" or at the time you set." if f.get("resume_at") else "."), key=f"hold:{fid}")
        return f"held: token limit reached ({used} of {limit} tokens)"

    def resume(self, fid: str) -> dict:
        f = self._feature(fid)
        features.update(fid, {"held": False, "resume_at": None, "token_baseline": self._total(fid)})
        for n in Notifier().unread():
            if n.get("key") == f"hold:{fid}":
                Notifier().mark_read(n["id"])
        return self.status(f["id"])

    def daily(self, now: float, limit: int | None) -> dict:
        """Tokens used in the last 24 hours across everything (a rolling window), against an optional limit."""
        used = sum((c.get("input_tokens") or 0) + (c.get("output_tokens") or 0)
                   for c in db.DocRepo("llm_calls").list() if now - c.get("at", 0) < 86_400)
        return {"limit": limit, "used": used, "percent": None if not limit else round(used / limit * 100, 2)}

    def daily_gate(self, now: float, limit: int | None) -> str | None:
        d = self.daily(now, limit)
        if limit and d["used"] >= limit:
            return f"daily token limit reached ({d['used']} of {limit} tokens in the last 24 hours)"
        return None

    def overview(self, now: float, daily_limit: int | None) -> dict:
        feats = []
        for f in features.list():
            s = self.status(f["id"])
            feats.append({"id": f["id"], "title": f.get("title", f["id"]), **s})
        return {"daily": self.daily(now, daily_limit), "features": feats}

    def set_window(self, percent: float, resets_at: float | None) -> dict:
        """The person enters how full the provider's window is (the app cannot read it) and when it resets."""
        s = db.kv_get("usage_window", {})
        s.update({"percent": percent, "resets_at": resets_at})
        if percent >= WINDOW_STOP and resets_at and s.get("scheduled_for") != resets_at:
            Scheduler().add("Usage window reset: work resumes",
                            {"kind": "reminder", "text": "The usage window has reset; held work continues."}, resets_at)
            s["scheduled_for"] = resets_at  # one resume per window, however often the figure is updated
        db.kv_set("usage_window", s)
        return self.window(time.time())

    def window(self, now: float) -> dict:
        s = db.kv_get("usage_window", {})
        return {"percent": s.get("percent"), "resets_at": s.get("resets_at"), "blocked": self.window_gate(now) is not None}

    def window_gate(self, now: float) -> str | None:
        s = db.kv_get("usage_window", {})
        pct, reset = s.get("percent"), s.get("resets_at")
        if pct is None or pct < WINDOW_STOP or (reset is not None and now >= reset):
            return None
        when = time.strftime("%H:%M", time.localtime(reset)) if reset else "the reset"
        return f"usage window at {pct:g}% (stop at {WINDOW_STOP:g}%); work resumes after the reset at {when}"

    def tick(self, now: float) -> list[str]:
        """Resume the held features whose resume time has come. Returns their ids."""
        due = [f["id"] for f in features.list() if f.get("held") and f.get("resume_at") and f["resume_at"] <= now]
        for fid in due:
            self.resume(fid)
        return due
