//! Release stages after peer review (docs/design/team-workflow.md).
//!
//! peer approved -> architect review -> deploy to QC -> QC result -> request production
//! -> human AND architect approve -> deployed to production.
//! Each step is a real signal from the role that owns it, is refused out of order, and records a hand-off.
//! A failure sends the task back to its owning developer. Nothing reaches production without the human's yes.
use super::docs_gate::DocsGate;
use super::flow::{bad, tasks, FlowError, Result};
use crate::db::{kv_get, kv_set, now};
use crate::handoffs;
use crate::inbox::Inbox;
use crate::quota::{QuotaError, QuotaGuard};
use serde_json::{json, Value};

pub const APPROVERS: [&str; 2] = ["human", "architect"];

fn s(t: &Value, k: &str) -> String {
    t[k].as_str().unwrap_or("").to_string()
}

fn updated(id: &str, patch: Value) -> Result<Value> {
    tasks()?.update(id, patch)?.ok_or_else(|| FlowError::Msg(format!("unknown task {id}")))
}

pub struct Release;

impl Release {
    /// The task, required to be at one of the release `stages` (None stands for "not in a release yet").
    fn task(&self, tid: &str, stages: &[Option<&str>]) -> Result<Value> {
        let Some(t) = tasks()?.get(tid)? else { return bad(format!("unknown task {tid}")) };
        let cur = t["release"].as_str();
        if !stages.is_empty() && !stages.contains(&cur) {
            let expected: Vec<&str> = stages.iter().map(|x| x.unwrap_or("None")).collect();
            return bad(format!("release stage is {}, expected {}", cur.unwrap_or("None"), expected.join(" or ")));
        }
        if t["status"] != "approved" && cur.is_none() {
            return bad(format!("task is in state {}, it must be peer approved first", s(&t, "status")));
        }
        Ok(t)
    }

    pub fn environments(&self) -> Result<Value> {
        let env = kv_get("environments", json!({}))?;
        Ok(json!({"qc": env["qc"], "prod": env["prod"]}))
    }

    /// Stops at 70% of the daily deployment limit and tells the human once.
    fn vercel(&self) -> Result<QuotaGuard> {
        let q = |e: QuotaError| FlowError::Msg(format!("Vercel limit guard: {e}"));
        let g = QuotaGuard::new("vercel_hobby", 70.0, None).map_err(q)?;
        g.check("deployments_per_day", None, 0).map_err(q)?;
        g.check("deployments_per_hour", None, 0).map_err(q)?;
        Ok(g)
    }

    fn deploy(&self, name: &str, t: &Value) -> Result<()> {
        let g = self.vercel()?; // a refused deploy changes nothing and is not counted
        let q = |e: QuotaError| FlowError::Msg(e.to_string());
        g.record("deployments_per_day", None).map_err(q)?;
        g.record("deployments_per_hour", None).map_err(q)?;
        let mut env = kv_get("environments", json!({}))?;
        let version: String = s(t, "head_sha").chars().take(7).collect();
        env[name] = json!({"task_id": t["id"], "name": t["name"], "version": version, "at": now()});
        kv_set("environments", &env)?;
        Ok(())
    }

    fn back_to_owner(&self, t: &Value, sender: &str, label: &str) -> Result<Value> {
        handoffs::record(sender, &s(t, "owner"), label, None)?;
        updated(&s(t, "id"), json!({"status": "developing", "release": null, "approvals": [], "tests": null}))
    }

    pub fn architect_review(&self, tid: &str, approve: bool) -> Result<Value> {
        let t = self.task(tid, &[None])?;
        if !approve {
            return self.back_to_owner(&t, "architect", "changes requested");
        }
        handoffs::record("architect", "cicd", "deploy to QC", None)?;
        updated(tid, json!({"release": "architect_approved"}))
    }

    pub fn deploy_qc(&self, tid: &str) -> Result<Value> {
        let t = self.task(tid, &[Some("architect_approved")])?;
        self.deploy("qc", &t)?;
        handoffs::record("cicd", "tester", "ready for QC", None)?;
        updated(tid, json!({"release": "in_qc"}))
    }

    pub fn qc_result(&self, tid: &str, passed: i64, failed: i64) -> Result<Value> {
        let t = self.task(tid, &[Some("in_qc")])?;
        if failed != 0 || passed <= 0 {
            return self.back_to_owner(&t, "tester", &format!("QC failed: {failed} failing"));
        }
        DocsGate.require(&t)?; // no sign-off before the docs gate is PASS (or the human's exception)
        handoffs::record("tester", "orchestrator", "QC signed off", None)?;
        updated(tid, json!({"release": "qc_passed", "qc": {"passed": passed, "failed": failed}}))
    }

    pub fn request_prod(&self, tid: &str) -> Result<Value> {
        let t = self.task(tid, &[Some("qc_passed")])?;
        handoffs::record("orchestrator", "architect", "production sign-off", None)?;
        Inbox::new(false).post(
            "approval",
            &format!("Deploy {} to production?", s(&t, "name")),
            "QC has signed off. Production needs your yes and the architect's sign-off.",
            json!({"task_id": tid}),
        )?;
        updated(tid, json!({"release": "awaiting_prod_approval", "approvals": []}))
    }

    pub fn approve_prod(&self, tid: &str, by: &str) -> Result<Value> {
        let t = self.task(tid, &[Some("awaiting_prod_approval")])?;
        if !APPROVERS.contains(&by) {
            return bad("only the human or the architect can approve production");
        }
        let mut got: Vec<String> = t["approvals"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect();
        got.push(by.to_string());
        got.sort();
        got.dedup();
        let mut want: Vec<String> = APPROVERS.iter().map(|a| a.to_string()).collect();
        want.sort();
        if got != want {
            return updated(tid, json!({"approvals": got}));
        }
        handoffs::record("architect", "cicd", "deploy to production", None)?;
        updated(tid, json!({"approvals": got, "release": "approved_for_prod"}))
    }

    pub fn deployed_prod(&self, tid: &str) -> Result<Value> {
        let t = self.task(tid, &[Some("approved_for_prod")])?;
        self.deploy("prod", &t)?;
        updated(tid, json!({"release": "in_prod"}))
    }
}
