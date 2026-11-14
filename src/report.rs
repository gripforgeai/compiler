//! The issues of a build, said so that an AI corrects the project alone: where (file › scene › element › property),
//! what is wrong, what is expected. One line each in a terminal, or the JSON report (`--json`,
//! `gripforge.build-report/1`) the MCP and the CLI read.

use serde_json::{json, Value as Json};

/// Where an issue is, in words: `project.gripforge › scene level › element Hall/Door › property kits.core.light.x`.
pub fn place(i: &Json, project_name: &str) -> String {
    let file = i["file"].as_str();
    let inline = file.and_then(|f| f.strip_prefix('#'));
    let mut parts = vec![match file {
        Some(f) if inline.is_none() => f.to_string(),
        _ => project_name.to_string(),
    }];
    if let Some(scene) = i["scene"].as_str().or(inline) {
        parts.push(format!("scene {scene}"));
    }
    if let Some(n) = i["node"].as_str() {
        parts.push(format!("element {n}"));
    }
    if let Some(f) = i["field"].as_str() {
        parts.push(format!("property {f}"));
    }
    parts.join(" › ")
}

/// One line: `error <code> — <where>: <message>. Expected: <fix>`.
pub fn line(i: &Json, project_name: &str) -> String {
    let fix = i["fix"]
        .as_str()
        .map(|f| format!(" Expected: {f}"))
        .unwrap_or_default();
    let promoted = if i["promoted"] == true {
        " (a warning of the tolerant validator: a build accepts no guess)"
    } else {
        ""
    };
    let said = format!("{}{promoted}", i["message"].as_str().unwrap_or(""));
    let dot = if said.ends_with('.') { "" } else { "." };
    format!(
        "{} {} — {}: {said}{dot}{fix}",
        i["level"].as_str().unwrap_or("error"),
        i["code"].as_str().unwrap_or(""),
        place(i, project_name)
    )
}

/// The JSON report of a build.
pub fn report(r: &crate::build::BuildResult) -> Json {
    let name = r.project_name.as_deref().unwrap_or("project.gripforge");
    let errors = r.issues.iter().filter(|i| i["level"] == "error").count();
    let issues: Vec<Json> = r
        .issues
        .iter()
        .map(|i| {
            let mut x = i.clone();
            x["where"] = json!(place(i, name));
            x
        })
        .collect();
    json!({
        "schema": "gripforge.build-report/1",
        "ok": r.ok,
        "project": r.project.as_ref().map(|p| p.to_string_lossy()),
        "out": r.out.as_ref().map(|p| p.to_string_lossy()),
        "errors": errors,
        "warnings": r.issues.len() - errors,
        "issues": issues,
        "steps": r.steps,
        "artifacts": r.artifacts,
    })
}
