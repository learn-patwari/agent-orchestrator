"""Role loader (FR-10): role file = YAML frontmatter + markdown body.

Frontmatter: name, description, tools (list), skills (list), model (hint).
Files without `tools` get the default allow-list for that role name (platform-enforced,
not a prompt hint, FR-12). Unknown tool names are reported by `validate`.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import yaml

READ = ["read_file", "list_dir", "git_read", "use_skill"]
DEVELOPERS = ("developer", "developer_1", "developer_2")  # all need an approved SRS before they may run
DEFAULT_TOOLS = {
    "orchestrator": READ,
    "architect": READ + ["write_file", "pipeline"],
    "developer": READ + ["write_file", "run_shell", "git_write"],
    "developer_1": READ + ["write_file", "run_shell", "git_write", "pipeline"],
    "developer_2": READ + ["write_file", "run_shell", "git_write", "pipeline"],
    "tester": READ + ["write_file", "run_shell", "pipeline"],
    "reviewer": READ,
    "cicd": READ + ["run_shell", "pipeline"],
    "incident-reporter": READ,
    "production-support": READ + ["write_file", "run_shell", "git_write"],
    "technical_writer": READ + ["write_file", "git_write"],  # prose only: no shell, no pipeline
    "technical-writer": READ + ["write_file", "git_write"],
    "incident_reporter": READ + ["incident"],
    "production_support": READ + ["write_file", "run_shell", "git_write", "incident"],
}


@dataclass
class Role:
    name: str
    description: str
    body: str
    tools: list[str] = field(default_factory=list)
    skills: list[str] = field(default_factory=list)
    model: str | None = None
    source: str = ""


def parse_role(text: str, source: str = "") -> Role:
    if not text.startswith("---"):
        raise ValueError(f"{source}: missing frontmatter")
    _, fm, body = text.split("---", 2)
    meta = yaml.safe_load(fm) or {}
    if not meta.get("name") or not meta.get("description"):
        raise ValueError(f"{source}: frontmatter needs name and description")
    skills = meta.get("skills")
    if skills is None:  # reference files list skills in a markdown section; none declared here
        skills = []
    return Role(
        name=meta["name"], description=meta["description"], body=body.strip(),
        tools=list(meta.get("tools") or DEFAULT_TOOLS.get(meta["name"], READ)),
        skills=list(skills), model=meta.get("model"), source=source,
    )


def load_roles(dirs: list[str | Path]) -> dict[str, Role]:
    roles: dict[str, Role] = {}
    for d in dirs:
        for f in sorted(Path(d).glob("*.md")):
            try:
                r = parse_role(f.read_text(encoding="utf-8"), str(f))
            except ValueError:
                continue
            roles.setdefault(r.name, r)  # earlier dir wins
    return roles


def validate(role: Role, known_tools: set[str], known_skills: set[str]) -> list[str]:
    problems = [f"unknown tool {t}" for t in role.tools if t not in known_tools]
    problems += [f"missing skill {s}" for s in role.skills if s not in known_skills]
    return problems
