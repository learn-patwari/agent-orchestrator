"""Tool layer (FR-12): files, sandboxed shell, git. Every path is jailed to the workspace.

Allow-lists are enforced by the runner via `ToolBox.call(role_tools, ...)`, not by the prompt.
Shell sandbox for MVP (A-15): command allow-list, cwd locked to workspace, scrubbed env, timeout.
No shell is spawned (argv list). Note: allow-listing python/node/npm still permits arbitrary code in the
workspace, so this is a guardrail, not isolation; container isolation is the planned hardening.
"""
from __future__ import annotations

import fnmatch
import os
import shlex
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from ..providers.base import ToolSpec
from .skills import SkillRegistry

MAX_OUT = 20_000
SHELL_ALLOW = {"python", "pytest", "pip", "node", "npm", "npx", "git", "ls", "dir", "cat", "echo",
               "flake8", "ruff", "tsc"}
OPERATORS = {"|", "||", "&", "&&", ";", ">", ">>", "<", "2>&1"}
PROTECTED = {"main", "master", "stage", "internal"}
GIT_READ = {"status", "diff", "log", "show", "branch", "rev-parse"}
GIT_WRITE = {"add", "commit", "checkout", "switch", "restore", "stash", "worktree", "merge-base"}
SAFE_ENV = ("PATH", "SYSTEMROOT", "HOME", "USERPROFILE", "TEMP", "TMP", "LANG", "COMSPEC", "PATHEXT")


class ToolError(Exception):
    pass


@dataclass
class Workspace:
    root: Path
    never_stage: list[str] = field(default_factory=lambda: [".env", ".env.*", "*.key", "*.pem", "secret*"])

    def resolve(self, rel: str) -> Path:
        p = (self.root / rel).resolve()
        if p != self.root.resolve() and self.root.resolve() not in p.parents:
            raise ToolError(f"path escapes workspace: {rel}")
        return p

    def denied(self, rel: str) -> bool:
        name = Path(rel).name
        return any(fnmatch.fnmatch(name, pat) or fnmatch.fnmatch(rel, pat) for pat in self.never_stage)


def split_args(cmd: str) -> list[str]:
    """Split a command line. On Windows keep backslashes (posix=False) but drop the surrounding quotes."""
    parts = shlex.split(cmd, posix=(os.name != "nt"))
    return [p[1:-1] if len(p) >= 2 and p[0] == p[-1] and p[0] in "\"'" else p for p in parts]


def _clip(s: str) -> str:
    return s if len(s) <= MAX_OUT else s[:MAX_OUT] + "\n...[truncated]"


def _run(argv: list[str], cwd: Path, timeout: int) -> str:
    env = {k: v for k, v in os.environ.items() if k in SAFE_ENV}
    try:
        r = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        raise ToolError(f"timed out after {timeout}s")
    except FileNotFoundError:
        raise ToolError(f"command not found: {argv[0]}")
    return _clip(f"exit={r.returncode}\n{r.stdout}{r.stderr}")


