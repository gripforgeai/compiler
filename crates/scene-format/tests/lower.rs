//! A scene file lowered into game.engine's documents, the required kits, the sections, and the packing of a project.

use gripforge_scene_format::{
    expand::expand_project, lower::lower, project, requires, tree::resolve, Registry,
};
use serde_json::{json, Value as Json};

fn files<'a>(list: &'a [(&'a str, Json)]) -> impl Fn(&str) -> Option<Json> + 'a {
    move |p| list.iter().find(|(k, _)| *k == p).map(|(_, v)| v.clone())
}

fn level() -> Json {
    json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Spawn", "type": "entry", "rotation": [0, 90, 0] },
        { "name": "Hall", "position": [0, 0, -6], "children": [
            { "name": "Crate", "type": "model", "model": { "asset": "models/crate.glb" }, "position": [2, 0, 0],
              "on": [{ "when": "interact", "do": [{ "emit": { "topic": "room.crate_used" } }] }] },
            { "name": "Door", "type": "trigger", "trigger": { "shape": "box", "size": [2, 3, 1] }, "position": [0, 1.5, -4],
              "on": [{ "when": "enter", "do": [{ "hide": {}, "on": "../Crate" }, { "emit": { "topic": "room.door" } }] }] }
        ] },
        { "name": "Sun", "type": "light" }
    ] })
}

