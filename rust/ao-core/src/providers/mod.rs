//! Model providers (phase 3): neutral types, the two HTTP protocols, the Claude login provider, presets, pricing.
pub mod base;
pub mod claude_cli;
pub mod http;
pub mod presets;
pub mod pricing;

use crate::db::{now, DbError, DocRepo};
use base::{Provider, Usage};
use serde_json::{json, Value};

/// Provider types users can add. "external" is reserved (FR-9) and stays hidden until its feature flag is on.
pub const ADAPTERS: [&str; 3] = ["openai", "anthropic", "claude_cli"];
pub const EXTERNAL_TYPE: &str = "external";

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("external providers are not available yet")]
    External,
    #[error("unknown provider type {0}")]
    Unknown(String),
    #[error("{0}")]
    Secret(String),
}

pub fn allowed_types(external_enabled: bool) -> Vec<&'static str> {
    let mut t = ADAPTERS.to_vec();
    if external_enabled {
        t.push(EXTERNAL_TYPE);
    }
    t
}

/// Builds the adapter for a stored provider record (the key is decrypted here and nowhere else).
pub fn build(record: &Value) -> Result<Box<dyn Provider>, BuildError> {
    build_with(record, None)
}

/// Like `build`, with the HTTP transport to use (tests inject one; the app passes none).
pub fn build_with(record: &Value, transport: Option<base::Transport>) -> Result<Box<dyn Provider>, BuildError> {
    let kind = record["type"].as_str().unwrap_or("");
    if kind == EXTERNAL_TYPE {
        return Err(BuildError::External);
    }
    if !ADAPTERS.contains(&kind) {
        return Err(BuildError::Unknown(kind.to_string()));
    }
    let base_url = record["base_url"].as_str().unwrap_or("");
    let key = crate::secrets::decrypt(record["api_key_enc"].as_str().unwrap_or("")).map_err(|e| BuildError::Secret(e.to_string()))?;
    Ok(match kind {
        "openai" => Box::new(http::HttpProvider::new(http::Protocol::OpenAi, base_url, &key, transport.clone())),
        "anthropic" => Box::new(http::HttpProvider::new(http::Protocol::Anthropic, base_url, &key, transport.clone())),
        _ => Box::new(claude_cli::ClaudeCliProvider::new(base_url, None)),
    })
}

/// Records one model call (tokens and cost) for the budget views.
pub fn record_call(role: &str, provider_id: &str, model: &str, usage: Usage, run_id: Option<&str>) -> Result<Value, DbError> {
    let table = pricing::get_table()?;
    DocRepo::new("llm_calls")?.create(
        json!({
            "role": role, "provider_id": provider_id, "model": model, "run_id": run_id,
            "input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens,
            "cost_usd": pricing::cost(model, usage.input_tokens, usage.output_tokens, &table), "at": now(),
        }),
        None,
    )
}
