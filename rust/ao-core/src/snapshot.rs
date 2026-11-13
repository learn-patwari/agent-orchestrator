//! One shared, read-only picture of the app for every agent.
//!
//! The database is the only place that is written. A publisher rebuilds this snapshot about once a second and swaps it in
//! as one unit; readers take a shared pointer to it, so reading costs no query, no lock held while working, and never
//! sees a half-written state. Asking "who is running" or "what did that run report" therefore needs no model call.
//!
//! Agent-to-agent messages are written to the `mailbox` collection with [`post`] and arrive in the next snapshot.
//! The snapshot may lag by one publish: budget gates, approvals and the merge gate keep reading the database directly.
use crate::db::{now, DbError, DocRepo};
use crate::orchestrator::status_table;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

/// Longest text kept in one message, task line or report excerpt.
pub const MAX_TEXT: usize = 600;
const TASK_CHARS: usize = 120;
const MAX_RUNS: usize = 40;
const MAX_MAILBOX: usize = 100;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub version: u64,
    pub at: f64,
    /// One row per role: state, task, steps, error.
    pub team: Vec<Value>,
    /// The latest runs, with shortened task and report text.
    pub runs: Vec<Value>,
    pub features: Vec<Value>,
    pub mailbox: Vec<Value>,
    pub open_inbox: usize,
}

fn cell() -> &'static RwLock<Arc<Snapshot>> {
    static CELL: OnceLock<RwLock<Arc<Snapshot>>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(Arc::new(Snapshot { version: 0, at: 0.0, team: vec![], runs: vec![], features: vec![], mailbox: vec![], open_inbox: 0 })))
}

/// The latest published snapshot. The returned copy never changes.
pub fn current() -> Arc<Snapshot> {
    cell().read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n).collect();
    t.push('…');
    t
}

fn text(v: &Value) -> String {
    v.as_str().map(String::from).unwrap_or_default()
}

/// Reads the database, builds a new snapshot and publishes it.
pub fn rebuild(roles: &[String]) -> Result<Arc<Snapshot>, DbError> {
    let mut names: Vec<String> = roles.to_vec();
    let all = DocRepo::new("runs")?.list()?;
    for r in &all {
        let n = text(&r["role"]);
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    }
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let team = status_table(&refs)?;
    let runs: Vec<Value> = all[all.len().saturating_sub(MAX_RUNS)..]
        .iter()
        .map(|r| {
            json!({"run_id": r["id"], "role": r["role"], "task": cut(&text(&r["task"]), TASK_CHARS), "status": r["status"], "steps": r["steps"],
                   "error": r["error"], "result": cut(&match &r["result"] { Value::Null => String::new(), Value::String(s) => s.clone(), v => v.to_string() }, MAX_TEXT)})
        })
        .collect();
    let features: Vec<Value> = DocRepo::new("features")?.list()?.iter().map(|f| json!({"id": f["id"], "title": f["title"], "status": f["status"]})).collect();
    let mail = DocRepo::new("mailbox")?.list()?;
    let mailbox: Vec<Value> = mail[mail.len().saturating_sub(MAX_MAILBOX)..].to_vec();
    let open_inbox = crate::inbox::Inbox::new(false).list(Some("open")).map(|v| v.len()).unwrap_or(0);
    let mut slot = cell().write().unwrap_or_else(|e| e.into_inner());
    let snap = Arc::new(Snapshot { version: slot.version + 1, at: now(), team, runs, features, mailbox, open_inbox });
    *slot = snap.clone();
    Ok(snap)
}

/// An agent leaves a message for another agent. Written to the database now, visible to readers in the next snapshot.
pub fn post(from: &str, to: &str, body: &str) -> Result<Value, String> {
    let (from, to, body) = (from.trim(), to.trim(), body.trim());
    if to.is_empty() {
        return Err("a message needs a recipient".into());
    }
    if body.is_empty() {
        return Err("a message needs text".into());
    }
    let doc = json!({"from": if from.is_empty() { "unknown" } else { from }, "to": to, "text": cut(body, MAX_TEXT), "at": now()});
    DocRepo::new("mailbox").and_then(|r| r.create(doc, None)).map_err(|e| e.to_string())
}

/// The messages addressed to one role in a snapshot, oldest first.
pub fn inbox_for(s: &Snapshot, role: &str) -> Vec<Value> {
    s.mailbox.iter().filter(|m| m["to"] == role).cloned().collect()
}

/// Keeps publishing until dropped.
pub struct Publisher {
    stop: Arc<AtomicBool>,
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Rebuilds the snapshot every `every` on its own thread. `roles` says which roles to list (called on every pass).
pub fn start_publisher(every: Duration, roles: impl Fn() -> Vec<String> + Send + 'static) -> Publisher {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            let _ = rebuild(&roles()); // a failed pass keeps the previous snapshot
            std::thread::sleep(every);
        }
    });
    Publisher { stop }
}
