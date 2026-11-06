import json

import pytest

from app import db
from app.pipeline.github import GhClient, parse_review, pr_facts_from_gh
from app.pipeline.merge_exec import MergeRefused, merge_batch
from app.pipeline.merge_gate import GateConfig

H1, H2 = "1" * 40, "2" * 40


@pytest.fixture(autouse=True)
def _db(env):
    db.init_db()


def facts(number, branch, base="internal", head=H1, **kw):
    f = {"number": number, "branch": branch, "base": base, "head_sha": head, "ci": "success", "mergeable": True,
         "tests": {"passed": 3, "failed": 0, "area": "x"}, "changed_files": ["a.py"],
         "reviews": [{"verdict": "APPROVE", "head_sha": head, "at": 1, "findings": []}]}
    f.update(kw)
    return f


class FakeGh:
    def __init__(self, prs):
        self.prs, self.calls = {p["number"]: dict(p) for p in prs}, []

    def facts(self, n):
        return dict(self.prs[n])

    def retarget(self, n, base):
        self.calls.append(("retarget", n, base))
        self.prs[n]["base"] = base

    def merge(self, cmd):
        self.calls.append(("merge", cmd))


def run(gh, numbers, **kw):
    return merge_batch(gh, numbers, GateConfig(), style=kw.pop("style", "merge"),
                       reviewer_can_merge=kw.pop("reviewer_can_merge", True), **kw)


def test_parse_review_reads_verdict_head_and_findings():
    body = "Verdict: APPROVE\nHead: " + H1 + "\n- [low] naming (resolved)\n- [high] sql injection\nnotes"
    r = parse_review(body, at=5)
    assert r["verdict"] == "APPROVE" and r["head_sha"] == H1 and r["at"] == 5
    assert [(f["severity"], f["resolved"]) for f in r["findings"]] == [("low", True), ("high", False)]
    assert parse_review("no verdict here", at=1) is None
    assert parse_review("Verdict: APPROVE\nno head line", at=1)["head_sha"] is None  # cannot match any head


def test_pr_facts_from_gh_json_maps_checks_files_and_reviews():
    raw = {"number": 9, "baseRefName": "internal", "headRefName": "task/x", "headRefOid": H1, "mergeable": "MERGEABLE",
           "statusCheckRollup": [{"conclusion": "SUCCESS"}, {"conclusion": "SUCCESS"}],
           "files": [{"path": "a.py"}], "comments": [{"body": f"Verdict: APPROVE\nHead: {H1}", "createdAt": "2026-01-01T00:00:01Z"}]}
    f = pr_facts_from_gh(raw, tests={"passed": 2, "failed": 0, "area": "x"})
    assert f["ci"] == "success" and f["mergeable"] is True and f["changed_files"] == ["a.py"]
    assert f["reviews"][0]["verdict"] == "APPROVE" and f["base"] == "internal"
    raw["statusCheckRollup"][1]["conclusion"] = "FAILURE"
    assert pr_facts_from_gh(raw, None)["ci"] == "failure"
    raw["statusCheckRollup"] = [{"conclusion": None, "status": "IN_PROGRESS"}]
    assert pr_facts_from_gh(raw, None)["ci"] == "pending"
    raw["statusCheckRollup"] = []
    assert pr_facts_from_gh(raw, None)["ci"] is None  # no checks is not green


def test_gh_client_builds_commands_and_never_uses_admin():
    seen = []
    c = GhClient("owner/repo", runner=lambda argv: (seen.append(argv), "{}")[1])
    c.retarget(4, "internal")
    c.merge(["gh", "pr", "merge", "4", "--merge", "--match-head-commit", H1])
    assert seen[0][:4] == ["gh", "pr", "edit", "4"] and "--base" in seen[0] and "internal" in seen[0]
    assert "--repo" in seen[1] and "owner/repo" in seen[1] and "--admin" not in seen[1]


def test_merge_authority_is_a_revocable_setting_off_means_nothing_merges():
    gh = FakeGh([facts(1, "task/a")])
    with pytest.raises(MergeRefused, match="authority"):
        run(gh, [1], reviewer_can_merge=False)
    assert gh.calls == []


def test_refuses_to_target_main_or_the_stage_branches():
    gh = FakeGh([facts(1, "task/a")])
    for bad in ("main", "master", "stage"):
        with pytest.raises(MergeRefused, match="human"):
            merge_batch(gh, [1], GateConfig(integration_branch=bad), "merge", reviewer_can_merge=True)


def test_batch_merges_ready_prs_pinned_to_head_and_audits():
    gh = FakeGh([facts(1, "task/a"), facts(2, "task/b", head=H2)])
    out = run(gh, [2, 1])
    assert [m["number"] for m in out["merged"]] == [1, 2]
    cmds = [c[1] for c in gh.calls if c[0] == "merge"]
    assert cmds[0][-1] == H1 and cmds[1][-1] == H2 and "--match-head-commit" in cmds[0]
    assert [e["event"] for e in db.DocRepo("audit").list()] == ["merge_done", "merge_done"]


def test_stacked_pr_is_retargeted_after_its_base_merges_then_merged():
    gh = FakeGh([facts(1, "task/a"), facts(2, "task/b", base="task/a")])
    out = run(gh, [2, 1])
    assert [m["number"] for m in out["merged"]] == [1, 2]
    kinds = [c[0] for c in gh.calls]
    assert kinds == ["merge", "retarget", "merge"] and gh.calls[1][1:] == (2, "internal")


def test_not_ready_pr_is_refused_with_reasons_and_its_dependents_are_skipped():
    gh = FakeGh([facts(1, "task/a", ci="failure"), facts(2, "task/b", base="task/a"), facts(3, "task/c")])
    out = run(gh, [1, 2, 3])
    assert [m["number"] for m in out["merged"]] == [3]
    refused = {r["number"]: r["reasons"] for r in out["refused"]}
    assert any("CI" in r for r in refused[1]) and any("depends" in r for r in refused[2])
    assert all(c[1][1] == "pr" and c[1][3] == "3" for c in gh.calls if c[0] == "merge")
    events = [e["event"] for e in db.DocRepo("audit").list()]
    assert events.count("merge_refused") == 2 and events.count("merge_done") == 1


def test_stale_head_after_review_is_refused_even_when_everything_else_is_green():
    f = facts(1, "task/a", head=H2, reviews=[{"verdict": "APPROVE", "head_sha": H1, "at": 1, "findings": []}])
    out = run(FakeGh([f]), [1])
    assert out["merged"] == [] and any("head" in r for r in out["refused"][0]["reasons"])


def test_head_is_rechecked_just_before_merge():
    class Racing(FakeGh):
        def facts(self, n):
            f = super().facts(n)
            if len([c for c in self.calls if c[0] == "facts"]) >= 1:
                f["head_sha"] = H2  # someone pushed between evaluation and merge
            self.calls.append(("facts", n))
            return f

    gh = Racing([facts(1, "task/a")])
    out = run(gh, [1])
    assert out["merged"] == [] and any("changed" in r for r in out["refused"][0]["reasons"])
    assert not [c for c in gh.calls if c[0] == "merge"]
