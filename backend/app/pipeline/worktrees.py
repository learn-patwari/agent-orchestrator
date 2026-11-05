"""Git worktree per task (FR-35): agents never work in the user's main checkout.

Each task gets its own worktree and branch `task/<name>`. A stacked task branches from another task's
branch. Removal refuses to throw away uncommitted work unless explicitly told to.
"""
from __future__ import annotations

import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,60}$")


class WorktreeError(Exception):
    pass


@dataclass
class Worktree:
    task: str
    branch: str
    path: Path


def _git(cwd: Path, *args: str) -> str:
    r = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        raise WorktreeError((r.stderr or r.stdout).strip())
    return r.stdout.strip()


class WorktreeManager:
    def __init__(self, repo: Path, root: Path):
        self.repo, self.root = Path(repo).resolve(), Path(root).resolve()
        if self.repo == self.root or self.repo in self.root.parents:
            raise WorktreeError("worktree root must not be inside the repository")
        self.root.mkdir(parents=True, exist_ok=True)

    def _branch(self, task: str) -> str:
        return f"task/{task}"

    def create(self, task: str, base: str) -> Worktree:
        if not NAME.match(task) or ".." in task:
            raise WorktreeError(f"invalid task name: {task!r}")
        path = self.root / task
        branch = self._branch(task)
        if path.exists() or any(w.task == task for w in self.list()):
            raise WorktreeError(f"task worktree exists: {task}")
        try:
            _git(self.repo, "rev-parse", "--verify", "--quiet", f"{base}^{{commit}}")
        except WorktreeError:
            raise WorktreeError(f"unknown base branch: {base}")
        _git(self.repo, "worktree", "add", "-b", branch, str(path), base)
        return Worktree(task, branch, path)

    def list(self) -> list[Worktree]:
        out = _git(self.repo, "worktree", "list", "--porcelain")
        found, cur = [], {}
        for line in out.splitlines() + [""]:
            if not line:
                if cur.get("worktree") and cur.get("branch", "").startswith("refs/heads/task/"):
                    p = Path(cur["worktree"]).resolve()
                    if self.root in p.parents:
                        b = cur["branch"].removeprefix("refs/heads/")
                        found.append(Worktree(b.removeprefix("task/"), b, p))
                cur = {}
                continue
            k, _, v = line.partition(" ")
            cur[k] = v
        return found

    def remove(self, task: str, discard_uncommitted: bool = False) -> None:
        wt = next((w for w in self.list() if w.task == task), None)
        if wt is None:
            raise WorktreeError(f"no such task worktree: {task}")
        if not discard_uncommitted and _git(wt.path, "status", "--porcelain"):
            raise WorktreeError("worktree has uncommitted changes")
        _git(self.repo, "worktree", "remove", "--force", str(wt.path))
