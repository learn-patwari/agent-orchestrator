//! Incidents handled by production support, levels L1 to L4.
//!
//! A failing production test report raises an incident (the incident reporter's job), once: the same open title is
//! never reported twice. Production support starts at L1 and escalates one level at a time: L2 needs a code change,
//! L3 is the architect's call, L4 asks the human and is the last level. A fix is handed to the developer who owns
//! the feature. Every hand-off is recorded for the office view. (The `incident` agent tool arrives with the runtime.)
use crate::db::{now, DbError, DocRepo};
use crate::handoffs;
use crate::inbox::{Inbox, InboxError};
use serde_json::{json, Value};

pub const SEVERITIES: [&str; 4] = ["low", "medium", "high", "critical"];
pub const MAX_LEVEL: i64 = 4;

#[derive(Debug, thiserror::Error)]
pub enum IncidentError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Inbox(#[from] InboxError),
}

type Result<T> = std::result::Result<T, IncidentError>;

fn repo() -> std::result::Result<DocRepo, DbError> {
    DocRepo::new("incidents")
}

fn key(title: &str) -> String {
    title.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(IncidentError::Invalid(msg.into()))
}

pub struct Incidents;

impl Incidents {
    pub fn list(&self, include_resolved: bool) -> Result<Vec<Value>> {
        let rows = repo()?.list()?;
        Ok(if include_resolved { rows } else { rows.into_iter().filter(|r| r["status"] != "resolved").collect() })
    }

    pub fn get(&self, iid: &str) -> Result<Value> {
        match repo()?.get(iid)? {
            Some(i) => Ok(i),
            None => bad(format!("unknown incident {iid}")),
        }
    }

    pub fn report(&self, title: &str, severity: &str, evidence: &str, source: &str, task_id: Option<&str>) -> Result<Value> {
        if !SEVERITIES.contains(&severity) {
            return bad(format!("severity must be one of {}", SEVERITIES.join(", ")));
        }
        if title.trim().is_empty() {
            return bad("an incident needs a title");
        }
        if let Some(same) = self.list(false)?.into_iter().find(|i| key(i["title"].as_str().unwrap_or("")) == key(title)) {
            return Ok(same);
        }
        let inc = repo()?.create(
            json!({"title": title.trim(), "severity": severity, "evidence": evidence, "source": source, "task_id": task_id,
                   "level": 1, "status": "open", "history": [], "fix_owner": null, "created_at": now()}),
            None,
        )?;
        handoffs::record("incident_reporter", "production_support", &format!("incident: {}", short(inc["title"].as_str().unwrap_or(""), 40)), None)?;
        Ok(inc)
    }

    pub fn production_tests(&self, task_id: &str, passed: i64, failed: i64, area: &str) -> Result<Option<Value>> {
        let Some(t) = DocRepo::new("tasks")?.get(task_id)? else { return bad(format!("unknown task {task_id}")) };
        if failed <= 0 {
            return Ok(None);
        }
        let name = t["name"].as_str().unwrap_or("");
        Ok(Some(self.report(
            &format!("Production tests failing: {name} / {area}"),
            "high",
            &format!("{failed} failing, {passed} passing in {area}"),
            "prod",
            Some(task_id),
        )?))
    }

    fn open(&self, iid: &str) -> Result<Value> {
        let i = self.get(iid)?;
        if i["status"] == "resolved" {
            return bad("this incident is already resolved");
        }
        Ok(i)
    }

    pub fn escalate(&self, iid: &str, reason: &str) -> Result<Value> {
        let i = self.open(iid)?;
        let current = i["level"].as_i64().unwrap_or(1);
        if current >= MAX_LEVEL {
            return bad("this incident is already at L4, the last level");
        }
        let level = current + 1;
        let mut history = i["history"].as_array().cloned().unwrap_or_default();
        history.push(json!({"level": level, "reason": reason, "at": now()}));
        let title = i["title"].as_str().unwrap_or("");
        if level == 3 {
            handoffs::record("production_support", "architect", &format!("L3: {}", short(title, 30)), None)?;
        }
        if level == MAX_LEVEL {
            Inbox::new(false).post("status", &format!("Incident needs you: {title}"), &format!("Escalated to L4. {reason}"), json!({}))?;
        }
        Ok(repo()?.update(iid, json!({"level": level, "history": history}))?.unwrap_or(Value::Null))
    }

    pub fn hand_to_developer(&self, iid: &str) -> Result<Value> {
        let i = self.open(iid)?;
        let task = match i["task_id"].as_str() {
            Some(tid) => DocRepo::new("tasks")?.get(tid)?,
            None => None,
        };
        let Some(t) = task else { return bad("a fix needs the task that owns the feature; this incident has none") };
        let owner = t["owner"].as_str().unwrap_or("");
        handoffs::record("production_support", owner, &format!("fix: {}", short(i["title"].as_str().unwrap_or(""), 36)), None)?;
        Ok(repo()?.update(iid, json!({"status": "fixing", "fix_owner": owner}))?.unwrap_or(Value::Null))
    }

    pub fn resolve(&self, iid: &str) -> Result<Value> {
        self.open(iid)?;
        Ok(repo()?.update(iid, json!({"status": "resolved", "resolved_at": now()}))?.unwrap_or(Value::Null))
    }
}
