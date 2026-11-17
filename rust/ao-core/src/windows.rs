//! Usage windows per provider: a repeating schedule (default 5 h), the reset the provider reports, and whether they agree.
//! The schedule lives on the provider record (`window`); what was last seen lives in kv `provider_windows`.
use crate::db::{kv_get, kv_set, DbError, DocRepo};
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, DbError>;

/// A reset within this many seconds of the expected one counts as the same.
pub const TOLERANCE_S: f64 = 300.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub length_s: f64,
    pub anchor: f64,
    pub ping: bool,
    pub token_limit: Option<i64>,
}

impl Config {
    pub fn from_record(rec: &Value) -> Option<Config> {
        let w = rec.get("window")?;
        let hours = w["length_hours"].as_f64().filter(|h| *h > 0.0).unwrap_or(5.0);
        Some(Config {
            enabled: w["enabled"].as_bool().unwrap_or(false),
            length_s: hours * 3600.0,
            anchor: w["anchor"].as_f64().unwrap_or(0.0),
            ping: w["ping"].as_bool().unwrap_or(false),
            token_limit: w["token_limit"].as_i64().filter(|n| *n > 0),
        })
    }
}

/// OpenRouter has no usage windows, so it never takes part.
fn is_openrouter(rec: &Value) -> bool {
    rec["type"] == "openrouter" || rec["base_url"].as_str().is_some_and(|u| u.contains("openrouter.ai"))
}

/// The configured providers that take part: window switched on, not OpenRouter.
fn active() -> Result<Vec<(Value, Config)>> {
    Ok(DocRepo::new("providers")?
        .list()?
        .into_iter()
        .filter(|p| !is_openrouter(p))
        .filter_map(|p| Config::from_record(&p).filter(|c| c.enabled).map(|c| (p, c)))
        .collect())
}

/// The start of the window that begins next: the anchor itself if that is still ahead, else the next multiple of the length.
pub fn next_trigger(cfg: &Config, at: f64) -> f64 {
    if at < cfg.anchor {
        return cfg.anchor;
    }
    cfg.anchor + ((at - cfg.anchor) / cfg.length_s).floor() * cfg.length_s + cfg.length_s
}

/// The start of the window that is running (or the first one, if it has not begun).
fn current_start(cfg: &Config, at: f64) -> f64 {
    if at < cfg.anchor {
        return cfg.anchor;
    }
    cfg.anchor + ((at - cfg.anchor) / cfg.length_s).floor() * cfg.length_s
}

/// "match" when the provider's reset is within a few minutes of ours, "differ" when not, "unknown" when it reports none.
pub fn verdict(expected: f64, actual: Option<f64>) -> &'static str {
    match actual {
        None => "unknown",
        Some(a) if (a - expected).abs() <= TOLERANCE_S => "match",
        Some(_) => "differ",
    }
}

fn duration_seconds(s: &str) -> Option<f64> {
    // "6m0s", "1h30m", "6.5s", "120"
    let s = s.trim();
    if let Ok(n) = s.parse::<f64>() {
        return Some(n);
    }
    let (mut total, mut num, mut any) = (0.0, String::new(), false);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let n: f64 = num.parse().ok()?;
        num.clear();
        any = true;
        total += match c {
            'h' => n * 3600.0,
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                n / 1000.0
            }
            'm' => n * 60.0,
            's' => n,
            _ => return None,
        };
    }
    (any && num.is_empty()).then_some(total)
}

/// The reset moment (unix seconds) from response headers, when the provider sends one.
pub fn parse_reset(headers: &[(String, String)], at: f64) -> Option<f64> {
    const NAMES: [&str; 6] =
        ["anthropic-ratelimit-tokens-reset", "anthropic-ratelimit-requests-reset", "x-ratelimit-reset-tokens", "x-ratelimit-reset-requests", "retry-after", "x-ratelimit-reset"];
    for name in NAMES {
        let Some((_, v)) = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) else { continue };
        if let Ok(d) = chrono::DateTime::parse_from_rfc3339(v.trim()) {
            return Some(d.timestamp() as f64);
        }
        if let Some(s) = duration_seconds(v) {
            return Some(at + s);
        }
    }
    None
}

fn state() -> Result<Value> {
    kv_get("provider_windows", json!({}))
}

fn update_state(pid: &str, f: impl FnOnce(&mut Value)) -> Result<()> {
    let mut all = state()?;
    if !all[pid].is_object() {
        all[pid] = json!({});
    }
    f(&mut all[pid]);
    kv_set("provider_windows", &all)
}

/// Remembers the reset a provider reported at `at`, and with it whether it agrees with our schedule.
pub fn record_actual(pid: &str, actual: Option<f64>, at: f64) -> Result<()> {
    let Some(rec) = DocRepo::new("providers")?.get(pid)? else { return Ok(()) };
    let cfg = Config::from_record(&rec).filter(|c| c.enabled);
    let expected = cfg.as_ref().map(|c| next_trigger(c, at));
    let verdict = expected.map_or("unknown", |e| verdict(e, actual));
    update_state(pid, |s| {
        s["expected"] = json!(expected);
        s["actual_reset"] = json!(actual);
        s["checked_at"] = json!(at);
        s["verdict"] = json!(verdict);
    })
}

/// One row per provider that takes part, for the Settings page.
pub fn rows(at: f64) -> Result<Vec<Value>> {
    let st = state()?;
    Ok(active()?
        .into_iter()
        .map(|(p, c)| {
            let id = p["id"].as_str().unwrap_or("");
            let s = &st[id];
            let next = next_trigger(&c, at);
            let actual = s["actual_reset"].as_f64();
            json!({
                "provider_id": id, "name": p["name"], "length_hours": c.length_s / 3600.0, "anchor": c.anchor, "ping": c.ping, "token_limit": c.token_limit,
                "next_trigger": next, "actual_reset": actual, "checked_at": s["checked_at"], "last_ping": s["last_ping"],
                "verdict": verdict(next, actual),
            })
        })
        .collect())
}

/// Providers whose window has started since they were last pinged and that asked for a ping.
pub fn due_pings(at: f64) -> Result<Vec<String>> {
    let st = state()?;
    Ok(active()?
        .into_iter()
        .filter(|(_, c)| c.ping && at >= c.anchor)
        .filter(|(p, c)| st[p["id"].as_str().unwrap_or("")]["last_ping"].as_f64().map_or(true, |last| last < current_start(c, at)))
        .filter_map(|(p, _)| p["id"].as_str().map(String::from))
        .collect())
}

pub fn mark_pinged(pid: &str, at: f64) -> Result<()> {
    update_state(pid, |s| s["last_ping"] = json!(at))
}

/// Providers whose reported reset differs from our schedule, for the attention feed: (provider id, name, expected, actual).
pub fn mismatches() -> Result<Vec<(String, String, Option<f64>, Option<f64>)>> {
    let st = state()?;
    Ok(active()?
        .into_iter()
        .filter_map(|(p, _)| {
            let id = p["id"].as_str()?.to_string();
            (st[&id]["verdict"] == "differ").then(|| (id.clone(), p["name"].as_str().unwrap_or("").to_string(), st[&id]["expected"].as_f64(), st[&id]["actual_reset"].as_f64()))
        })
        .collect())
}
