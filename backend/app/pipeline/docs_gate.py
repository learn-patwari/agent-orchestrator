"""Docs gate: no release is signed off until its documents pass the check.

cicd asks the platform to run the check (`docs_check`); the platform reads the files itself, so a PASS is a fact and
not an agent's claim. The tester may not sign off (`qc_result` with no failures) until the gate is PASS. Only the
human can ship with a documented exception. The check never echoes a secret it finds.
"""
from __future__ import annotations

import json
import re
import time
from pathlib import Path

from .. import secrets_store
from .flow import FlowError, tasks

LINK = re.compile(r"!?\[[^\]]*\]\(([^)\s]+)[^)]*\)")
GUIDE_DIRS = ("docs/user-guides", "docs/guides")


def _read(p: Path) -> str:
    return p.read_text(encoding="utf-8", errors="replace") if p.is_file() else ""


def _json_version(p: Path) -> str | None:
    try:
        return json.loads(_read(p)).get("version")
    except (ValueError, AttributeError):
        return None


def _section(text: str, heading: str) -> str:
    m = re.search(rf"^##\s+{re.escape(heading)}\s*$(.*?)(?=^##\s|\Z)", text, re.M | re.S | re.I)
    return m.group(1) if m else ""


def check_docs(root, version: str, flagged: list[dict] | None = None, gaps: list[dict] | None = None,
               endpoints: list[str] | None = None) -> dict:
    root = Path(root)
    failed: list[str] = []
    notes_rel = f"docs/releases/v{version}.md"
    notes = _read(root / notes_rel)
    if not (root / notes_rel).is_file():
        failed.append(f"{notes_rel} is missing")
    for rel, getter in (("frontend/package.json", lambda: _json_version(root / "frontend/package.json")),
                        ("backend/VERSION", lambda: (_read(root / "backend/VERSION").strip() or None))):
        if not (root / rel).is_file():
            failed.append(f"{rel} is missing, so the version cannot be matched")
        elif getter() != version:
            failed.append(f"{rel} says {getter()}, the release is {version}")
    changelog = _read(root / "CHANGELOG.md")
    first = next((ln for ln in changelog.splitlines() if ln.startswith("## ")), "")
    if not changelog:
        failed.append("CHANGELOG.md is missing")
    elif version not in first:
        failed.append(f"CHANGELOG.md does not have the v{version} entry on top")
    for f in flagged or []:
        pr, guide = f.get("pr"), f.get("guide", "")
        updated = (root / guide).is_file() and guide in notes
        gap = any(g.get("pr") == pr and str(g.get("owner", "")).strip() for g in gaps or [])
        if not (updated or gap):
            failed.append(f"PR #{pr} affects guide {guide}: not updated and not listed as an open gap with an owner")
    for e in endpoints or []:
        docs = [p for d in ("docs/api", "docs/schema") for p in (root / d).rglob("*.md")] if root.exists() else []
        if not any(e in _read(p) for p in docs):
            failed.append(f"endpoint {e} is not in docs/api or docs/schema")
    md_files = [p for p in [root / notes_rel, root / "CHANGELOG.md"] if p.is_file()]
    for d in (*GUIDE_DIRS, "docs/api", "docs/schema"):
        md_files += sorted((root / d).rglob("*.md")) if (root / d).is_dir() else []
    for p in md_files:
        rel = p.relative_to(root).as_posix()
        for n, line in enumerate(_read(p).splitlines(), 1):
            if secrets_store.looks_like_secret(line):
                failed.append(f"{rel}:{n}: looks like a secret (not shown)")
        for target in LINK.findall(_read(p)):
            if re.match(r"^(https?:|mailto:|#)", target):
                continue
            path = target.split("#", 1)[0]
            if path and not (p.parent / path).exists():
                failed.append(f"{rel}: {target} does not resolve")
    if "not verified" in notes.lower() and "not verified" not in _section(notes, "Needs your attention").lower():
        failed.append("Not verified items are not repeated under \"Needs your attention\"")
    return {"status": "FAIL" if failed else "PASS", "failed": failed}


def passes(t: dict) -> bool:
    g = t.get("docs_gate") or {}
    return g.get("status") == "PASS" or bool(g.get("exception"))


def state(t: dict) -> str:
    g = t.get("docs_gate") or {}
    return "EXCEPTION" if g.get("exception") else g.get("status", "MISSING")


class DocsGate:
    def _task(self, tid: str) -> dict:
        t = tasks.get(tid)
        if t is None:
            raise FlowError(f"unknown task {tid}")
        return t

    def run(self, tid: str, version: str, endpoints=None, flagged=None, gaps=None) -> dict:
        t = self._task(tid)
        root = t.get("worktree")
        res = check_docs(root, version, flagged, gaps, endpoints) if root else {
            "status": "FAIL", "failed": ["the task has no worktree to check"]}
        tasks.update(tid, {"docs_gate": {**res, "version": version, "at": time.time(),
                                         "exception": (t.get("docs_gate") or {}).get("exception")}})
        return res

    def exception(self, tid: str, reason: str, by: str) -> dict:
        t = self._task(tid)
        if by != "human":
            raise FlowError("only the human can ship with a documented exception")
        if not (reason or "").strip():
            raise FlowError("a documented exception needs a reason")
        g = t.get("docs_gate") or {"status": "FAIL", "failed": ["the docs gate has not run"]}
        return tasks.update(tid, {"docs_gate": {**g, "exception": {"reason": reason.strip(), "by": by,
                                                                   "at": time.time()}}})

    def require(self, t: dict) -> None:
        if not passes(t):
            g = t.get("docs_gate")
            why = "it has not run" if not g else f"it is {g.get('status')}: {'; '.join(g.get('failed', [])[:3])}"
            raise FlowError(f"waiting for docs gate ({why}); cicd runs the docs check, the tester never does")
