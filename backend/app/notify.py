"""In-app notifications (A-9, FR-30). Email/webhook/push plug in later behind the same `send`."""
from __future__ import annotations

import time

from . import db


class Notifier:
    def __init__(self):
        self.repo = db.DocRepo("notifications")

    def send(self, kind: str, title: str, body: str = "", key: str | None = None) -> dict | None:
        """Create an unread notification. A `key` suppresses duplicates while one is still unread."""
        if key and any(n.get("key") == key and not n["read"] for n in self.repo.list()):
            return None
        return self.repo.create({"kind": kind, "title": title, "body": body, "key": key,
                                 "read": False, "at": time.time()})

    def unread(self) -> list[dict]:
        return [n for n in self.repo.list() if not n["read"]]

    def mark_read(self, nid: str) -> dict | None:
        return self.repo.update(nid, {"read": True})
