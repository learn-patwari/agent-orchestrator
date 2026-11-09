"""Release stages after peer review (docs/design/team-workflow.md).

peer approved -> architect review -> deploy to QC -> QC result -> request production
-> human AND architect approve -> deployed to production.
Each step is a real signal from the role that owns it, is refused out of order, and records a hand-off.
A failure sends the task back to its owning developer. Nothing reaches production without the human's yes.
"""
from __future__ import annotations

import time

from .. import db
from ..inbox import Inbox
from . import handoffs
from ..quota import QuotaBlocked, QuotaGuard
from .docs_gate import DocsGate
from .flow import FlowError, tasks

APPROVERS = ("human", "architect")


class Release:
    def _task(self, tid: str, *releases) -> dict:
        t = tasks.get(tid)
        if t is None:
            raise FlowError(f"unknown task {tid}")
        if releases and t.get("release") not in releases:
            raise FlowError(f"release stage is {t.get('release')}, expected {' or '.join(map(str, releases))}")
        if t["status"] != "approved" and t.get("release") is None:
            raise FlowError(f"task is in state {t['status']}, it must be peer approved first")
        return t

    def environments(self) -> dict:
        env = db.kv_get("environments", {})
        return {"qc": env.get("qc"), "prod": env.get("prod")}

    def _vercel(self) -> QuotaGuard:
        g = QuotaGuard("vercel_hobby")  # stops at 70% of the daily deployment limit, tells the human once
        try:
            g.check("deployments_per_day")
            g.check("deployments_per_hour")
        except QuotaBlocked as e:
            raise FlowError(f"Vercel limit guard: {e}")
        return g

    def _deploy(self, name: str, t: dict) -> None:
        g = self._vercel()  # a refused deploy changes nothing and is not counted
        g.record("deployments_per_day")
        g.record("deployments_per_hour")
        env = db.kv_get("environments", {})
        env[name] = {"task_id": t["id"], "name": t["name"], "version": (t.get("head_sha") or "")[:7],
                     "at": time.time()}
        db.kv_set("environments", env)

    def _back_to_owner(self, t: dict, sender: str, label: str) -> dict:
        handoffs.record(sender, t["owner"], label)
        return tasks.update(t["id"], {"status": "developing", "release": None, "approvals": [], "tests": None})

    def architect_review(self, tid: str, approve: bool) -> dict:
        t = self._task(tid, None)
        if not approve:
            return self._back_to_owner(t, "architect", "changes requested")
        handoffs.record("architect", "cicd", "deploy to QC")
        return tasks.update(tid, {"release": "architect_approved"})

    def deploy_qc(self, tid: str) -> dict:
        t = self._task(tid, "architect_approved")
        self._deploy("qc", t)
        handoffs.record("cicd", "tester", "ready for QC")
        return tasks.update(tid, {"release": "in_qc"})

    def qc_result(self, tid: str, passed: int, failed: int) -> dict:
        t = self._task(tid, "in_qc")
        if failed or passed <= 0:
            return self._back_to_owner(t, "tester", f"QC failed: {failed} failing")
        DocsGate().require(t)  # no sign-off before the docs gate is PASS (or the human's exception)
        handoffs.record("tester", "orchestrator", "QC signed off")
        return tasks.update(tid, {"release": "qc_passed", "qc": {"passed": passed, "failed": failed}})

    def request_prod(self, tid: str) -> dict:
        t = self._task(tid, "qc_passed")
        handoffs.record("orchestrator", "architect", "production sign-off")
        Inbox().post("approval", f"Deploy {t['name']} to production?",
                     "QC has signed off. Production needs your yes and the architect's sign-off.", task_id=tid)
        return tasks.update(tid, {"release": "awaiting_prod_approval", "approvals": []})

    def approve_prod(self, tid: str, by: str) -> dict:
        t = self._task(tid, "awaiting_prod_approval")
        if by not in APPROVERS:
            raise FlowError("only the human or the architect can approve production")
        got = sorted(set(t.get("approvals", [])) | {by})
        if got != sorted(APPROVERS):
            return tasks.update(tid, {"approvals": got})
        handoffs.record("architect", "cicd", "deploy to production")
        return tasks.update(tid, {"approvals": got, "release": "approved_for_prod"})

    def deployed_prod(self, tid: str) -> dict:
        t = self._task(tid, "approved_for_prod")
        self._deploy("prod", t)
        return tasks.update(tid, {"release": "in_prod"})
