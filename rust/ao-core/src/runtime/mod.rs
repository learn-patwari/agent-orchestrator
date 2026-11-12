//! The agent runtime (phase 4): roles, skills, the sandboxed tool layer and the run loop.
pub mod roles;
pub mod runner;
pub mod skills;
pub mod tools;

use serde_json::Value;

/// Python's `str(value)` for the few YAML scalar types a frontmatter can hold.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        other => other.to_string(),
    }
}

/// Splits `---` frontmatter from the body, like Python's `text.split("---", 2)`; None when it is not there.
pub(crate) fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    if !text.starts_with("---") {
        return None;
    }
    let rest = &text[3..];
    let i = rest.find("---")?;
    Some((&rest[..i], &rest[i + 3..]))
}

/// Parses the YAML frontmatter into a JSON object (empty frontmatter is an empty object).
pub(crate) fn parse_yaml(fm: &str) -> Option<serde_json::Map<String, Value>> {
    match serde_yaml::from_str::<Value>(fm).ok()? {
        Value::Null => Some(Default::default()),
        Value::Object(m) => Some(m),
        _ => None,
    }
}
