//! Usage windows over the API: set while adding a provider, listed with next trigger and reported reset, checked by a tiny ping.
mod common;
use common::{env, ok_json, Client};
use serde_json::{json, Value};

async fn add(c: &Client, base: &str, window: Value) -> String {
    c.post("/api/providers", json!({"name": "p", "type": "openai", "base_url": base, "api_key": "sk-test-abcdefghijkl", "window": window})).await.1["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn the_window_is_set_while_adding_a_provider_and_listed_with_its_next_trigger() {
    let e = env();
    let c = e.c.authed().await;
    let w = json!({"enabled": true, "length_hours": 5, "anchor": 0, "ping": true});
    add(&c, "https://api.example.com/v1", w.clone()).await;
    add(&c, "https://openrouter.ai/api/v1", w).await; // no windows for OpenRouter
    let (s, rows) = c.get("/api/providers/windows").await;
    assert_eq!(s, 200);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["length_hours"], 5.0);
    assert!(rows[0]["next_trigger"].as_f64().unwrap() > ao_core::db::now());
    assert_eq!(rows[0]["verdict"], "unknown"); // nothing reported yet
}

#[tokio::test]
async fn a_bad_window_length_is_refused() {
    let e = env();
    let c = e.c.authed().await;
    let (s, _) = c.post("/api/providers", json!({"name": "p", "type": "openai", "base_url": "https://x/v1", "api_key": "k", "window": {"enabled": true, "length_hours": 0}})).await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn checking_now_pings_the_provider_and_records_the_reset_it_reports() {
    let e = env();
    let c = e.c.authed().await;
    let pid = add(&c, "https://windows-test.example/v1", json!({"enabled": true, "length_hours": 5, "anchor": 0, "ping": true})).await;
    let (s, r) = c.post(&format!("/api/providers/{pid}/window/check"), json!({})).await;
    assert_eq!(s, 200);
    assert_eq!(r["ok"], false, "no role uses the provider, so there is no model to ping with");
    c.put("/api/role-models/architect", json!({"provider_id": pid, "model": "m"})).await;
    e.c.upstream.set(|_| ok_json(200, json!({"choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}, "model": "m"})));
    let expected = ao_core::windows::rows(ao_core::db::now()).unwrap()[0]["next_trigger"].as_f64().unwrap();
    // the provider answers with its reset: the headers of the last answer from that host
    ao_core::providers::http::remember_headers("https://windows-test.example/v1/chat/completions", vec![("x-ratelimit-reset-tokens".into(), format!("{}s", (expected - ao_core::db::now()) as i64))]);
    let (_, r) = c.post(&format!("/api/providers/{pid}/window/check"), json!({})).await;
    assert_eq!(r["ok"], true, "{r}");
    let (_, rows) = c.get("/api/providers/windows").await;
    assert_eq!(rows[0]["verdict"], "match");
    assert!(rows[0]["last_ping"].as_f64().is_some());
}