class ToolBox:
    def __init__(self, ws: Workspace, skills: SkillRegistry | None = None, shell_timeout: int = 120):
        self.ws, self.skills, self.shell_timeout = ws, skills, shell_timeout
        obj = {"type": "object"}
        self.specs: dict[str, list[ToolSpec]] = {
            "read_file": [ToolSpec("read_file", "Read a text file in the workspace.", {**obj, "properties": {"path": {"type": "string"}}, "required": ["path"]})],
            "list_dir": [ToolSpec("list_dir", "List a directory in the workspace.", {**obj, "properties": {"path": {"type": "string"}}})],
            "write_file": [ToolSpec("write_file", "Write a text file in the workspace.", {**obj, "properties": {"path": {"type": "string"}, "content": {"type": "string"}}, "required": ["path", "content"]})],
            "run_shell": [ToolSpec("run_shell", "Run an allow-listed command in the workspace (no shell operators).", {**obj, "properties": {"command": {"type": "string"}}, "required": ["command"]})],
            "git_read": [ToolSpec("git_read", "Run a read-only git command (status, diff, log, show, branch).", {**obj, "properties": {"args": {"type": "string"}}, "required": ["args"]})],
            "git_write": [ToolSpec("git_write", "Run a local git command (add, commit, checkout, switch, restore, stash, worktree). Never push.", {**obj, "properties": {"args": {"type": "string"}}, "required": ["args"]})],
            "use_skill": [ToolSpec("use_skill", "Load the full instructions of a listed skill by name.", {**obj, "properties": {"name": {"type": "string"}}, "required": ["name"]})],
        }

    def specs_for(self, allowed: list[str]) -> list[ToolSpec]:
        return [s for t in allowed for s in self.specs.get(t, [])]

    def call(self, allowed: list[str], name: str, args: dict) -> str:
        """Dispatch a model-requested tool. Raises ToolError for refusals and failures."""
        if name not in allowed:
            raise ToolError(f"tool {name} is not allowed for this role")
        fn = getattr(self, f"_t_{name}", None)
        if fn is None:
            raise ToolError(f"unknown tool {name}")
        try:
            return fn(**args)
        except TypeError as e:
            raise ToolError(f"bad arguments for {name}: {e}")

    # --- tools ---
    def _t_read_file(self, path: str) -> str:
        p = self.ws.resolve(path)
        if self.ws.denied(path):
            raise ToolError(f"{path} is on the never-stage/secret list")
        if not p.is_file():
            raise ToolError(f"not a file: {path}")
        return _clip(p.read_text(encoding="utf-8", errors="replace"))

    def _t_list_dir(self, path: str = ".") -> str:
        p = self.ws.resolve(path)
        if not p.is_dir():
            raise ToolError(f"not a directory: {path}")
        return "\n".join(sorted(f"{c.name}{'/' if c.is_dir() else ''}" for c in p.iterdir() if c.name != ".git"))

    def _t_write_file(self, path: str, content: str) -> str:
        p = self.ws.resolve(path)
        if ".git" in p.relative_to(self.ws.root.resolve()).parts:
            raise ToolError("writing inside .git is not allowed")
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, encoding="utf-8")
        return f"wrote {len(content)} chars to {path}"

    def _t_run_shell(self, command: str) -> str:
        argv = split_args(command)
        if any(t in OPERATORS for t in argv):  # no shell is used, so these would be inert; refuse clearly
            raise ToolError("shell operators are not allowed")
        if not argv or argv[0] not in SHELL_ALLOW:
            raise ToolError(f"command not on the allow-list: {argv[0] if argv else ''}")
        if argv[0] == "git":
            raise ToolError("use the git_read / git_write tools for git")
        return _run(argv, self.ws.root, self.shell_timeout)

    def _git(self, args: str, allowed_sub: set[str]) -> list[str]:
        argv = split_args(args)
        if not argv or argv[0] not in allowed_sub:
            raise ToolError(f"git subcommand not allowed: {argv[0] if argv else ''}")
        return argv

    def _t_git_read(self, args: str) -> str:
        return _run(["git", *self._git(args, GIT_READ)], self.ws.root, 60)

    def _t_git_write(self, args: str) -> str:
        argv = self._git(args, GIT_WRITE)
        if argv[0] == "add":
            for a in argv[1:]:
                if a in (".", "-A", "--all"):
                    continue  # checked below via staged-file scan
                if self.ws.denied(a):
                    raise ToolError(f"refusing to stage denylisted path: {a}")
        if argv[0] == "checkout" and any(a in ("--force", "-f") for a in argv):
            raise ToolError("forced checkout is a destructive operation and needs a human gate")
        if argv[0] == "commit":
            names = _run(["git", "diff", "--cached", "--name-only"], self.ws.root, 30).splitlines()[1:]
            bad = [f for f in names if self.ws.denied(f)]
            if bad:
                raise ToolError(f"commit refused, denylisted files staged: {bad}")
        out = _run(["git", *argv], self.ws.root, 60)
        if argv[0] == "add":
            staged = _run(["git", "diff", "--cached", "--name-only"], self.ws.root, 30).splitlines()[1:]
            bad = [f for f in staged if self.ws.denied(f)]
            if bad:
                _run(["git", "rm", "--cached", "--quiet", "--ignore-unmatch", *bad], self.ws.root, 30)
                raise ToolError(f"unstaged denylisted files: {bad}")
        return out

    def _t_use_skill(self, name: str) -> str:
        body = self.skills.load_body(name) if self.skills else None
        if body is None:
            raise ToolError(f"unknown skill {name}")
        return body
