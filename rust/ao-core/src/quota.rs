//! Generic platform quota guard. Counts usage in rolling windows against a per-provider limits table and stops at a
//! threshold (default 70%), telling the human once. A limit error from the platform stops everything until the window
//! can have reset; nothing here retries. Vercel Hobby is seeded; other platforms bring their own table.
use crate::db::{kv_get, kv_set, now, DbError};
use crate::notify::Notifier;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const DAY: f64 = 86400.0;
pub const HOUR: f64 = 3600.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metric {
    pub limit: u64,
    /// 0 means a concurrency limit rather than a rolling window
    pub window_s: f64,
}

pub type Limits = HashMap<String, HashMap<String, Metric>>;

pub fn limits_for(provider: &str) -> Option<HashMap<String, Metric>> {
    match provider {
        "vercel_hobby" => Some(HashMap::from([
            ("deployments_per_day".to_string(), Metric { limit: 100, window_s: DAY }),
            ("deployments_per_hour".to_string(), Metric { limit: 100, window_s: HOUR }),
            ("concurrent_builds".to_string(), Metric { limit: 1, window_s: 0.0 }),
        ])),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QuotaError {
    #[error("{0}")]
    Blocked(String),
    #[error("unknown platform {0}")]
    UnknownPlatform(String),
    #[error("unknown metric {0}")]
    UnknownMetric(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

#[derive(Debug, PartialEq)]
pub struct Usage {
    pub used: u64,
    pub limit: u64,
    pub percent: f64,
}

pub struct QuotaGuard {
    provider: String,
    limits: HashMap<String, Metric>,
    stop_at: f64,
    key: String,
}

impl QuotaGuard {
    pub fn new(provider: &str, stop_at_percent: f64, limits: Option<Limits>) -> Result<Self, QuotaError> {
        let found = match limits {
            Some(mut t) => t.remove(provider),
            None => limits_for(provider),
        };
        let limits = found.ok_or_else(|| QuotaError::UnknownPlatform(provider.to_string()))?;
        Ok(QuotaGuard { provider: provider.to_string(), limits, stop_at: stop_at_percent, key: format!("quota:{provider}") })
    }

    fn state(&self) -> Result<Value, DbError> {
        kv_get(&self.key, json!({"events": {}, "stopped_until": 0}))
    }

    fn metric(&self, metric: &str) -> Result<Metric, QuotaError> {
        self.limits.get(metric).copied().ok_or_else(|| QuotaError::UnknownMetric(format!("{metric} for {}", self.provider)))
    }

    pub fn record(&self, metric: &str, at: Option<f64>) -> Result<(), QuotaError> {
        self.metric(metric)?;
        let mut s = self.state()?;
        let events = s["events"].as_object_mut().ok_or(DbError::NotObject)?;
        let list = events.entry(metric.to_string()).or_insert_with(|| json!([]));
        list.as_array_mut().ok_or(DbError::NotObject)?.push(json!(at.unwrap_or_else(now)));
        kv_set(&self.key, &s)?;
        Ok(())
    }

    fn used(&self, s: &Value, metric: &str, at: f64) -> u64 {
        let w = self.limits[metric].window_s;
        s["events"][metric].as_array().map_or(0, |a| a.iter().filter(|t| at - t.as_f64().unwrap_or(0.0) < w).count() as u64)
    }

    pub fn usage(&self, at: Option<f64>) -> Result<HashMap<String, Usage>, QuotaError> {
        let at = at.unwrap_or_else(now);
        let s = self.state()?;
        let mut out = HashMap::new();
        for (m, spec) in &self.limits {
            if spec.window_s > 0.0 {
                let used = self.used(&s, m, at);
                out.insert(m.clone(), Usage { used, limit: spec.limit, percent: (used as f64 / spec.limit as f64 * 100.0 * 100.0).round() / 100.0 });
            }
        }
        Ok(out)
    }

    fn block(&self, metric: &str, msg: String) -> Result<(), QuotaError> {
        Notifier::new().send(
            "quota_block",
            &format!("{} quota guard stopped work", self.provider),
            &format!("{metric}: {msg}"),
            Some(&format!("quota:{}:{metric}", self.provider)),
        )?;
        Err(QuotaError::Blocked(msg))
    }

    pub fn check(&self, metric: &str, at: Option<f64>, in_flight: u64) -> Result<(), QuotaError> {
        let spec = self.metric(metric)?;
        let at = at.unwrap_or_else(now);
        let s = self.state()?;
        if at < s["stopped_until"].as_f64().unwrap_or(0.0) {
            return self.block(metric, "the platform returned a limit error; stopped until the window can have reset (no retries)".into());
        }
        if spec.window_s == 0.0 {
            // concurrency limit
            if in_flight >= spec.limit {
                return self.block(metric, format!("{in_flight} in flight, limit {}", spec.limit));
            }
            return Ok(());
        }
        let used = self.used(&s, metric, at);
        let pct = used as f64 / spec.limit as f64 * 100.0;
        if pct >= self.stop_at {
            return self.block(metric, format!("{used} of {} used ({pct:.0}%), stop threshold is {:.0}%", spec.limit, self.stop_at));
        }
        Ok(())
    }

    /// The platform said no (for example api-deployments-free-per-day): stop for a full day and do not retry.
    pub fn limit_hit(&self, error_code: &str, at: Option<f64>) -> Result<(), QuotaError> {
        let at = at.unwrap_or_else(now);
        let mut s = self.state()?;
        s["stopped_until"] = json!(at + DAY);
        kv_set(&self.key, &s)?;
        Notifier::new().send(
            "quota_block",
            &format!("{} limit error: {error_code}", self.provider),
            "Work that needs the platform is stopped and will not retry.",
            Some(&format!("quota:{}:limit", self.provider)),
        )?;
        Ok(())
    }
}
