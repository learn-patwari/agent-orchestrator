//! Cost accounting from a user-editable per-model price table (A-18, FR-8).
//! Prices are USD per 1M tokens. An unknown model gives no cost (never guessed).
use crate::db::{kv_get, kv_set, DbError};
use serde_json::{json, Value};

const KEY: &str = "price_table";

pub fn get_table() -> Result<Value, DbError> {
    kv_get(KEY, json!({}))
}

pub fn set_table(table: &Value) -> Result<(), String> {
    let obj = table.as_object().ok_or("the price table must be an object")?;
    for (model, p) in obj {
        if !(p.get("input").is_some() && p.get("output").is_some()) {
            return Err(format!("{model}: need input and output prices"));
        }
    }
    kv_set(KEY, table).map_err(|e| e.to_string())
}

pub fn cost(model: &str, input_tokens: u64, output_tokens: u64, table: &Value) -> Option<f64> {
    let p = table.get(model).filter(|p| p.as_object().is_some_and(|o| !o.is_empty()))?;
    let (i, o) = (p["input"].as_f64()?, p["output"].as_f64()?);
    let raw = (input_tokens as f64 * i + output_tokens as f64 * o) / 1_000_000.0;
    Some((raw * 1e8).round() / 1e8) // python: round(x, 8)
}
