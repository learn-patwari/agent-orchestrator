import pytest

from app import db
from app.notify import Notifier
from app.quota import PROVIDER_LIMITS, QuotaBlocked, QuotaGuard

H = 3600
DAY = 86400


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def test_vercel_hobby_limits_are_seeded_from_the_documented_values():
    v = PROVIDER_LIMITS["vercel_hobby"]
    assert v["deployments_per_day"]["limit"] == 100 and v["deployments_per_day"]["window_s"] == DAY
    assert v["deployments_per_hour"]["limit"] == 100 and v["concurrent_builds"]["limit"] == 1


def test_allows_until_the_stop_threshold_then_blocks():
    g = QuotaGuard("vercel_hobby", stop_at_percent=70)
    for i in range(69):
        g.record("deployments_per_day", at=1000 + i)
    g.check("deployments_per_day", now=2000)  # 69 of 100: still fine
    g.record("deployments_per_day", at=1100)
    with pytest.raises(QuotaBlocked, match="70"):
        g.check("deployments_per_day", now=2000)  # 70 of 100: stop, tell the orchestrator


def test_old_events_fall_out_of_the_rolling_window():
    g = QuotaGuard("vercel_hobby", stop_at_percent=70)
    for i in range(80):
        g.record("deployments_per_day", at=100 + i)
    with pytest.raises(QuotaBlocked):
        g.check("deployments_per_day", now=500)
    g.check("deployments_per_day", now=100 + 80 + DAY)  # a day later they no longer count


def test_hour_and_day_windows_are_independent():
    g = QuotaGuard("vercel_hobby", stop_at_percent=50)
    for i in range(60):
        g.record("deployments_per_hour", at=10_000 + i)
    with pytest.raises(QuotaBlocked):
        g.check("deployments_per_hour", now=10_100)
    g.check("deployments_per_day", now=10_100)  # day counter untouched


def test_concurrent_builds_limit_of_one_blocks_a_second_build():
    g = QuotaGuard("vercel_hobby", stop_at_percent=70)
    g.check("concurrent_builds", now=1, in_flight=0)
    with pytest.raises(QuotaBlocked):
        g.check("concurrent_builds", now=1, in_flight=1)


def test_blocking_notifies_once_and_names_the_metric():
    g = QuotaGuard("vercel_hobby", stop_at_percent=10)
    for i in range(15):
        g.record("deployments_per_day", at=100 + i)
    for _ in range(3):
        with pytest.raises(QuotaBlocked):
            g.check("deployments_per_day", now=200)
    n = Notifier().unread()
    assert len(n) == 1 and n[0]["kind"] == "quota_block" and "deployments_per_day" in n[0]["body"]


def test_a_limit_error_from_the_platform_stops_everything_and_never_retries():
    g = QuotaGuard("vercel_hobby", stop_at_percent=70)
    g.limit_hit("api-deployments-free-per-day", now=500)
    for metric in ("deployments_per_day", "deployments_per_hour"):
        with pytest.raises(QuotaBlocked, match="limit error"):
            g.check(metric, now=501)
    g.check("deployments_per_day", now=500 + DAY + 1)  # stop lasts until the window can have reset


def test_unknown_provider_or_metric_is_an_error_not_a_free_pass():
    with pytest.raises(ValueError):
        QuotaGuard("nope")
    with pytest.raises(ValueError):
        QuotaGuard("vercel_hobby").check("made_up", now=1)


def test_usage_report_shows_used_limit_and_percent():
    g = QuotaGuard("vercel_hobby")
    for i in range(5):
        g.record("deployments_per_day", at=100 + i)
    r = g.usage(now=200)["deployments_per_day"]
    assert r == {"used": 5, "limit": 100, "percent": 5.0}


def test_custom_limits_table_can_be_supplied_for_other_platforms():
    table = {"acme_free": {"builds_per_day": {"limit": 10, "window_s": DAY}}}
    g = QuotaGuard("acme_free", stop_at_percent=50, limits=table)
    for i in range(5):
        g.record("builds_per_day", at=100 + i)
    with pytest.raises(QuotaBlocked):
        g.check("builds_per_day", now=200)
