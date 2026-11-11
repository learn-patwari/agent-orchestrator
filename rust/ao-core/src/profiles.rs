//! Architecture profiles for token use.
//!
//! A profile sets how much an agent run may spend: steps, total tokens, output size per call, and how early the
//! context is compacted. Three ship (lean, balanced, thorough); the person can add their own and pick the active one.
//! The active profile gives every new run its limits. Nothing here changes a run already in flight.
use crate::db::{kv_get, kv_set, DbError};
use crate::runtime::runner::Limits;
use serde_json::{json, Map, Value};

pub const FIELDS: [&str; 5] = ["max_steps", "max_total_tokens", "max_output_tokens", "compact_at_chars", "keep_recent"];
const BUILTIN: [(&str, [i64; 5]); 3] = [
    ("lean", [15, 150_000, 1024, 20_000, 4]),
    ("balanced", [30, 500_000, 2048, 60_000, 6]),
    ("thorough", [60, 1_500_000, 4096, 120_000, 10]),
];
pub const DEFAULT: &str = "balanced";
const KEY: &str = "profiles";

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

pub type Result<T> = std::result::Result<T, ProfileError>;

fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(ProfileError::Msg(m.into()))
}

fn builtin(name: &str) -> Option<Map<String, Value>> {
    BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, v)| FIELDS.iter().zip(v).map(|(k, x)| (k.to_string(), json!(x))).collect())
}

pub struct Profiles;

impl Profiles {
    fn state(&self) -> Result<Map<String, Value>> {
        let v = kv_get(KEY, json!({"active": DEFAULT, "custom": {}}))?;
        Ok(v.as_object().cloned().unwrap_or_default())
    }

    fn names(&self) -> Result<Vec<String>> {
        let mut n: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
        for k in self.state()?.get("custom").and_then(|c| c.as_object()).into_iter().flat_map(|c| c.keys()) {
            if !n.contains(k) {
                n.push(k.clone());
            }
        }
        Ok(n)
    }

    pub fn get(&self, name: &str) -> Result<Value> {
        let p = match builtin(name) {
            Some(p) => Some(p),
            None => self.state()?.get("custom").and_then(|c| c.get(name)).and_then(|p| p.as_object()).cloned(),
        };
        let Some(p) = p else { return bad(format!("unknown profile {name}")) };
        let mut out = Map::new();
        out.insert("name".into(), json!(name));
        let tokens = p["max_total_tokens"].clone();
        out.extend(p);
        out.insert("builtin".into(), json!(builtin(name).is_some()));
        out.insert("est_run_tokens".into(), tokens);
        Ok(Value::Object(out))
    }

    pub fn list(&self) -> Result<Vec<Value>> {
        self.names()?.iter().map(|n| self.get(n)).collect()
    }

    pub fn active(&self) -> Result<Value> {
        let st = self.state()?;
        let name = st.get("active").and_then(|a| a.as_str()).unwrap_or(DEFAULT).to_string();
        if self.names()?.contains(&name) {
            self.get(&name)
        } else {
            self.get(DEFAULT)
        }
    }

    pub fn set_active(&self, name: &str) -> Result<Value> {
        self.get(name)?;
        let mut s = self.state()?;
        s.insert("active".into(), json!(name));
        kv_set(KEY, &Value::Object(s))?;
        self.active()
    }

    pub fn save(&self, name: &str, values: &Value) -> Result<Value> {
        let name = name.trim();
        if name.is_empty() {
            return bad("a profile needs a name");
        }
        if builtin(name).is_some() {
            return bad(format!("{name} is a built-in profile; save your own under another name"));
        }
        let given: Vec<&String> = values.as_object().map(|o| o.keys().collect()).unwrap_or_default();
        if given.len() != FIELDS.len() || !FIELDS.iter().all(|k| values.get(k).is_some()) {
            return bad(format!("a profile sets exactly: {}", FIELDS.join(", ")));
        }
        for k in FIELDS {
            if values[k].as_i64().is_none_or(|n| n <= 0) {
                return bad(format!("{k} must be a whole number above zero"));
            }
        }
        let mut s = self.state()?;
        let mut custom = s.get("custom").and_then(|c| c.as_object()).cloned().unwrap_or_default();
        custom.insert(name.into(), Value::Object(FIELDS.iter().map(|k| (k.to_string(), values[k].clone())).collect()));
        s.insert("custom".into(), Value::Object(custom));
        kv_set(KEY, &Value::Object(s))?;
        self.get(name)
    }

    pub fn delete(&self, name: &str) -> Result<Value> {
        if builtin(name).is_some() {
            return bad(format!("{name} is a built-in profile and cannot be deleted"));
        }
        let s = self.state()?;
        let mut custom = s.get("custom").and_then(|c| c.as_object()).cloned().unwrap_or_default();
        if custom.remove(name).is_none() {
            return bad(format!("unknown profile {name}"));
        }
        let active = s.get("active").and_then(|a| a.as_str()).unwrap_or(DEFAULT);
        kv_set(KEY, &json!({"active": if active == name { DEFAULT } else { active }, "custom": custom}))?;
        self.active()
    }

    pub fn limits(&self) -> Result<Limits> {
        let a = self.active()?;
        let n = |k: &str| a[k].as_i64().unwrap_or(0);
        Ok(Limits {
            max_steps: n("max_steps"),
            max_total_tokens: n("max_total_tokens"),
            max_output_tokens: n("max_output_tokens") as u32,
            compact_at_chars: n("compact_at_chars") as usize,
            keep_recent: n("keep_recent") as usize,
            ..Limits::default()
        })
    }
}
