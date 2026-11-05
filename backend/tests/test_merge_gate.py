import pytest

from app.pipeline.merge_gate import GateConfig, evaluate, merge_command, parse_verdict, plan_batch

HEAD = "a" * 40
CFG = GateConfig()


def pr(**kw):
    base = {"number": 7, "branch": "task/x", "base": "internal", "head_sha": HEAD, "ci": "success",
            "mergeable": True, "tests": {"passed": 12, "failed": 0, "area": "inbox"},
            "changed_files": ["backend/app/inbox.py", "backend/tests/test_inbox.py"],
            "reviews": [{"verdict": "APPROVE", "head_sha": HEAD, "at": 2, "findings": []}]}
    base.update(kw)
    return base


def reasons(p, cfg=CFG):
    return evaluate(p, cfg)["reasons"]


def test_all_conditions_met_is_ok():
    assert evaluate(pr(), CFG) == {"ok": True, "reasons": []}


def test_parse_verdict_only_from_first_line():
    assert parse_verdict("Verdict: APPROVE\nlooks good") == "APPROVE"
    assert parse_verdict("verdict: request_changes\nfix x") == "REQUEST_CHANGES"
    assert parse_verdict("Verdict: HOLD") == "HOLD"
    assert parse_verdict("Looks good.\nVerdict: APPROVE") is None  # a later line must not count
    assert parse_verdict("Verdict: MAYBE") is None
    assert parse_verdict("") is None


def test_base_must_be_the_integration_branch():
    assert any("base" in r for r in reasons(pr(base="main")))
    assert any("base" in r for r in reasons(pr(base="task/other")))  # stacked PR not yet retargeted


def test_approval_must_be_on_the_current_head():
    stale = pr(reviews=[{"verdict": "APPROVE", "head_sha": "b" * 40, "at": 2, "findings": []}])
    assert any("head" in r for r in reasons(stale))
    assert any("review" in r for r in reasons(pr(reviews=[])))


def test_latest_review_wins_over_earlier_approval():
    rs = [{"verdict": "APPROVE", "head_sha": HEAD, "at": 1, "findings": []},
          {"verdict": "REQUEST_CHANGES", "head_sha": HEAD, "at": 2, "findings": []}]
    assert any("APPROVE" in r for r in reasons(pr(reviews=rs)))
    rs.append({"verdict": "APPROVE", "head_sha": HEAD, "at": 3, "findings": []})
    assert evaluate(pr(reviews=rs), CFG)["ok"]


def test_unresolved_medium_or_higher_findings_block_but_low_and_resolved_do_not():
    f = lambda sev, res: {"severity": sev, "resolved": res, "title": "x"}
    blocked = pr(reviews=[{"verdict": "APPROVE", "head_sha": HEAD, "at": 2, "findings": [f("medium", False)]}])
    assert any("finding" in r for r in reasons(blocked))
    ok = pr(reviews=[{"verdict": "APPROVE", "head_sha": HEAD, "at": 2,
                      "findings": [f("low", False), f("high", True)]}])
    assert evaluate(ok, CFG)["ok"]


@pytest.mark.parametrize("ci,mergeable", [("failure", True), ("pending", True), (None, True), ("success", False)])
def test_ci_must_be_green_and_pr_mergeable(ci, mergeable):
    assert not evaluate(pr(ci=ci, mergeable=mergeable), CFG)["ok"]


@pytest.mark.parametrize("tests", [None, {"passed": 0, "failed": 0, "area": "x"}, {"passed": 5, "failed": 1, "area": "x"}])
def test_tests_must_have_run_with_counts_and_none_failing(tests):
    assert any("tests" in r for r in reasons(pr(tests=tests)))


@pytest.mark.parametrize("path", [".env", "backend/.env.local", "keys/server.pem", "backend/seed_super_admin.py",
                                  "vercel.json", "frontend/vercel.json", ".github/workflows/deploy.yml"])
def test_secret_and_production_config_changes_block_merge(path):
    assert any("secret" in r or "production" in r for r in reasons(pr(changed_files=["a.py", path])))


@pytest.mark.parametrize("path", ["plugins/software-delivery/roles/reviewer.md", "plugins/x/policies.md",
                                  ".development/agents/reviewer.md"])
def test_pr_changing_reviewer_permissions_is_never_auto_merged(path):
    assert any("permission" in r for r in reasons(pr(changed_files=[path])))


def test_too_many_review_rounds_escalates():
    rs = [{"verdict": "REQUEST_CHANGES", "head_sha": f"{i}" * 40, "at": i, "findings": []} for i in range(1, 4)]
    rs.append({"verdict": "APPROVE", "head_sha": HEAD, "at": 9, "findings": []})
    out = evaluate(pr(reviews=rs), GateConfig(review_round_limit=3))
    assert not out["ok"] and any("rounds" in r for r in out["reasons"])


def test_all_failures_are_reported_together():
    out = evaluate(pr(base="main", ci="failure", tests=None, reviews=[]), CFG)
    assert len(out["reasons"]) >= 4


def test_merge_command_pins_head_and_uses_configured_style_and_never_admin():
    cmd = merge_command(pr(), "merge")
    assert cmd == ["gh", "pr", "merge", "7", "--merge", "--match-head-commit", HEAD]
    assert "--squash" in merge_command(pr(), "squash") and "--rebase" in merge_command(pr(), "rebase")
    assert "--admin" not in cmd
    with pytest.raises(ValueError):
        merge_command(pr(), "fast-forward")


def test_batch_orders_stacked_prs_base_first_and_skips_orphans():
    a = pr(number=1, branch="task/a", base="internal")
    b = pr(number=2, branch="task/b", base="task/a")  # stacked on a
    c = pr(number=3, branch="task/c", base="task/gone")  # base not in this batch
    out = plan_batch([b, c, a], "internal")
    assert [p["number"] for p in out["order"]] == [1, 2]
    assert [(s["number"], "base" in s["reason"]) for s in out["skipped"]] == [(3, True)]


def test_batch_detects_cycles():
    a = pr(number=1, branch="task/a", base="task/b")
    b = pr(number=2, branch="task/b", base="task/a")
    out = plan_batch([a, b], "internal")
    assert out["order"] == [] and len(out["skipped"]) == 2
