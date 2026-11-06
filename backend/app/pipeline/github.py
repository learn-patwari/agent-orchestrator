"""GitHub via the `gh` CLI (A-4): PR facts for the merge gate, retarget, merge. Runner is injectable."""
from __future__ import annotations

import json
import re
import subprocess
from datetime import datetime
from typing import Callable

from .merge_gate import parse_verdict

HEAD_LINE = re.compile(r"^head:\s*([0-9a-f]{40})\s*$", re.I)
FINDING = re.compile(r"^-\s*\[(low|medium|high|critical)\]\s*(.+?)\s*$", re.I)
FAIL = {"FAILURE", "CANCELLED", "TIMED_OUT", "ERROR", "ACTION_REQUIRED", "STARTUP_FAILURE"}
GOOD = {"SUCCESS", "NEUTRAL", "SKIPPED"}
FIELDS = "number,baseRefName,headRefName,headRefOid,mergeable,statusCheckRollup,files,comments,reviews"


def parse_review(body: str, at: float) -> dict | None:
    """Review convention: line 1 `Verdict: X`, line 2 `Head: <40-hex sha>`, then `- [severity] finding` lines
    (append `(resolved)` once fixed). A review without a Head line can never match the current head."""
    verdict = parse_verdict(body)
    if verdict is None:
        return None
    lines = body.strip().splitlines()
    head = None
    if len(lines) > 1:
        m = HEAD_LINE.match(lines[1].strip())
        head = m.group(1).lower() if m else None
    findings = []
    for ln in lines[2:]:
        m = FINDING.match(ln.strip())
        if m:
            text = m.group(2)
            findings.append({"severity": m.group(1).lower(), "title": re.sub(r"\s*\(resolved\)$", "", text, flags=re.I),
                             "resolved": text.lower().endswith("(resolved)")})
    return {"verdict": verdict, "head_sha": head, "at": at, "findings": findings}


def _ts(s: str | None) -> float:
    if not s:
        return 0.0
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def pr_facts_from_gh(raw: dict, tests: dict | None) -> dict:
    checks = raw.get("statusCheckRollup") or []
    if not checks:
        ci = None
    elif any((c.get("conclusion") or "") in FAIL for c in checks):
        ci = "failure"
    elif any(not c.get("conclusion") for c in checks):
        ci = "pending"
    else:
        ci = "success" if all(c["conclusion"] in GOOD for c in checks) else "failure"
    reviews = []
    for c in raw.get("comments") or []:
        r = parse_review(c.get("body", ""), _ts(c.get("createdAt")))
        if r:
            reviews.append(r)
    for c in raw.get("reviews") or []:
        r = parse_review(c.get("body", ""), _ts(c.get("submittedAt")))
        if r:
            reviews.append(r)
    return {"number": raw["number"], "branch": raw["headRefName"], "base": raw["baseRefName"],
            "head_sha": raw["headRefOid"], "ci": ci, "mergeable": raw.get("mergeable") == "MERGEABLE",
            "tests": tests, "changed_files": [f["path"] for f in raw.get("files") or []], "reviews": reviews}


def _default_runner(argv: list[str]) -> str:
    r = subprocess.run(argv, capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        raise RuntimeError((r.stderr or r.stdout).strip())
    return r.stdout


class GhClient:
    def __init__(self, repo: str, runner: Callable[[list[str]], str] | None = None,
                 tests_lookup: Callable[[int], dict | None] | None = None):
        self.repo, self.run = repo, runner or _default_runner
        self.tests_lookup = tests_lookup or (lambda n: None)

    def facts(self, n: int) -> dict:
        out = self.run(["gh", "pr", "view", str(n), "--repo", self.repo, "--json", FIELDS])
        return pr_facts_from_gh(json.loads(out), self.tests_lookup(n))

    def retarget(self, n: int, base: str) -> None:
        self.run(["gh", "pr", "edit", str(n), "--repo", self.repo, "--base", base])

    def merge(self, cmd: list[str]) -> None:
        if "--admin" in cmd:
            raise ValueError("admin merges are not allowed")
        self.run([*cmd, "--repo", self.repo])
