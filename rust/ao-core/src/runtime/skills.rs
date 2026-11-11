//! Skill registry (FR-11, FR-18): folders with a SKILL.md (frontmatter name, description).
//! Search paths are ordered by precedence (first wins: plugin-local, project, personal). The model sees only the
//! name and description; the body is loaded when a skill is used. The registry is read-only.
use super::{parse_yaml, py_str, split_frontmatter};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf, // SKILL.md
    pub source: String, // the search path it came from
}

#[derive(Debug, Clone)]
pub struct Estimate {
    pub tokens: usize,
    pub unknown: Vec<String>,
    pub bodies: Vec<(String, String)>,
}

fn parse(path: &Path) -> Option<(serde_json::Map<String, Value>, String)> {
    let text = String::from_utf8_lossy(&std::fs::read(path).ok()?).into_owned();
    let (fm, body) = split_frontmatter(&text)?;
    let meta = parse_yaml(fm)?;
    meta.get("name").filter(|n| !n.is_null() && **n != Value::String(String::new()))?;
    Some((meta, body.trim().to_string()))
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    pub search_paths: Vec<PathBuf>,
}

impl SkillRegistry {
    pub fn new(search_paths: &[PathBuf]) -> Self {
        SkillRegistry { search_paths: search_paths.to_vec() }
    }

    /// name -> info, in catalog order (first search path first, folders sorted within it).
    pub fn scan(&self) -> Vec<SkillInfo> {
        let mut found: BTreeMap<String, ()> = BTreeMap::new();
        let mut out = vec![];
        for root in &self.search_paths {
            let mut mds: Vec<PathBuf> = std::fs::read_dir(root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path().join("SKILL.md"))
                .filter(|p| p.is_file())
                .collect();
            mds.sort();
            for md in mds {
                let Some((meta, _)) = parse(&md) else { continue };
                let name = py_str(&meta["name"]);
                if found.insert(name.clone(), ()).is_none() {
                    out.push(SkillInfo {
                        description: meta.get("description").map(py_str).unwrap_or_default(),
                        name,
                        path: md,
                        source: root.to_string_lossy().into_owned(),
                    });
                }
            }
        }
        out
    }

    pub fn catalog(&self, only: Option<&[&str]>) -> Vec<Value> {
        let skills = self.scan();
        let names: Vec<String> = match only {
            Some(o) => o.iter().map(|s| s.to_string()).collect(),
            None => skills.iter().map(|s| s.name.clone()).collect(),
        };
        names
            .iter()
            .filter_map(|n| skills.iter().find(|s| &s.name == n))
            .map(|s| json!({"name": s.name, "description": s.description, "source": s.source}))
            .collect()
    }

    /// What preloading these skills would cost: about 4 characters per token. Unknown names are reported.
    pub fn estimate(&self, names: &[&str]) -> Estimate {
        let (mut bodies, mut unknown): (Vec<(String, String)>, Vec<String>) = (vec![], vec![]);
        let mut seen = vec![];
        for n in names {
            if seen.contains(n) {
                continue;
            }
            seen.push(n);
            match self.load_body(n) {
                Some(b) => bodies.push((n.to_string(), b)),
                None => unknown.push(n.to_string()),
            }
        }
        Estimate { tokens: bodies.iter().map(|(_, b)| b.chars().count() / 4).sum(), unknown, bodies }
    }

    pub fn load_body(&self, name: &str) -> Option<String> {
        let info = self.scan().into_iter().find(|s| s.name == name)?;
        parse(&info.path).map(|(_, b)| b)
    }
}
