"""The `pipeline` tool: agents report their own step and the platform checks the role is allowed to.

Authority is enforced here, not in the prompt: only a task's owner reports its work done, only the other
developer reviews it, and each release step belongs to one role. The architect may sign off production
but can never stand in for the human, whose approval only comes from the UI.
"""
from __future__ import annotations

import json
from typing import Callable

from ..runtime.tools import ToolError
from .flow import Flow, FlowError, tasks
from .docs_gate import DocsGate, state as docs_state
from .release import Release

ACTIONS = ("list", "dev_done", "tests", "review", "architect_review", "deploy_qc", "qc_result",
           "approve_prod", "deploy_prod", "prod_tests", "docs_check")
RELEASE_OWNER = {"architect_review": "architect", "approve_prod": "architect", "deploy_qc": "cicd",
                 "deploy_prod": "cicd", "qc_result": "tester", "prod_tests": "tester", "docs_check": "cicd"}
FIELDS = ("id", "name", "owner", "reviewer", "status", "release", "approvals", "pr_number", "head_sha")


def summary(t: dict) -> dict:
    return {**{k: t.get(k) for k in FIELDS}, "docs_gate": docs_state(t) if t.get("docs_gate") else None}


class PipelineTool:
    def __init__(self, actor: str, flow: Callable[[], Flow]):
        self.actor, self.flow = actor, flow

    def __call__(self, action: str, task_id: str | None = None, pr_number: int | None = None,
                 head_sha: str | None = None, passed: int | None = None, failed: int | None = None,
                 area: str = "", body: str = "", approve: bool | None = None, version: str = "",
                 endpoints: list | None = None, flagged: list | None = None, gaps: list | None = None) -> str:
        if action not in ACTIONS:
            raise ToolError(f"unknown pipeline action {action}; use one of {', '.join(ACTIONS)}")
        if action == "list":
            return json.dumps([summary(t) for t in tasks.list()])
        if not task_id:
            raise ToolError("task_id is required")
        t = tasks.get(task_id)
        if t is None:
            raise ToolError(f"unknown task {task_id}")
        try:
            return json.dumps(summary(self._act(action, t, pr_number, head_sha, passed, failed, area, body, approve,
                                                 version, endpoints, flagged, gaps)))
        except FlowError as e:
            raise ToolError(str(e))

    def _need(self, role: str) -> None:
        if self.actor != role:
            raise ToolError(f"only the {role} role can do this step")

    def _act(self, action, t, pr_number, head_sha, passed, failed, area, body, approve, version="",
             endpoints=None, flagged=None, gaps=None) -> dict:
        tid = t["id"]
        if action in RELEASE_OWNER:
            self._need(RELEASE_OWNER[action])
            r = Release()
            if action == "docs_check":
                if not version:
                    raise ToolError("version is required")
                DocsGate().run(tid, version, endpoints, flagged, gaps)
                return tasks.get(tid)
            if action == "architect_review":
                if approve is None:
                    raise ToolError("approve (true or false) is required")
                return r.architect_review(tid, bool(approve))
            if action == "approve_prod":
                return r.approve_prod(tid, "architect")
            if action == "deploy_qc":
                return r.deploy_qc(tid)
            if action == "deploy_prod":
                return r.deployed_prod(tid)
            if action == "prod_tests":
                if passed is None or failed is None:
                    raise ToolError("passed and failed counts are required")
                from ..incidents import IncidentError, Incidents
                try:
                    Incidents().production_tests(tid, passed, failed, area)
                except IncidentError as e:
                    raise ToolError(str(e))
                return t
            if passed is None or failed is None:
                raise ToolError("passed and failed counts are required")
            return r.qc_result(tid, passed, failed)
        flow = self.flow()
        if action == "review":
            if self.actor != t["reviewer"]:
                raise ToolError(f"only the peer reviewer ({t['reviewer']}) can review this task")
            return flow.record_review(tid, body)
        if self.actor != t["owner"]:
            raise ToolError(f"only the owner ({t['owner']}) can report this step")
        if action == "dev_done":
            if pr_number is None or not head_sha:
                raise ToolError("pr_number and head_sha are required")
            return flow.dev_done(tid, pr_number, head_sha)
        if passed is None or failed is None:
            raise ToolError("passed and failed counts are required")
        return flow.record_tests(tid, passed, failed, area)
