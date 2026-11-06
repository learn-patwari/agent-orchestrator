"""Dev -> test -> review state machine per task (FR-35, FR-36).

developing -> dev_done -> tested -> approved -> merged
tests fail          -> developing
REQUEST_CHANGES     -> changes_requested -> (author pushes) dev_done ... ; at the round limit -> escalated
HOLD                -> held
Review text is read with the gate's own parser (first-line Verdict, second-line Head). A review of an older
head is refused, so it can never count as approval.
"""
from __future__ import annotations

import time

from .. import db
from ..features import NotApproved, require_approved
from ..inbox import Inbox
from .github import parse_review
from .worktrees import WorktreeManager

tasks = db.DocRepo("tasks")


class FlowError(Exception):
    pass


class Flow:
    def __init__(self, wt: WorktreeManager, integration: str = "internal", review_round_limit: int = 3):
        self.wt, self.integration, self.limit = wt, integration, review_round_limit

    def _get(self, tid: str, *states: str) -> dict:
        t = tasks.get(tid)
        if t is None:
            raise FlowError(f"unknown task {tid}")
        if states and t["status"] not in states:
            raise FlowError(f"task is in state {t['status']}, expected {' or '.join(states)}")
        return t

    def start(self, feature_id: str, name: str, stacked_on: str | None = None) -> dict:
        try:
            require_approved(feature_id)
        except NotApproved:
            raise FlowError("the feature's SRS must be approved before development starts")
        base = self.integration
        if stacked_on:
            base = self._get(stacked_on)["branch"]
        w = self.wt.create(name, base)
        return tasks.create({"feature_id": feature_id, "name": name, "branch": w.branch, "worktree": str(w.path),
                             "base": base, "status": "developing", "rounds": 0, "tests": None, "pr_number": None,
                             "head_sha": None, "created_at": time.time()})

    def dev_done(self, tid: str, pr_number: int, head_sha: str) -> dict:
        t = self._get(tid, "developing", "changes_requested")
        return tasks.update(tid, {"status": "dev_done", "pr_number": pr_number, "head_sha": head_sha, "tests": None})

    def record_tests(self, tid: str, passed: int, failed: int, area: str) -> dict:
        t = self._get(tid, "dev_done")
        if not t["pr_number"]:
            raise FlowError("task state has no PR yet")
        res = {"passed": passed, "failed": failed, "area": area}
        return tasks.update(tid, {"tests": res, "status": "tested" if failed == 0 and passed > 0 else "developing"})

    def record_review(self, tid: str, body: str) -> dict:
        t = self._get(tid, "tested")
        r = parse_review(body, time.time())
        if r is None:
            raise FlowError("review needs a first-line verdict: Verdict: APPROVE|REQUEST_CHANGES|HOLD")
        if r["head_sha"] != t["head_sha"]:
            raise FlowError("review is not for the current head commit")
        if r["verdict"] == "APPROVE":
            return tasks.update(tid, {"status": "approved"})
        if r["verdict"] == "HOLD":
            Inbox().post("status", f"Task {t['name']} is on hold", "The reviewer put this PR on hold.")
            return tasks.update(tid, {"status": "held"})
        rounds = t["rounds"] + 1
        if rounds >= self.limit:
            Inbox().post("status", f"Task {t['name']} needs you",
                         f"{rounds} review rounds reached the limit of {self.limit}; automatic rounds stopped.")
            return tasks.update(tid, {"status": "escalated", "rounds": rounds})
        return tasks.update(tid, {"status": "changes_requested", "rounds": rounds})

    def tests_for_pr(self, pr_number: int) -> dict | None:
        t = next((x for x in tasks.list() if x["pr_number"] == pr_number), None)
        return t["tests"] if t else None

    def mark_merged(self, pr_number: int) -> dict | None:
        t = next((x for x in tasks.list() if x["pr_number"] == pr_number), None)
        return tasks.update(t["id"], {"status": "merged"}) if t else None
