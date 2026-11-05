"""Human inbox, assumptions list and gates (FR-24, FR-25, FR-26).

Questions carry a recommended default. Accepting it (or "decide with defaults" mode) records an
assumption the human can overrule later. Gated actions always wait for an explicit human approval,
which is single-use and audited.
"""
from __future__ import annotations

import time

from . import db
from .notify import Notifier

GATED = {
    "merge_main": "Merge into the main branch",
    "production_config": "Change production configuration or environment",
    "credentials": "Use or provide credentials",
    "destructive_git": "Destructive git operation (force push, reset, branch delete)",
    "delete_data": "Permanently delete data",
    "srs_approval": "Approve an SRS so development can start",
}


class GateBlocked(Exception):
    pass


class Conflict(ValueError):
    """The item is already resolved."""


def _audit(event: str, **fields) -> dict:
    return db.DocRepo("audit").create({"event": event, "at": time.time(), **fields})


class Inbox:
    def __init__(self, decide_with_defaults: bool = False):
        self.items = db.DocRepo("inbox")
        self.assumed = db.DocRepo("assumptions")
        self.auto = decide_with_defaults

    def post(self, kind: str, title: str, body: str = "", **extra) -> dict:
        item = self.items.create({"kind": kind, "title": title, "body": body, "status": "open",
                                  "created_at": time.time(), **extra})
        if kind in ("question", "approval"):  # status items never interrupt the human
            Notifier().send(kind, title, body, key=f"inbox:{item['id']}")
        return item

    def ask(self, title: str, body: str, options: list[str] | None = None, default: str | None = None,
            run_id: str | None = None) -> dict:
        item = self.post("question", title, body, options=options or [], default=default, run_id=run_id)
        if self.auto and default is not None:
            return self._resolve_default(item, "auto_default")
        return item

    def _get_open(self, item_id: str) -> dict:
        item = self.items.get(item_id)
        if item is None:
            raise KeyError(item_id)
        if item["status"] != "open":
            raise Conflict(f"item {item_id} is already {item['status']}")
        return item

    def _resolve_default(self, item: dict, by: str) -> dict:
        self.assumed.create({"question": item["title"], "assumed": item["default"], "item_id": item["id"],
                             "status": "assumed", "source": by, "created_at": time.time()})
        return self.items.update(item["id"], {"status": "resolved", "answer": item["default"], "resolved_by": by})

    def answer(self, item_id: str, answer: str) -> dict:
        item = self._get_open(item_id)
        if item["kind"] == "approval":
            raise ValueError("approvals are decided with approve/reject, not answered")
        return self.items.update(item["id"], {"status": "resolved", "answer": answer, "resolved_by": "human"})

    def accept_default(self, item_id: str) -> dict:
        item = self._get_open(item_id)
        if item.get("default") is None:
            raise ValueError("this question has no recommended default")
        return self._resolve_default(item, "accepted_default")

    def decide(self, item_id: str, approve: bool) -> dict:
        item = self._get_open(item_id)
        if item["kind"] != "approval":
            raise ValueError("only approval items can be approved or rejected")
        status = "approved" if approve else "rejected"
        _audit(f"gate_{status}", item_id=item_id, action=item["action"], run_id=item.get("run_id"))
        return self.items.update(item_id, {"status": status, "resolved_by": "human"})

    def list(self, status: str | None = None) -> list[dict]:
        return [i for i in self.items.list() if status is None or i["status"] == status]

    def assumptions(self) -> list[dict]:
        return self.assumed.list()

    def overrule(self, assumption_id: str, new_value: str) -> dict:
        a = self.assumed.get(assumption_id)
        if a is None:
            raise KeyError(assumption_id)
        return self.assumed.update(assumption_id, {"status": "overruled", "overruled_to": new_value})


def request_gate(action: str, detail: str, run_id: str | None = None, decide_with_defaults: bool = False) -> dict:
    """Open an approval request. `decide_with_defaults` is accepted but never auto-approves a gate."""
    if action not in GATED:
        raise ValueError(f"unknown gated action {action}")
    item = Inbox().post("approval", GATED[action], detail, action=action, run_id=run_id)
    _audit("gate_requested", item_id=item["id"], action=action, run_id=run_id)
    return item


def check_gate(action: str, approval_id: str | None) -> None:
    """Raise GateBlocked unless a human approved exactly this action; approval is consumed."""
    items = db.DocRepo("inbox")
    item = items.get(approval_id) if approval_id else None
    ok = bool(item) and item["kind"] == "approval" and item["action"] == action and item["status"] == "approved"
    if not ok:
        _audit("gate_blocked", action=action, item_id=approval_id)
        raise GateBlocked(f"{action} needs human approval")
    items.update(approval_id, {"status": "consumed"})
    _audit("gate_passed", action=action, item_id=approval_id)
