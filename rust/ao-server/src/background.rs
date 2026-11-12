//! The local background loop: schedules run only while the app is running (`scheduler_loop` in the Python app).
use crate::routes::settings;
use crate::runs::{restart_daily, restart_held};
use crate::AppState;
use ao_core::budget::Budget;
use ao_core::db::now;
use ao_core::inbox::Inbox;
use ao_core::notify::Notifier;
use ao_core::scheduler::{budget_percent_used, Scheduler};
use serde_json::{json, Value};
use std::time::Duration;

fn run_job(job: &Value) -> Result<(), String> {
    if job["payload"]["kind"] == "reminder" {
        let text = job["payload"]["text"].as_str().unwrap_or("");
        let name = job["name"].as_str().unwrap_or("");
        Inbox::new(false).post("status", name, text, json!({})).map_err(|e| e.to_string())?;
        Notifier::new().send("reminder", name, text, Some(&format!("job:{}", job["id"].as_str().unwrap_or("")))).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// One pass: due schedules run, features held until a set time continue, runs held by a guard that has room again continue.
pub fn tick_once(st: &AppState) {
    // never let a failure end the loop; single job failures are recorded by the scheduler itself
    if let Ok(s) = settings() {
        let pause = s["pause_at_percent"].as_f64().unwrap_or(98.0);
        if let Ok(pct) = budget_percent_used(s["daily_budget_usd"].as_f64(), None) {
            let _ = Scheduler::new(pause).tick(now(), pct, &mut run_job);
        }
    }
    if let Ok(fids) = Budget.tick(now()) {
        for fid in fids {
            let _ = restart_held(st, &fid);
        }
        let _ = restart_daily(st);
    }
}

/// Starts the loop on its own thread. `AO_NO_SCHEDULER=1` switches it off (tests).
pub fn start(st: AppState, every: Duration) {
    if std::env::var("AO_NO_SCHEDULER").is_ok_and(|v| !v.is_empty()) {
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(every);
        tick_once(&st);
    });
}
