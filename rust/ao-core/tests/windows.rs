//! Usage windows per provider: a repeating schedule, the reset the provider reports, and whether they agree.
use ao_core::db::{self, DocRepo};
use ao_core::windows::{self, Config};
use serde_json::json;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn with_env<R>(f: impl FnOnce() -> R) -> R {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", data.path());
    db::init_db().unwrap();
    f()
}

const H: f64 = 3600.0;

#[test]
fn the_next_trigger_is_the_next_window_start_after_now() {
    let cfg = Config { enabled: true, length_s: 5.0 * H, anchor: 1000.0, ping: true, token_limit: None };
    assert_eq!(windows::next_trigger(&cfg, 500.0), 1000.0); // before the first window
    assert_eq!(windows::next_trigger(&cfg, 1000.0 + 1.0), 1000.0 + 5.0 * H);
    assert_eq!(windows::next_trigger(&cfg, 1000.0 + 12.0 * H), 1000.0 + 15.0 * H); // several windows later
}

#[test]
fn a_reset_within_a_few_minutes_matches_and_a_bigger_gap_differs() {
    assert_eq!(windows::verdict(10_000.0, Some(10_000.0 + 120.0)), "match");
    assert_eq!(windows::verdict(10_000.0, Some(10_000.0 + 1800.0)), "differ");
    assert_eq!(windows::verdict(10_000.0, None), "unknown");
}

#[test]
fn the_reset_is_read_from_rate_limit_headers() {
    let h = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
    assert_eq!(windows::parse_reset(&h("anthropic-ratelimit-tokens-reset", "2026-01-01T00:00:00Z"), 0.0), Some(1_767_225_600.0));
    assert_eq!(windows::parse_reset(&h("x-ratelimit-reset-tokens", "1h30m"), 1000.0), Some(1000.0 + 5400.0));
    assert_eq!(windows::parse_reset(&h("x-ratelimit-reset-requests", "6.5s"), 1000.0), Some(1006.5));
    assert_eq!(windows::parse_reset(&h("retry-after", "120"), 1000.0), Some(1120.0));
    assert_eq!(windows::parse_reset(&h("content-type", "json"), 1000.0), None);
}

#[test]
fn rows_show_the_next_trigger_the_reported_reset_and_the_verdict() {
    with_env(|| {
        let repo = DocRepo::new("providers").unwrap();
        let on = repo.create(json!({"name": "A", "type": "anthropic", "window": {"enabled": true, "length_hours": 5, "anchor": 0, "ping": true}}), None).unwrap();
        repo.create(json!({"name": "OR", "type": "openai", "base_url": "https://openrouter.ai/api/v1", "window": {"enabled": true, "length_hours": 5, "anchor": 0}}), None).unwrap();
        repo.create(json!({"name": "Plain", "type": "openai"}), None).unwrap();
        let pid = on["id"].as_str().unwrap();
        let at = 7.0 * H;
        windows::record_actual(pid, Some(10.0 * H + 60.0), at).unwrap();
        let rows = windows::rows(at).unwrap();
        assert_eq!(rows.len(), 1, "OpenRouter and providers without a window are left out");
        assert_eq!(rows[0]["next_trigger"], 10.0 * H);
        assert_eq!(rows[0]["actual_reset"], 10.0 * H + 60.0);
        assert_eq!(rows[0]["verdict"], "match");
    });
}

#[test]
fn a_provider_is_pinged_once_per_window_and_only_when_asked() {
    with_env(|| {
        let repo = DocRepo::new("providers").unwrap();
        let a = repo.create(json!({"name": "A", "type": "anthropic", "window": {"enabled": true, "length_hours": 5, "anchor": 0, "ping": true}}), None).unwrap();
        repo.create(json!({"name": "B", "type": "openai", "window": {"enabled": true, "length_hours": 5, "anchor": 0, "ping": false}}), None).unwrap();
        let pid = a["id"].as_str().unwrap().to_string();
        assert_eq!(windows::due_pings(6.0 * H).unwrap(), vec![pid.clone()]); // the window starting at 5h has begun
        windows::mark_pinged(&pid, 6.0 * H).unwrap();
        assert!(windows::due_pings(7.0 * H).unwrap().is_empty());
        assert_eq!(windows::due_pings(10.5 * H).unwrap(), vec![pid]); // the next window began at 10h
    });
}

#[test]
fn a_differing_reset_waits_in_the_attention_feed_until_it_matches_again() {
    with_env(|| {
        let repo = DocRepo::new("providers").unwrap();
        let a = repo.create(json!({"name": "A", "type": "anthropic", "window": {"enabled": true, "length_hours": 5, "anchor": 0}}), None).unwrap();
        let pid = a["id"].as_str().unwrap();
        windows::record_actual(pid, Some(11.0 * H), 7.0 * H).unwrap(); // expected 10h, provider says 11h
        let items = ao_core::attention::items().unwrap();
        let w = items.iter().find(|i| i["id"] == format!("window:{pid}")).expect("mismatch notice");
        assert_eq!(w["link"], "/providers/");
        windows::record_actual(pid, Some(10.0 * H), 7.0 * H).unwrap();
        assert!(ao_core::attention::items().unwrap().iter().all(|i| i["id"] != format!("window:{pid}")));
    });
}
