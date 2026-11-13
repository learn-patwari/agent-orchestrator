//! Release helpers: SemVer bump suggestion and release-notes draft from conventional commit subjects.
//! Pure functions. The human edits the notes and decides; nothing here tags or publishes.
use regex::Regex;
use std::sync::LazyLock;

static SUBJECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?P<type>[a-z]+)(\([^)]*\))?(?P<bang>!)?:\s*(?P<text>.+)$").expect("valid pattern"));
static SEMVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v?(\d+)\.(\d+)\.(\d+)$").expect("valid pattern"));

struct Change {
    kind: String,
    text: String,
    breaking: bool,
}

fn parse(msg: &str) -> Option<Change> {
    let first = msg.trim().lines().next().unwrap_or("");
    let m = SUBJECT.captures(first)?;
    Some(Change {
        kind: m["type"].to_lowercase(),
        text: m["text"].trim().to_string(),
        breaking: m.name("bang").is_some() || msg.contains("BREAKING CHANGE"),
    })
}

pub fn suggest_bump(messages: &[&str]) -> Option<&'static str> {
    let parsed: Vec<Change> = messages.iter().filter_map(|m| parse(m)).collect();
    if parsed.is_empty() {
        None
    } else if parsed.iter().any(|p| p.breaking) {
        Some("major")
    } else if parsed.iter().any(|p| p.kind == "feat") {
        Some("minor")
    } else {
        Some("patch")
    }
}

pub fn next_version(current: Option<&str>, bump: &str) -> Result<String, String> {
    if !["major", "minor", "patch"].contains(&bump) {
        return Err(format!("unknown bump {bump}"));
    }
    let Some(cur) = current else {
        return Ok(match bump {
            "major" => "1.0.0",
            "minor" => "0.1.0",
            _ => "0.0.1",
        }
        .to_string());
    };
    let c = SEMVER.captures(cur).ok_or_else(|| format!("not a SemVer version: {cur}"))?;
    let n = |i: usize| c[i].parse::<u64>().map_err(|e| e.to_string());
    let (a, b, p) = (n(1)?, n(2)?, n(3)?);
    Ok(match bump {
        "major" => format!("{}.0.0", a + 1),
        "minor" => format!("{a}.{}.0", b + 1),
        _ => format!("{a}.{b}.{}", p + 1),
    })
}

pub fn generate_notes(version: &str, messages: &[&str]) -> String {
    let parsed: Vec<Change> = messages.iter().filter_map(|m| parse(m)).collect();
    let sections: [(&str, Vec<&Change>); 3] = [
        ("Breaking changes", parsed.iter().filter(|p| p.breaking).collect()),
        ("Features", parsed.iter().filter(|p| p.kind == "feat" && !p.breaking).collect()),
        ("Fixes", parsed.iter().filter(|p| p.kind == "fix" && !p.breaking).collect()),
    ];
    let mut out = vec![format!("# {version}"), String::new()];
    let mut shown = false;
    for (title, items) in sections {
        if !items.is_empty() {
            shown = true;
            out.push(format!("## {title}"));
            out.extend(items.iter().map(|p| format!("- {}", p.text)));
            out.push(String::new());
        }
    }
    if !shown {
        out.push("No user-facing changes.".to_string());
    }
    format!("{}\n", out.join("\n").trim_end())
}
