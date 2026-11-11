//! Registry of external agents, like a service registry.
//!
//! An external agent registers itself (name, endpoint, what it can do) with the registry key the human gave it, and
//! keeps sending heartbeats with its own secret token. It is `pending` until the human approves it; an agent with no
//! heartbeat inside TTL seconds is not live. Tasks go only to an approved, live agent and only while the
//! external-providers feature flag is on. What an external agent replies is untrusted data.
use crate::db::{kv_get, kv_set, now, DbError, DocRepo};
use crate::secrets::redact;
use crate::settings::external_enabled;
use base64::Engine;
use rand::RngCore;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Seconds without a heartbeat before an agent is no longer live.
pub const TTL: f64 = 60.0;
pub const MAX_REPLY: usize = 4000;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

pub type Result<T> = std::result::Result<T, RegistryError>;

fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(RegistryError::Msg(m.into()))
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn same(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Like Python's `secrets.token_urlsafe(24)`.
fn token() -> String {
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn slug(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let mut out = String::new();
    let mut pending_dash = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
            if pending_dash {
                out.push('-');
                pending_dash = false;
            }
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    if pending_dash {
        out.push('-');
    }
    out.trim_matches('-').to_string()
}

pub fn registry_key() -> std::result::Result<String, DbError> {
    if let Some(k) = kv_get("registry_key", Value::Null)?.as_str().filter(|k| !k.is_empty()) {
        return Ok(k.to_string());
    }
    let key = token();
    kv_set("registry_key", &json!(key))?;
    Ok(key)
}

pub fn key_ok(candidate: Option<&str>) -> bool {
    match (candidate.filter(|c| !c.is_empty()), registry_key()) {
        (Some(c), Ok(k)) => same(c, &k),
        _ => false,
    }
}

fn public(a: &Value, at: f64) -> Value {
    let live = a["status"] == "approved" && a["last_seen"].as_f64().is_some_and(|s| at - s <= TTL);
    let mut m = Map::new();
    for k in ["name", "endpoint", "capabilities", "description", "status", "last_seen"] {
        m.insert(k.into(), a.get(k).cloned().unwrap_or(Value::Null));
    }
    m.insert("live".into(), json!(live));
    Value::Object(m)
}

/// An http(s) URL with a host, like `urlparse` plus the scheme and netloc checks.
fn http_url(u: &str) -> bool {
    let Some((scheme, rest)) = u.split_once("://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    (scheme == "http" || scheme == "https") && !host.is_empty()
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub struct AgentRegistry;

impl AgentRegistry {
    fn repo(&self) -> Result<DocRepo> {
        Ok(DocRepo::new("ext_agents")?)
    }

    fn get(&self, name: &str) -> Result<Value> {
        self.repo()?.get(&slug(name))?.map_or_else(|| bad(format!("unknown agent {name}")), Ok)
    }

    pub fn register(&self, name: &str, endpoint: &str, capabilities: &[&str], description: &str, token_: Option<&str>) -> Result<Value> {
        let slug = slug(name);
        if slug.is_empty() {
            return bad("an agent needs a name");
        }
        if !http_url(endpoint) {
            return bad("the endpoint must be an http or https URL");
        }
        let caps: Vec<String> = capabilities.iter().take(20).map(|c| cut(c, 80)).collect();
        let fields = json!({"name": name.trim(), "endpoint": endpoint, "capabilities": caps, "description": cut(description, 300)});
        let repo = self.repo()?;
        if let Some(existing) = repo.get(&slug)? {
            let ok = token_.is_some_and(|t| !t.is_empty() && same(existing["token_hash"].as_str().unwrap_or(""), &hash(t)));
            if !ok {
                return bad("that name is taken; send the agent's token to update it");
            }
            let doc = repo.update(&slug, fields)?.unwrap_or(Value::Null);
            return Ok(json!({"agent": public(&doc, now()), "token": null}));
        }
        let secret = token();
        let mut doc = fields;
        if let Some(m) = doc.as_object_mut() {
            m.insert("status".into(), json!("pending"));
            m.insert("token_hash".into(), json!(hash(&secret)));
            m.insert("last_seen".into(), Value::Null);
        }
        let doc = repo.create(doc, Some(&slug))?;
        Ok(json!({"agent": public(&doc, now()), "token": secret}))
    }

    pub fn heartbeat(&self, name: &str, token_: &str, at: Option<f64>) -> Result<()> {
        let a = self.get(name)?;
        if !same(a["token_hash"].as_str().unwrap_or(""), &hash(token_)) {
            return bad("wrong token");
        }
        self.repo()?.update(a["id"].as_str().unwrap_or(""), json!({"last_seen": at.unwrap_or_else(now)}))?;
        Ok(())
    }

    fn set_status(&self, name: &str, status: &str) -> Result<Value> {
        let a = self.get(name)?;
        let doc = self.repo()?.update(a["id"].as_str().unwrap_or(""), json!({"status": status}))?.unwrap_or(Value::Null);
        Ok(public(&doc, now()))
    }

    pub fn approve(&self, name: &str) -> Result<Value> {
        self.set_status(name, "approved")
    }

    pub fn reject(&self, name: &str) -> Result<Value> {
        self.set_status(name, "rejected")
    }

    pub fn list(&self, at: Option<f64>) -> Result<Vec<Value>> {
        let at = at.unwrap_or_else(now);
        Ok(self.repo()?.list()?.iter().map(|a| public(a, at)).collect())
    }

    pub fn live(&self, at: Option<f64>) -> Result<Vec<Value>> {
        Ok(self.list(at)?.into_iter().filter(|a| a["live"] == true).collect())
    }

    pub fn send_task(&self, name: &str, task: &str, post: &dyn Fn(&str, &Value) -> std::result::Result<String, String>, at: Option<f64>) -> Result<Value> {
        let a = self.get(name)?;
        let who = a["name"].as_str().unwrap_or("").to_string();
        if a["status"] != "approved" {
            return bad(format!("{who} is not approved by the human"));
        }
        if !external_enabled()? {
            return bad("sending work to external agents is off: turn on the external providers flag");
        }
        if !self.live(at)?.iter().any(|x| x["name"] == a["name"]) {
            return bad(format!("{who} is not live (no recent heartbeat)"));
        }
        let reply = post(a["endpoint"].as_str().unwrap_or(""), &json!({"task": task})).map_err(RegistryError::Msg)?;
        Ok(json!({"agent": who, "reply": cut(&redact(&reply), MAX_REPLY)}))
    }
}
