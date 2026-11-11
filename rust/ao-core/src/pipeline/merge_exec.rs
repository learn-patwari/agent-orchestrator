//! Batch merge executor (FR-37): evaluate each PR with the gate, merge base-first, retarget stacked PRs, re-check the
//! head right before merging, pin it, and audit every outcome. Merging into main or stage is refused.
use super::github::GhClient;
use super::merge_gate::{evaluate, merge_command, plan_batch, GateConfig};
use crate::db::{now, DbError, DocRepo};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const HUMAN_ONLY: [&str; 3] = ["main", "master", "stage"];

#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("{0}")]
    Refused(String),
    #[error("github: {0}")]
    Gh(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// What the executor needs from GitHub; `GhClient` is the real one.
pub trait Gh {
    fn facts(&self, n: i64) -> Result<Value, String>;
    fn retarget(&self, n: i64, base: &str) -> Result<(), String>;
    fn merge(&self, cmd: &[String]) -> Result<(), String>;
}

impl Gh for GhClient {
    fn facts(&self, n: i64) -> Result<Value, String> {
        GhClient::facts(self, n)
    }
    fn retarget(&self, n: i64, base: &str) -> Result<(), String> {
        GhClient::retarget(self, n, base)
    }
    fn merge(&self, cmd: &[String]) -> Result<(), String> {
        GhClient::merge(self, cmd)
    }
}

fn audit(event: &str, fields: Value) -> Result<(), DbError> {
    let mut m = Map::new();
    m.insert("event".into(), json!(event));
    m.insert("at".into(), json!(now()));
    if let Value::Object(f) = fields {
        m.extend(f);
    }
    DocRepo::new("audit")?.create(Value::Object(m), None)?;
    Ok(())
}

pub fn merge_batch(gh: &dyn Gh, numbers: &[i64], cfg: &GateConfig, style: &str, reviewer_can_merge: bool) -> Result<Value, MergeError> {
    if !reviewer_can_merge {
        return Err(MergeError::Refused("reviewer merge authority is switched off; the human merges".into()));
    }
    if HUMAN_ONLY.contains(&cfg.integration_branch.as_str()) {
        return Err(MergeError::Refused(format!("{} is merged by the human only", cfg.integration_branch)));
    }
    let mut facts: Vec<(i64, Value)> = vec![];
    for n in numbers {
        let f = gh.facts(*n).map_err(MergeError::Gh)?;
        match facts.iter_mut().find(|(k, _)| k == n) {
            Some(slot) => slot.1 = f,
            None => facts.push((*n, f)),
        }
    }
    let all: Vec<Value> = facts.iter().map(|(_, f)| f.clone()).collect();
    let plan = plan_batch(&all, &cfg.integration_branch);
    let (mut merged, mut refused): (Vec<Value>, Vec<Value>) = (vec![], plan["skipped"].as_array().unwrap().iter().map(|s| json!({"number": s["number"], "reasons": [s["reason"]]})).collect());
    let (mut done, mut failed): (HashSet<String>, HashSet<String>) = (HashSet::new(), HashSet::new());
    for p in plan["order"].as_array().unwrap() {
        let mut p = p.clone();
        let n = p["number"].as_i64().unwrap_or(0);
        let branch = p["branch"].as_str().unwrap_or("").to_string();
        let base = p["base"].as_str().unwrap_or("").to_string();
        if base != cfg.integration_branch {
            if done.contains(&base) {
                gh.retarget(n, &cfg.integration_branch).map_err(MergeError::Gh)?;
                p["base"] = json!(cfg.integration_branch);
            } else {
                refused.push(json!({"number": n, "reasons": [format!("depends on PR for {base} which was not merged")]}));
                failed.insert(branch);
                audit("merge_refused", json!({"number": n, "reasons": ["base PR not merged"]}))?;
                continue;
            }
        }
        let verdict = evaluate(&p, cfg);
        if verdict["ok"] != true {
            refused.push(json!({"number": n, "reasons": verdict["reasons"]}));
            failed.insert(branch);
            audit("merge_refused", json!({"number": n, "reasons": verdict["reasons"]}))?;
            continue;
        }
        let fresh = gh.facts(n).map_err(MergeError::Gh)?;
        if fresh["head_sha"] != p["head_sha"] {
            let why = json!(["head commit changed after the gate was evaluated"]);
            refused.push(json!({"number": n, "reasons": why}));
            failed.insert(branch);
            audit("merge_refused", json!({"number": n, "reasons": why}))?;
            continue;
        }
        gh.merge(&merge_command(&p, style).map_err(MergeError::Refused)?).map_err(MergeError::Gh)?;
        merged.push(json!({"number": n, "head_sha": p["head_sha"]}));
        done.insert(branch);
        audit("merge_done", json!({"number": n, "head_sha": p["head_sha"], "style": style}))?;
    }
    refused.sort_by_key(|r| r["number"].as_i64().unwrap_or(0));
    Ok(json!({"merged": merged, "refused": refused}))
}
