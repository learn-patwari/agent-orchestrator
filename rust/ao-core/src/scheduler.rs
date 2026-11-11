//! Local scheduler: one-shot and recurring jobs, paused while the LLM budget is at or above a threshold.
//! Schedules are local to this app: they run only while the app is running (`tick` is driven by its background loop).
use crate::db::{now, DbError, DocRepo};
use crate::notify::Notifier;
use serde_json::{json, Value};

/// Share of today's (rolling 24 h) cost cap already spent, from recorded LLM calls. None = no cap.
pub fn budget_percent_used(daily_cap_usd: Option<f64>, at: Option<f64>) -> Result<Option<f64>, DbError> {
    let Some(cap) = daily_cap_usd.filter(|c| *c != 0.0) else { return Ok(None) };
    let since = at.unwrap_or_else(now) - 86400.0;
    let spent: f64 = DocRepo::new("llm_calls")?
        .list()?
        .iter()
        .filter(|c| c["at"].as_f64().unwrap_or(0.0) >= since)
        .map(|c| c["cost_usd"].as_f64().unwrap_or(0.0))
        .sum();
    Ok(Some((spent / cap * 100.0 * 1e4).round() / 1e4))
}

pub struct TickOut {
    pub paused: bool,
    pub ran: Vec<Value>,
}

pub struct Scheduler {
    repo: DocRepo,
    pause_at: f64,
}

impl Scheduler {
    pub fn new(pause_at_percent: f64) -> Self {
        Scheduler { repo: DocRepo::new("schedules").expect("schedules table"), pause_at: pause_at_percent }
    }

    pub fn add(&self, name: &str, payload: Value, run_at: f64, every_s: Option<f64>) -> Result<Value, DbError> {
        self.repo.create(
            json!({"name": name, "payload": payload, "run_at": run_at, "every_s": every_s, "status": "scheduled", "error": null}),
            None,
        )
    }

    pub fn list(&self) -> Result<Vec<Value>, DbError> {
        self.repo.list()
    }

    pub fn tick(&self, at: f64, budget_percent: Option<f64>, run: &mut dyn FnMut(&Value) -> Result<(), String>) -> Result<TickOut, DbError> {
        let notifier = Notifier::new();
        if let Some(b) = budget_percent.filter(|b| *b >= self.pause_at) {
            notifier.send(
                "budget_pause",
                "LLM budget reached the pause threshold",
                &format!("Used {b:.0}% (pause at {:.0}%). Scheduled work waits and resumes automatically when usage drops.", self.pause_at),
                Some("budget_pause"),
            )?;
            return Ok(TickOut { paused: true, ran: vec![] });
        }
        for n in notifier.unread()? {
            // the pause is over: clear the stale notice
            if n["key"] == "budget_pause" {
                notifier.mark_read(n["id"].as_str().unwrap_or(""))?;
            }
        }
        let mut ran = vec![];
        for job in self.repo.list()? {
            let id = job["id"].as_str().unwrap_or("").to_string();
            if job["status"] != "scheduled" || job["run_at"].as_f64().unwrap_or(0.0) > at {
                continue;
            }
            if let Err(e) = run(&job) {
                // one bad job must not stop the others
                self.repo.update(&id, json!({"status": "failed", "error": e}))?;
                notifier.send("job_failed", &format!("Scheduled job {} failed", job["name"].as_str().unwrap_or("")), &e, Some(&format!("job:{id}")))?;
                continue;
            }
            match job["every_s"].as_f64().filter(|e| *e != 0.0) {
                Some(every) => {
                    // next slot after now: a long gap runs once, not once per missed slot
                    let mut next = job["run_at"].as_f64().unwrap_or(0.0) + every;
                    while next <= at {
                        next += every;
                    }
                    self.repo.update(&id, json!({"run_at": next}))?;
                }
                None => {
                    self.repo.update(&id, json!({"status": "done"}))?;
                }
            }
            ran.push(job);
        }
        Ok(TickOut { paused: false, ran })
    }
}
