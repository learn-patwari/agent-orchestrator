"""Generic platform quota guard (FR-40).

Counts usage in rolling windows against a per-provider limits table and stops at a threshold (default 70%),
telling the human once. A limit error from the platform stops everything until the window can have reset;
nothing here retries. Vercel Hobby is seeded (docs checked 2026-11-05); other platforms bring their own table.
"""
from __future__ import annotations

import time

from . import db
from .notify import Notifier

DAY = 86400
HOUR = 3600
PROVIDER_LIMITS = {
    "vercel_hobby": {
        "deployments_per_day": {"limit": 100, "window_s": DAY},
        "deployments_per_hour": {"limit": 100, "window_s": HOUR},
        "concurrent_builds": {"limit": 1, "window_s": 0},
    },
}


class QuotaBlocked(Exception):
    pass


class QuotaGuard:
    def __init__(self, provider: str, stop_at_percent: float = 70.0, limits: dict | None = None):
        table = limits if limits is not None else PROVIDER_LIMITS
        if provider not in table:
            raise ValueError(f"unknown platform {provider}")
        self.provider, self.limits, self.stop_at = provider, table[provider], stop_at_percent
        self._key = f"quota:{provider}"

    def _state(self) -> dict:
        return db.kv_get(self._key, {"events": {}, "stopped_until": 0})

    def record(self, metric: str, at: float | None = None) -> None:
        self._metric(metric)
        s = self._state()
        s["events"].setdefault(metric, []).append(at if at is not None else time.time())
        db.kv_set(self._key, s)

    def _metric(self, metric: str) -> dict:
        if metric not in self.limits:
            raise ValueError(f"unknown metric {metric} for {self.provider}")
        return self.limits[metric]

    def _used(self, s: dict, metric: str, now: float) -> int:
        w = self.limits[metric]["window_s"]
        return sum(1 for t in s["events"].get(metric, []) if now - t < w)

    def usage(self, now: float | None = None) -> dict:
        now = now if now is not None else time.time()
        s = self._state()
        out = {}
        for m, spec in self.limits.items():
            if spec["window_s"]:
                used = self._used(s, m, now)
                out[m] = {"used": used, "limit": spec["limit"], "percent": round(used / spec["limit"] * 100, 2)}
        return out

    def _block(self, metric: str, msg: str) -> None:
        Notifier().send("quota_block", f"{self.provider} quota guard stopped work",
                        f"{metric}: {msg}", key=f"quota:{self.provider}:{metric}")
        raise QuotaBlocked(msg)

    def check(self, metric: str, now: float | None = None, in_flight: int = 0) -> None:
        spec = self._metric(metric)
        now = now if now is not None else time.time()
        s = self._state()
        if now < s["stopped_until"]:
            self._block(metric, "the platform returned a limit error; stopped until the window can have reset "
                                "(no retries)")
        if not spec["window_s"]:  # concurrency limit
            if in_flight >= spec["limit"]:
                self._block(metric, f"{in_flight} in flight, limit {spec['limit']}")
            return
        used = self._used(s, metric, now)
        pct = used / spec["limit"] * 100
        if pct >= self.stop_at:
            self._block(metric, f"{used} of {spec['limit']} used ({pct:.0f}%), stop threshold is "
                                f"{self.stop_at:.0f}%")

    def limit_hit(self, error_code: str, now: float | None = None) -> None:
        """The platform said no (e.g. api-deployments-free-per-day): stop for a full day, do not retry."""
        now = now if now is not None else time.time()
        s = self._state()
        s["stopped_until"] = now + DAY
        db.kv_set(self._key, s)
        Notifier().send("quota_block", f"{self.provider} limit error: {error_code}",
                        "Work that needs the platform is stopped and will not retry.",
                        key=f"quota:{self.provider}:limit")
