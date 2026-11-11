//! GitHub through the `gh` CLI (A-4): PR facts for the merge gate, retarget, merge. The runner is injectable.
use super::merge_gate::parse_verdict;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::LazyLock;

static HEAD_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^head:\s*([0-9a-f]{40})\s*$").expect("valid pattern"));
static FINDING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^-\s*\[(low|medium|high|critical)\]\s*(.+?)\s*$").expect("valid pattern"));
static RESOLVED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\(resolved\)$").expect("valid pattern"));

const FAIL: [&str; 6] = ["FAILURE", "CANCELLED", "TIMED_OUT", "ERROR", "ACTION_REQUIRED", "STARTUP_FAILURE"];
const GOOD: [&str; 3] = ["SUCCESS", "NEUTRAL", "SKIPPED"];
const FIELDS: &str = "number,baseRefName,headRefName,headRefOid,mergeable,statusCheckRollup,files,comments,reviews";

/// Review convention: line 1 `Verdict: X`, line 2 `Head: <40-hex sha>`, then `- [severity] finding` lines
/// (append `(resolved)` once fixed). A review without a Head line can never match the current head.
pub fn parse_review(body: &str, at: f64) -> Option<Value> {
    let verdict = parse_verdict(body)?;
    let lines: Vec<&str> = body.trim().lines().collect();
    let head = lines.get(1).and_then(|l| HEAD_LINE.captures(l.trim())).map(|m| m[1].to_lowercase());
    let findings: Vec<Value> = lines
        .iter()
        .skip(2)
        .filter_map(|l| FINDING.captures(l.trim()))
        .map(|m| {
            let text = m[2].to_string();
            json!({"severity": m[1].to_lowercase(), "title": RESOLVED.replace(&text, "").to_string(), "resolved": text.to_lowercase().ends_with("(resolved)")})
        })
        .collect();
    Some(json!({"verdict": verdict, "head_sha": head, "at": at, "findings": findings}))
}

/// Seconds since the epoch of an ISO 8601 time such as 2026-01-01T00:00:01Z (0 when missing or unreadable).
fn ts(s: Option<&str>) -> f64 {
    let Some(s) = s.filter(|s| !s.is_empty()) else { return 0.0 };
    let re = Regex::new(r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$").expect("valid pattern");
    let Some(m) = re.captures(s) else { return 0.0 };
    let n = |i: usize| m[i].parse::<i64>().unwrap_or(0);
    let (y, mo, d) = (n(1), n(2), n(3));
    // days since 1970-01-01 (civil-from-days inverse)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut secs = (days * 86400 + n(4) * 3600 + n(5) * 60 + n(6)) as f64;
    if let Some(f) = m.get(7) {
        secs += f.as_str().parse::<f64>().unwrap_or(0.0);
    }
    if let Some(z) = m.get(8).map(|z| z.as_str()).filter(|z| *z != "Z") {
        let sign = if z.starts_with('-') { -1.0 } else { 1.0 };
        let digits: String = z[1..].chars().filter(|c| c.is_ascii_digit()).collect();
        let off = digits[..2].parse::<f64>().unwrap_or(0.0) * 3600.0 + digits[2..].parse::<f64>().unwrap_or(0.0) * 60.0;
        secs -= sign * off;
    }
    secs
}

pub fn pr_facts_from_gh(raw: &Value, tests: Option<Value>) -> Value {
    let checks = raw["statusCheckRollup"].as_array().cloned().unwrap_or_default();
    let concl = |c: &Value| c["conclusion"].as_str().unwrap_or("").to_string();
    let ci = if checks.is_empty() {
        Value::Null
    } else if checks.iter().any(|c| FAIL.contains(&concl(c).as_str())) {
        json!("failure")
    } else if checks.iter().any(|c| concl(c).is_empty()) {
        json!("pending")
    } else if checks.iter().all(|c| GOOD.contains(&concl(c).as_str())) {
        json!("success")
    } else {
        json!("failure")
    };
    let mut reviews: Vec<Value> = vec![];
    for c in raw["comments"].as_array().into_iter().flatten() {
        reviews.extend(parse_review(c["body"].as_str().unwrap_or(""), ts(c["createdAt"].as_str())));
    }
    for c in raw["reviews"].as_array().into_iter().flatten() {
        reviews.extend(parse_review(c["body"].as_str().unwrap_or(""), ts(c["submittedAt"].as_str())));
    }
    let files: Vec<Value> = raw["files"].as_array().into_iter().flatten().map(|f| f["path"].clone()).collect();
    json!({"number": raw["number"], "branch": raw["headRefName"], "base": raw["baseRefName"], "head_sha": raw["headRefOid"], "ci": ci,
           "mergeable": raw["mergeable"] == "MERGEABLE", "tests": tests, "changed_files": files, "reviews": reviews})
}

pub type GhRunner = Box<dyn Fn(&[String]) -> Result<String, String> + Send + Sync>;
pub type TestsLookup = Box<dyn Fn(i64) -> Option<Value> + Send + Sync>;

fn default_runner(argv: &[String]) -> Result<String, String> {
    let env: HashMap<String, String> = std::env::vars().collect(); // gh needs the user's own login and settings
    match crate::proc::run(argv, None, &env, "", 120.0) {
        Ok((0, out, _)) => Ok(out),
        Ok((_, out, err)) => Err(if err.trim().is_empty() { out } else { err }.trim().to_string()),
        Err(e) => Err(format!("{e:?}")),
    }
}

pub struct GhClient {
    repo: String,
    run: GhRunner,
    tests_lookup: TestsLookup,
}

impl GhClient {
    pub fn new(repo: &str, runner: Option<GhRunner>, tests_lookup: Option<TestsLookup>) -> Self {
        GhClient { repo: repo.into(), run: runner.unwrap_or_else(|| Box::new(default_runner)), tests_lookup: tests_lookup.unwrap_or_else(|| Box::new(|_| None)) }
    }

    fn args(&self, parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    pub fn facts(&self, n: i64) -> Result<Value, String> {
        let out = (self.run)(&self.args(&["gh", "pr", "view", &n.to_string(), "--repo", &self.repo, "--json", FIELDS]))?;
        let raw: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
        Ok(pr_facts_from_gh(&raw, (self.tests_lookup)(n)))
    }

    pub fn retarget(&self, n: i64, base: &str) -> Result<(), String> {
        (self.run)(&self.args(&["gh", "pr", "edit", &n.to_string(), "--repo", &self.repo, "--base", base])).map(|_| ())
    }

    pub fn merge(&self, cmd: &[String]) -> Result<(), String> {
        if cmd.iter().any(|c| c == "--admin") {
            return Err("admin merges are not allowed".into());
        }
        let mut full = cmd.to_vec();
        full.extend(["--repo".to_string(), self.repo.clone()]);
        (self.run)(&full).map(|_| ())
    }
}
