//! Docs gate: no release is signed off until its documents pass the check.
//!
//! cicd asks the platform to run the check (`docs_check`); the platform reads the files itself, so a PASS is a fact and
//! not an agent's claim. The tester may not sign off (`qc_result` with no failures) until the gate is PASS. Only the
//! human can ship with a documented exception. The check never echoes a secret it finds.
use super::flow::{bad, tasks, FlowError, Result};
use crate::db::now;
use crate::secrets::looks_like_secret;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!?\[[^\]]*\]\(([^)\s]+)[^)]*\)").expect("valid pattern"));
static EXTERNAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(https?:|mailto:|#)").expect("valid pattern"));
static ATTENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^##\s+Needs your attention\s*$").expect("valid pattern"));
static ANY_H2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s").expect("valid pattern"));
const GUIDE_DIRS: [&str; 2] = ["docs/user-guides", "docs/guides"];

fn read(p: &Path) -> String {
    if p.is_file() {
        std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
    } else {
        String::new()
    }
}

fn json_version(p: &Path) -> Option<String> {
    let v: Value = serde_json::from_str(&read(p)).ok()?;
    match v.get("version")? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// The text under the "## Needs your attention" heading, up to the next "## " heading.
fn attention_section(text: &str) -> String {
    let mut out = vec![];
    let mut inside = false;
    for line in text.lines() {
        if inside && ANY_H2.is_match(line) {
            break;
        }
        if inside {
            out.push(line);
        } else if ATTENTION.is_match(line) {
            inside = true;
        }
    }
    out.join("\n")
}

fn md_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                out.push(p);
            }
        }
    }
    let mut out = vec![];
    walk(dir, &mut out);
    out.sort();
    out
}

fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}

pub fn check_docs(root: &Path, version: &str, flagged: &[Value], gaps: &[Value], endpoints: &[&str]) -> Value {
    let mut failed: Vec<String> = vec![];
    let notes_rel = format!("docs/releases/v{version}.md");
    let notes = read(&root.join(&notes_rel));
    if !root.join(&notes_rel).is_file() {
        failed.push(format!("{notes_rel} is missing"));
    }
    let versions: [(&str, Option<String>); 2] = [
        ("frontend/package.json", json_version(&root.join("frontend/package.json"))),
        ("backend/VERSION", Some(read(&root.join("backend/VERSION")).trim().to_string()).filter(|s| !s.is_empty())),
    ];
    for (rel, got) in versions {
        if !root.join(rel).is_file() {
            failed.push(format!("{rel} is missing, so the version cannot be matched"));
        } else if got.as_deref() != Some(version) {
            failed.push(format!("{rel} says {}, the release is {version}", got.as_deref().unwrap_or("None")));
        }
    }
    let changelog = read(&root.join("CHANGELOG.md"));
    let first = changelog.lines().find(|l| l.starts_with("## ")).unwrap_or("");
    if changelog.is_empty() {
        failed.push("CHANGELOG.md is missing".into());
    } else if !first.contains(version) {
        failed.push(format!("CHANGELOG.md does not have the v{version} entry on top"));
    }
    for f in flagged {
        let guide = f["guide"].as_str().unwrap_or("");
        let updated = root.join(guide).is_file() && notes.contains(guide);
        let gap = gaps.iter().any(|g| g["pr"] == f["pr"] && !g["owner"].as_str().unwrap_or("").trim().is_empty());
        if !(updated || gap) {
            failed.push(format!("PR #{} affects guide {guide}: not updated and not listed as an open gap with an owner", show(&f["pr"])));
        }
    }
    for e in endpoints {
        let docs: Vec<PathBuf> = if root.exists() { ["docs/api", "docs/schema"].iter().flat_map(|d| md_files(&root.join(d))).collect() } else { vec![] };
        if !docs.iter().any(|p| read(p).contains(e)) {
            failed.push(format!("endpoint {e} is not in docs/api or docs/schema"));
        }
    }
    let mut files: Vec<PathBuf> = [root.join(&notes_rel), root.join("CHANGELOG.md")].into_iter().filter(|p| p.is_file()).collect();
    for d in GUIDE_DIRS.iter().chain(["docs/api", "docs/schema"].iter()) {
        files.extend(md_files(&root.join(d)));
    }
    for p in &files {
        let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
        let text = read(p);
        for (n, line) in text.lines().enumerate() {
            if looks_like_secret(line) {
                failed.push(format!("{rel}:{}: looks like a secret (not shown)", n + 1));
            }
        }
        for c in LINK.captures_iter(&text) {
            let target = &c[1];
            if EXTERNAL.is_match(target) {
                continue;
            }
            let path = target.split('#').next().unwrap_or("");
            if !path.is_empty() && !p.parent().unwrap_or(root).join(path).exists() {
                failed.push(format!("{rel}: {target} does not resolve"));
            }
        }
    }
    if notes.to_lowercase().contains("not verified") && !attention_section(&notes).to_lowercase().contains("not verified") {
        failed.push("Not verified items are not repeated under \"Needs your attention\"".into());
    }
    json!({"status": if failed.is_empty() { "PASS" } else { "FAIL" }, "failed": failed})
}

