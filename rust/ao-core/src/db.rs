//! SQLite persistence behind a small repository interface. Same file and tables as the Python backend:
//! every entity is a JSON document (`id`, `created_at`, `data`) in its own table, plus a `kv` table.
use crate::config::db_path;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TABLES: &[&str] = &[
    "users", "projects", "features", "tasks", "agents", "runs", "messages", "approvals", "assumptions", "budgets",
    "providers", "mcp_servers", "plugins", "llm_calls", "inbox", "audit", "notifications", "schedules", "srs_versions",
    "handoffs", "incidents", "ext_agents",
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("unknown table {0}")]
    UnknownTable(String),
    #[error("a document must be a JSON object")]
    NotObject,
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

type Result<T> = std::result::Result<T, DbError>;

fn connect() -> Result<Connection> {
    Ok(Connection::open(db_path())?)
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

pub fn init_db() -> Result<()> {
    let con = connect()?;
    for t in TABLES {
        con.execute(
            &format!("CREATE TABLE IF NOT EXISTS {t} (id TEXT PRIMARY KEY, created_at REAL NOT NULL, data TEXT NOT NULL)"),
            [],
        )?;
    }
    con.execute("CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT NOT NULL)", [])?;
    Ok(())
}

pub struct DocRepo {
    table: &'static str,
}

impl DocRepo {
    pub fn new(table: &str) -> Result<Self> {
        TABLES
            .iter()
            .find(|t| **t == table)
            .map(|t| DocRepo { table: t })
            .ok_or_else(|| DbError::UnknownTable(table.to_string()))
    }

    pub fn create(&self, doc: Value, id: Option<&str>) -> Result<Value> {
        let id = id.map(String::from).unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let Value::Object(mut m) = doc else { return Err(DbError::NotObject) };
        m.insert("id".into(), Value::String(id.clone()));
        let doc = Value::Object(m);
        connect()?.execute(
            &format!("INSERT INTO {} (id, created_at, data) VALUES (?1, ?2, ?3)", self.table),
            params![id, now(), serde_json::to_string(&doc)?],
        )?;
        Ok(doc)
    }

    pub fn get(&self, id: &str) -> Result<Option<Value>> {
        let row: Option<String> = connect()?
            .query_row(&format!("SELECT data FROM {} WHERE id=?1", self.table), params![id], |r| r.get(0))
            .optional()?;
        Ok(row.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    pub fn list(&self) -> Result<Vec<Value>> {
        let con = connect()?;
        let mut st = con.prepare(&format!("SELECT data FROM {} ORDER BY created_at, rowid", self.table))?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = vec![];
        for r in rows {
            out.push(serde_json::from_str(&r?)?);
        }
        Ok(out)
    }

    /// Merges the patch's top-level keys into the document; the id never changes.
    pub fn update(&self, id: &str, patch: Value) -> Result<Option<Value>> {
        let Some(Value::Object(mut cur)) = self.get(id)? else { return Ok(None) };
        if let Value::Object(p) = patch {
            for (k, v) in p {
                cur.insert(k, v);
            }
        }
        cur.insert("id".into(), Value::String(id.to_string()));
        let doc = Value::Object(cur);
        connect()?.execute(
            &format!("UPDATE {} SET data=?1 WHERE id=?2", self.table),
            params![serde_json::to_string(&doc)?, id],
        )?;
        Ok(Some(doc))
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let n = connect()?.execute(&format!("DELETE FROM {} WHERE id=?1", self.table), params![id])?;
        Ok(n > 0)
    }
}

pub fn kv_get(key: &str, default: Value) -> Result<Value> {
    let row: Option<String> =
        connect()?.query_row("SELECT v FROM kv WHERE k=?1", params![key], |r| r.get(0)).optional()?;
    Ok(match row {
        Some(s) => serde_json::from_str(&s)?,
        None => default,
    })
}

pub fn kv_set(key: &str, value: &Value) -> Result<()> {
    connect()?.execute(
        "INSERT INTO kv (k,v) VALUES (?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

#[allow(dead_code)]
fn _object(m: Map<String, Value>) -> Value {
    Value::Object(m)
}
