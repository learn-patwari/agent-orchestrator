import subprocess
from pathlib import Path

import pytest

from app.pipeline.worktrees import WorktreeError, WorktreeManager


def git(cwd, *a):
    return subprocess.run(["git", *a], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


@pytest.fixture
def repo(tmp_path):
    r = tmp_path / "repo"
    r.mkdir()
    git(r, "init", "-q", "-b", "internal")
    git(r, "config", "user.email", "t@t")
    git(r, "config", "user.name", "t")
    (r / "a.txt").write_text("a")
    git(r, "add", "a.txt")
    git(r, "commit", "-q", "-m", "init")
    return r


def mgr(repo, tmp_path):
    return WorktreeManager(repo, tmp_path / "worktrees")


def test_create_makes_isolated_worktree_on_new_branch_from_base(repo, tmp_path):
    wt = mgr(repo, tmp_path).create("task-1", base="internal")
    assert wt.path.is_dir() and (wt.path / "a.txt").read_text() == "a"
    assert git(wt.path, "rev-parse", "--abbrev-ref", "HEAD") == wt.branch == "task/task-1"
    assert git(repo, "rev-parse", "--abbrev-ref", "HEAD") == "internal"  # main checkout untouched
    (wt.path / "b.txt").write_text("b")
    assert not (repo / "b.txt").exists()


def test_two_tasks_do_not_share_files_or_branches(repo, tmp_path):
    m = mgr(repo, tmp_path)
    a, b = m.create("t1", "internal"), m.create("t2", "internal")
    assert a.path != b.path and a.branch != b.branch
    (a.path / "x").write_text("1")
    assert not (b.path / "x").exists()


def test_stacked_worktree_branches_from_another_task_branch(repo, tmp_path):
    m = mgr(repo, tmp_path)
    a = m.create("base-task", "internal")
    (a.path / "f.txt").write_text("f")
    git(a.path, "add", "f.txt")
    git(a.path, "commit", "-q", "-m", "work")
    b = m.create("next-task", a.branch)
    assert (b.path / "f.txt").read_text() == "f"


def test_refuses_bad_names_existing_tasks_and_unknown_base(repo, tmp_path):
    m = mgr(repo, tmp_path)
    for bad in ("../evil", "a b", "", "x/../y", "-rf"):
        with pytest.raises(WorktreeError):
            m.create(bad, "internal")
    m.create("ok", "internal")
    with pytest.raises(WorktreeError, match="exists"):
        m.create("ok", "internal")
    with pytest.raises(WorktreeError, match="base"):
        m.create("other", "no-such-branch")


def test_list_and_remove_refuses_uncommitted_work_unless_forced_by_flag(repo, tmp_path):
    m = mgr(repo, tmp_path)
    wt = m.create("t1", "internal")
    assert [w.task for w in m.list()] == ["t1"]
    (wt.path / "dirty.txt").write_text("unsaved")
    with pytest.raises(WorktreeError, match="uncommitted"):
        m.remove("t1")
    assert wt.path.exists()
    m.remove("t1", discard_uncommitted=True)
    assert not wt.path.exists() and m.list() == []


def test_worktree_root_must_not_be_inside_the_repo(repo):
    with pytest.raises(WorktreeError, match="inside"):
        WorktreeManager(repo, repo / "wt")