pub fn passes(t: &Value) -> bool {
    let g = &t["docs_gate"];
    g["status"] == "PASS" || truthy(&g["exception"])
}

fn truthy(v: &Value) -> bool {
    !(v.is_null() || *v == json!(false) || *v == json!("") || *v == json!({}) || *v == json!([]) || *v == json!(0))
}

pub fn state(t: &Value) -> String {
    let g = &t["docs_gate"];
    if truthy(&g["exception"]) {
        "EXCEPTION".into()
    } else {
        g["status"].as_str().unwrap_or("MISSING").to_string()
    }
}

pub struct DocsGate;

fn task(tid: &str) -> Result<Value> {
    match tasks()?.get(tid)? {
        Some(t) => Ok(t),
        None => bad(format!("unknown task {tid}")),
    }
}

fn updated(id: &str, patch: Value) -> Result<Value> {
    tasks()?.update(id, patch)?.ok_or_else(|| FlowError::Msg(format!("unknown task {id}")))
}

impl DocsGate {
    pub fn run(&self, tid: &str, version: &str, endpoints: &[&str], flagged: &[Value], gaps: &[Value]) -> Result<Value> {
        let t = task(tid)?;
        let res = match t["worktree"].as_str().filter(|r| !r.is_empty()) {
            Some(root) => check_docs(Path::new(root), version, flagged, gaps, endpoints),
            None => json!({"status": "FAIL", "failed": ["the task has no worktree to check"]}),
        };
        let mut gate: Map<String, Value> = res.as_object().cloned().unwrap_or_default();
        gate.insert("version".into(), json!(version));
        gate.insert("at".into(), json!(now()));
        gate.insert("exception".into(), t["docs_gate"]["exception"].clone());
        updated(tid, json!({"docs_gate": gate}))?;
        Ok(res)
    }

    pub fn exception(&self, tid: &str, reason: &str, by: &str) -> Result<Value> {
        let t = task(tid)?;
        if by != "human" {
            return bad("only the human can ship with a documented exception");
        }
        if reason.trim().is_empty() {
            return bad("a documented exception needs a reason");
        }
        let mut g = match t["docs_gate"].as_object() {
            Some(g) if !g.is_empty() => g.clone(),
            _ => json!({"status": "FAIL", "failed": ["the docs gate has not run"]}).as_object().cloned().unwrap_or_default(),
        };
        g.insert("exception".into(), json!({"reason": reason.trim(), "by": by, "at": now()}));
        updated(tid, json!({"docs_gate": g}))
    }

    pub fn require(&self, t: &Value) -> Result<()> {
        if passes(t) {
            return Ok(());
        }
        let g = &t["docs_gate"];
        let why = if g.as_object().is_none_or(|o| o.is_empty()) {
            "it has not run".to_string()
        } else {
            let first: Vec<&str> = g["failed"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).take(3).collect();
            format!("it is {}: {}", show(&g["status"]), first.join("; "))
        };
        bad(format!("waiting for docs gate ({why}); cicd runs the docs check, the tester never does"))
    }
}
