"""Hand-offs the office view animates. One is recorded for every real transition, never for a guess."""
from __future__ import annotations

import threading
import time

from .. import db

_repo = db.DocRepo("handoffs")
_lock = threading.Lock()
_last = [0.0]


def record(sender: str, receiver: str, label: str, run_id: str | None = None) -> dict:
    with _lock:  # strictly increasing timestamps, so the order survives a coarse clock
        at = max(time.time(), _last[0] + 1e-6)
        _last[0] = at
    return _repo.create({"from": sender, "to": receiver, "label": label, "at": at, "run_id": run_id})
