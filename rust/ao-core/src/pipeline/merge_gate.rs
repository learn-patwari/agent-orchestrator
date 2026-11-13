//! Reviewer merge gate (FR-37): the conditions under which a reviewer may merge into the integration branch.
//! Pure functions over PR facts (JSON, same shape as the Python dicts) so the gate is easy to test and cannot be
//! talked around by prompt text. The verdict is read only from the FIRST line of a review (shared GitHub account:
//! there are no real approvals).
use crate::runtime::tools::fnmatch;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::LazyLock;

const BLOCKING: [&str; 3] = ["medium", "high", "critical"];

static VERDICT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^verdict:\s*(approve|request_changes|hold)\s*$").expect("valid pattern"));

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub integration_branch: String,
    pub review_round_limit: i64,
    pub secret_patterns: Vec<String>,
    pub production_patterns: Vec<String>,
    pub permission_patterns: Vec<String>,
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

impl Default for GateConfig {
    fn default() -> Self {
        GateConfig {
            integration_branch: "internal".into(),
            review_round_limit: 3,
            secret_patterns: strings(&[".env", ".env.*", "*.pem", "*.key", "seed_super_admin*", "secret*", "*credentials*"]),
            production_patterns: strings(&["vercel.json", ".github/workflows/*", "Jenkinsfile", "*.tf", "Dockerfile", "docker-compose*.yml"]),
            permission_patterns: strings(&["*/reviewer.md", "*/policies.md", "*/mcp.json", "*/plugin.json"]),
        }
    }
}

pub fn parse_verdict(body: &str) -> Option<String> {
    let first = body.trim().lines().next().unwrap_or("");
    VERDICT.captures(first.trim()).map(|m| m[1].to_uppercase())
}

fn matches(path: &str, patterns: &[String]) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    patterns.iter().any(|p| fnmatch(path, p) || fnmatch(name, p))
}

pub fn evaluate(pr: &Value, cfg: &GateConfig) -> Value {
    let mut why: Vec<String> = vec![];
    let s = |k: &str| pr[k].as_str().unwrap_or("").to_string();
    if s("base") != cfg.integration_branch {
        why.push(format!("base is {}, not the integration branch {}", s("base"), cfg.integration_branch));
    }
    let mut reviews: Vec<&Value> = pr["reviews"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    reviews.sort_by(|a, b| a["at"].as_f64().partial_cmp(&b["at"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    match reviews.last() {
        None => why.push("no review yet".into()),
        Some(latest) => {
            let verdict = latest["verdict"].as_str().unwrap_or("");
            if verdict != "APPROVE" {
                why.push(format!("latest review is {verdict}, not APPROVE"));
            } else if latest["head_sha"] != pr["head_sha"] {
                why.push("latest APPROVE is not on the current head commit (new commits since review)".into());
            }
            let open = latest["findings"].as_array().map(|f| f.iter().filter(|f| BLOCKING.contains(&f["severity"].as_str().unwrap_or("")) && f["resolved"] != true).count()).unwrap_or(0);
            if open > 0 {
                why.push(format!("{open} unresolved medium+ finding(s)"));
            }
        }
    }
    let rounds = reviews.iter().filter(|r| r["verdict"] == "REQUEST_CHANGES").count() as i64;
    if rounds >= cfg.review_round_limit {
        why.push(format!("{rounds} review rounds reached the limit of {}: escalate to the human", cfg.review_round_limit));
    }
    if pr["ci"] != "success" {
        let ci = match &pr["ci"] {
            Value::String(c) => c.clone(),
            _ => "None".into(),
        };
        why.push(format!("CI is {ci}, not success"));
    }
    if pr["mergeable"] != true {
        why.push("PR is not mergeable".into());
    }
    let t = &pr["tests"];
    if t.is_null() || t["passed"].as_i64().unwrap_or(0) <= 0 {
        why.push("no passing tests recorded for the area (tests must run with counts)".into());
    } else if t["failed"].as_i64().unwrap_or(0) > 0 {
        why.push(format!("tests failing: {}", t["failed"].as_i64().unwrap_or(0)));
    }
    let files: Vec<&str> = pr["changed_files"].as_array().map(|a| a.iter().filter_map(|f| f.as_str()).collect()).unwrap_or_default();
    if files.iter().any(|f| matches(f, &cfg.secret_patterns)) {
        why.push("changes touch secret/credential files".into());
    }
    if files.iter().any(|f| matches(f, &cfg.production_patterns)) {
        why.push("changes touch production/deployment configuration (human decides)".into());
    }
    if files.iter().any(|f| matches(f, &cfg.permission_patterns)) {
        why.push("changes touch reviewer permissions or policies (never auto-merged)".into());
    }
    json!({"ok": why.is_empty(), "reasons": why})
}

pub fn merge_command(pr: &Value, style: &str) -> Result<Vec<String>, String> {
    let flag = match style {
        "merge" => "--merge",
        "squash" => "--squash",
        "rebase" => "--rebase",
        _ => return Err(format!("unknown merge style {style}")),
    };
    Ok(["gh", "pr", "merge", &pr["number"].to_string(), flag, "--match-head-commit", pr["head_sha"].as_str().unwrap_or("")].iter().map(|s| s.to_string()).collect())
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Ok,
    Bad,
}

fn visit<'a>(p: &'a Value, by_branch: &HashMap<&str, &'a Value>, integration: &str, state: &mut HashMap<String, State>, order: &mut Vec<&'a Value>, chain: &mut Vec<String>) -> bool {
    let b = p["branch"].as_str().unwrap_or("").to_string();
    match state.get(&b) {
        Some(State::Ok) => return true,
        Some(State::Bad) => return false,
        None => {}
    }
    if chain.contains(&b) {
        return false;
    }
    let base = p["base"].as_str().unwrap_or("");
    let ok = if base == integration {
        true
    } else if let Some(parent) = by_branch.get(base) {
        chain.push(b.clone());
        let r = visit(parent, by_branch, integration, state, order, chain);
        chain.pop();
        r
    } else {
        false
    };
    state.insert(b, if ok { State::Ok } else { State::Bad });
    if ok {
        order.push(p);
    }
    ok
}

/// Orders PRs so a stacked PR merges after its base PR; skips PRs whose base is unknown or cyclic.
pub fn plan_batch(prs: &[Value], integration: &str) -> Value {
    let by_branch: HashMap<&str, &Value> = prs.iter().map(|p| (p["branch"].as_str().unwrap_or(""), p)).collect();
    let (mut order, mut state) = (vec![], HashMap::new());
    let mut sorted: Vec<&Value> = prs.iter().collect();
    sorted.sort_by_key(|p| p["number"].as_i64().unwrap_or(0));
    for p in sorted {
        visit(p, &by_branch, integration, &mut state, &mut order, &mut vec![]);
    }
    let mut skipped: Vec<Value> = prs
        .iter()
        .filter(|p| state.get(p["branch"].as_str().unwrap_or("")) != Some(&State::Ok))
        .map(|p| {
            let base = p["base"].as_str().unwrap_or("");
            let reason = if by_branch.contains_key(base) { "stack cycle or broken chain".to_string() } else { format!("base {base} is neither the integration branch nor in this batch") };
            json!({"number": p["number"], "reason": reason})
        })
        .collect();
    skipped.sort_by_key(|s| s["number"].as_i64().unwrap_or(0));
    json!({"order": order, "skipped": skipped})
}
