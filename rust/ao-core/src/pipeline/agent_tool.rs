//! The `pipeline` tool: agents report their own step and the platform checks the role is allowed to.
//!
//! Authority is enforced here, not in the prompt: only a task's owner reports its work done, only the other developer
//! reviews it, and each release step belongs to one role. The architect may sign off production but can never stand
//! in for the human, whose approval only comes from the UI.
use super::docs_gate::{state as docs_state, DocsGate};
use super::flow::{tasks, Flow, FlowError};
use super::release::Release;
use crate::incidents::Incidents;
use crate::runtime::tools::{Hook, ToolError};
use serde_json::{json, Value};

pub const ACTIONS: [&str; 11] = ["list", "dev_done", "tests", "review", "architect_review", "deploy_qc", "qc_result", "approve_prod", "deploy_prod", "prod_tests", "docs_check"];
const FIELDS: [&str; 9] = ["id", "name", "owner", "reviewer", "status", "release", "approvals", "pr_number", "head_sha"];

fn release_owner(action: &str) -> Option<&'static str> {
    match action {
        "architect_review" | "approve_prod" => Some("architect"),
        "deploy_qc" | "deploy_prod" | "docs_check" => Some("cicd"),
        "qc_result" | "prod_tests" => Some("tester"),
        _ => None,
    }
}

pub fn summary(t: &Value) -> Value {
    let mut m = serde_json::Map::new();
    for k in FIELDS {
        m.insert(k.into(), t.get(k).cloned().unwrap_or(Value::Null));
    }
    let gate = &t["docs_gate"];
    let has_gate = !(gate.is_null() || gate.as_object().is_some_and(|o| o.is_empty()));
    m.insert("docs_gate".into(), if has_gate { json!(docs_state(t)) } else { Value::Null });
    Value::Object(m)
}

type R<T> = Result<T, ToolError>;

fn te(e: FlowError) -> ToolError {
    ToolError(e.to_string())
}

fn refuse<T>(m: impl Into<String>) -> R<T> {
    Err(ToolError(m.into()))
}

pub struct PipelineTool {
    actor: String,
    flow: Option<Flow>,
}

struct Args<'a> {
    pr_number: Option<i64>,
    head_sha: &'a str,
    passed: Option<i64>,
    failed: Option<i64>,
    area: &'a str,
    body: &'a str,
    approve: Option<bool>,
    version: &'a str,
    endpoints: Vec<&'a str>,
    flagged: Vec<Value>,
    gaps: Vec<Value>,
}

impl PipelineTool {
    pub fn new(actor: &str, flow: Option<Flow>) -> Self {
        PipelineTool { actor: actor.into(), flow }
    }

    /// The hook a `ToolBox` calls for the `pipeline` tool, bound to this role.
    pub fn into_hook(self) -> Hook {
        Box::new(move |args: &Value| self.call(args))
    }

    pub fn call(&self, args: &Value) -> R<String> {
        let action = args["action"].as_str().unwrap_or("");
        if !ACTIONS.contains(&action) {
            return refuse(format!("unknown pipeline action {action}; use one of {}", ACTIONS.join(", ")));
        }
        if action == "list" {
            let rows: Vec<Value> = tasks().map_err(|e| ToolError(e.to_string()))?.list().map_err(|e| ToolError(e.to_string()))?.iter().map(summary).collect();
            return Ok(Value::Array(rows).to_string());
        }
        let Some(task_id) = args["task_id"].as_str().filter(|t| !t.is_empty()) else { return refuse("task_id is required") };
        let t = tasks().map_err(|e| ToolError(e.to_string()))?.get(task_id).map_err(|e| ToolError(e.to_string()))?;
        let Some(t) = t else { return refuse(format!("unknown task {task_id}")) };
        let s = |k: &str| args[k].as_str().unwrap_or("");
        let a = Args {
            pr_number: args["pr_number"].as_i64(),
            head_sha: s("head_sha"),
            passed: args["passed"].as_i64(),
            failed: args["failed"].as_i64(),
            area: s("area"),
            body: s("body"),
            approve: args["approve"].as_bool(),
            version: s("version"),
            endpoints: args["endpoints"].as_array().into_iter().flatten().filter_map(|e| e.as_str()).collect(),
            flagged: args["flagged"].as_array().cloned().unwrap_or_default(),
            gaps: args["gaps"].as_array().cloned().unwrap_or_default(),
        };
        Ok(summary(&self.act(action, &t, &a)?).to_string())
    }

    fn need(&self, role: &str) -> R<()> {
        if self.actor == role {
            Ok(())
        } else {
            refuse(format!("only the {role} role can do this step"))
        }
    }

    fn act(&self, action: &str, t: &Value, a: &Args) -> R<Value> {
        let tid = t["id"].as_str().unwrap_or("");
        let counts = || -> R<(i64, i64)> {
            match (a.passed, a.failed) {
                (Some(p), Some(f)) => Ok((p, f)),
                _ => refuse("passed and failed counts are required"),
            }
        };
        if let Some(role) = release_owner(action) {
            self.need(role)?;
            let r = Release;
            return match action {
                "docs_check" => {
                    if a.version.is_empty() {
                        return refuse("version is required");
                    }
                    DocsGate.run(tid, a.version, &a.endpoints, &a.flagged, &a.gaps).map_err(te)?;
                    tasks().map_err(|e| ToolError(e.to_string()))?.get(tid).map_err(|e| ToolError(e.to_string()))?.ok_or_else(|| ToolError(format!("unknown task {tid}")))
                }
                "architect_review" => match a.approve {
                    Some(ok) => r.architect_review(tid, ok).map_err(te),
                    None => refuse("approve (true or false) is required"),
                },
                "approve_prod" => r.approve_prod(tid, "architect").map_err(te),
                "deploy_qc" => r.deploy_qc(tid).map_err(te),
                "deploy_prod" => r.deployed_prod(tid).map_err(te),
                "prod_tests" => {
                    let (p, f) = counts()?;
                    Incidents.production_tests(tid, p, f, a.area).map_err(|e| ToolError(e.to_string()))?;
                    Ok(t.clone())
                }
                _ => {
                    let (p, f) = counts()?;
                    r.qc_result(tid, p, f).map_err(te)
                }
            };
        }
        let Some(flow) = &self.flow else { return refuse("the pipeline flow is not available in this run") };
        let reviewer = t["reviewer"].as_str().unwrap_or("");
        let owner = t["owner"].as_str().unwrap_or("");
        if action == "review" {
            if self.actor != reviewer {
                return refuse(format!("only the peer reviewer ({reviewer}) can review this task"));
            }
            return flow.record_review(tid, a.body).map_err(te);
        }
        if self.actor != owner {
            return refuse(format!("only the owner ({owner}) can report this step"));
        }
        if action == "dev_done" {
            return match (a.pr_number, a.head_sha.is_empty()) {
                (Some(pr), false) => flow.dev_done(tid, pr, a.head_sha).map_err(te),
                _ => refuse("pr_number and head_sha are required"),
            };
        }
        let (p, f) = counts()?;
        flow.record_tests(tid, p, f, a.area).map_err(te)
    }
}
