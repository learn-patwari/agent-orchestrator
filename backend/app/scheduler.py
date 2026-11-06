"""Local scheduler (FR-28): one-shot and recurring jobs, paused while the LLM budget is >= threshold.

Schedules are local to this app: they run only while the app is running (`tick` is driven by the
app's background loop). Nothing here promises cloud execution.
"""
from __future__ import annotations

import time
from typing import Callable

from . import db
from .notify import Notifier


def budget_percent_used(daily_cap_usd: float | None, now: float | None = None) -> float | None:
    """Share of today's (rolling 24h) cost cap already spent, from recorded LLM calls. None = no cap."""
    if not daily_cap_usd:
        return None
    since = (now or time.time()) - 86400
    spent = sum((c.get("cost_usd") or 0.0) for c in db.DocRepo("llm_calls").list() if c.get("at", 0) >= since)
    return round(spent / daily_cap_usd * 100, 4)


class Scheduler:
    def __init__(self, pause_at_percent: float = 80.0):
        self.repo = db.DocRepo("schedules")
        self.pause_at = pause_at_percent

    def add(self, name: str, payload: dict, run_at: float, every_s: float | None = None) -> dict:
        return self.repo.create({"name": name, "payload": payload, "run_at": run_at, "every_s": every_s,
                                 "status": "scheduled", "error": None})

    def list(self) -> list[dict]:
        return self.repo.list()

    def tick(self, now: float, budget_percent: float | None, run: Callable[[dict], None]) -> dict:
        notifier = Notifier()
        if budget_percent is not None and budget_percent >= self.pause_at:
            notifier.send("budget_pause", "LLM budget reached the pause threshold",
                          f"Used {budget_percent:.0f}% (pause at {self.pause_at:.0f}%). Scheduled work waits "
                          "and resumes automatically when usage drops.", key="budget_pause")
            return {"paused": True, "ran": []}
        for n in notifier.unread():  # the pause is over: clear the stale notice
            if n.get("key") == "budget_pause":
                notifier.mark_read(n["id"])
        ran = []
        for job in self.repo.list():
            if job["status"] != "scheduled" or job["run_at"] > now:
                continue
            try:
                run(job)
            except Exception as e:  # one bad job must not stop the others
                self.repo.update(job["id"], {"status": "failed", "error": str(e)})
                notifier.send("job_failed", f"Scheduled job {job['name']} failed", str(e), key=f"job:{job['id']}")
                continue
            ran.append(job)
            if job["every_s"]:  # next slot after now: a long gap runs once, not once per missed slot
                nxt = job["run_at"] + job["every_s"]
                while nxt <= now:
                    nxt += job["every_s"]
                self.repo.update(job["id"], {"run_at": nxt})
            else:
                self.repo.update(job["id"], {"status": "done"})
        return {"paused": False, "ran": ran}
