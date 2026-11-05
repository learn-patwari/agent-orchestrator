"""Batch merge executor (FR-37): evaluate each PR with the gate, merge base-first, retarget stacked PRs,
re-check the head right before merging, pin it, audit every outcome. Merging into main/stage is refused."""
from __future__ import annotations

import time

from .. import db
from .merge_gate import GateConfig, evaluate, merge_command, plan_batch

HUMAN_ONLY = {"main", "master", "stage"}


class MergeRefused(Exception):
    pass


def _audit(event: str, **f) -> None:
    db.DocRepo("audit").create({"event": event, "at": time.time(), **f})


def merge_batch(gh, numbers: list[int], cfg: GateConfig, style: str, reviewer_can_merge: bool) -> dict:
    if not reviewer_can_merge:
        raise MergeRefused("reviewer merge authority is switched off; the human merges")
    if cfg.integration_branch in HUMAN_ONLY:
        raise MergeRefused(f"{cfg.integration_branch} is merged by the human only")
    facts = {n: gh.facts(n) for n in numbers}
    plan = plan_batch(list(facts.values()), cfg.integration_branch)
    merged, refused = [], [{"number": s["number"], "reasons": [s["reason"]]} for s in plan["skipped"]]
    done_branches: set[str] = set()
    failed_branches: set[str] = set()
    for p in plan["order"]:
        n = p["number"]
        if p["base"] != cfg.integration_branch:
            if p["base"] in done_branches:
                gh.retarget(n, cfg.integration_branch)
                p = {**p, "base": cfg.integration_branch}
            else:
                refused.append({"number": n, "reasons": [f"depends on PR for {p['base']} which was not merged"]})
                failed_branches.add(p["branch"])
                _audit("merge_refused", number=n, reasons=["base PR not merged"])
                continue
        verdict = evaluate(p, cfg)
        if not verdict["ok"]:
            refused.append({"number": n, "reasons": verdict["reasons"]})
            failed_branches.add(p["branch"])
            _audit("merge_refused", number=n, reasons=verdict["reasons"])
            continue
        fresh = gh.facts(n)
        if fresh["head_sha"] != p["head_sha"]:
            why = ["head commit changed after the gate was evaluated"]
            refused.append({"number": n, "reasons": why})
            failed_branches.add(p["branch"])
            _audit("merge_refused", number=n, reasons=why)
            continue
        gh.merge(merge_command(p, style))
        merged.append({"number": n, "head_sha": p["head_sha"]})
        done_branches.add(p["branch"])
        _audit("merge_done", number=n, head_sha=p["head_sha"], style=style)
    return {"merged": merged, "refused": sorted(refused, key=lambda r: r["number"])}
