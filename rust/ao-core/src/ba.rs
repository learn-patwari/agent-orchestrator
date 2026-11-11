//! BA/Architect helpers: robust JSON parsing, lane planning, SRS assembly from the template.
//!
//! Lane rule: tasks that touch a common file (directly or through a chain of tasks) are put in one lane and stacked in
//! plan order, so parallel developers never write the same file. `wave` is the dependency depth: tasks in the same wave
//! with different lanes can run in parallel.
use crate::runtime::py_str;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct BAError(pub String);

fn err<T>(m: impl Into<String>) -> Result<T, BAError> {
    Err(BAError(m.into()))
}

static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^```(?:json)?\s*|\s*```$").expect("valid pattern"));

pub fn parse_json(text: &str) -> Result<Value, BAError> {
    let t = FENCE.replace_all(text.trim(), "").into_owned();
    let mut candidates = vec![t.clone()];
    if let (Some(i), Some(j)) = (t.find('{'), t.rfind('}')) {
        if i <= j {
            candidates.push(t[i..=j].to_string());
        }
    }
    for c in candidates {
        if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(&c) {
            return Ok(v);
        }
    }
    err("the model did not return valid JSON")
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().map(py_str).collect()
}

fn find(parent: &mut HashMap<String, String>, x: &str) -> String {
    let mut x = x.to_string();
    loop {
        let p = parent[&x].clone();
        if p == x {
            return x;
        }
        let gp = parent[&p].clone();
        parent.insert(x.clone(), gp.clone());
        x = gp;
    }
}

fn wave(tid: &str, tasks: &HashMap<String, Vec<String>>, depth: &mut HashMap<String, i64>, seen: &mut Vec<String>) -> Result<i64, BAError> {
    if seen.iter().any(|s| s == tid) {
        return err(format!("dependency cycle through {tid}"));
    }
    if let Some(d) = depth.get(tid) {
        return Ok(*d);
    }
    seen.push(tid.to_string());
    let mut best = -1;
    for d in &tasks[tid] {
        best = best.max(wave(d, tasks, depth, seen)?);
    }
    seen.pop();
    depth.insert(tid.to_string(), 1 + best);
    Ok(1 + best)
}

pub fn lanes(plan: &[Value]) -> Result<Vec<Value>, BAError> {
    let mut tasks: Vec<&Value> = vec![];
    for ph in plan {
        let Some(ts) = ph["tasks"].as_array() else { return err("a phase needs tasks") };
        tasks.extend(ts.iter());
    }
    let ids: Vec<String> = tasks.iter().map(|t| py_str(&t["id"])).collect();
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() != ids.len() {
        return err("duplicate task ids in plan");
    }
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    for (t, id) in tasks.iter().zip(&ids) {
        let d = strs(&t["depends_on"]);
        if let Some(missing) = d.iter().find(|x| !ids.contains(x)) {
            return err(format!("task {id} depends on unknown task {missing}"));
        }
        deps.insert(id.clone(), d);
    }
    let mut depth = HashMap::new();
    for id in &ids {
        wave(id, &deps, &mut depth, &mut vec![])?;
    }
    let mut parent: HashMap<String, String> = ids.iter().map(|i| (i.clone(), i.clone())).collect();
    let mut owner: HashMap<String, String> = HashMap::new();
    for (t, id) in tasks.iter().zip(&ids) {
        for f in strs(&t["files"]) {
            if let Some(o) = owner.get(&f).cloned() {
                let (a, b) = (find(&mut parent, id), find(&mut parent, &o));
                parent.insert(a, b);
            } else {
                owner.insert(f, id.clone());
            }
        }
    }
    let mut lane_name: HashMap<String, String> = HashMap::new();
    let mut last_in_lane: HashMap<String, String> = HashMap::new();
    let mut out = vec![];
    for ph in plan {
        let mut new_tasks = vec![];
        for t in ph["tasks"].as_array().into_iter().flatten() {
            let id = py_str(&t["id"]);
            let root = find(&mut parent, &id);
            let n = lane_name.len();
            let lane = lane_name.entry(root).or_insert_with(|| format!("L{}", n + 1)).clone();
            let mut m: Map<String, Value> = t.as_object().cloned().unwrap_or_default();
            m.insert("lane".into(), json!(lane));
            m.insert("wave".into(), json!(depth[&id]));
            m.insert("stacked_on".into(), last_in_lane.get(&lane).map_or(Value::Null, |s| json!(s)));
            new_tasks.push(Value::Object(m));
            last_in_lane.insert(lane, id);
        }
        let mut p: Map<String, Value> = ph.as_object().cloned().unwrap_or_default();
        p.insert("tasks".into(), Value::Array(new_tasks));
        out.push(Value::Object(p));
    }
    Ok(out)
}

