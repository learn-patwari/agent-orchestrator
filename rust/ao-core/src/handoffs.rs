//! Hand-offs the office view animates. One is recorded for every real transition, never for a guess.
use crate::db::{now, DbError, DocRepo};
use serde_json::{json, Value};
use std::sync::Mutex;

static LAST: Mutex<f64> = Mutex::new(0.0);

pub fn record(sender: &str, receiver: &str, label: &str, run_id: Option<&str>) -> Result<Value, DbError> {
    let at = {
        // strictly increasing timestamps, so the order survives a coarse clock
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let at = now().max(*last + 1e-6);
        *last = at;
        at
    };
    DocRepo::new("handoffs")?.create(json!({"from": sender, "to": receiver, "label": label, "at": at, "run_id": run_id}), None)
}
