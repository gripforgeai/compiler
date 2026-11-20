//! `sources` of a project (the Rust files of the game's code, declared by the project: crate::sources): accepted by
//! the tolerant validator, the same here as in the compiler, and never packed into the game the players load (the code
//! is compiled into game.main, or not run in the development mode).

use gripforge_scene_format::{project::pack, project::validate_project, Registry};
use serde_json::{json, Value as Json};

fn project(sources: Json) -> Json {
    json!({ "id": "arena", "sources": sources, "requires": ["game.engine"],
        "installed": [{ "id": "game.engine", "version": "1.0.0", "config": { "startScene": "level" } }],
        "data": { "scenes": [{ "id": "level", "role": "level", "requires": ["game.engine"],
            "nodes": [{ "name": "Spawn", "type": "entry" }] }] } })
}

fn codes(p: &Json) -> Vec<&'static str> {
    validate_project(p, &|_| None, &Registry::base())
        .iter()
        .map(|i| i.code)
        .collect()
}

#[test]
fn declared_sources_are_a_known_key_checked_and_not_packed() {
    let p = project(json!(["rules.rs", "code/weapons.rs"]));
    assert!(codes(&p).is_empty(), "{:?}", codes(&p));
    let game = pack(&p, &|_| None, &Registry::base(), &|_| None).expect("packed");
    assert!(
        game["project"].get("sources").is_none(),
        "{}",
        game["project"]
    );
    assert_eq!(codes(&project(json!(["../x.rs"]))), ["source_outside"]);
    assert_eq!(
        codes(&project(json!(["a.rs", "a.rs"]))),
        ["source_duplicate"]
    );
    assert_eq!(codes(&project(json!(["a.txt"]))), ["source_not_rust"]);
    assert_eq!(codes(&project(json!({ "a": 1 }))), ["sources_invalid"]);
}
