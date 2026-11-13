//! A code knowledge graph of a repository: files, their symbols (functions, classes, ...) and which file imports which.
//! An agent asks it ("where is X", "what does this file hold", "what breaks if I change it", "which tests cover it") instead
//! of reading whole files, which is where most of the tokens go. Built by a plain scan (no model call, no network) and kept
//! as one JSON file per repository under the data folder. Answers about a single file are read from disk, so they are fresh;
//! cross-file answers (who imports it) come from the last scan.
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const SKIP_DIRS: [&str; 16] = [
    ".git", "node_modules", "target", "dist", "build", ".next", "out", "__pycache__", ".venv", "venv", ".idea", ".vscode", "coverage", "vendor", ".claude", ".pytest_cache",
];
const MAX_FILE_BYTES: u64 = 400 * 1024;
const MAX_HITS: usize = 25;
const MAX_OUTLINE: usize = 150;
const ACTIONS: &str = "find, outline, imports, importers, impact, tests_for, stats";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub path: String,
    pub lang: String,
    pub bytes: u64,
    pub lines: usize,
    pub is_test: bool,
    pub symbols: Vec<Symbol>,
    /// Files in the repository this file imports (repository-relative, `/` separators).
    pub imports: Vec<String>,
    /// Packages from outside the repository (standard library, dependencies).
    pub external: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Graph {
    pub root: String,
    pub built_at: f64,
    pub files: Vec<FileNode>,
}

fn lang_of(p: &Path) -> Option<&'static str> {
    match p.extension()?.to_str()? {
        "py" => Some("python"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" | "mjs" => Some("javascript"),
        "rs" => Some("rust"),
        _ => None,
    }
}

fn rx(p: &str) -> Regex {
    Regex::new(p).expect("valid pattern")
}

