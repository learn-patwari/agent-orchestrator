//! Phase 1: storage, secrets and auth must read what the Python backend wrote (backend/tests/test_platform.py).
use ao_core::{auth, config, db, secrets};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn py() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixtures().join("py.json")).unwrap()).unwrap()
}

/// Runs `f` with a private data dir (and the fixture's secret key). Env vars are global, so tests take turns.
fn with_data<R>(copy_python_db: bool, f: impl FnOnce() -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", dir.path());
    std::env::set_var("AO_SECRET_KEY", py()["secret_key"].as_str().unwrap());
    if copy_python_db {
        std::fs::copy(fixtures().join("py.db"), dir.path().join("orchestrator.db")).unwrap();
    } else {
        db::init_db().unwrap();
    }
    f()
}

// ---------------------------------------------------------------- db

#[test]
fn reads_a_database_written_by_the_python_backend() {
    with_data(true, || {
        let f = db::DocRepo::new("features").unwrap().get("feat1").unwrap().unwrap();
        assert_eq!(f["title"], "from python");
        assert_eq!(f["n"], 1.5);
        assert_eq!(f["nested"], json!({"a": [1, 2]}));
        assert_eq!(db::kv_get("profiles", json!(null)).unwrap(), json!({"x": [1, 2, 3]}));
        assert_eq!(db::DocRepo::new("users").unwrap().list().unwrap().len(), 1);
    });
}

#[test]
fn init_db_is_idempotent_and_keeps_python_data() {
    with_data(true, || {
        db::init_db().unwrap();
        assert!(db::DocRepo::new("features").unwrap().get("feat1").unwrap().is_some());
        assert!(db::DocRepo::new("ext_agents").is_ok());
    });
}

#[test]
fn docrepo_create_get_list_update_delete() {
    with_data(false, || {
        let r = db::DocRepo::new("features").unwrap();
        let a = r.create(json!({"title": "a"}), None).unwrap();
        let id = a["id"].as_str().unwrap().to_string();
        assert_eq!(id.len(), 32); // uuid4 hex, like the Python ids
        let b = r.create(json!({"title": "b"}), Some("fixed")).unwrap();
        assert_eq!(b["id"], "fixed");
        assert_eq!(r.get(&id).unwrap().unwrap()["title"], "a");
        let titles: Vec<_> = r.list().unwrap().iter().map(|d| d["title"].clone()).collect();
        assert_eq!(titles, vec![json!("a"), json!("b")]);
        let u = r.update(&id, json!({"title": "z", "extra": 1, "id": "hijack"})).unwrap().unwrap();
        assert_eq!(u["title"], "z");
        assert_eq!(u["extra"], 1);
        assert_eq!(u["id"], json!(id)); // the id cannot be changed by a patch
        assert!(r.update("nope", json!({})).unwrap().is_none());
        assert!(r.delete(&id).unwrap());
        assert!(!r.delete(&id).unwrap());
        assert!(r.get(&id).unwrap().is_none());
    });
}

#[test]
fn only_known_tables_are_accepted() {
    assert!(db::DocRepo::new("features; DROP TABLE users").is_err());
    assert!(db::DocRepo::new("nope").is_err());
}

#[test]
fn kv_round_trips_and_overwrites() {
    with_data(false, || {
        assert_eq!(db::kv_get("k", json!("dflt")).unwrap(), json!("dflt"));
        db::kv_set("k", &json!({"a": 1})).unwrap();
        db::kv_set("k", &json!([1, 2])).unwrap();
        assert_eq!(db::kv_get("k", json!(null)).unwrap(), json!([1, 2]));
    });
}

#[test]
fn data_dir_comes_from_the_environment_and_is_created() {
    with_data(false, || {
        assert!(config::data_dir().is_dir());
        assert!(config::db_path().ends_with("orchestrator.db"));
    });
}

// ---------------------------------------------------------------- secrets

