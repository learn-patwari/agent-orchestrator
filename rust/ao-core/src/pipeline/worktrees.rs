//! A git worktree per task (FR-35): agents never work in the user's main checkout.
//! Each task gets its own worktree and branch `task/<name>`. A stacked task branches from another task's branch.
//! Removal refuses to throw away uncommitted work unless explicitly told to.
use crate::runtime::tools::resolve_path;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,60}$").expect("valid pattern"));

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("{0}")]
    Git(String),
}

fn bad<T>(m: impl Into<String>) -> Result<T, WorktreeError> {
    Err(WorktreeError::Git(m.into()))
}

#[derive(Debug, Clone)]
pub struct Worktree {
    pub task: String,
    pub branch: String,
    pub path: PathBuf,
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, WorktreeError> {
    let mut cmd = Command::new("git");
    crate::proc::hide_window(&mut cmd);
    let o = cmd.args(args).current_dir(cwd).output().map_err(|e| WorktreeError::Git(e.to_string()))?;
    if !o.status.success() {
        let (err, out) = (String::from_utf8_lossy(&o.stderr), String::from_utf8_lossy(&o.stdout));
        return bad(if err.trim().is_empty() { out } else { err }.trim().to_string());
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo: PathBuf,
    root: PathBuf,
}

impl WorktreeManager {
    pub fn new(repo: &Path, root: &Path) -> Result<Self, WorktreeError> {
        let (repo, root) = (resolve_path(repo), resolve_path(root));
        if root.starts_with(&repo) {
            return bad("worktree root must not be inside the repository");
        }
        std::fs::create_dir_all(&root).map_err(|e| WorktreeError::Git(e.to_string()))?;
        Ok(WorktreeManager { repo, root })
    }

    pub fn create(&self, task: &str, base: &str) -> Result<Worktree, WorktreeError> {
        if !NAME.is_match(task) || task.contains("..") {
            return bad(format!("invalid task name: '{task}'"));
        }
        let path = self.root.join(task);
        let branch = format!("task/{task}");
        if path.exists() || self.list()?.iter().any(|w| w.task == task) {
            return bad(format!("task worktree exists: {task}"));
        }
        if git(&self.repo, &["rev-parse", "--verify", "--quiet", &format!("{base}^{{commit}}")]).is_err() {
            return bad(format!("unknown base branch: {base}"));
        }
        git(&self.repo, &["worktree", "add", "-b", &branch, &path.to_string_lossy(), base])?;
        Ok(Worktree { task: task.into(), branch, path })
    }

    pub fn list(&self) -> Result<Vec<Worktree>, WorktreeError> {
        let out = git(&self.repo, &["worktree", "list", "--porcelain"])?;
        let (mut found, mut cur) = (vec![], std::collections::HashMap::<String, String>::new());
        for line in out.lines().chain(std::iter::once("")) {
            if line.is_empty() {
                if let (Some(w), Some(b)) = (cur.get("worktree"), cur.get("branch")) {
                    if b.starts_with("refs/heads/task/") {
                        let p = resolve_path(Path::new(w));
                        if p != self.root && p.starts_with(&self.root) {
                            let branch = b.trim_start_matches("refs/heads/").to_string();
                            found.push(Worktree { task: branch.trim_start_matches("task/").into(), branch, path: p });
                        }
                    }
                }
                cur.clear();
                continue;
            }
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            cur.insert(k.into(), v.into());
        }
        Ok(found)
    }

    pub fn remove(&self, task: &str, discard_uncommitted: bool) -> Result<(), WorktreeError> {
        let Some(wt) = self.list()?.into_iter().find(|w| w.task == task) else {
            return bad(format!("no such task worktree: {task}"));
        };
        if !discard_uncommitted && !git(&wt.path, &["status", "--porcelain"])?.is_empty() {
            return bad("worktree has uncommitted changes");
        }
        git(&self.repo, &["worktree", "remove", "--force", &wt.path.to_string_lossy()])?;
        Ok(())
    }
}
