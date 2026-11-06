"""Skill registry (FR-11, FR-18): folders with SKILL.md (frontmatter name, description).

Search paths are ordered by precedence (first wins: plugin-local, project, personal).
The model sees only name + description; the body is loaded when a skill is invoked.
The registry is read-only: it never writes into a skills folder.
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import yaml


@dataclass
class SkillInfo:
    name: str
    description: str
    path: Path  # SKILL.md
    source: str  # the search path it came from


def _parse(path: Path) -> tuple[dict, str] | None:
    text = path.read_text(encoding="utf-8", errors="replace")
    if not text.startswith("---"):
        return None
    try:
        _, fm, body = text.split("---", 2)
        meta = yaml.safe_load(fm) or {}
    except (ValueError, yaml.YAMLError):
        return None
    if not isinstance(meta, dict) or not meta.get("name"):
        return None
    return meta, body.strip()


class SkillRegistry:
    def __init__(self, search_paths: list[str | Path]):
        self.search_paths = [Path(p) for p in search_paths]

    def scan(self) -> dict[str, SkillInfo]:
        found: dict[str, SkillInfo] = {}
        for root in self.search_paths:
            if not root.is_dir():
                continue
            for md in sorted(root.glob("*/SKILL.md")):
                parsed = _parse(md)
                if not parsed:
                    continue
                meta, _ = parsed
                found.setdefault(str(meta["name"]), SkillInfo(
                    str(meta["name"]), str(meta.get("description", "")), md, str(root)))
        return found

    def catalog(self, only: list[str] | None = None) -> list[dict]:
        skills = self.scan()
        names = only if only is not None else list(skills)
        return [{"name": n, "description": skills[n].description, "source": skills[n].source}
                for n in names if n in skills]

    def load_body(self, name: str) -> str | None:
        info = self.scan().get(name)
        if not info:
            return None
        parsed = _parse(info.path)
        return parsed[1] if parsed else None