#[test]
fn decrypts_a_fernet_token_made_by_python() {
    with_data(false, || {
        let p = py();
        assert_eq!(secrets::decrypt(p["fernet_token"].as_str().unwrap()).unwrap(), p["fernet_plain"].as_str().unwrap());
    });
}

#[test]
fn encrypt_hides_the_secret_and_decrypts_back() {
    with_data(false, || {
        let enc = secrets::encrypt("sk-abcdefghijklmnopqrstuvwxyz");
        assert!(!enc.contains("sk-abc"));
        assert!(secrets::decrypt(&enc).unwrap().starts_with("sk-abc"));
    });
}

#[test]
fn a_key_file_is_created_when_no_env_key_is_set() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", dir.path());
    std::env::remove_var("AO_SECRET_KEY");
    let enc = secrets::encrypt("hello");
    assert!(dir.path().join("secret.key").is_file());
    assert_eq!(secrets::decrypt(&enc).unwrap(), "hello");
}

#[test]
fn redact_mask_and_secret_scan() {
    assert!(secrets::redact("key sk-abcdefghijklmnopqrstuvwxyz here").contains("[REDACTED]"));
    assert!(!secrets::redact("key sk-abcdefghijklmnopqrstuvwxyz here").contains("sk-abc"));
    assert!(secrets::looks_like_secret("my password: hunter2hunter2"));
    assert!(secrets::looks_like_secret("Authorization: Bearer abcdefghijklmnop1234"));
    assert!(secrets::looks_like_secret("token ghp_abcdefghijklmnopqrstuvwx"));
    assert!(!secrets::looks_like_secret("build a login page"));
    assert_eq!(secrets::mask("sk-abcdefghij"), "****ghij");
    assert_eq!(secrets::mask("short"), "****");
}

// ---------------------------------------------------------------- auth

#[test]
fn verifies_a_password_hashed_by_python_and_rejects_others() {
    with_data(true, || {
        assert!(auth::has_user().unwrap());
        assert!(auth::verify_password("correct-horse-1").unwrap());
        assert!(!auth::verify_password("wrong-pass-123").unwrap());
    });
}

#[test]
fn first_run_sets_one_password_of_at_least_eight_characters() {
    with_data(false, || {
        assert!(matches!(auth::set_password("short"), Err(auth::AuthError::TooShort)));
        assert!(!auth::has_user().unwrap());
        auth::set_password("correct-horse-1").unwrap();
        assert!(matches!(auth::set_password("another-pass-2"), Err(auth::AuthError::AlreadySetUp)));
        assert!(auth::verify_password("correct-horse-1").unwrap());
        assert!(!auth::verify_password("another-pass-2").unwrap());
    });
}

#[test]
fn verify_with_no_user_is_false() {
    with_data(false, || assert!(!auth::verify_password("anything-at-all").unwrap()));
}

#[test]
fn accepts_a_session_token_made_by_python_until_it_expires() {
    with_data(false, || {
        let t = py()["session_token"].as_str().unwrap().to_string();
        let exp: u64 = t.split('.').next().unwrap().parse().unwrap();
        assert!(auth::valid_token_at(Some(&t), exp - 1));
        assert!(!auth::valid_token_at(Some(&t), exp + 1));
    });
}

#[test]
fn tokens_are_signed_and_tamper_proof() {
    with_data(false, || {
        let t = auth::make_token();
        assert!(auth::valid_token(Some(&t)));
        let mut parts: Vec<String> = t.split('.').map(String::from).collect();
        parts[0] = (parts[0].parse::<u64>().unwrap() + 999).to_string(); // a longer life, same signature
        assert!(!auth::valid_token(Some(&parts.join("."))));
        assert!(!auth::valid_token(Some("garbage")));
        assert!(!auth::valid_token(Some("1.2.3.4")));
        assert!(!auth::valid_token(None));
        assert_eq!(t.split('.').count(), 3);
    });
}
