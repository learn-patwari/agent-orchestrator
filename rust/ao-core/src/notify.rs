//! In-app notifications. Email, webhook and push plug in later behind the same `send`.
use crate::db::{now, DbError, DocRepo};
use serde_json::{json, Value};

pub struct Notifier {
    repo: DocRepo,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Notifier {
    pub fn new() -> Self {
        Notifier { repo: DocRepo::new("notifications").expect("notifications table exists") }
    }

    /// Creates an unread notification. A `key` suppresses duplicates while one is still unread.
    pub fn send(&self, kind: &str, title: &str, body: &str, key: Option<&str>) -> Result<Option<Value>, DbError> {
        if let Some(k) = key {
            let dup = self.repo.list()?.iter().any(|n| n["key"].as_str() == Some(k) && !is_read(n));
            if dup {
                return Ok(None);
            }
        }
        let doc = json!({"kind": kind, "title": title, "body": body, "key": key, "read": false, "at": now()});
        Ok(Some(self.repo.create(doc, None)?))
    }

    pub fn unread(&self) -> Result<Vec<Value>, DbError> {
        Ok(self.repo.list()?.into_iter().filter(|n| !is_read(n)).collect())
    }

    pub fn mark_read(&self, id: &str) -> Result<Option<Value>, DbError> {
        self.repo.update(id, json!({"read": true}))
    }
}

fn is_read(n: &Value) -> bool {
    n["read"].as_bool().unwrap_or(false)
}
