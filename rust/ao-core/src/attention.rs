//! Everything that waits for the human, in one list. Each item says what it is and where to go.
use crate::db::{kv_get, kv_set, now, DbError, DocRepo};
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, DbError>;

/// Finished or failed runs are only worth a notice for a day.
const RUN_NOTICE_SECONDS: f64 = 86_400.0;

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn seen() -> Result<Vec<String>> {
    Ok(kv_get("attention_seen", json!([]))?.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect())
}

/// Newest first: open approvals and questions, and runs that finished or failed since you last looked.
pub fn items() -> Result<Vec<Value>> {
    let mut out: Vec<Value> = vec![];
    for i in DocRepo::new("inbox")?.list()? {
        if i["status"] != "open" || !(i["kind"] == "approval" || i["kind"] == "question") {
            continue;
        }
        let link = match i["feature_id"].as_str().filter(|f| !f.is_empty()) {
            Some(f) => format!("/features/?id={f}"),
            None => "/inbox/".to_string(),
        };
        out.push(json!({"id": format!("inbox:{}", text(&i["id"])), "kind": i["kind"], "title": i["title"], "detail": short(&text(&i["body"]), 160), "link": link, "at": i["created_at"]}));
    }
    let gone = seen()?;
    let cutoff = now() - RUN_NOTICE_SECONDS;
    for r in DocRepo::new("runs")?.list()? {
        let kind = match r["status"].as_str() {
            Some("done") => "task_done",
            Some("failed") => "task_failed",
            _ => continue,
        };
        let at = r["finished_at"].as_f64().unwrap_or(0.0);
        let id = format!("run:{}", text(&r["id"]));
        if at < cutoff || gone.contains(&id) {
            continue;
        }
        let verb = if kind == "task_done" { "finished" } else { "failed" };
        let detail = if kind == "task_failed" { short(&text(&r["error"]), 160) } else { short(&text(&r["task"]), 160) };
        out.push(json!({"id": id, "kind": kind, "title": format!("{} {verb}: {}", text(&r["role"]), short(&text(&r["task"]), 60)), "detail": detail, "link": "/status/", "at": at}));
    }
    for (pid, name, expected, actual) in crate::windows::mismatches()? {
        let fmt = |t: Option<f64>| t.and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0)).map_or("?".into(), |d| d.with_timezone(&chrono::Local).format("%H:%M").to_string());
        out.push(json!({"id": format!("window:{pid}"), "kind": "warning", "title": format!("{name}: usage window differs from the schedule"),
            "detail": format!("expected reset {}, provider says {}", fmt(expected), fmt(actual)), "link": "/providers/", "at": now()}));
    }
    out.sort_by(|a, b| b["at"].as_f64().unwrap_or(0.0).total_cmp(&a["at"].as_f64().unwrap_or(0.0)));
    Ok(out)
}

pub fn count() -> Result<usize> {
    Ok(items()?.len())
}

/// Dismisses run notices (approvals and questions leave the feed by themselves once answered).
pub fn mark_seen(ids: &[String]) -> Result<()> {
    let mut all = seen()?;
    for id in ids {
        if id.starts_with("run:") && !all.contains(id) {
            all.push(id.clone());
        }
    }
    kv_set("attention_seen", &json!(all))
}
