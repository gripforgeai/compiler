//! The verbs of a rule or a binding checked before game.engine compiles them: a verb game.engine cannot compile leaves
//! its whole rule out (its report says `script_invalid`, model/compile.rs), so the validator says it first, as an
//! error with what to write. Only the forms this crate can judge without game.engine are checked here; today the
//! `call` verb, whose one form is `{ "call": "<service>.<method>", "args": [...] }`.
//!
//! Used by: the project's scripts (`project.rs`, `data.scripts` and a scene's `scripts`) and the nodes' bindings
//! (`tree.rs`, `on[].do` / `on[].else`).

use crate::issue::Issue;
use serde_json::{json, Value as Json};

/// The keys of a verb that hold more verbs (a branch): checked too.
const NESTED: [&str; 4] = ["then", "else", "do", "actions"];

/// The issues of a list of verbs at `field` (`data.scripts[0].rules[1].then`).
pub fn check_list(list: Option<&Json>, field: &str) -> Vec<Issue> {
    let mut out = Vec::new();
    for (k, v) in list
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        check_verb(v, &format!("{field}[{k}]"), &mut out);
    }
    out
}

fn check_verb(v: &Json, field: &str, out: &mut Vec<Issue>) {
    let Some(o) = v.as_object() else { return };
    if let Some(call) = o.get("call") {
        let ok = call
            .as_str()
            .and_then(|p| p.rfind('.'))
            .is_some_and(|d| d > 0 && d + 1 < call.as_str().map_or(0, str::len));
        if !ok {
            out.push(
                Issue::error(
                    "script_invalid",
                    format!(
                        "call is {call}: game.engine compiles no such call and leaves the whole rule out"
                    ),
                    fix_of(call, o.get("args")),
                )
                .at("", &format!("{field}.call")),
            );
        }
    }
    for key in NESTED {
        for (k, inner) in o
            .get(key)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            check_verb(inner, &format!("{field}.{key}[{k}]"), out);
        }
    }
}

/// What to write instead: the object form `{ service, method, args }` turned into the one game.engine reads.
fn fix_of(call: &Json, args: Option<&Json>) -> String {
    let (service, method) = (
        call.get("service").and_then(Json::as_str),
        call.get("method").and_then(Json::as_str),
    );
    match (service, method) {
        (Some(s), Some(m)) => {
            let args = call.get("args").or(args).cloned().unwrap_or(json!([]));
            format!(
                "write {}",
                json!({ "call": format!("{s}.{m}"), "args": args })
            )
        }
        _ => "write { \"call\": \"<service>.<method>\", \"args\": [...] }".to_string(),
    }
}

/// The issues of the scripts of a project: `data.scripts[].rules[]` and the `scripts` of a scene written inline.
pub fn check_scripts(project: &Json) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut scripts = |list: Option<&Json>, at: &str| {
        for (i, s) in list
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            for (j, r) in s
                .get("rules")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                for key in ["then", "else"] {
                    out.extend(check_list(
                        r.get(key),
                        &format!("{at}[{i}].rules[{j}].{key}"),
                    ));
                }
            }
        }
    };
    scripts(project.pointer("/data/scripts"), "data.scripts");
    for (s, scene) in project
        .pointer("/data/scenes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        scripts(scene.get("scripts"), &format!("data.scenes[{s}].scripts"));
    }
    out
}
