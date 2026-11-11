//! Single local user: password set on first run, signed session token. Hashes and tokens are
//! byte-compatible with the Python backend (scrypt n=2^14 r=8 p=1, HMAC-SHA256 over `exp.nonce`).
use crate::db::{DbError, DocRepo};
use crate::secrets;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde_json::json;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

pub const SESSION_TTL: u64 = 7 * 24 * 3600;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("password must be at least 8 characters")]
    TooShort,
    #[error("already set up")]
    AlreadySetUp,
    #[error(transparent)]
    Db(#[from] DbError),
}

fn users() -> Result<DocRepo, DbError> {
    DocRepo::new("users")
}

fn hash(password: &str, salt: &[u8]) -> String {
    let params = scrypt::Params::new(14, 8, 1, 64).expect("valid scrypt parameters");
    let mut out = [0u8; 64];
    scrypt::scrypt(password.as_bytes(), salt, &params, &mut out).expect("scrypt output length");
    hex::encode(out)
}

fn eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

pub fn has_user() -> Result<bool, DbError> {
    Ok(!users()?.list()?.is_empty())
}

pub fn set_password(password: &str) -> Result<(), AuthError> {
    if password.chars().count() < 8 {
        return Err(AuthError::TooShort);
    }
    if has_user()? {
        return Err(AuthError::AlreadySetUp);
    }
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    users()?.create(json!({"salt": hex::encode(salt), "hash": hash(password, &salt)}), None)?;
    Ok(())
}

pub fn verify_password(password: &str) -> Result<bool, AuthError> {
    let list = users()?.list()?;
    let Some(u) = list.first() else { return Ok(false) };
    let (Some(salt), Some(want)) = (u["salt"].as_str(), u["hash"].as_str()) else { return Ok(false) };
    let Ok(salt) = hex::decode(salt) else { return Ok(false) };
    Ok(eq(&hash(password, &salt), want))
}

fn sign(payload: &str) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(secrets::key().as_bytes()).expect("HMAC accepts any key length");
    m.update(payload.as_bytes());
    hex::encode(m.finalize().into_bytes())
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn make_token() -> String {
    let mut n = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut n);
    let p = format!("{}.{}", now_secs() + SESSION_TTL, hex::encode(n));
    format!("{p}.{}", sign(&p))
}

pub fn valid_token_at(token: Option<&str>, now: u64) -> bool {
    let Some(token) = token.filter(|t| !t.is_empty()) else { return false };
    let parts: Vec<&str> = token.split('.').collect();
    let [exp, nonce, sig] = parts[..] else { return false };
    let Ok(exp_n) = exp.parse::<u64>() else { return false };
    eq(sig, &sign(&format!("{exp}.{nonce}"))) && exp_n > now
}

pub fn valid_token(token: Option<&str>) -> bool {
    valid_token_at(token, now_secs())
}