static PY_DEF: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:async\s+)?def\s+(\w+)"));
static PY_CLASS: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*class\s+(\w+)"));
static PY_IMPORT: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*import\s+([\w.,\s]+?)\s*(?:#.*)?$"));
static PY_FROM: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*from\s+(\.*)([\w.]*)\s+import\s+(.+)"));
static TS_FUNC: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*(\w+)"));
static TS_CLASS: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+(\w+)"));
static TS_CONST: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*export\s+(?:declare\s+)?(?:const|let|var)\s+(\w+)"));
static TS_IFACE: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:export\s+)?interface\s+(\w+)"));
static TS_TYPE: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:export\s+)?type\s+(\w+)\s*[=<]"));
static TS_ENUM: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:export\s+)?(?:const\s+)?enum\s+(\w+)"));
static TS_IMPORT: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?s)\b(?:import|export)\s+(?:[^'";]*?\s+from\s+)?['"]([^'"]+)['"]|\brequire\(\s*['"]([^'"]+)['"]\s*\)|\bimport\(\s*['"]([^'"]+)['"]\s*\)"#));
static RS_ITEM: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(fn|struct|enum|trait|type|mod)\s+(\w+)|\bimpl(?:<[^>]*>)?\s+(?:[\w:]+(?:<[^>]*>)?\s+for\s+)?(\w+)"));
static RS_MOD: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;"));
static RS_USE: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([\w:]+)"));

fn symbols(lang: &str, text: &str) -> Vec<Symbol> {
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let mut add = |kind: &str, name: &str| out.push(Symbol { name: name.into(), kind: kind.into(), line: n });
        match lang {
            "python" => {
                if let Some(c) = PY_CLASS.captures(line) {
                    add("class", &c[1]);
                } else if let Some(c) = PY_DEF.captures(line) {
                    add("function", &c[1]);
                }
            }
            "rust" => {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for c in RS_ITEM.captures_iter(line) {
                    match (c.get(1), c.get(2), c.get(3)) {
                        (Some(k), Some(nm), _) => add(if k.as_str() == "fn" { "function" } else { k.as_str() }, nm.as_str()),
                        (_, _, Some(t)) => add("impl", t.as_str()),
                        _ => {}
                    }
                }
            }
            _ => {
                if let Some(c) = TS_CLASS.captures(line) {
                    add("class", &c[1]);
                } else if let Some(c) = TS_FUNC.captures(line) {
                    add("function", &c[1]);
                } else if let Some(c) = TS_IFACE.captures(line) {
                    add("interface", &c[1]);
                } else if let Some(c) = TS_TYPE.captures(line) {
                    add("type", &c[1]);
                } else if let Some(c) = TS_ENUM.captures(line) {
                    add("enum", &c[1]);
                } else if let Some(c) = TS_CONST.captures(line) {
                    add("const", &c[1]);
                }
            }
        }
    }
    out
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    let mut parts: Vec<&str> = lower.split('/').collect();
    let name = parts.pop().unwrap_or("");
    parts.iter().any(|p| matches!(*p, "tests" | "test" | "__tests__" | "spec" | "specs"))
        || name.starts_with("test_")
        || name.contains("_test.")
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// `a/b/../c` to `a/c`; None when it climbs out of the repository.
fn normalise(parts: impl Iterator<Item = String>) -> Option<String> {
    let mut out: Vec<String> = vec![];
    for p in parts {
        match p.as_str() {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            _ => out.push(p),
        }
    }
    Some(out.join("/"))
}

fn dir_of(path: &str) -> Vec<String> {
    let mut v: Vec<String> = path.split('/').map(String::from).collect();
    v.pop();
    v
}

fn first_existing(known: &HashSet<String>, candidates: Vec<Option<String>>) -> Option<String> {
    candidates.into_iter().flatten().find(|c| known.contains(c))
}

fn resolve_python(known: &HashSet<String>, from: &str, dots: usize, module: &str, names: &[String]) -> Vec<String> {
    let mut base: Vec<String> = if dots > 0 { dir_of(from) } else { vec![] };
    for _ in 1..dots {
        base.pop();
    }
    let modparts: Vec<String> = module.split('.').filter(|s| !s.is_empty()).map(String::from).collect();
    let join = |extra: &[String], tail: &str| normalise(base.iter().cloned().chain(modparts.iter().cloned()).chain(extra.iter().cloned())).map(|p| if p.is_empty() { tail.trim_start_matches('/').to_string() } else { format!("{p}{tail}") });
    let mut found = vec![];
    if !modparts.is_empty() {
        if let Some(f) = first_existing(known, vec![join(&[], ".py"), join(&[], "/__init__.py")]) {
            found.push(f);
        }
    }
    for n in names {
        if let Some(f) = first_existing(known, vec![join(std::slice::from_ref(n), ".py"), join(std::slice::from_ref(n), "/__init__.py")]) {
            found.push(f);
        }
    }
    found
}

fn resolve_relative(known: &HashSet<String>, from: &str, spec: &str) -> Option<String> {
    let target = normalise(dir_of(from).into_iter().chain(spec.split('/').map(String::from)))?;
    let mut c: Vec<Option<String>> = vec![Some(target.clone())];
    for e in ["ts", "tsx", "js", "jsx", "mjs"] {
        c.push(Some(format!("{target}.{e}")));
    }
    for e in ["ts", "tsx", "js", "jsx"] {
        c.push(Some(format!("{target}/index.{e}")));
    }
    first_existing(known, c)
}

fn resolve_rust_mod(known: &HashSet<String>, from: &str, name: &str) -> Option<String> {
    let mut d = dir_of(from);
    let file = from.rsplit('/').next().unwrap_or("");
    if !matches!(file, "lib.rs" | "main.rs" | "mod.rs") {
        d.push(file.trim_end_matches(".rs").to_string());
    }
    let j = |tail: &str| normalise(d.iter().cloned().chain([name.to_string()]).chain(tail.split('/').filter(|s| !s.is_empty()).map(String::from)));
    let a = j("").map(|p| format!("{p}.rs"));
    let b = j("mod.rs");
    first_existing(known, vec![a, b])
}

fn resolve_rust_use(known: &HashSet<String>, from: &str, path: &str) -> Option<String> {
    let segs: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segs.first() != Some(&"crate") {
        return None;
    }
    let parts: Vec<&str> = from.split('/').collect();
    let src = parts.iter().position(|p| *p == "src")?;
    let root: Vec<String> = parts[..=src].iter().map(|s| s.to_string()).collect();
    for take in (1..segs.len()).rev() {
        let t = normalise(root.iter().cloned().chain(segs[1..=take].iter().map(|s| s.to_string())))?;
        if let Some(f) = first_existing(known, vec![Some(format!("{t}.rs")), Some(format!("{t}/mod.rs"))]) {
            return Some(f);
        }
    }
    None
}

fn package_of(spec: &str) -> String {
    let s: Vec<&str> = spec.split('/').collect();
    if spec.starts_with('@') && s.len() > 1 { format!("{}/{}", s[0], s[1]) } else { s[0].to_string() }
}

fn parse(lang: &str, path: &str, text: &str, known: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let (mut imports, mut external): (Vec<String>, Vec<String>) = (vec![], vec![]);
    let mut add = |found: Option<String>, ext: &str| match found {
        Some(f) if f != path => {
            if !imports.contains(&f) {
                imports.push(f)
            }
        }
        Some(_) => {}
        None => {
            if !ext.is_empty() && !external.iter().any(|e| e == ext) {
                external.push(ext.to_string())
            }
        }
    };
    match lang {
        "python" => {
            for line in text.lines() {
                if let Some(c) = PY_FROM.captures(line) {
                    let names: Vec<String> = c[3].trim_matches(|ch| ch == '(' || ch == ')' || ch == ' ').split(',').map(|n| n.trim().split_whitespace().next().unwrap_or("").to_string()).filter(|n| !n.is_empty() && n != "*").collect();
                    let found = resolve_python(known, path, c[1].len(), &c[2], &names);
                    if found.is_empty() {
                        add(None, c[2].split('.').next().unwrap_or(""));
                    }
                    for f in found {
                        add(Some(f), "");
                    }
                } else if let Some(c) = PY_IMPORT.captures(line) {
                    for m in c[1].split(',') {
                        let m = m.trim().split_whitespace().next().unwrap_or("");
                        if m.is_empty() {
                            continue;
                        }
                        let found = resolve_python(known, path, 0, m, &[]);
                        if found.is_empty() {
                            add(None, m.split('.').next().unwrap_or(""));
                        }
                        for f in found {
                            add(Some(f), "");
                        }
                    }
                }
            }
        }
        "rust" => {
            for line in text.lines() {
                if let Some(c) = RS_MOD.captures(line) {
                    add(resolve_rust_mod(known, path, &c[1]), "");
                } else if let Some(c) = RS_USE.captures(line) {
                    let first = c[1].split("::").next().unwrap_or("");
                    match resolve_rust_use(known, path, &c[1]) {
                        Some(f) => add(Some(f), ""),
                        None if !matches!(first, "crate" | "self" | "super") => add(None, first),
                        None => {}
                    }
                }
            }
        }
        _ => {
            for c in TS_IMPORT.captures_iter(text) {
                let spec = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).map_or("", |m| m.as_str());
                if spec.starts_with('.') {
                    add(resolve_relative(known, path, spec), "");
                } else if !spec.is_empty() {
                    add(None, &package_of(spec));
                }
            }
        }
    }
    (imports, external)
}

