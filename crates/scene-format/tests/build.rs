//! The tolerant validator (a wrong field warns and the scene still loads; only a required kit missing refuses), a
//! scene inline in the project beside one naming a carried sub-scene, the stable `id` of a node and a rotation written
//! as a quaternion.

use gripforge_scene_format::{
    expand::expand_project, issue::has_errors, lower::lower, ops::apply_command,
    project::validate_project, tree::resolve, Registry,
};
use serde_json::{json, Value as Json};

fn files<'a>(list: &'a [(&'a str, Json)]) -> impl Fn(&str) -> Option<Json> + 'a {
    move |p| list.iter().find(|(k, _)| *k == p).map(|(_, v)| v.clone())
}

fn shelf() -> Json {
    json!({ "nodes": [{ "name": "Book2", "id": "book", "type": "shape" }, { "name": "Lamp", "type": "light" }] })
}

/// One scene inline (annex), one in its file (level) instancing the shelf.
fn project() -> Json {
    json!({ "id": "arena", "requires": ["game.engine"], "installed": [{ "id": "game.engine", "config": { "startScene": "level" } }],
        "data": { "scenes": [
            { "id": "level", "role": "level", "file": "scenes/level.gfscene" },
            { "id": "annex", "role": "level", "requires": ["game.engine"], "nodes": [
                { "name": "Crate", "id": "annex_crate", "type": "model", "model": { "asset": "crate" } },
                { "name": "Rock", "type": "model", "model": { "asset": "boulder" } }
            ] }
        ] },
        "meta": { "author": "test" } })
}

fn level() -> Json {
    json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Spawn", "type": "entry" },
        { "name": "Shelf", "type": "scene", "scene": "scenes/shelf.gfscene", "position": [4, 0, 0],
          "overrides": { "book": { "visible": false } } }
    ] })
}

fn disk() -> Vec<(&'static str, Json)> {
    vec![
        ("scenes/level.gfscene", level()),
        ("scenes/shelf.gfscene", shelf()),
    ]
}

#[test]
fn a_file_with_invalid_fields_loads_with_the_defaults_and_warns() {
    let scene = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Box", "type": "shape", "position": "here", "shape": { "radius": "big" }, "visible": "yes",
          "kits": { "game.engine": "not an object" }, "on": [{ "when": "fly", "do": [] }] }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    assert!(!has_errors(&t.issues), "only warnings: {:?}", t.issues);
    let codes: Vec<&str> = t.issues.iter().map(|i| i.code).collect();
    for c in [
        "transform_invalid",
        "prop_invalid",
        "field_invalid",
        "section_invalid",
        "binding_invalid",
    ] {
        assert!(codes.contains(&c), "{c} reported: {codes:?}");
    }
    let l = lower("s", &t, &Registry::base());
    assert_eq!(
        l.content["instances"][0]["at"],
        json!([0, 0, 0]),
        "the node is placed, at the default position"
    );
    let p = json!({ "id": "p", "requires": ["game.engine"], "installed": [{ "id": "game.engine" }],
        "data": { "scenes": [{ "id": "s", "file": "s.gfscene" }] } });
    assert!(!has_errors(&validate_project(
        &p,
        &files(&list),
        &Registry::base()
    )));
}

#[test]
fn a_required_kit_missing_stays_the_one_error() {
    let mut p = project();
    p["data"]["scenes"][1]["requires"] = json!(["game.engine", "core.light"]);
    let list = disk();
    let issues = validate_project(&p, &files(&list), &Registry::base());
    let errors: Vec<&str> = issues
        .iter()
        .filter(|i| i.is_error())
        .map(|i| i.code)
        .collect();
    assert_eq!(errors, ["kit_required_missing"]);
}

#[test]
fn an_inline_scene_is_checked_and_given_to_game_engine_like_a_file() {
    let list = disk();
    let p = project();
    let issues = validate_project(&p, &files(&list), &Registry::base());
    assert!(!has_errors(&issues), "{issues:?}");
    let mut carried = p.clone();
    carried["data"]["scene_files"] =
        json!({ "scenes/level.gfscene": level(), "scenes/shelf.gfscene": shelf() });
    let e = expand_project(&carried, &Registry::base()).expect("expanded");
    let annex = &e["data"]["scenes"][1];
    assert_eq!(annex["instances"][0]["id"], json!("annex_crate"));
    assert!(
        annex.get("nodes").is_none(),
        "the inline file keys are lowered, not left"
    );
}

#[test]
fn a_node_is_found_by_its_path_or_by_its_id() {
    let scene = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Crate", "id": "crate_1", "type": "shape" },
        { "name": "Other", "id": "crate_1", "type": "shape" },
        { "name": "Door", "type": "trigger", "on": [{ "when": "enter", "do": [{ "hide": {}, "on": "crate_1" }] }] }
    ] });
    let list = [("s.gfscene", scene.clone())];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    assert!(t
        .issues
        .iter()
        .any(|i| i.code == "id_duplicate" && i.node == "Other"));
    let l = lower("s", &t, &Registry::base());
    let ids: Vec<&Json> = l.content["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["id"])
        .collect();
    assert_eq!(ids, [&json!("crate_1"), &json!("other")]);
    assert_eq!(
        l.scripts[0]["rules"][0]["then"][0]["on"],
        json!("crate_1"),
        "a binding names the node by its id"
    );
    let text = serde_json::to_string_pretty(&scene).unwrap();
    let none = |_: &str| None;
    let out = apply_command(
        &text,
        "s.gfscene",
        &json!({ "type": "update", "id": "crate_1", "node": { "visible": false } }),
        &none,
    )
    .expect("found by id");
    let v: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(v["nodes"][0]["visible"], json!(false));
}

