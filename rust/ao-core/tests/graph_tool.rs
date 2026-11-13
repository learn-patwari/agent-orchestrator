//! The `graph` tool agents use, the saved graph per repository, and the saved-tokens counter shown in the UI.
use ao_core::db;
use ao_core::graph::{ensure, graph_path, usage};
use ao_core::runtime::roles::default_tools;
use ao_core::runtime::tools::{ToolBox, Workspace};
use serde_json::json;
use std::path::Path;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn with_env<R>(f: impl FnOnce(&Path) -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    f(ws.path())
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn big_python() -> String {
    (0..300).map(|i| format!("def f{i}(x):\n    y = x + {i}\n    return y * 2 + len(str(y))\n\n")).collect()
}

#[test]
fn the_graph_tool_answers_from_the_workspace_and_is_in_every_roles_default_tools() {
    with_env(|root| {
        put(root, "app/db.py", "def get():\n    return 1\n");
        put(root, "app/main.py", "from app import db\n\ndef start():\n    return db.get()\n");
        let t = ToolBox::new(Workspace::new(root), None);
        let allowed = ["graph"];
        let found = t.call(&allowed, "graph", &json!({"action": "find", "query": "start"})).unwrap();
        assert!(found.contains("app/main.py"), "{found}");
        let by = t.call(&allowed, "graph", &json!({"action": "importers", "query": "app/db.py"})).unwrap();
        assert!(by.contains("app/main.py"), "{by}");
        assert!(t.call(&allowed, "graph", &json!({"action": "explode"})).unwrap_err().to_string().contains("unknown graph action"));
        assert!(t.call(&["read_file"], "graph", &json!({"action": "stats"})).is_err(), "a role without the tool is refused");
        for role in ["developer_1", "tester", "architect", "orchestrator", "someone_new"] {
            assert!(default_tools(role).contains(&"graph".to_string()), "{role}");
        }
        assert!(t.names().contains(&"graph"));
    });
}

#[test]
fn an_outline_counts_the_tokens_it_saved_and_other_questions_count_as_queries() {
    with_env(|root| {
        put(root, "big.py", &big_python());
        let t = ToolBox::new(Workspace::new(root), None);
        assert_eq!(usage()["queries"], 0);
        t.call(&["graph"], "graph", &json!({"action": "outline", "query": "big.py"})).unwrap();
        let u = usage();
        assert_eq!(u["queries"], 1);
        assert!(u["tokens_saved"].as_u64().unwrap() > 1000, "{u}"); // ~18k bytes of file read as a ~3k outline
        t.call(&["graph"], "graph", &json!({"action": "stats"})).unwrap();
        let after = usage();
        assert_eq!(after["queries"], 2);
        assert_eq!(after["tokens_saved"], u["tokens_saved"]); // only an outline replaces reading a file
    });
}

#[test]
fn secret_files_are_not_outlined() {
    with_env(|root| {
        put(root, "secret_keys.py", "TOKEN = 'abc'\n");
        let t = ToolBox::new(Workspace::new(root), None);
        let e = t.call(&["graph"], "graph", &json!({"action": "outline", "query": "secret_keys.py"})).unwrap_err().to_string();
        assert!(e.contains("secret"), "{e}");
    });
}

#[test]
fn the_graph_is_saved_per_repository_and_rebuilt_only_when_asked() {
    with_env(|root| {
        put(root, "a.py", "def a():\n    pass\n");
        let first = ensure(root, false);
        assert!(graph_path(root).is_file());
        assert_eq!(first.files.len(), 1);
        put(root, "b.py", "def b():\n    pass\n");
        assert_eq!(ensure(root, false).files.len(), 1, "the saved graph is reused");
        std::thread::sleep(std::time::Duration::from_millis(15));
        let again = ensure(root, true);
        assert_eq!(again.files.len(), 2);
        assert!(again.built_at > first.built_at);
        let other = tempfile::tempdir().unwrap();
        assert_ne!(graph_path(root), graph_path(other.path()));
    });
}
