//! A rule or a binding whose verb game.engine cannot compile (it leaves the whole rule out, `script_invalid` in its
//! report) is an error of the validator, with what to write: the `call` verb written as an object
//! `{ service, method, args }` instead of `{ "call": "<service>.<method>", "args": [...] }`.

use gripforge_scene_format::{project::validate_project, tree::resolve, Registry};
use serde_json::{json, Value as Json};

fn files<'a>(list: &'a [(&'a str, Json)]) -> impl Fn(&str) -> Option<Json> + 'a {
    move |p| list.iter().find(|(k, _)| *k == p).map(|(_, v)| v.clone())
}

fn project(call: Json) -> Json {
    json!({ "id": "p", "requires": ["game.engine"], "installed": [{ "id": "game.engine" }],
        "data": {
            "scenes": [{ "id": "s", "role": "level", "requires": ["game.engine"], "nodes": [] }],
            "scripts": [{ "id": "pause", "rules": [{
                "when": { "event": "core.input.action", "match": { "action": "pause" } },
                "then": [{ "emit": { "topic": "x" } }, call]
            }] }]
        } })
}

#[test]
fn a_call_written_as_an_object_is_an_error_with_its_fix() {
    let p =
        project(json!({ "call": { "service": "menu.main", "method": "togglePause", "args": [] } }));
    let issues = validate_project(&p, &|_| None, &Registry::base());
    let bad: Vec<_> = issues
        .iter()
        .filter(|i| i.code == "script_invalid")
        .collect();
    assert_eq!(bad.len(), 1, "{issues:?}");
    assert!(bad[0].is_error());
    assert_eq!(bad[0].field, "data.scripts[0].rules[0].then[1].call");
    assert!(
        bad[0].fix.contains(r#""call":"menu.main.togglePause""#),
        "{}",
        bad[0].fix
    );
}

#[test]
fn a_call_written_as_service_dot_method_is_valid() {
    let p = project(json!({ "call": "menu.main.togglePause", "args": [] }));
    let issues = validate_project(&p, &|_| None, &Registry::base());
    assert!(
        !issues.iter().any(|i| i.code == "script_invalid"),
        "{issues:?}"
    );
    for wrong in [json!("togglePause"), json!(".x"), json!("menu.")] {
        let p = project(json!({ "call": wrong }));
        let issues = validate_project(&p, &|_| None, &Registry::base());
        assert!(
            issues.iter().any(|i| i.code == "script_invalid"),
            "{wrong}: {issues:?}"
        );
    }
}

#[test]
fn a_binding_of_a_node_with_a_call_object_is_an_error() {
    let scene = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Door", "type": "trigger", "on": [{ "when": "enter",
            "do": [{ "call": { "service": "shop.prices", "method": "open" } }] }] }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    let bad: Vec<_> = t
        .issues
        .iter()
        .filter(|i| i.code == "script_invalid")
        .collect();
    assert_eq!(bad.len(), 1, "{:?}", t.issues);
    assert!(bad[0].is_error() && bad[0].node == "Door", "{:?}", bad[0]);
}
