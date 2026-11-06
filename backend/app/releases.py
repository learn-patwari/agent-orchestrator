"""Release helpers (FR-41): SemVer bump suggestion and release-notes draft from conventional commit subjects.

Pure functions. The human edits the notes and decides; nothing here tags or publishes.
"""
from __future__ import annotations

import re

SUBJECT = re.compile(r"^(?P<type>[a-z]+)(\([^)]*\))?(?P<bang>!)?:\s*(?P<text>.+)$", re.I)
SEMVER = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)$")


def _parse(msg: str) -> dict | None:
    first = msg.strip().splitlines()[0] if msg.strip() else ""
    m = SUBJECT.match(first)
    if not m:
        return None
    breaking = bool(m.group("bang")) or "BREAKING CHANGE" in msg
    return {"type": m.group("type").lower(), "text": m.group("text").strip(), "breaking": breaking}


def suggest_bump(messages: list[str]) -> str | None:
    parsed = [p for p in map(_parse, messages) if p]
    if not parsed:
        return None
    if any(p["breaking"] for p in parsed):
        return "major"
    if any(p["type"] == "feat" for p in parsed):
        return "minor"
    return "patch"


def next_version(current: str | None, bump: str) -> str:
    if bump not in ("major", "minor", "patch"):
        raise ValueError(f"unknown bump {bump}")
    if current is None:
        return {"major": "1.0.0", "minor": "0.1.0", "patch": "0.0.1"}[bump]
    m = SEMVER.match(current)
    if not m:
        raise ValueError(f"not a SemVer version: {current}")
    a, b, c = map(int, m.groups())
    return {"major": f"{a + 1}.0.0", "minor": f"{a}.{b + 1}.0", "patch": f"{a}.{b}.{c + 1}"}[bump]


def generate_notes(version: str, messages: list[str]) -> str:
    parsed = [p for p in map(_parse, messages) if p]
    sections = [("Breaking changes", [p for p in parsed if p["breaking"]]),
                ("Features", [p for p in parsed if p["type"] == "feat" and not p["breaking"]]),
                ("Fixes", [p for p in parsed if p["type"] == "fix" and not p["breaking"]])]
    out = [f"# {version}", ""]
    shown = False
    for title, items in sections:
        if items:
            shown = True
            out += [f"## {title}", *[f"- {p['text']}" for p in items], ""]
    if not shown:
        out.append("No user-facing changes.")
    return "\n".join(out).rstrip() + "\n"
