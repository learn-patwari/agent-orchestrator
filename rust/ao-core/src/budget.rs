//! Token limit per feature.
//!
//! A feature may carry a token limit. Its runs are counted against it; when the limit is reached the feature is held:
//! running work stops at its next step and new runs for it are refused. The app cannot read a provider's quota window
//! (for example a 5-hour Claude window), so the human says when it clears: set `resume_at`, or press Resume. Resuming
//! starts a fresh allowance (the same limit again, counted from now) and the held runs continue where they stopped.
use crate::db::{kv_get, kv_set, now, DbError, DocRepo};
use crate::notify::Notifier;
use crate::scheduler::Scheduler;
use serde_json::{json, Value};

/// Percent of the 5-hour window at which new work stops.
pub const WINDOW_STOP: f64 = 98.0;

#[derive(Debug, thiserror::Error)]
pub enum BudgetError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

pub type Result<T> = std::result::Result<T, BudgetError>;

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn truthy(v: &Value) -> bool {
    !(v.is_null() || *v == json!(false) || num(v) == 0.0 && v.is_number())
}

/// Python's `%g` for the figures shown to the human.
fn g(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

pub struct Budget;

impl Budget {
    fn feature(&self, fid: &str) -> Result<Value> {
        DocRepo::new("features")?.get(fid)?.ok_or_else(|| BudgetError::Msg(format!("unknown feature {fid}")))
    }

    fn total(&self, fid: &str) -> Result<i64> {
        Ok(DocRepo::new("runs")?.list()?.iter().filter(|r| r["feature_id"] == fid).map(|r| num(&r["tokens"]) as i64).sum())
    }

    fn used(&self, f: &Value) -> Result<i64> {
        Ok((self.total(f["id"].as_str().unwrap_or(""))? - num(&f["token_baseline"]) as i64).max(0))
    }

    pub fn set(&self, fid: &str, limit: Option<i64>, resume_at: Option<f64>) -> Result<Value> {
        let f = self.feature(fid)?;
        if limit.is_some_and(|l| l <= 0) {
            return Err(BudgetError::Msg("the token limit must be above zero".into()));
        }
        let mut patch = json!({"token_limit": limit, "resume_at": resume_at});
        if limit.is_none_or(|l| self.used(&f).map_or(false, |u| u < l)) {
            patch["held"] = json!(false); // a raised or removed limit lifts the hold
        }
        Ok(DocRepo::new("features")?.update(fid, patch)?.unwrap_or(Value::Null))
    }

    pub fn status(&self, fid: &str) -> Result<Value> {
        let f = self.feature(fid)?;
        let used = self.used(&f)?;
        let limit = f["token_limit"].as_i64();
        Ok(json!({"limit": limit, "used": used, "remaining": limit.map(|l| (l - used).max(0)),
                  "held": truthy(&f["held"]), "resume_at": f["resume_at"]}))
    }

    /// None when work may go on; otherwise the reason it is held (and the feature is marked held).
    pub fn gate(&self, fid: Option<&str>) -> Result<Option<String>> {
        let features = DocRepo::new("features")?;
        let f = match fid {
            Some(id) if !id.is_empty() => features.get(id)?,
            _ => None,
        };
        let Some(f) = f else { return Ok(None) };
        let Some(limit) = f["token_limit"].as_i64().filter(|l| *l != 0) else { return Ok(None) };
        let used = self.used(&f)?;
        let held = truthy(&f["held"]);
        if !held && used < limit {
            return Ok(None);
        }
        if !held {
            let id = f["id"].as_str().unwrap_or("");
            features.update(id, json!({"held": true, "held_at": now()}))?;
            let title = f["title"].as_str().unwrap_or(id);
            let tail = if truthy(&f["resume_at"]) { " or at the time you set." } else { "." };
            Notifier::new().send(
                "budget_hold",
                &format!("Feature held: {title}"),
                &format!("Token limit reached ({used} of {limit}). Work resumes when you press Resume{tail}"),
                Some(&format!("hold:{id}")),
            )?;
        }
        Ok(Some(format!("held: token limit reached ({used} of {limit} tokens)")))
    }

    pub fn resume(&self, fid: &str) -> Result<Value> {
        self.feature(fid)?;
        DocRepo::new("features")?.update(fid, json!({"held": false, "resume_at": null, "token_baseline": self.total(fid)?}))?;
        let n = Notifier::new();
        for u in n.unread()? {
            if u["key"] == format!("hold:{fid}") {
                n.mark_read(u["id"].as_str().unwrap_or(""))?;
            }
        }
        self.status(fid)
    }

    /// Tokens used in the last 24 hours across everything (a rolling window), against an optional limit.
    pub fn daily(&self, at: f64, limit: Option<i64>) -> Result<Value> {
        let used: i64 = DocRepo::new("llm_calls")?
            .list()?
            .iter()
            .filter(|c| at - num(&c["at"]) < 86_400.0)
            .map(|c| (num(&c["input_tokens"]) + num(&c["output_tokens"])) as i64)
            .sum();
        let percent = limit.filter(|l| *l != 0).map(|l| (used as f64 / l as f64 * 100.0 * 100.0).round() / 100.0);
        Ok(json!({"limit": limit, "used": used, "percent": percent}))
    }

    pub fn daily_gate(&self, at: f64, limit: Option<i64>) -> Result<Option<String>> {
        let d = self.daily(at, limit)?;
        let used = d["used"].as_i64().unwrap_or(0);
        Ok(limit.filter(|l| *l != 0 && used >= *l).map(|l| format!("daily token limit reached ({used} of {l} tokens in the last 24 hours)")))
    }

    pub fn overview(&self, at: f64, daily_limit: Option<i64>) -> Result<Value> {
        let mut feats = vec![];
        for f in DocRepo::new("features")?.list()? {
            let id = f["id"].as_str().unwrap_or("");
            let mut row = json!({"id": id, "title": f["title"].as_str().map_or(json!(id), |t| json!(t))});
            if let (Some(r), Value::Object(s)) = (row.as_object_mut(), self.status(id)?) {
                r.extend(s);
            }
            feats.push(row);
        }
        Ok(json!({"daily": self.daily(at, daily_limit)?, "features": feats}))
    }

    /// The person enters how full the provider's window is (the app cannot read it) and when it resets.
    pub fn set_window(&self, percent: f64, resets_at: Option<f64>) -> Result<Value> {
        let mut s = kv_get("usage_window", json!({}))?;
        s["percent"] = json!(percent);
        s["resets_at"] = json!(resets_at);
        if let Some(reset) = resets_at.filter(|r| *r != 0.0) {
            if percent >= WINDOW_STOP && s["scheduled_for"].as_f64() != Some(reset) {
                Scheduler::new(80.0).add("Usage window reset: work resumes", json!({"kind": "reminder", "text": "The usage window has reset; held work continues."}), reset, None)?;
                s["scheduled_for"] = json!(reset); // one resume per window, however often the figure is updated
            }
        }
        kv_set("usage_window", &s)?;
        self.window(now())
    }

    pub fn window(&self, at: f64) -> Result<Value> {
        let s = kv_get("usage_window", json!({}))?;
        Ok(json!({"percent": s["percent"], "resets_at": s["resets_at"], "blocked": self.window_gate(at)?.is_some()}))
    }

    pub fn window_gate(&self, at: f64) -> Result<Option<String>> {
        let s = kv_get("usage_window", json!({}))?;
        let (Some(pct), reset) = (s["percent"].as_f64(), s["resets_at"].as_f64()) else { return Ok(None) };
        if pct < WINDOW_STOP || reset.is_some_and(|r| at >= r) {
            return Ok(None);
        }
        let when = match reset.filter(|r| *r != 0.0) {
            Some(r) => chrono::DateTime::from_timestamp(r as i64, 0).map_or("the reset".into(), |d| d.with_timezone(&chrono::Local).format("%H:%M").to_string()),
            None => "the reset".into(),
        };
        Ok(Some(format!("usage window at {}% (stop at {}%); work resumes after the reset at {when}", g(pct), g(WINDOW_STOP))))
    }

    /// Resume the held features whose resume time has come. Returns their ids.
    pub fn tick(&self, at: f64) -> Result<Vec<String>> {
        let due: Vec<String> = DocRepo::new("features")?
            .list()?
            .iter()
            .filter(|f| truthy(&f["held"]) && f["resume_at"].as_f64().is_some_and(|r| r != 0.0 && r <= at))
            .filter_map(|f| f["id"].as_str().map(String::from))
            .collect();
        for id in &due {
            self.resume(id)?;
        }
        Ok(due)
    }
}