fn read_text(p: &Path) -> Option<String> {
    if std::fs::metadata(p).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(std::fs::read(p).ok()?).ok().filter(|t| !t.contains('\0'))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                collect(root, &p, out);
            }
        } else if let (Some(lang), Some(text)) = (lang_of(&p), read_text(&p).filter(|_| lang_of(&p).is_some())) {
            if let Ok(rel) = p.strip_prefix(root) {
                out.push((rel.to_string_lossy().replace('\\', "/"), lang.to_string(), text));
            }
        }
    }
}

/// Scans `root` and returns its graph. Never fails: unreadable folders and files are skipped.
pub fn build(root: &Path) -> Graph {
    let mut found = vec![];
    collect(root, root, &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let known: HashSet<String> = found.iter().map(|f| f.0.clone()).collect();
    let files = found
        .iter()
        .map(|(path, lang, text)| {
            let (imports, external) = parse(lang, path, text, &known);
            FileNode { path: path.clone(), lang: lang.clone(), bytes: text.len() as u64, lines: text.lines().count(), is_test: is_test_path(path), symbols: symbols(lang, text), imports, external }
        })
        .collect();
    Graph { root: root.to_string_lossy().into_owned(), built_at: crate::db::now(), files }
}

fn clean(path: &str) -> String {
    path.trim().replace('\\', "/").trim_start_matches("./").to_string()
}

impl Graph {
    pub fn outline(&self, path: &str) -> Option<&FileNode> {
        let p = clean(path);
        self.files.iter().find(|f| f.path == p)
    }

    pub fn imports(&self, path: &str) -> Vec<String> {
        self.outline(path).map(|f| f.imports.clone()).unwrap_or_default()
    }

    pub fn importers(&self, path: &str) -> Vec<String> {
        let p = clean(path);
        self.files.iter().filter(|f| f.imports.contains(&p)).map(|f| f.path.clone()).collect()
    }

    /// Every file that would be affected by changing `path`: its importers, their importers, and so on.
    pub fn impact(&self, path: &str) -> Vec<String> {
        let start = clean(path);
        let (mut seen, mut queue): (BTreeSet<String>, VecDeque<String>) = (BTreeSet::new(), VecDeque::from([start.clone()]));
        while let Some(cur) = queue.pop_front() {
            for f in self.importers(&cur) {
                if f != start && seen.insert(f.clone()) {
                    queue.push_back(f);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Test files that import `path`, or are named after it (`test_x.py`, `x_test.rs`, `x.test.ts`).
    pub fn tests_for(&self, path: &str) -> Vec<String> {
        let p = clean(path);
        let stem = p.rsplit('/').next().unwrap_or("").split('.').next().unwrap_or("").to_lowercase();
        let named = |f: &FileNode| {
            let n = f.path.rsplit('/').next().unwrap_or("").to_lowercase();
            n.starts_with(&format!("test_{stem}.")) || n.starts_with(&format!("{stem}_test.")) || n.starts_with(&format!("{stem}.test.")) || n.starts_with(&format!("{stem}.spec."))
        };
        self.files.iter().filter(|f| f.is_test && f.path != p && (f.imports.contains(&p) || named(f))).map(|f| f.path.clone()).collect()
    }

    /// Symbols matching `query`: exact names first, then prefixes, then anything containing it.
    pub fn find(&self, query: &str) -> Vec<(String, Symbol)> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        let mut hits: Vec<(u8, String, Symbol)> = vec![];
        for f in &self.files {
            for s in &f.symbols {
                let n = s.name.to_lowercase();
                let rank = if n == q { 0 } else if n.starts_with(&q) { 1 } else if n.contains(&q) { 2 } else { continue };
                hits.push((rank, f.path.clone(), s.clone()));
            }
        }
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.line.cmp(&b.2.line)));
        hits.into_iter().take(MAX_HITS).map(|(_, p, s)| (p, s)).collect()
    }

    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "files": self.files.len(),
            "symbols": self.files.iter().map(|f| f.symbols.len()).sum::<usize>(),
            "import_links": self.files.iter().map(|f| f.imports.len()).sum::<usize>(),
            "test_files": self.files.iter().filter(|f| f.is_test).count(),
            "source_bytes": self.files.iter().map(|f| f.bytes).sum::<u64>(),
            "by_language": langs_json(self),
            "built_at": self.built_at,
        })
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(path, serde_json::to_vec(self).map_err(std::io::Error::other)?)
    }

    pub fn load(path: &Path) -> Option<Graph> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
}

