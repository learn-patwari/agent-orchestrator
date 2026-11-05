"""Reviewer merge gate (FR-37): the conditions under which a reviewer may merge into the integration branch.

Pure functions over PR facts so the gate is easy to test and cannot be talked around by prompt text.
The verdict is read only from the FIRST line of a review (shared GitHub account: no real approvals).
"""
from __future__ import annotations

import fnmatch
import re
from dataclasses import dataclass, field

BLOCKING = {"medium", "high", "critical"}
STYLES = {"merge": "--merge", "squash": "--squash", "rebase": "--rebase"}
VERDICT = re.compile(r"^verdict:\s*(approve|request_changes|hold)\s*$", re.I)


@dataclass
class GateConfig:
    integration_branch: str = "internal"
    review_round_limit: int = 3
    secret_patterns: list[str] = field(default_factory=lambda: [
        ".env", ".env.*", "*.pem", "*.key", "seed_super_admin*", "secret*", "*credentials*"])
    production_patterns: list[str] = field(default_factory=lambda: [
        "vercel.json", ".github/workflows/*", "Jenkinsfile", "*.tf", "Dockerfile", "docker-compose*.yml"])
    permission_patterns: list[str] = field(default_factory=lambda: [
        "*/reviewer.md", "*/policies.md", "*/mcp.json", "*/plugin.json"])


def parse_verdict(body: str) -> str | None:
    first = (body or "").strip().splitlines()[0] if (body or "").strip() else ""
    m = VERDICT.match(first.strip())
    return m.group(1).upper() if m else None


def _matches(path: str, patterns: list[str]) -> bool:
    name = path.rsplit("/", 1)[-1]
    return any(fnmatch.fnmatch(path, p) or fnmatch.fnmatch(name, p) for p in patterns)


def evaluate(pr: dict, cfg: GateConfig) -> dict:
    why: list[str] = []
    if pr["base"] != cfg.integration_branch:
        why.append(f"base is {pr['base']}, not the integration branch {cfg.integration_branch}")
    reviews = sorted(pr.get("reviews", []), key=lambda r: r["at"])
    latest = reviews[-1] if reviews else None
    if latest is None:
        why.append("no review yet")
    else:
        if latest["verdict"] != "APPROVE":
            why.append(f"latest review is {latest['verdict']}, not APPROVE")
        elif latest["head_sha"] != pr["head_sha"]:
            why.append("latest APPROVE is not on the current head commit (new commits since review)")
        open_f = [f for f in latest.get("findings", []) if f["severity"] in BLOCKING and not f.get("resolved")]
        if open_f:
            why.append(f"{len(open_f)} unresolved medium+ finding(s)")
    rounds = sum(1 for r in reviews if r["verdict"] == "REQUEST_CHANGES")
    if rounds >= cfg.review_round_limit:
        why.append(f"{rounds} review rounds reached the limit of {cfg.review_round_limit}: escalate to the human")
    if pr.get("ci") != "success":
        why.append(f"CI is {pr.get('ci')}, not success")
    if not pr.get("mergeable"):
        why.append("PR is not mergeable")
    t = pr.get("tests")
    if not t or t.get("passed", 0) <= 0:
        why.append("no passing tests recorded for the area (tests must run with counts)")
    elif t.get("failed", 0) > 0:
        why.append(f"tests failing: {t['failed']}")
    files = pr.get("changed_files", [])
    if any(_matches(f, cfg.secret_patterns) for f in files):
        why.append("changes touch secret/credential files")
    if any(_matches(f, cfg.production_patterns) for f in files):
        why.append("changes touch production/deployment configuration (human decides)")
    if any(_matches(f, cfg.permission_patterns) for f in files):
        why.append("changes touch reviewer permissions or policies (never auto-merged)")
    return {"ok": not why, "reasons": why}


def merge_command(pr: dict, style: str) -> list[str]:
    if style not in STYLES:
        raise ValueError(f"unknown merge style {style}")
    return ["gh", "pr", "merge", str(pr["number"]), STYLES[style], "--match-head-commit", pr["head_sha"]]


def plan_batch(prs: list[dict], integration: str) -> dict:
    """Order PRs so a stacked PR is merged after its base PR; skip PRs whose base is unknown or cyclic."""
    by_branch = {p["branch"]: p for p in prs}
    order: list[dict] = []
    skipped: list[dict] = []
    state: dict[str, str] = {}

    def visit(p: dict, chain: tuple = ()) -> bool:
        b = p["branch"]
        if state.get(b) == "ok":
            return True
        if state.get(b) == "bad" or b in chain:
            return False
        if p["base"] == integration:
            ok = True
        elif p["base"] in by_branch:
            ok = visit(by_branch[p["base"]], chain + (b,))
        else:
            ok = False
        state[b] = "ok" if ok else "bad"
        if ok:
            order.append(p)
        return ok

    for p in sorted(prs, key=lambda x: x["number"]):
        visit(p)
    for p in prs:
        if state.get(p["branch"]) != "ok":
            cyc = p["base"] in by_branch
            skipped.append({"number": p["number"], "reason": "stack cycle or broken chain" if cyc
                            else f"base {p['base']} is neither the integration branch nor in this batch"})
    skipped.sort(key=lambda s: s["number"])
    return {"order": order, "skipped": skipped}