#[test]
fn two_nodes_giving_the_same_game_engine_id_are_both_placed() {
    let scene = json!({ "nodes": [
        { "name": "A", "type": "shape", "kits": { "game.engine": { "id": "same" } } },
        { "name": "B", "type": "shape", "kits": { "game.engine": { "id": "same" } } }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    let l = lower("s", &t, &Registry::base());
    assert_eq!(l.content["instances"][1]["id"], json!("same_2"));
    assert!(l
        .issues
        .iter()
        .any(|i| i.code == "id_collision" && !i.is_error()));
}

#[test]
fn a_rotation_written_as_a_quaternion_reads_as_its_degrees() {
    let h = std::f64::consts::FRAC_PI_4; // half of 90°
    let scene = json!({ "nodes": [
        { "name": "Q", "rotation": { "x": 0, "y": h.sin(), "z": 0, "w": h.cos() } },
        { "name": "D", "rotation": [0, 90, 0] }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    assert!(t.issues.is_empty(), "{:?}", t.issues);
    let (q, d) = (t.nodes[0].world.rotation, t.nodes[1].world.rotation);
    for i in 0..4 {
        assert!((q[i] - d[i]).abs() < 1e-9, "{q:?} vs {d:?}");
    }
}

/// A project with core.physics installed and one scene whose crate carries a core.physics section.
fn physics_project(run: Option<Json>) -> Json {
    let mut p = json!({ "id": "arena", "requires": ["game.engine", "core.physics"],
        "installed": [{ "id": "game.engine", "config": { "startScene": "level" } }, { "id": "core.physics" }],
        "data": { "scenes": [{ "id": "level", "role": "level", "requires": ["game.engine", "core.physics"], "nodes": [
            { "name": "Crate", "type": "shape", "kits": { "core.physics": { "body": "dynamic" } } },
            { "name": "Wall", "type": "shape", "kits": { "core.physics": { "body": "static" } } }
        ] }] } });
    if let Some(run) = run {
        p["run"] = run;
    }
    p
}

#[test]
fn collision_nodes_without_a_running_physics_warn_with_the_line_to_write() {
    // The kit declares that its sections need the physics running (`needsRun`, docs/design/noyau-sans-noms-de-kits.md):
    // the scene format names no kit. Without that declaration, nothing to say.
    let none = |_: &str| None;
    let base = validate_project(&physics_project(None), &none, &Registry::base());
    assert!(base.iter().all(|i| i.code != "physics_not_run"), "{base:?}");
    let mut reg = Registry::base();
    reg.add_entries(
        "core.physics",
        &json!([{ "id": "core.physics.collider", "for": ["scene"], "at": "node", "needsRun": "physics",
            "create": "core.physics.elementCreate", "set": "core.physics.elementSet",
            "remove": "core.physics.elementRemove", "get": "core.physics.elementGet",
            "props": { "body": { "type": "enum", "choices": ["static", "dynamic"], "default": "static" } } }]),
    );
    for run in [
        None,
        Some(json!({ "seed": 7 })),
        Some(json!({ "physics": "none" })),
    ] {
        let issues = validate_project(&physics_project(run), &none, &reg);
        let found: Vec<_> = issues
            .iter()
            .filter(|i| i.code == "physics_not_run")
            .collect();
        assert_eq!(found.len(), 1, "one warning for the project: {issues:?}");
        assert!(
            !has_errors(&issues),
            "a warning, never an error: {issues:?}"
        );
        assert_eq!(found[0].node, "Crate");
        assert!(
            found[0].message.contains("and 1 more"),
            "{}",
            found[0].message
        );
        assert!(
            found[0].fix.contains("\"physics\": \"rapier\""),
            "{}",
            found[0].fix
        );
    }
    let running = validate_project(
        &physics_project(Some(json!({ "physics": "rapier" }))),
        &none,
        &reg,
    );
    assert!(
        running.iter().all(|i| i.code != "physics_not_run"),
        "{running:?}"
    );
    // Without a core.physics node there is nothing to say.
    let mut plain = project();
    plain["run"] = json!({ "physics": "none" });
    let issues = validate_project(&plain, &files(&disk()), &reg);
    assert!(issues.iter().all(|i| i.code != "physics_not_run"));
}

#[test]
fn the_schema_path_of_an_editor_is_a_known_key_of_the_project_and_of_a_scene_file() {
    let schema = "../core/scene-format/schema/gripforge.full.schema.json";
    let mut p = project();
    p["$schema"] = json!(schema);
    let mut list = disk();
    list[0].1["$schema"] = json!(schema);
    let issues = validate_project(&p, &files(&list), &Registry::base());
    let unknown: Vec<_> = issues
        .iter()
        .filter(|i| i.code == "field_unknown")
        .collect();
    assert!(unknown.is_empty(), "{unknown:?}");
}