fn langs_json(g: &Graph) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for f in &g.files {
        let n = m.get(&f.lang).and_then(|v| v.as_u64()).unwrap_or(0);
        m.insert(f.lang.clone(), (n + 1).into());
    }
    m.into()
}

fn list(title: &str, items: &[String]) -> String {
    if items.is_empty() { format!("{title}: none") } else { format!("{title} ({}):\n{}", items.len(), items.iter().map(|i| format!("  {i}")).collect::<Vec<_>>().join("\n")) }
}

fn render_outline(f: &FileNode, g: &Graph) -> String {
    let mut out = format!("{} ({}, {} lines{})", f.path, f.lang, f.lines, if f.is_test { ", test" } else { "" });
    for s in f.symbols.iter().take(MAX_OUTLINE) {
        out += &format!("\n  {} {} :{}", s.kind, s.name, s.line);
    }
    if f.symbols.len() > MAX_OUTLINE {
        out += &format!("\n  (+{} more symbols; use find with a name)", f.symbols.len() - MAX_OUTLINE);
    }
    if !f.imports.is_empty() {
        out += &format!("\nimports: {}", f.imports.join(", "));
    }
    if !f.external.is_empty() {
        out += &format!("\nexternal: {}", f.external.join(", "));
    }
    let by = g.importers(&f.path);
    if !by.is_empty() {
        out += &format!("\nimported by: {}", by.join(", "));
    }
    out
}

