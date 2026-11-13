//! Role loader (FR-10): a role file is YAML frontmatter plus a markdown body.
//! Frontmatter: name, description, tools (list), skills (list), model (hint). A file without `tools` gets the
//! default allow-list for its role name; the platform enforces it, it is not a prompt hint (FR-12).
use super::{parse_yaml, py_str, split_frontmatter};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const READ: [&str; 5] = ["read_file", "list_dir", "git_read", "use_skill", "graph"];
/// All of these need an approved SRS before they may run.
pub const DEVELOPERS: [&str; 3] = ["developer", "developer_1", "developer_2"];

pub fn default_tools(role: &str) -> Vec<String> {
    let with = |extra: &[&str]| READ.iter().chain(extra.iter()).map(|s| s.to_string()).collect::<Vec<_>>();
    match role {
        "orchestrator" | "reviewer" | "incident-reporter" => with(&[]),
        "architect" => with(&["write_file", "pipeline"]),
        "developer" | "production-support" => with(&["write_file", "run_shell", "git_write"]),
        "developer_1" | "developer_2" => with(&["write_file", "run_shell", "git_write", "pipeline"]),
        "tester" => with(&["write_file", "run_shell", "pipeline"]),
        "cicd" => with(&["run_shell", "pipeline"]),
        "technical_writer" | "technical-writer" => with(&["write_file", "git_write"]), // prose only: no shell, no pipeline
        "incident_reporter" => with(&["incident"]),
        "production_support" => with(&["write_file", "run_shell", "git_write", "incident"]),
        _ => with(&[]),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Role {
    pub name: String,
    pub description: String,
    pub body: String,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub model: Option<String>,
    pub source: String,
}

impl Role {
    pub fn new(name: &str, description: &str, body: &str, tools: &[&str], skills: &[&str]) -> Self {
        Role {
            name: name.into(),
            description: description.into(),
            body: body.into(),
            tools: tools.iter().map(|s| s.to_string()).collect(),
            skills: skills.iter().map(|s| s.to_string()).collect(),
            model: None,
            source: String::new(),
        }
    }
}

fn list_of(v: Option<&Value>) -> Option<Vec<String>> {
    let a = v?.as_array()?;
    if a.is_empty() {
        return None; // an empty list counts as not given, like Python's `or`
    }
    Some(a.iter().map(py_str).collect())
}

pub fn parse_role(text: &str, source: &str) -> Result<Role, String> {
    let (fm, body) = split_frontmatter(text).ok_or_else(|| format!("{source}: missing frontmatter"))?;
    let meta = parse_yaml(fm).ok_or_else(|| format!("{source}: frontmatter is not a mapping"))?;
    let truthy = |k: &str| meta.get(k).filter(|v| !v.is_null() && **v != Value::String(String::new()));
    let (Some(name), Some(desc)) = (truthy("name"), truthy("description")) else {
        return Err(format!("{source}: frontmatter needs name and description"));
    };
    let name = py_str(name);
    Ok(Role {
        tools: list_of(meta.get("tools")).unwrap_or_else(|| default_tools(&name)),
        skills: list_of(meta.get("skills")).unwrap_or_default(),
        model: meta.get("model").filter(|v| !v.is_null()).map(py_str),
        description: py_str(desc),
        body: body.trim().to_string(),
        name,
        source: source.to_string(),
    })
}

/// Loads every `*.md` role file; a file in an earlier folder wins over the same role name in a later one.
pub fn load_roles(dirs: &[PathBuf]) -> BTreeMap<String, Role> {
    let mut roles = BTreeMap::new();
    for d in dirs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md") && p.is_file())
            .collect();
        files.sort();
        for f in files {
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            let Ok(r) = parse_role(&text, &f.to_string_lossy()) else { continue };
            roles.entry(r.name.clone()).or_insert(r);
        }
    }
    roles
}

pub fn validate(role: &Role, known_tools: &[&str], known_skills: &[&str]) -> Vec<String> {
    let mut problems: Vec<String> = role.tools.iter().filter(|t| !known_tools.contains(&t.as_str())).map(|t| format!("unknown tool {t}")).collect();
    problems.extend(role.skills.iter().filter(|s| !known_skills.contains(&s.as_str())).map(|s| format!("missing skill {s}")));
    problems
}
