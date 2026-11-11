//! Human inbox, assumptions list and gates.
//!
//! Questions carry a recommended default. Accepting it (or "decide with defaults" mode) records an assumption the
//! human can overrule later. Gated actions always wait for an explicit human approval, which is single-use and audited.
use crate::db::{now, DbError, DocRepo};
use crate::notify::Notifier;
use serde_json::{json, Map, Value};

pub const GATED: &[(&str, &str)] = &[
    ("merge_main", "Merge into the main branch"),
    ("production_config", "Change production configuration or environment"),
    ("credentials", "Use or provide credentials"),
    ("destructive_git", "Destructive git operation (force push, reset, branch delete)"),
    ("delete_data", "Permanently delete data"),
    ("srs_approval", "Approve an SRS so development can start"),
];

#[derive(Debug, thiserror::Error)]
pub enum InboxError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("{0}")]
    Blocked(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

fn audit(event: &str, fields: Value) -> Result<Value, DbError> {
    let mut m = Map::new();
    m.insert("event".into(), json!(event));
    m.insert("at".into(), json!(now()));
    if let Value::Object(f) = fields {
        m.extend(f);
    }
    DocRepo::new("audit")?.create(Value::Object(m), None)
}

pub struct Inbox {
    items: DocRepo,
    assumed: DocRepo,
    auto: bool,
}

impl Inbox {
    pub fn new(decide_with_defaults: bool) -> Self {
        Inbox {
            items: DocRepo::new("inbox").expect("inbox table"),
            assumed: DocRepo::new("assumptions").expect("assumptions table"),
            auto: decide_with_defaults,
        }
    }

    pub fn post(&self, kind: &str, title: &str, body: &str, extra: Value) -> Result<Value, InboxError> {
        let mut m = Map::new();
        for (k, v) in [("kind", json!(kind)), ("title", json!(title)), ("body", json!(body)), ("status", json!("open")), ("created_at", json!(now()))] {
            m.insert(k.into(), v);
        }
        if let Value::Object(e) = extra {
            m.extend(e);
        }
        let item = self.items.create(Value::Object(m), None)?;
        if kind == "question" || kind == "approval" {
            // status items never interrupt the human
            let key = format!("inbox:{}", item["id"].as_str().unwrap_or(""));
            Notifier::new().send(kind, title, body, Some(&key))?;
        }
        Ok(item)
    }

    pub fn ask(&self, title: &str, body: &str, options: &[&str], default: Option<&str>, run_id: Option<&str>) -> Result<Value, InboxError> {
        let item = self.post("question", title, body, json!({"options": options, "default": default, "run_id": run_id}))?;
        if self.auto && default.is_some() {
            return self.resolve_default(&item, "auto_default");
        }
        Ok(item)
    }

    fn get_open(&self, id: &str) -> Result<Value, InboxError> {
        let item = self.items.get(id)?.ok_or_else(|| InboxError::NotFound(id.to_string()))?;
        let status = item["status"].as_str().unwrap_or("");
        if status != "open" {
            return Err(InboxError::Conflict(format!("item {id} is already {status}")));
        }
        Ok(item)
    }

    fn resolve_default(&self, item: &Value, by: &str) -> Result<Value, InboxError> {
        self.assumed.create(
            json!({"question": item["title"], "assumed": item["default"], "item_id": item["id"],
                   "status": "assumed", "source": by, "created_at": now()}),
            None,
        )?;
        let id = item["id"].as_str().unwrap_or("");
        let out = self.items.update(id, json!({"status": "resolved", "answer": item["default"], "resolved_by": by}))?;
        Ok(out.unwrap_or(Value::Null))
    }

    pub fn answer(&self, id: &str, answer: &str) -> Result<Value, InboxError> {
        let item = self.get_open(id)?;
        if item["kind"] == "approval" {
            return Err(InboxError::Invalid("approvals are decided with approve/reject, not answered".into()));
        }
        Ok(self.items.update(id, json!({"status": "resolved", "answer": answer, "resolved_by": "human"}))?.unwrap_or(Value::Null))
    }

    pub fn accept_default(&self, id: &str) -> Result<Value, InboxError> {
        let item = self.get_open(id)?;
        if item["default"].is_null() {
            return Err(InboxError::Invalid("this question has no recommended default".into()));
        }
        self.resolve_default(&item, "accepted_default")
    }

    pub fn decide(&self, id: &str, approve: bool) -> Result<Value, InboxError> {
        let item = self.get_open(id)?;
        if item["kind"] != "approval" {
            return Err(InboxError::Invalid("only approval items can be approved or rejected".into()));
        }
        let status = if approve { "approved" } else { "rejected" };
        audit(&format!("gate_{status}"), json!({"item_id": id, "action": item["action"], "run_id": item["run_id"]}))?;
        Ok(self.items.update(id, json!({"status": status, "resolved_by": "human"}))?.unwrap_or(Value::Null))
    }

    pub fn list(&self, status: Option<&str>) -> Result<Vec<Value>, InboxError> {
        Ok(self.items.list()?.into_iter().filter(|i| status.is_none_or(|s| i["status"] == s)).collect())
    }

    pub fn assumptions(&self) -> Result<Vec<Value>, InboxError> {
        Ok(self.assumed.list()?)
    }

    pub fn overrule(&self, id: &str, new_value: &str) -> Result<Value, InboxError> {
        self.assumed.get(id)?.ok_or_else(|| InboxError::NotFound(id.to_string()))?;
        Ok(self.assumed.update(id, json!({"status": "overruled", "overruled_to": new_value}))?.unwrap_or(Value::Null))
    }
}

/// Opens an approval request. It never auto-approves, whatever mode the inbox is in.
pub fn request_gate(action: &str, detail: &str, run_id: Option<&str>) -> Result<Value, InboxError> {
    let Some((_, title)) = GATED.iter().find(|(k, _)| *k == action) else {
        return Err(InboxError::Invalid(format!("unknown gated action {action}")));
    };
    let item = Inbox::new(false).post("approval", title, detail, json!({"action": action, "run_id": run_id}))?;
    audit("gate_requested", json!({"item_id": item["id"], "action": action, "run_id": run_id}))?;
    Ok(item)
}

/// Blocks unless a human approved exactly this action; the approval is consumed.
pub fn check_gate(action: &str, approval_id: Option<&str>) -> Result<(), GateError> {
    let items = DocRepo::new("inbox")?;
    let item = match approval_id {
        Some(id) => items.get(id)?,
        None => None,
    };
    let ok = item.as_ref().is_some_and(|i| i["kind"] == "approval" && i["action"] == action && i["status"] == "approved");
    if !ok {
        audit("gate_blocked", json!({"action": action, "item_id": approval_id}))?;
        return Err(GateError::Blocked(format!("{action} needs human approval")));
    }
    let id = approval_id.unwrap_or("");
    items.update(id, json!({"status": "consumed"}))?;
    audit("gate_passed", json!({"action": action, "item_id": id}))?;
    Ok(())
}
