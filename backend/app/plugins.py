"""Plugins (FR-20 to FR-22): a folder with plugin.json, roles/, skills/, workflows/, templates/ and optional mcp.json.

A plugin is validated on install and again on enable; one with problems is kept but stays disabled, and the problems
name the file and line. Enabling adds its roles and skills folders to the ones the app already loads. Nothing is
executed from a plugin: it is data (role files, skill text, workflow YAML, templates, MCP config without secrets).
"""
from __future__ import annotations

import json
import re
import shutil
import time
from pathlib import Path

import yaml

from . import db, secrets_store
from .config import data_dir
from .runtime.roles import parse_role
from .runtime.skills import _parse as parse_skill

SHIPPED = Path(__file__).resolve().parents[2] / "plugins"
NAME = re.compile(r"^[a-z0-9][a-z0-9_-]*$")
plugins_repo = db.DocRepo("plugins")


def _store() -> Path:
    return data_dir() / "plugins"


def _line(path: Path, key: str) -> int:
    for i, ln in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        if ln.startswith(key):
            return i
    return 1


def _strings(node):
    if isinstance(node, str):
        yield node
    elif isinstance(node, dict):
        for v in node.values():
            yield from _strings(v)
    elif isinstance(node, list):
        for v in node:
            yield from _strings(v)


def read_manifest(path: Path) -> tuple[dict | None, list[str]]:
    mf = path / "plugin.json"
    if not mf.exists():
        return None, [f"{path.name}: plugin.json is missing"]
    try:
        data = json.loads(mf.read_text(encoding="utf-8"))
    except ValueError as e:
        return None, [f"plugin.json: not valid JSON ({e})"]
    problems = []
    if not isinstance(data, dict):
        return None, ["plugin.json: must be a JSON object"]
    for k in ("name", "version", "description"):
        if not isinstance(data.get(k), str) or not data[k].strip():
            problems.append(f"plugin.json: {k} is required")
    if isinstance(data.get("name"), str) and not NAME.match(data["name"]):
        problems.append("plugin.json: name uses lowercase letters, digits, - and _")
    return data, problems


def validate_plugin(path: Path, known_roles: set[str], known_tools: set[str], known_skills: set[str]) -> list[str]:
    path = Path(path)
    _, problems = read_manifest(path)
    plugin_skills = set()
    for md in sorted((path / "skills").glob("*/SKILL.md")):
        parsed = parse_skill(md)
        if parsed is None:
            problems.append(f"{md.relative_to(path)}: needs frontmatter with a name")
        else:
            plugin_skills.add(str(parsed[0]["name"]))
    skills = known_skills | plugin_skills
    plugin_roles = set()
    for md in sorted((path / "roles").glob("*.md")):
        rel = md.relative_to(path)
        try:
            role = parse_role(md.read_text(encoding="utf-8"), str(rel))
        except (ValueError, yaml.YAMLError) as e:
            problems.append(f"{rel}:1: {e}")
            continue
        plugin_roles.add(role.name)
        for t in role.tools:
            if t not in known_tools:
                problems.append(f"{rel}:{_line(md, 'tools:')}: unknown tool {t}")
        for s in role.skills:
            if s not in skills:
                problems.append(f"{rel}:{_line(md, 'skills:')}: missing skill {s}")
    roles = known_roles | plugin_roles
    for wf in sorted([*(path / "workflows").glob("*.yaml"), *(path / "workflows").glob("*.yml")]):
        rel = wf.relative_to(path)
        try:
            doc = yaml.safe_load(wf.read_text(encoding="utf-8")) or {}
        except yaml.YAMLError as e:
            problems.append(f"{rel}: not valid YAML ({e})")
            continue
        for st in (doc.get("stages") or []) if isinstance(doc, dict) else []:
            if not isinstance(st, dict) or st.get("role") not in roles:
                problems.append(f"{rel}: stage {st.get('name') if isinstance(st, dict) else st} names unknown role "
                                f"{st.get('role') if isinstance(st, dict) else '?'}")
    mcp = path / "mcp.json"
    if mcp.exists():
        try:
            cfg = json.loads(mcp.read_text(encoding="utf-8"))
        except ValueError as e:
            problems.append(f"mcp.json: not valid JSON ({e})")
            cfg = {}
        for name, srv in (cfg.get("servers") or {}).items() if isinstance(cfg, dict) else []:
            if isinstance(srv, dict) and "env" in srv:
                problems.append(f"mcp.json: server {name} carries secrets in env; use env_refs (a variable name)")
        if any(secrets_store.looks_like_secret(s) for s in _strings(cfg)):
            problems.append("mcp.json: contains something that looks like a secret; plugins never carry secrets")
    return problems


class Plugins:
    def list(self) -> list[dict]:
        return plugins_repo.list()

    def available(self) -> list[dict]:
        out = []
        for d in sorted(SHIPPED.glob("*/plugin.json")):
            data, _ = read_manifest(d.parent)
            if data:
                out.append({"name": data.get("name", d.parent.name), "description": data.get("description", ""),
                            "version": data.get("version", ""), "path": str(d.parent)})
        return out

    def install(self, source: str, roles: set[str], tools: set[str], skills: set[str]) -> dict:
        src = Path(source)
        if not src.is_dir():
            raise ValueError(f"{source} is not a folder")
        manifest, mp = read_manifest(src)
        if manifest is None:
            raise ValueError(mp[0])
        name = manifest.get("name", "")
        if not NAME.match(name):
            raise ValueError("; ".join(mp) or "plugin.json: bad name")
        dest = _store() / name
        if dest.exists():
            shutil.rmtree(dest)
        shutil.copytree(src, dest)
        problems = validate_plugin(dest, roles, tools, skills)
        doc = {"name": name, "version": manifest.get("version", ""), "description": manifest.get("description", ""),
               "enabled": False, "problems": problems, "installed_at": time.time()}
        if plugins_repo.get(name):
            return plugins_repo.update(name, doc)
        return plugins_repo.create(doc, id=name)

    def _get(self, name: str) -> dict:
        d = plugins_repo.get(name)
        if d is None:
            raise KeyError(name)
        return d

    def enable(self, name: str, roles: set[str], tools: set[str], skills: set[str]) -> dict:
        d = self._get(name)
        problems = validate_plugin(_store() / name, roles, tools, skills)
        if problems:
            plugins_repo.update(name, {"problems": problems, "enabled": False})
            raise ValueError(f"{name} has problems: {'; '.join(problems)}")
        return plugins_repo.update(d["id"], {"enabled": True, "problems": []})

    def disable(self, name: str) -> dict:
        return plugins_repo.update(self._get(name)["id"], {"enabled": False})

    def remove(self, name: str) -> bool:
        if not plugins_repo.get(name):
            return False
        plugins_repo.delete(name)
        shutil.rmtree(_store() / name, ignore_errors=True)
        return True

    def dirs(self) -> tuple[list[str], list[str]]:
        """Roles and skills folders of the enabled plugins, to be added to what the app loads."""
        rd, sd = [], []
        for p in plugins_repo.list():
            if p.get("enabled"):
                base = _store() / p["name"]
                if (base / "roles").is_dir():
                    rd.append(str(base / "roles"))
                if (base / "skills").is_dir():
                    sd.append(str(base / "skills"))
        return rd, sd
