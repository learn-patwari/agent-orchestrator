//! The code knowledge graph: what a scan finds, what the queries answer, and that answers are smaller than the files.
use ao_core::graph::{answer, build, Graph};
use std::fs;
use std::path::Path;

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn sample() -> (tempfile::TempDir, Graph) {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    put(r, "app/main.py", "from app import db\nimport os\n\nclass App:\n    def run(self):\n        return db.get()\n\ndef start():\n    pass\n");
    put(r, "app/db.py", "def get():\n    return 1\n\ndef put(x):\n    return x\n");
    put(r, "tests/test_db.py", "from app.db import get\n\ndef test_get():\n    assert get() == 1\n");
    put(r, "web/a.ts", "import { b } from \"./b\";\nimport C from \"./c\";\nexport function a() { return b(); }\nexport const limit = 3;\nexport interface Row { id: string }\n");
    put(r, "web/b.ts", "export function b() { return 1; }\n");
    put(r, "web/c/index.tsx", "export default function C() { return null; }\n");
    put(r, "src/lib.rs", "pub mod util;\npub fn run() {}\n");
    put(r, "src/util.rs", "pub struct Thing;\nimpl Thing { pub fn new() -> Self { Thing } }\npub enum Kind { A }\npub trait Tool {}\n");
    put(r, "node_modules/x/index.js", "export function hidden() {}\n");
    put(r, "target/debug/junk.rs", "pub fn junk() {}\n");
    let g = build(r);
    (d, g)
}

#[test]
fn a_scan_skips_dependencies_and_build_output() {
    let (_d, g) = sample();
    let paths: Vec<&str> = g.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"app/db.py") && paths.contains(&"web/c/index.tsx"));
    assert!(!paths.iter().any(|p| p.starts_with("node_modules") || p.starts_with("target")), "{paths:?}");
}

#[test]
fn symbols_are_found_in_python_typescript_and_rust() {
    let (_d, g) = sample();
    let names = |p: &str| -> Vec<String> { g.outline(p).unwrap().symbols.iter().map(|s| format!("{} {}", s.kind, s.name)).collect() };
    assert_eq!(names("app/main.py"), ["class App", "function run", "function start"]);
    assert_eq!(names("web/a.ts"), ["function a", "const limit", "interface Row"]);
    assert_eq!(names("src/util.rs"), ["struct Thing", "impl Thing", "function new", "enum Kind", "trait Tool"]);
}

#[test]
fn imports_resolve_to_files_inside_the_repository() {
    let (_d, g) = sample();
    assert_eq!(g.imports("app/main.py"), ["app/db.py"]);
    assert_eq!(g.imports("tests/test_db.py"), ["app/db.py"]);
    assert_eq!(g.imports("web/a.ts"), ["web/b.ts", "web/c/index.tsx"]);
    assert_eq!(g.imports("src/lib.rs"), ["src/util.rs"]);
    assert!(g.outline("app/main.py").unwrap().external.contains(&"os".to_string()));
}

#[test]
fn importers_impact_and_tests_follow_the_imports() {
    let (_d, g) = sample();
    assert_eq!(g.importers("app/db.py"), ["app/main.py", "tests/test_db.py"]);
    assert_eq!(g.tests_for("app/db.py"), ["tests/test_db.py"]);
    assert!(g.tests_for("web/b.ts").is_empty());
    assert_eq!(g.impact("web/b.ts"), ["web/a.ts"]);
    assert_eq!(g.impact("app/db.py"), ["app/main.py", "tests/test_db.py"]);
}

#[test]
fn find_prefers_exact_names_and_is_not_case_sensitive() {
    let (_d, g) = sample();
    let hits = g.find("get");
    assert_eq!(hits[0].1.name, "get"); // the exact name first
    assert!(g.find("THING").iter().any(|(p, s)| p == "src/util.rs" && s.name == "Thing"));
    assert!(g.find("nothing_like_this").is_empty());
}

#[test]
fn answers_are_text_for_a_model_and_unknowns_are_errors() {
    let (d, g) = sample();
    let out = answer(&g, d.path(), "outline", "app/main.py").unwrap();
    assert!(out.contains("class App") && out.contains("app/db.py"), "{out}");
    assert!(answer(&g, d.path(), "find", "start").unwrap().contains("app/main.py"));
    assert!(answer(&g, d.path(), "importers", "app/db.py").unwrap().contains("tests/test_db.py"));
    assert!(answer(&g, d.path(), "stats", "").unwrap().contains("files"));
    assert!(answer(&g, d.path(), "outline", "nope.py").is_err());
    assert!(answer(&g, d.path(), "explode", "x").is_err());
    assert!(answer(&g, d.path(), "find", "").is_err());
}

#[test]
fn an_outline_is_read_fresh_from_disk_so_edits_show_up() {
    let (d, g) = sample();
    put(d.path(), "app/db.py", "def get():\n    return 1\n\ndef brand_new():\n    pass\n");
    assert!(answer(&g, d.path(), "outline", "app/db.py").unwrap().contains("brand_new"));
}

#[test]
fn an_outline_of_a_big_file_is_far_smaller_than_the_file() {
    let d = tempfile::tempdir().unwrap();
    let body: String = (0..400).map(|i| format!("def f{i}(x):\n    y = x + {i}\n    return y * 2 + len(str(y))\n\n")).collect();
    put(d.path(), "big.py", &body);
    let g = build(d.path());
    let out = answer(&g, d.path(), "outline", "big.py").unwrap();
    assert!(out.len() * 4 < body.len(), "outline {} bytes vs file {} bytes", out.len(), body.len());
}

#[test]
fn a_graph_round_trips_through_its_file() {
    let (d, g) = sample();
    let f = d.path().join("g.json");
    g.save(&f).unwrap();
    let back = Graph::load(&f).unwrap();
    assert_eq!(back.files.len(), g.files.len());
    assert_eq!(back.imports("web/a.ts"), g.imports("web/a.ts"));
    assert!(Graph::load(&d.path().join("missing.json")).is_none());
}

#[test]
fn huge_files_and_binary_files_are_left_out() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "ok.py", "def f():\n    pass\n");
    put(d.path(), "huge.py", &"x = 1\n".repeat(200_000)); // over the size cap
    fs::write(d.path().join("blob.py"), [0u8, 159, 146, 150, 0, 1]).unwrap(); // not text
    let g = build(d.path());
    let paths: Vec<&str> = g.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["ok.py"]);
}