fn s(v: &Value) -> String {
    py_str(v)
}

fn joined(v: &Value) -> String {
    strs(v).join(", ")
}

fn truthy_list(v: &Value) -> bool {
    v.as_array().is_some_and(|a| !a.is_empty())
}

pub fn build_srs(f: &Value) -> String {
    let a = &f["analysis"];
    let imp = &a["impact"];
    let qa: Vec<&Value> = f["qa"].as_array().into_iter().flatten().collect();
    let decisions: Vec<&&Value> = qa.iter().filter(|q| q["by"] == "human").collect();
    let assumed: Vec<&&Value> = qa.iter().filter(|q| q["by"] != "human").collect();
    let text = f["text"].as_str().unwrap_or("");
    let mut lines: Vec<String> = vec![
        format!("# SRS: {}", s(&f["title"])),
        String::new(),
        "Status: Draft   Owner (human): ______   Date: ______".into(),
        String::new(),
        "> Written by the architect in BA/SRS mode. **D-n** is a decision you gave. **[A]** is a default chosen where you did not answer; you may overrule it.".into(),
        String::new(),
        "## 1. Purpose and background".into(),
        a.get("restatement").map_or(text.to_string(), s),
        String::new(),
        a.get("why").map_or(String::new(), s),
        String::new(),
        format!("Original request: {text}"),
        String::new(),
        "## 2. Decisions and defaults".into(),
    ];
    if qa.is_empty() {
        lines.push("No open decisions were raised.".into());
    } else {
        lines.push("| ID | Decision |".into());
        lines.push("|---|---|".into());
        for (i, q) in decisions.iter().enumerate() {
            lines.push(format!("| D-{} | {}: {} |", i + 1, s(&q["question"]), s(&q["answer"])));
        }
        for (i, q) in assumed.iter().enumerate() {
            lines.push(format!("| A-{} | {}: {} **[A]** ({}) |", i + 1, s(&q["question"]), s(&q["answer"]), s(&q["by"]).replace('_', " ")));
        }
    }
    lines.push(String::new());
    lines.push("## 3. Impact on the existing system (architect view)".into());
    for (label, key) in [("Modules", "modules"), ("Data", "data"), ("Security", "security")] {
        lines.push(if truthy_list(&imp[key]) { format!("{label}: {}", joined(&imp[key])) } else { format!("{label}: none identified") });
    }
    lines.extend(["".into(), "## 9. Risks".into(), "| Risk | Impact | Mitigation |".into(), "|---|---|---|".into()]);
    if truthy_list(&imp["risks"]) {
        lines.extend(strs(&imp["risks"]).iter().map(|r| format!("| {r} | to assess | to be defined with the human |")));
    } else {
        lines.push("| none identified | | |".into());
    }
    lines.push(String::new());
    lines.push("## 10. Development plan".into());
    for ph in f["plan"].as_array().into_iter().flatten() {
        lines.extend([
            String::new(),
            format!("### {}: {}", s(&ph["id"]), s(&ph["name"])),
            "| Task | Files | Lane | Wave | Stacked on | Depends on |".into(),
            "|---|---|---|---|---|---|".into(),
        ]);
        for t in ph["tasks"].as_array().into_iter().flatten() {
            let get = |k: &str| t.get(k).map_or(String::new(), s);
            let stacked = if t["stacked_on"].is_null() || t["stacked_on"] == "" { "-".to_string() } else { s(&t["stacked_on"]) };
            let deps = if truthy_list(&t["depends_on"]) { joined(&t["depends_on"]) } else { "-".into() };
            lines.push(format!("| {}: {} | {} | {} | {} | {} | {} |", s(&t["id"]), get("title"), joined(&t["files"]), get("lane"), get("wave"), stacked, deps));
        }
    }
    lines.extend(["".into(), "## 11. Sign-off".into(), "| Item | Value |".into(), "|---|---|".into(), "| Approved by | |".into(), "| Date | |".into(), String::new()]);
    lines.join("\n")
}
