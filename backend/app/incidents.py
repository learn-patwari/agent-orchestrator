"""Incidents handled by production support, levels L1 to L4.

A failing production test report raises an incident (the incident reporter's job), once: the same open title is
never reported twice. Production support starts at L1 and escalates one level at a time: L2 needs a code change,
L3 is the architect's call, L4 asks the human and is the last level. A fix is handed to the developer who owns
the feature and is reviewed through the normal pipeline. Every hand-off is recorded for the office view.
"""
from __future__ import annotations

import json
import time

from . import db
from .inbox import Inbox
from .pipeline import handoffs
from .runtime.tools import ToolError

SEVERITIES = ("low", "medium", "high", "critical")
MAX_LEVEL = 4
incidents = db.DocRepo("incidents")


class IncidentError(Exception):
    pass


def _key(title: str) -> str:
    return " ".join(title.lower().split())


class Incidents:
    def list(self, include_resolved: bool = True) -> list[dict]:
        rows = incidents.list()
        return rows if include_resolved else [r for r in rows if r["status"] != "resolved"]

    def get(self, iid: str) -> dict:
        i = incidents.get(iid)
        if i is None:
            raise IncidentError(f"unknown incident {iid}")
        return i

    def report(self, title: str, severity: str, evidence: str, source: str = "prod",
               task_id: str | None = None) -> dict:
        if severity not in SEVERITIES:
            raise IncidentError(f"severity must be one of {', '.join(SEVERITIES)}")
        if not title.strip():
            raise IncidentError("an incident needs a title")
        same = next((i for i in self.list(False) if _key(i["title"]) == _key(title)), None)
        if same:
            return same
        inc = incidents.create({"title": title.strip(), "severity": severity, "evidence": evidence, "source": source,
                                "task_id": task_id, "level": 1, "status": "open", "history": [],
                                "fix_owner": None, "created_at": time.time()})
        handoffs.record("incident_reporter", "production_support", f"incident: {inc['title'][:40]}")
        return inc

    def production_tests(self, task_id: str, passed: int, failed: int, area: str) -> dict | None:
        t = db.DocRepo("tasks").get(task_id)
        if t is None:
            raise IncidentError(f"unknown task {task_id}")
        if failed <= 0:
            return None
        return self.report(f"Production tests failing: {t['name']} / {area}", "high",
                           f"{failed} failing, {passed} passing in {area}", source="prod", task_id=task_id)

    def _open(self, iid: str) -> dict:
        i = self.get(iid)
        if i["status"] == "resolved":
            raise IncidentError("this incident is already resolved")
        return i

    def escalate(self, iid: str, reason: str) -> dict:
        i = self._open(iid)
        if i["level"] >= MAX_LEVEL:
            raise IncidentError("this incident is already at L4, the last level")
        level = i["level"] + 1
        history = [*i["history"], {"level": level, "reason": reason, "at": time.time()}]
        if level == 3:
            handoffs.record("production_support", "architect", f"L3: {i['title'][:30]}")
        if level == MAX_LEVEL:
            Inbox().post("status", f"Incident needs you: {i['title']}", f"Escalated to L4. {reason}")
        return incidents.update(iid, {"level": level, "history": history})

    def hand_to_developer(self, iid: str) -> dict:
        i = self._open(iid)
        t = db.DocRepo("tasks").get(i["task_id"]) if i.get("task_id") else None
        if t is None:
            raise IncidentError("a fix needs the task that owns the feature; this incident has none")
        handoffs.record("production_support", t["owner"], f"fix: {i['title'][:36]}")
        return incidents.update(iid, {"status": "fixing", "fix_owner": t["owner"]})

    def resolve(self, iid: str) -> dict:
        self._open(iid)
        return incidents.update(iid, {"status": "resolved", "resolved_at": time.time()})


class IncidentTool:
    """The `incident` tool. The reporter reports; production support escalates, hands fixes on and resolves."""

    SUPPORT = ("escalate", "hand_to_developer", "resolve")

    def __init__(self, actor: str):
        self.actor = actor

    def __call__(self, action: str, title: str = "", severity: str = "", evidence: str = "", task_id: str | None = None,
                 incident_id: str | None = None, reason: str = "") -> str:
        inc = Incidents()
        try:
            if action == "list":
                return json.dumps([_row(i) for i in inc.list(False)])
            if action == "report":
                self._need("incident_reporter")
                return json.dumps(_row(inc.report(title, severity, evidence, "prod", task_id)))
            if action in self.SUPPORT:
                self._need("production_support")
                if not incident_id:
                    raise ToolError("incident_id is required")
                if action == "escalate":
                    return json.dumps(_row(inc.escalate(incident_id, reason)))
                if action == "resolve":
                    return json.dumps(_row(inc.resolve(incident_id)))
                return json.dumps(_row(inc.hand_to_developer(incident_id)))
        except IncidentError as e:
            raise ToolError(str(e))
        raise ToolError(f"unknown incident action {action}; use list, report, escalate, hand_to_developer or resolve")

    def _need(self, role: str) -> None:
        if self.actor != role:
            raise ToolError(f"only the {role} role can do this step")


def _row(i: dict) -> dict:
    return {k: i.get(k) for k in ("id", "title", "severity", "level", "status", "task_id", "fix_owner")}