/// Answers one graph question as text for a model. `root` is the folder the file paths are relative to.
pub fn answer(g: &Graph, root: &Path, action: &str, arg: &str) -> Result<String, String> {
    let known = |p: &str| g.outline(p).is_some();
    let need_file = |p: &str| if known(p) { Ok(()) } else { Err(format!("{p} is not in the graph (rebuild it if the file is new)")) };
    match action {
        "find" => {
            if arg.trim().is_empty() {
                return Err("find needs a name to look for".into());
            }
            let hits = g.find(arg);
            Ok(if hits.is_empty() { format!("no symbol matches '{arg}'") } else { hits.iter().map(|(p, s)| format!("{p}:{} {} {}", s.line, s.kind, s.name)).collect::<Vec<_>>().join("\n") })
        }
        "outline" => {
            let rel = clean(arg);
            let fresh = lang_of(Path::new(&rel)).and_then(|lang| {
                let text = read_text(&root.join(&rel))?;
                let known_paths: HashSet<String> = g.files.iter().map(|f| f.path.clone()).collect();
                let (imports, external) = parse(lang, &rel, &text, &known_paths);
                Some(FileNode { path: rel.clone(), lang: lang.into(), bytes: text.len() as u64, lines: text.lines().count(), is_test: is_test_path(&rel), symbols: symbols(lang, &text), imports, external })
            });
            match fresh.or_else(|| g.outline(&rel).cloned()) {
                Some(f) => Ok(render_outline(&f, g)),
                None => Err(format!("{arg} is not a file in the graph")),
            }
        }
        "imports" => need_file(arg).map(|_| list("imports", &g.imports(arg))),
        "importers" => need_file(arg).map(|_| list("imported by", &g.importers(arg))),
        "impact" => need_file(arg).map(|_| list("changing it can affect", &g.impact(arg))),
        "tests_for" => need_file(arg).map(|_| list("tests", &g.tests_for(arg))),
        "stats" => {
            let s = g.stats();
            Ok(format!("{} files, {} symbols, {} import links, {} test files; languages: {}", s["files"], s["symbols"], s["import_links"], s["test_files"], s["by_language"]))
        }
        other => Err(format!("unknown graph action '{other}'; use one of: {ACTIONS}")),
    }
}

/// Where the graph of `repo` is kept: one file per repository under the data folder.
pub fn graph_path(repo: &Path) -> PathBuf {
    let key = hex::encode(&<sha2::Sha256 as sha2::Digest>::digest(repo.to_string_lossy().to_lowercase().as_bytes())[..6]);
    crate::config::data_dir().join("graph").join(format!("{key}.json"))
}

/// The saved graph of `repo`, built (and saved) when there is none or when `rebuild` is set.
pub fn ensure(repo: &Path, rebuild: bool) -> Graph {
    let p = graph_path(repo);
    if !rebuild {
        if let Some(g) = Graph::load(&p) {
            return g;
        }
    }
    let g = build(repo);
    let _ = g.save(&p);
    g
}

/// What the graph has saved so far: how many questions it answered and, for outlines, a rough count of the tokens
/// not spent reading whole files (4 bytes to a token). An estimate, not a bill.
pub fn usage() -> serde_json::Value {
    let v = crate::db::kv_get("graph_usage", serde_json::json!({})).unwrap_or_default();
    serde_json::json!({"queries": v["queries"].as_u64().unwrap_or(0), "tokens_saved": v["tokens_saved"].as_u64().unwrap_or(0)})
}

pub fn record(tokens_saved: u64) {
    let u = usage();
    let next = serde_json::json!({"queries": u["queries"].as_u64().unwrap_or(0) + 1, "tokens_saved": u["tokens_saved"].as_u64().unwrap_or(0) + tokens_saved});
    let _ = crate::db::kv_set("graph_usage", &next);
}
