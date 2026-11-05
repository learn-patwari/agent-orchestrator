import time

import pytest

from app import db
from app.inbox import Inbox, request_gate
from app.notify import Notifier
from app.scheduler import Scheduler, budget_percent_used


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def test_inbox_items_create_unread_notifications():
    Inbox().ask("DB?", "Which?", default="a")
    request_gate("merge_main", "merge PR 5")
    Inbox().post("status", "build done")  # status items do not interrupt the human
    n = Notifier().unread()
    assert [x["kind"] for x in n] == ["question", "approval"]


def test_mark_read_and_dedupe():
    nt = Notifier()
    a = nt.send("run_failed", "Run r1 failed", key="run:r1")
    assert nt.send("run_failed", "Run r1 failed", key="run:r1") is None  # same key, still unread
    nt.mark_read(a["id"])
    assert nt.unread() == []
    assert nt.send("run_failed", "again", key="run:r1") is not None  # allowed after read


def test_one_shot_job_runs_once_when_due():
    s = Scheduler()
    s.add("continue", {"x": 1}, run_at=100.0)
    ran = []
    assert s.tick(now=50.0, budget_percent=None, run=ran.append)["ran"] == []
    out = s.tick(now=100.0, budget_percent=None, run=ran.append)
    assert [j["name"] for j in out["ran"]] == ["continue"] and ran[0]["payload"] == {"x": 1}
    assert s.tick(now=200.0, budget_percent=None, run=ran.append)["ran"] == []
    assert s.list()[0]["status"] == "done"


def test_recurring_job_reschedules_without_catching_up_every_missed_slot():
    s = Scheduler()
    s.add("check-in", {}, run_at=100.0, every_s=60)
    ran = []
    s.tick(now=100.0, budget_percent=None, run=ran.append)
    s.tick(now=100.0, budget_percent=None, run=ran.append)  # same instant: not due again
    assert len(ran) == 1
    s.tick(now=1000.0, budget_percent=None, run=ran.append)  # long gap: runs once, not 15 times
    assert len(ran) == 2
    assert s.list()[0]["run_at"] > 1000.0 and s.list()[0]["status"] == "scheduled"


def test_budget_at_or_above_threshold_pauses_jobs_and_notifies_once():
    s = Scheduler(pause_at_percent=80)
    s.add("continue", {}, run_at=0.0)
    ran = []
    out = s.tick(now=10.0, budget_percent=85.0, run=ran.append)
    assert out["paused"] is True and ran == []
    s.tick(now=11.0, budget_percent=90.0, run=ran.append)
    assert [n["kind"] for n in Notifier().unread()] == ["budget_pause"]  # one notice, not one per tick
    out = s.tick(now=12.0, budget_percent=40.0, run=ran.append)  # window reset: resumes by itself
    assert out["paused"] is False and len(ran) == 1


def test_failing_job_is_recorded_and_does_not_stop_others():
    s = Scheduler()
    s.add("bad", {}, run_at=0.0)
    s.add("good", {}, run_at=0.0)
    ran = []

    def run(job):
        if job["name"] == "bad":
            raise RuntimeError("boom")
        ran.append(job["name"])

    s.tick(now=1.0, budget_percent=None, run=run)
    st = {j["name"]: j for j in s.list()}
    assert ran == ["good"] and st["bad"]["status"] == "failed" and "boom" in st["bad"]["error"]


def test_budget_percent_used_from_calls_and_daily_cap():
    assert budget_percent_used(None) is None  # no cap configured
    db.DocRepo("llm_calls").create({"role": "x", "cost_usd": 2.0, "at": time.time()})
    db.DocRepo("llm_calls").create({"role": "x", "cost_usd": None, "at": time.time()})  # unknown price ignored
    db.DocRepo("llm_calls").create({"role": "x", "cost_usd": 50.0, "at": time.time() - 3 * 86400})  # old
    assert budget_percent_used(10.0) == 20.0
