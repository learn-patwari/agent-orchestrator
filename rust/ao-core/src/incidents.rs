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

/// The `incident` tool bound to the acting role: the reporter reports; production support escalates, hands fixes on and
/// resolves.
pub struct IncidentTool {
    actor: String,
}

fn row(i: &Value) -> Value {
    let mut o = serde_json::Map::new();
    for k in ["id", "title", "severity", "level", "status", "task_id", "fix_owner"] {
        o.insert(k.into(), i.get(k).cloned().unwrap_or(Value::Null));
    }
    Value::Object(o)
}

impl IncidentTool {
    pub fn new(actor: &str) -> Self {
        IncidentTool { actor: actor.into() }
    }

    pub fn into_hook(self) -> crate::runtime::tools::Hook {
        Box::new(move |args: &Value| self.call(args))
    }

    fn need(&self, role: &str) -> std::result::Result<(), crate::runtime::tools::ToolError> {
        if self.actor != role {
            return Err(crate::runtime::tools::ToolError(format!("only the {role} role can do this step")));
        }
        Ok(())
    }

    pub fn call(&self, args: &Value) -> std::result::Result<String, crate::runtime::tools::ToolError> {
        use crate::runtime::tools::ToolError;
        let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("");
        let opt = |k: &str| args.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty());
        let action = s("action");
        let inc = Incidents;
        let fail = |e: IncidentError| ToolError(e.to_string());
        let dump = |v: Value| crate::providers::base::py_dumps(&v);
        match action {
            "list" => Ok(dump(Value::Array(inc.list(false).map_err(fail)?.iter().map(row).collect()))),
            "report" => {
                self.need("incident_reporter")?;
                Ok(dump(row(&inc.report(s("title"), s("severity"), s("evidence"), "prod", opt("task_id")).map_err(fail)?)))
            }
            "escalate" | "hand_to_developer" | "resolve" => {
                self.need("production_support")?;
                let Some(id) = opt("incident_id") else { return Err(ToolError("incident_id is required".into())) };
                let r = match action {
                    "escalate" => inc.escalate(id, s("reason")),
                    "resolve" => inc.resolve(id),
                    _ => inc.hand_to_developer(id),
                };
                Ok(dump(row(&r.map_err(fail)?)))
            }
            _ => Err(ToolError(format!("unknown incident action {action}; use list, report, escalate, hand_to_developer or resolve"))),
        }
    }
}
