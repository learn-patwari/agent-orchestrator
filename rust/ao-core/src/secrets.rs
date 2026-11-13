//! Encrypt secrets (API keys) at rest and redact them from text. Fernet tokens and `secret.key` are
//! byte-compatible with the Python backend.
use crate::config::data_dir;
use fernet::Fernet;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("the secret key is not a valid Fernet key")]
    BadKey,
    #[error("could not decrypt: wrong key or damaged data")]
    Decrypt,
}

/// The key as text (a url-safe base64 Fernet key): AO_SECRET_KEY, else `<data>/secret.key` (created on first use).
pub fn key() -> String {
    if let Ok(k) = std::env::var("AO_SECRET_KEY") {
        if !k.is_empty() {
            return k;
        }
    }
    let f = data_dir().join("secret.key");
    if let Ok(k) = std::fs::read_to_string(&f) {
        return k.trim().to_string();
    }
    let k = Fernet::generate_key();
    let _ = std::fs::write(&f, &k);
    k
}

fn fernet() -> Result<Fernet, SecretError> {
    Fernet::new(&key()).ok_or(SecretError::BadKey)
}

pub fn encrypt(plain: &str) -> String {
    fernet().expect("a valid secret key").encrypt(plain.as_bytes())
}

pub fn decrypt(token: &str) -> Result<String, SecretError> {
    let bytes = fernet()?.decrypt(token).map_err(|_| SecretError::Decrypt)?;
    String::from_utf8(bytes).map_err(|_| SecretError::Decrypt)
}

pub fn mask(plain: &str) -> String {
    let n = plain.chars().count();
    if n > 8 {
        format!("****{}", plain.chars().skip(n - 4).collect::<String>())
    } else {
        "****".to_string()
    }
}

static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"sk-[A-Za-z0-9_\-]{16,}",
        r"gh[pousr]_[A-Za-z0-9]{20,}",
        r"(?i)bearer\s+[A-Za-z0-9._\-]{16,}",
        r"(?i)(api[_-]?key|password|secret|token)\s*[:=]\s*\S{6,}",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("valid pattern"))
    .collect()
});

pub fn looks_like_secret(text: &str) -> bool {
    PATTERNS.iter().any(|p| p.is_match(text))
}

pub fn redact(text: &str) -> String {
    let mut t = text.to_string();
    for p in PATTERNS.iter() {
        t = p.replace_all(&t, "[REDACTED]").into_owned();
    }
    t
}
