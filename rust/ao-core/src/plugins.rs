//! Plugins: a folder with plugin.json, roles/, skills/, workflows/, templates/ and optional mcp.json.
//!
//! A plugin is validated on install and again on enable; one with problems is kept but stays disabled, and the problems
//! name the file and line. Enabling adds its roles and skills folders to the ones the app already loads. Nothing is
//! executed from a plugin: it is data (role files, skill text, workflow YAML, templates, MCP config without secrets).
use crate::config::{data_dir, shipped_plugins_dir};
use crate::db::{now, DbError, DocRepo};
use crate::providers::base::py_dumps;
use crate::runtime::py_str;
use crate::runtime::roles::parse_role;
use crate::runtime::skills::parse as parse_skill;
use crate::secrets::looks_like_secret;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9_-]*$").expect("valid pattern"));

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("{0}")]
    Msg(String),
    #[error("{0}")]
    NotFound(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

type Result<T> = std::result::Result<T, PluginError>;

fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(PluginError::Msg(m.into()))
}

pub fn store() -> PathBuf {
    data_dir().join("plugins")
}

fn rel(path: &Path, base: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn files(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| keep(p)).collect();
    v.sort();
    v
}

fn read(p: &Path) -> String {
    std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// The line a key starts on (1 when it is not there), so a problem can name file and line.
fn line_of(path: &Path, key: &str) -> usize {
    read(path).lines().position(|l| l.starts_with(key)).map_or(1, |i| i + 1)
}

fn strings(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::String(s) => out.push(s.clone()),
        Value::Object(m) => m.values().for_each(|v| strings(v, out)),
        Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
        _ => {}
    }
}

pub fn read_manifest(path: &Path) -> (Option<Value>, Vec<String>) {
    let mf = path.join("plugin.json");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !mf.exists() {
        return (None, vec![format!("{name}: plugin.json is missing")]);
    }
    let data: Value = match serde_json::from_str(&read(&mf)) {
        Ok(d) => d,
        Err(e) => return (None, vec![format!("plugin.json: not valid JSON ({e})")]),
    };
    if !data.is_object() {
        return (None, vec!["plugin.json: must be a JSON object".into()]);
    }
    let mut problems = vec![];
    for k in ["name", "version", "description"] {
        if !data[k].as_str().is_some_and(|s| !s.trim().is_empty()) {
            problems.push(format!("plugin.json: {k} is required"));
        }
    }
    if data["name"].as_str().is_some_and(|n| !NAME.is_match(n)) {
        problems.push("plugin.json: name uses lowercase letters, digits, - and _".into());
    }
    (Some(data), problems)
}

