//! Dev -> test -> review state machine per task (FR-35, FR-36).
//!
//! developing -> dev_done -> tested -> approved -> merged
//! tests fail          -> developing
//! REQUEST_CHANGES     -> changes_requested -> (author pushes) dev_done ... ; at the round limit -> escalated
//! HOLD                -> held
//! Review text is read with the gate's own parser (first-line Verdict, second-line Head). A review of an older head
//! is refused, so it can never count as approval.
use super::github::parse_review;
use super::worktrees::{WorktreeError, WorktreeManager};
use crate::db::{now, DbError, DocRepo};
use crate::handoffs;
use crate::inbox::{Inbox, InboxError};
use serde_json::{json, Value};

pub const PEERS: [&str; 2] = ["developer_1", "developer_2"]; // each reviews the other's work

#[derive(Debug, thiserror::Error)]
pub enum FlowError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Inbox(#[from] InboxError),
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
}

pub(crate) type Result<T> = std::result::Result<T, FlowError>;

pub(crate) fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(FlowError::Msg(m.into()))
}

pub fn tasks() -> std::result::Result<DocRepo, DbError> {
    DocRepo::new("tasks")
}

/// A feature may start development only after the human approved its SRS (also checked by the features module).
fn require_approved(feature_id: &str) -> Result<()> {
    match DocRepo::new("features")?.get(feature_id)? {
        Some(f) if f["status"] == "approved" => Ok(()),
        _ => bad("the feature's SRS must be approved before development starts"),
    }
}

fn s(t: &Value, k: &str) -> String {
    t[k].as_str().unwrap_or("").to_string()
}

fn updated(id: &str, patch: Value) -> Result<Value> {
    tasks()?.update(id, patch)?.ok_or_else(|| FlowError::Msg(format!("unknown task {id}")))
}

#[derive(Clone)]
pub struct Flow {
    wt: WorktreeManager,
    integration: String,
    limit: i64,
}

impl Flow {
    pub fn new(wt: WorktreeManager, integration: &str, review_round_limit: i64) -> Self {
        Flow { wt, integration: integration.into(), limit: review_round_limit }
    }

    /// The task, optionally required to be in one of `states`.
    pub(crate) fn get(&self, tid: &str, states: &[&str]) -> Result<Value> {
        let Some(t) = tasks()?.get(tid)? else { return bad(format!("unknown task {tid}")) };
        if !states.is_empty() && !states.contains(&t["status"].as_str().unwrap_or("")) {
            return bad(format!("task is in state {}, expected {}", s(&t, "status"), states.join(" or ")));
        }
        Ok(t)
    }

    pub fn start(&self, feature_id: &str, name: &str, stacked_on: Option<&str>, owner: &str) -> Result<Value> {
        let Some(pos) = PEERS.iter().position(|p| *p == owner) else { return bad("the owner must be developer_1 or developer_2") };
        require_approved(feature_id)?;
        let base = match stacked_on {
            Some(b) => s(&self.get(b, &[])?, "branch"),
            None => self.integration.clone(),
        };
        let w = self.wt.create(name, &base)?;
        Ok(tasks()?.create(
            json!({"feature_id": feature_id, "name": name, "branch": w.branch, "worktree": w.path.to_string_lossy(), "base": base,
                   "status": "developing", "rounds": 0, "tests": null, "pr_number": null, "head_sha": null, "created_at": now(),
                   "owner": owner, "reviewer": PEERS[1 - pos], "release": null}),
            None,
        )?)
    }

    pub fn dev_done(&self, tid: &str, pr_number: i64, head_sha: &str) -> Result<Value> {
        let t = self.get(tid, &["developing", "changes_requested"])?;
        handoffs::record(&s(&t, "owner"), &s(&t, "reviewer"), "peer review", None)?;
        updated(tid, json!({"status": "dev_done", "pr_number": pr_number, "head_sha": head_sha, "tests": null}))
    }

    pub fn record_tests(&self, tid: &str, passed: i64, failed: i64, area: &str) -> Result<Value> {
        let t = self.get(tid, &["dev_done"])?;
        if t["pr_number"].is_null() || t["pr_number"] == 0 {
            return bad("task state has no PR yet");
        }
        let status = if failed == 0 && passed > 0 { "tested" } else { "developing" };
        updated(tid, json!({"tests": {"passed": passed, "failed": failed, "area": area}, "status": status}))
    }

    pub fn record_review(&self, tid: &str, body: &str) -> Result<Value> {
        let t = self.get(tid, &["tested"])?;
        let Some(r) = parse_review(body, now()) else {
            return bad("review needs a first-line verdict: Verdict: APPROVE|REQUEST_CHANGES|HOLD");
        };
        if r["head_sha"] != t["head_sha"] {
            return bad("review is not for the current head commit");
        }
        let (name, owner, reviewer) = (s(&t, "name"), s(&t, "owner"), s(&t, "reviewer"));
        match r["verdict"].as_str().unwrap_or("") {
            "APPROVE" => {
                handoffs::record(&reviewer, "architect", "code review", None)?;
                updated(tid, json!({"status": "approved"}))
            }
            "HOLD" => {
                Inbox::new(false).post("status", &format!("Task {name} is on hold"), "The reviewer put this PR on hold.", json!({}))?;
                updated(tid, json!({"status": "held"}))
            }
            _ => {
                let rounds = t["rounds"].as_i64().unwrap_or(0) + 1;
                if rounds >= self.limit {
                    let body = format!("{rounds} review rounds reached the limit of {}; automatic rounds stopped.", self.limit);
                    Inbox::new(false).post("status", &format!("Task {name} needs you"), &body, json!({}))?;
                    return updated(tid, json!({"status": "escalated", "rounds": rounds}));
                }
                handoffs::record(&reviewer, &owner, "changes requested", None)?;
                updated(tid, json!({"status": "changes_requested", "rounds": rounds}))
            }
        }
    }

    fn by_pr(pr_number: i64) -> Result<Option<Value>> {
        Ok(tasks()?.list()?.into_iter().find(|x| x["pr_number"] == pr_number))
    }

    pub fn tests_for_pr(&self, pr_number: i64) -> Result<Option<Value>> {
        Ok(Self::by_pr(pr_number)?.map(|t| t["tests"].clone()).filter(|t| !t.is_null()))
    }

    pub fn mark_merged(&self, pr_number: i64) -> Result<Option<Value>> {
        match Self::by_pr(pr_number)? {
            Some(t) => Ok(Some(updated(&s(&t, "id"), json!({"status": "merged"}))?)),
            None => Ok(None),
        }
    }
}