#[test]
fn a_scene_becomes_instances_entries_zones_and_the_script_of_its_bindings() {
    let list = [("level.gfscene", level())];
    let t = resolve("level.gfscene", &Registry::base(), &files(&list));
    let l = lower("level", &t, &Registry::base());
    assert!(l.issues.is_empty(), "{:?}", l.issues);
    assert_eq!(l.content["entries"][0]["id"], json!("spawn"));
    assert!(
        (l.content["entries"][0]["yaw"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs()
            < 1e-9
    );
    assert_eq!(
        l.content["instances"],
        json!([{ "id": "hall_crate", "archetype": "gfscene_hall_crate", "at": [2, 0, -6] }])
    );
    assert_eq!(
        l.archetypes,
        vec![
            json!({ "id": "gfscene_hall_crate", "kind": "prop", "asset": { "slot": "models/crate.glb" },
                "components": { "model.placement": { "mode": "native" } } })
        ]
    );
    assert_eq!(
        l.content["zones"],
        json!([{ "id": "hall_door", "shape": "box", "center": [0, 1.5, -10], "size": [2, 3, 1] }])
    );
    let rules = &l.scripts[0]["rules"];
    assert_eq!(l.scripts[0]["on"], json!("scene:level"));
    assert_eq!(rules[0]["when"], json!({ "interact": "hall_crate" }));
    assert_eq!(rules[1]["when"], json!({ "enter": "hall_door" }));
    assert_eq!(
        rules[1]["then"][0]["on"],
        json!("hall_crate"),
        "a relative path becomes the node's game.engine id"
    );
    assert!(
        l.content.get("lights").is_none(),
        "a light is dev.builder's, not game.engine's"
    );
}

#[test]
fn two_nodes_that_give_the_same_id_and_a_tilted_node_are_reported() {
    let scene = json!({ "nodes": [
        { "name": "A_b", "type": "shape" },
        { "name": "A", "children": [{ "name": "b", "type": "shape" }] },
        { "name": "Tilted", "type": "shape", "rotation": [30, 0, 0] }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    let codes: Vec<&str> = lower("s", &t, &Registry::base())
        .issues
        .iter()
        .map(|i| i.code)
        .collect();
    assert_eq!(codes, ["id_collision", "rotation_dropped"]);
}

#[test]
fn version_ranges() {
    for (v, r, ok) in [
        ("1.22.0", "^1", true),
        ("2.0.0", "^1", false),
        ("1.2.9", "~1.2", true),
        ("1.3.0", "~1.2", false),
        ("1.4.0", ">=1.4.0", true),
        ("1.3.9", ">=1.4", false),
        ("0.2.5", "^0.2", true),
        ("0.3.0", "^0.2", false),
        ("1.2.3", "1.2.3", true),
        ("9.9.9", "*", true),
    ] {
        assert_eq!(requires::satisfies(v, r), ok, "{v} in {r}");
    }
}

fn project_of(installed: Json) -> Json {
    json!({ "id": "arena", "requires": ["game.engine"], "installed": installed,
        "data": { "scenes": [{ "id": "level", "role": "level", "file": "level.gfscene" }] } })
}

#[test]
fn a_missing_required_kit_refuses_the_scene_and_says_what_to_install() {
    let mut scene = level();
    scene["requires"] = json!(["game.engine", { "id": "core.physics", "version": "^1" }]);
    let mut p = project_of(json!([{ "id": "game.engine", "version": "1.22.0" }]));
    p["data"]["scene_files"] = json!({ "level.gfscene": scene });
    let x = expand_project(&p, &Registry::base()).expect("expanded");
    let s = &x["data"]["scenes"][0];
    assert!(
        s.get("instances").is_none() && s.get("zones").is_none(),
        "nothing of a refused scene is placed: {s}"
    );
    let i = &x["data"]["scene_file_issues"][0];
    assert_eq!(
        (i["code"].as_str(), i["level"].as_str()),
        (Some("kit_required_missing"), Some("error"))
    );
    assert!(
        i["fix"]
            .as_str()
            .unwrap()
            .contains("install core.physics ^1"),
        "{i}"
    );
}

#[test]
fn a_section_is_checked_against_its_kit_and_the_requires_of_the_file() {
    let api = json!({ "id": "core.physics", "expose": [
        { "id": "core.physics.collider", "for": ["scene"], "at": "node", "nodeTypes": ["model", "shape"],
          "props": { "body": { "type": "enum", "choices": ["static", "dynamic"], "default": "static" } },
          "create": "core.physics.elementCreate", "remove": "core.physics.elementRemove" }
    ] });
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    let scene = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Crate", "type": "shape", "kits": { "core.physics": { "body": "flying", "mass": 3 } } }
    ] });
    let list = [("level.gfscene", scene)];
    let installed = [("game.engine".to_string(), None)];
    let issues = project::validate_scene(
        "level",
        "level.gfscene",
        &files(&list),
        &reg,
        Some(&installed),
    );
    let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
    assert_eq!(
        codes,
        [
            "section_invalid",
            "section_field_unknown",
            "section_not_required"
        ]
    );
    assert!(
        issues[0].fix.contains("one of static, dynamic"),
        "{}",
        issues[0].fix
    );
    assert_eq!(issues[0].field, "kits.core.physics.body");
    assert_eq!(issues[0].node, "Crate");
}

#[test]
fn a_section_of_the_scene_is_checked_against_the_elements_exposed_on_the_scene() {
    let api = json!({ "id": "core.physics", "expose": [
        { "id": "core.physics.world", "for": ["scene"], "at": "scene",
          "props": { "gravity": { "type": "f64x3", "default": [0, -9.81, 0] } },
          "create": "core.physics.elementCreate", "remove": "core.physics.elementRemove" },
        { "id": "core.physics.collider", "for": ["scene"], "at": "node", "nodeTypes": ["shape"],
          "props": { "size": { "type": "f64x3", "default": [1, 1, 1] } },
          "create": "core.physics.elementCreate", "remove": "core.physics.elementRemove" }
    ] });
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    let scene = json!({ "requires": ["game.engine", "core.physics"],
        "kits": { "core.physics": { "gravity": "down" } },
        "nodes": [
            { "name": "Floor", "type": "group", "kits": { "core.physics": { "size": [4, 1, 4] } } },
            { "name": "Wall", "type": "shape", "kits": { "core.physics": { "element": "world" } } }
        ] });
    let list = [("level.gfscene", scene)];
    let installed = [
        ("game.engine".to_string(), None),
        ("core.physics".to_string(), None),
    ];
    let issues = project::validate_scene(
        "level",
        "level.gfscene",
        &files(&list),
        &reg,
        Some(&installed),
    );
    let got: Vec<(&str, &str, &str)> = issues
        .iter()
        .map(|i| (i.code, i.node.as_str(), i.field.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("section_invalid", "", "kits.core.physics.gravity"),
            ("section_type_mismatch", "Floor", "kits.core.physics"),
            ("section_invalid", "Wall", "kits.core.physics.element"),
        ],
        "the scene's own section is read against the elements on the scene, a node's against those on a node"
    );
    let tree = gripforge_scene_format::tree::resolve("level.gfscene", &reg, &files(&list));
    assert!(
        tree.kits["core.physics"].get("gravity").is_none(),
        "the bad gravity is dropped: the default applies"
    );
}

#[test]
fn a_project_is_packed_with_its_scene_files_and_its_lock() {
    let shelf = json!({ "nodes": [{ "name": "Book", "type": "shape" }] });
    let mut scene = level();
    scene["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "Shelf", "type": "scene", "scene": "shelf.gfscene" }));
    let list = [("level.gfscene", scene), ("shelf.gfscene", shelf)];
    let p = project_of(json!([{ "id": "game.engine" }]));
    let version = |id: &str| (id == "game.engine").then(|| "1.22.0".to_string());
    let game = project::pack(&p, &files(&list), &Registry::base(), &version).expect("packed");
    let sf = game["project"]["data"]["scene_files"].as_object().unwrap();
    assert_eq!(
        sf.keys().collect::<Vec<_>>(),
        ["level.gfscene", "shelf.gfscene"]
    );
    assert_eq!(
        game["lock"]["kits"]["game.engine"]["version"],
        json!("1.22.0")
    );
    let none = |_: &str| None;
    let refused =
        project::pack(&p, &files(&list), &Registry::base(), &none).expect_err("no version");
    assert_eq!(
        refused.iter().find(|i| i.is_error()).map(|i| i.code),
        Some("kit_version_unknown")
    );
}

#[test]
fn the_project_level_of_a_kit_is_checked_against_its_config_schema() {
    let api = json!({ "id": "core.physics", "configSchema": { "type": "object", "additionalProperties": false,
        "properties": { "gravity": { "type": "array", "default": [0, -9.81, 0] }, "defaultMass": { "type": "number", "default": 1 } } } });
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    let project = json!({ "id": "t", "requires": ["game.engine", "core.physics"],
        "installed": [{ "id": "game.engine" }, { "id": "core.physics", "config": { "gravity": "down", "defaultMass": 2, "wind": 1 } }],
        "data": { "scenes": [{ "id": "a", "role": "level", "nodes": [] }] } });
    let list: [(&str, Json); 0] = [];
    let issues = project::validate_project(&project, &files(&list), &reg);
    let got: Vec<(&str, &str)> = issues.iter().map(|i| (i.code, i.field.as_str())).collect();
    assert!(
        got.contains(&("config_invalid", "installed.core.physics.config.gravity")),
        "{got:?}"
    );
    assert!(
        got.contains(&("config_field_unknown", "installed.core.physics.config.wind")),
        "{got:?}"
    );
    assert!(!got.iter().any(|g| g.1.ends_with("defaultMass")), "{got:?}");
}