pub fn validate_plugin(path: &Path, known_roles: &[&str], known_tools: &[&str], known_skills: &[&str]) -> Vec<String> {
    let (_, mut problems) = read_manifest(path);
    let mut plugin_skills: HashSet<String> = HashSet::new();
    for dir in files(&path.join("skills"), |p| p.is_dir()) {
        let md = dir.join("SKILL.md");
        if !md.is_file() {
            continue;
        }
        match parse_skill(&md) {
            None => problems.push(format!("{}: needs frontmatter with a name", rel(&md, path))),
            Some((meta, _)) => {
                plugin_skills.insert(py_str(&meta["name"]));
            }
        }
    }
    let skills: HashSet<&str> = known_skills.iter().copied().chain(plugin_skills.iter().map(|s| s.as_str())).collect();
    let mut plugin_roles: HashSet<String> = HashSet::new();
    for md in files(&path.join("roles"), |p| p.extension().is_some_and(|x| x == "md")) {
        let r = rel(&md, path);
        match parse_role(&read(&md), &r) {
            Err(e) => {
                problems.push(format!("{r}:1: {e}"));
                continue;
            }
            Ok(role) => {
                for t in &role.tools {
                    if !known_tools.contains(&t.as_str()) {
                        problems.push(format!("{r}:{}: unknown tool {t}", line_of(&md, "tools:")));
                    }
                }
                for s in &role.skills {
                    if !skills.contains(s.as_str()) {
                        problems.push(format!("{r}:{}: missing skill {s}", line_of(&md, "skills:")));
                    }
                }
                plugin_roles.insert(role.name);
            }
        }
    }
    let roles: HashSet<&str> = known_roles.iter().copied().chain(plugin_roles.iter().map(|s| s.as_str())).collect();
    for wf in files(&path.join("workflows"), |p| p.extension().is_some_and(|x| x == "yaml" || x == "yml")) {
        let r = rel(&wf, path);
        let doc: Value = match serde_yaml::from_str::<Value>(&read(&wf)) {
            Ok(Value::Null) => json!({}),
            Ok(d) => d,
            Err(e) => {
                problems.push(format!("{r}: not valid YAML ({e})"));
                continue;
            }
        };
        for st in doc["stages"].as_array().into_iter().flatten() {
            let role = &st["role"];
            if !st.is_object() {
                problems.push(format!("{r}: stage {} names unknown role ?", py_str(st)));
            } else if !role.as_str().is_some_and(|x| roles.contains(x)) {
                problems.push(format!("{r}: stage {} names unknown role {}", py_str(&st["name"]), py_str(role)));
            }
        }
    }
    let mcp = path.join("mcp.json");
    if mcp.exists() {
        let cfg = match serde_json::from_str::<Value>(&read(&mcp)) {
            Ok(c) => c,
            Err(e) => {
                problems.push(format!("mcp.json: not valid JSON ({e})"));
                json!({})
            }
        };
        for (name, srv) in cfg["servers"].as_object().into_iter().flatten() {
            if srv.get("env").is_some() {
                problems.push(format!("mcp.json: server {name} carries secrets in env; use env_refs (a variable name)"));
            }
        }
        let mut all = vec![];
        strings(&cfg, &mut all);
        if all.iter().any(|s| looks_like_secret(s)) {
            problems.push("mcp.json: contains something that looks like a secret; plugins never carry secrets".into());
        }
    }
    problems
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

fn io(e: std::io::Error) -> PluginError {
    PluginError::Msg(e.to_string())
}

fn repo() -> Result<DocRepo> {
    Ok(DocRepo::new("plugins")?)
}

pub struct Plugins;

impl Plugins {
    pub fn list(&self) -> Result<Vec<Value>> {
        Ok(repo()?.list()?)
    }

    pub fn available(&self) -> Result<Vec<Value>> {
        let mut out = vec![];
        for dir in files(&shipped_plugins_dir(), |p| p.join("plugin.json").is_file()) {
            if let (Some(data), _) = read_manifest(&dir) {
                let fallback = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(json!({"name": data["name"].as_str().unwrap_or(&fallback), "description": data["description"].as_str().unwrap_or(""),
                                "version": data["version"].as_str().unwrap_or(""), "path": dir.to_string_lossy()}));
            }
        }
        Ok(out)
    }

    pub fn install(&self, source: &str, roles: &[&str], tools: &[&str], skills: &[&str]) -> Result<Value> {
        let src = Path::new(source);
        if !src.is_dir() {
            return bad(format!("{source} is not a folder"));
        }
        let (manifest, mp) = read_manifest(src);
        let Some(manifest) = manifest else { return bad(mp.first().cloned().unwrap_or_default()) };
        let name = manifest["name"].as_str().unwrap_or("").to_string();
        if !NAME.is_match(&name) {
            return bad(if mp.is_empty() { "plugin.json: bad name".to_string() } else { mp.join("; ") });
        }
        let dest = store().join(&name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(io)?;
        }
        copy_dir(src, &dest).map_err(io)?;
        let problems = validate_plugin(&dest, roles, tools, skills);
        let doc = json!({"name": name, "version": manifest["version"].as_str().unwrap_or(""), "description": manifest["description"].as_str().unwrap_or(""),
                         "enabled": false, "problems": problems, "installed_at": now()});
        let r = repo()?;
        if r.get(&name)?.is_some() {
            return Ok(r.update(&name, doc)?.unwrap_or(Value::Null));
        }
        Ok(r.create(doc, Some(&name))?)
    }

    fn get(&self, name: &str) -> Result<Value> {
        repo()?.get(name)?.ok_or_else(|| PluginError::NotFound(name.to_string()))
    }

    pub fn enable(&self, name: &str, roles: &[&str], tools: &[&str], skills: &[&str]) -> Result<Value> {
        let d = self.get(name)?;
        let problems = validate_plugin(&store().join(name), roles, tools, skills);
        if !problems.is_empty() {
            repo()?.update(name, json!({"problems": problems, "enabled": false}))?;
            return bad(format!("{name} has problems: {}", problems.join("; ")));
        }
        Ok(repo()?.update(d["id"].as_str().unwrap_or(name), json!({"enabled": true, "problems": []}))?.unwrap_or(Value::Null))
    }

    pub fn disable(&self, name: &str) -> Result<Value> {
        let d = self.get(name)?;
        Ok(repo()?.update(d["id"].as_str().unwrap_or(name), json!({"enabled": false}))?.unwrap_or(Value::Null))
    }

    pub fn remove(&self, name: &str) -> Result<bool> {
        let r = repo()?;
        if r.get(name)?.is_none() {
            return Ok(false);
        }
        r.delete(name)?;
        let _ = std::fs::remove_dir_all(store().join(name));
        Ok(true)
    }

    /// Roles and skills folders of the enabled plugins, to be added to what the app loads.
    pub fn dirs(&self) -> Result<(Vec<String>, Vec<String>)> {
        Ok(dirs()?)
    }

    /// Agent builder: turn an agent definition into a one-role plugin, validate it, install it (disabled).
    pub fn build_agent(&self, d: &Value, roles: &[&str], tools: &[&str], skills: &[&str]) -> Result<Value> {
        let name = d["name"].as_str().map_or(String::new(), |n| n.trim().to_string());
        if !NAME.is_match(&name) {
            return bad("the agent name uses lowercase letters, digits, - and _");
        }
        let text = |k: &str| d[k].as_str().map_or(String::new(), |s| s.trim().to_string());
        if text("description").is_empty() || text("instructions").is_empty() {
            return bad("an agent needs a description and instructions");
        }
        let list = |k: &str| -> Vec<String> { d[k].as_array().into_iter().flatten().map(py_str).collect() };
        let meta = json!({"name": name, "description": text("description"), "tools": list("tools"), "skills": list("skills")});
        let tmp = tempfile_dir()?;
        let base = tmp.join(format!("agent-{name}"));
        std::fs::create_dir_all(base.join("roles")).map_err(io)?;
        let manifest = json!({"name": format!("agent-{name}"), "version": "1.0.0", "description": text("description")});
        std::fs::write(base.join("plugin.json"), py_dumps(&manifest)).map_err(io)?;
        let front = serde_yaml::to_string(&meta).map_err(|e| PluginError::Msg(e.to_string()))?;
        std::fs::write(base.join("roles").join(format!("{name}.md")), format!("---\n{front}---\n{}\n", text("instructions"))).map_err(io)?;
        let problems = validate_plugin(&base, roles, tools, skills);
        let out = if problems.is_empty() { self.install(&base.to_string_lossy(), roles, tools, skills) } else { bad(problems.join("; ")) };
        let _ = std::fs::remove_dir_all(&tmp);
        let mut doc = out?;
        doc["plugin"] = doc["name"].clone();
        Ok(doc)
    }

    pub fn export_zip(&self, name: &str) -> Result<Vec<u8>> {
        self.get(name)?;
        let base = store().join(name);
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let mut all = vec![];
            walk(&base, &mut all);
            all.sort();
            for f in all {
                z.start_file(rel(&f, &base), opts).map_err(|e| PluginError::Msg(e.to_string()))?;
                z.write_all(&std::fs::read(&f).map_err(io)?).map_err(io)?;
            }
            z.finish().map_err(|e| PluginError::Msg(e.to_string()))?;
        }
        Ok(buf.into_inner())
    }
}

fn walk(d: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn tempfile_dir() -> Result<PathBuf> {
    let d = std::env::temp_dir().join(format!("ao-agent-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).map_err(io)?;
    Ok(d)
}

/// Roles and skills folders of the enabled plugins, to be added to what the app loads.
pub fn dirs() -> std::result::Result<(Vec<String>, Vec<String>), DbError> {
    let (mut roles, mut skills) = (vec![], vec![]);
    for p in DocRepo::new("plugins")?.list()? {
        if p["enabled"] == true {
            let base = store().join(p["name"].as_str().unwrap_or(""));
            if base.join("roles").is_dir() {
                roles.push(base.join("roles").to_string_lossy().into_owned());
            }
            if base.join("skills").is_dir() {
                skills.push(base.join("skills").to_string_lossy().into_owned());
            }
        }
    }
    Ok((roles, skills))
}
