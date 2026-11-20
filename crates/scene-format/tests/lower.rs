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
fn a_scene_becomes_instances_entries_the_zones_of_world_zones_and_the_script_of_its_bindings() {
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
        l.content["data"]["zones"],
        json!([{ "id": "hall_door", "shape": "box", "center": [0, 1.5, -10], "size": [2, 3, 1] }])
    );
    let rules = &l.scripts[0]["rules"];
    assert_eq!(l.scripts[0]["on"], json!("scene:level"));
    assert_eq!(rules[0]["when"], json!({ "interact": "hall_crate" }));
    assert_eq!(
        rules[1]["when"],
        json!({ "event": "world.zones.entered", "match": { "zone": "hall_door" } })
    );
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
        s.get("instances").is_none() && s.get("data").is_none_or(|d| d.get("zones").is_none()),
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

#[test]
fn the_common_key_debugger_of_a_kit_config_is_accepted_by_a_closed_schema_as_game_engine_accepts_it(
) {
    let api = json!({ "id": "core.physics", "configSchema": { "type": "object", "additionalProperties": false,
        "properties": { "defaultMass": { "type": "number", "default": 1 } } } });
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    let project = json!({ "id": "t", "requires": ["game.engine", "core.physics"],
        "installed": [{ "id": "game.engine" }, { "id": "core.physics", "config": { "debugger": { "icon": "cube" } } }],
        "data": { "scenes": [{ "id": "a", "role": "level", "nodes": [] }] } });
    let list: [(&str, Json); 0] = [];
    let issues = project::validate_project(&project, &files(&list), &reg);
    assert!(
        !issues.iter().any(|i| i.field.ends_with("config.debugger")),
        "{issues:?}"
    );
}

#[test]
fn the_spec_of_an_element_names_the_type_asset_archetype_and_entity_of_its_node() {
    use gripforge_scene_format::place;
    let api = json!({ "id": "core.physics", "expose": [
        { "id": "core.physics.body", "for": ["scene"], "at": "node",
          "props": { "kind": { "type": "string", "default": "" } },
          "create": "core.physics.elementCreate", "remove": "core.physics.elementRemove" }
    ] });
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    let scene = json!({ "requires": ["game.engine", "core.physics"], "nodes": [
        { "name": "Hall", "children": [
            { "name": "Door", "type": "model", "model": { "asset": "models/door.glb" },
              "kits": { "game.engine": { "archetype": "door" }, "core.physics": { "kind": "door" } } },
            { "name": "Wall", "id": "north_wall", "type": "shape", "kits": { "core.physics": {} } }
        ] }
    ] });
    let list = [("level.gfscene", scene)];
    let tree = resolve("level.gfscene", &reg, &files(&list));
    let specs: Vec<Json> = place::wanted(&tree, "level.gfscene")
        .iter()
        .filter_map(|w| {
            let (el, props, _) = place::resolve_section(&reg, w)?;
            let node = tree.nodes.iter().find(|n| n.path == w.node);
            Some(place::element_spec(
                "level",
                w,
                &el,
                &props,
                node,
                None,
                Json::Null,
            ))
        })
        .collect();
    let got: Vec<(Json, Json, Json, Json)> = specs
        .iter()
        .map(|s| {
            (
                s["type"].clone(),
                s["asset"].clone(),
                s["archetype"].clone(),
                s["entity"].clone(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                json!("model"),
                json!("models/door.glb"),
                json!("door"),
                json!("hall_door")
            ),
            (json!("shape"), Json::Null, Json::Null, json!("north_wall")),
        ],
        "a kit gets what it needs to give one rule to a whole kind of element"
    );
}

#[test]
fn collision_and_animation_nodes_are_children_of_the_tree_read_by_their_kit() {
    use gripforge_scene_format::place;
    let api = json!({ "id": "core.physics", "expose": [
        { "id": "core.physics.collision", "for": ["scene"], "at": "node", "nodeType": "collision", "nodeTypes": ["collision"],
          "props": { "shape": { "type": "enum", "choices": ["box", "sphere"], "default": "box" },
                     "type": { "type": "enum", "choices": ["static", "dynamic"], "default": "static" },
                     "size": { "type": "f64x3", "default": [1, 1, 1] } },
          "create": "core.physics.elementCreate", "remove": "core.physics.elementRemove" }
    ] });
    let anim = json!({ "id": "core.animation", "expose": [
        { "id": "core.animation.clips", "for": ["scene"], "at": "node", "nodeType": "animation", "nodeTypes": ["animation"],
          "props": { "loop": { "type": "string", "default": "" } },
          "create": "core.animation.elementCreate", "remove": "core.animation.elementRemove" }
    ] });
    let mut reg = Registry::base();
    assert!(
        !reg.node_types.contains_key("collision"),
        "not a base type: core.physics declares it"
    );
    reg.add_kit_api(&api);
    reg.add_kit_api(&anim);
    // A group with no model nor entity (an invisible wall), moved and turned: its two shapes follow it.
    let scene = json!({ "requires": ["core.physics", "core.animation"], "nodes": [
        { "name": "Wall", "type": "group", "position": [10, 0, 0], "rotation": [0, 90, 0], "children": [
            { "name": "Low", "type": "collision", "position": [0, 0.5, 0], "collision": { "size": [4, 1, 0.2] } },
            { "name": "High", "type": "collision", "position": [0, 2, 0], "collision": { "shape": "sphere", "type": "dynamic", "speed": 3 } }
        ] },
        { "name": "Mill", "type": "model", "model": { "asset": "models/mill.glb" }, "children": [
            { "name": "Turn", "type": "animation", "animation": { "loop": "Spin" } }
        ] }
    ] });
    let list = [("level.gfscene", scene)];
    let tree = resolve("level.gfscene", &reg, &files(&list));
    let issues: Vec<(&str, &str)> = tree
        .issues
        .iter()
        .map(|i| (i.code, i.field.as_str()))
        .collect();
    assert_eq!(
        issues,
        [("section_field_unknown", "kits.core.physics.speed")],
        "a typed node is checked against its kit's element, not as a node type of its own"
    );
    let found: Vec<(String, String, Json, Json, Json)> = place::wanted(&tree, "level.gfscene")
        .iter()
        .filter_map(|w| {
            let (el, props, _) = place::resolve_section(&reg, w)?;
            let node = tree.nodes.iter().find(|n| n.path == w.node)?;
            let up = tree.nodes.iter().find(|p| p.path == node.parent);
            let spec = place::element_spec("level", w, &el, &props, Some(node), up, Json::Null);
            Some((
                w.kit.clone(),
                el.name,
                spec["parentNode"].clone(),
                spec["parentType"].clone(),
                spec["props"].clone(),
            ))
        })
        .collect();
    assert_eq!(found.len(), 3, "two shapes and one animation: {found:?}");
    assert_eq!(
        (found[0].0.as_str(), found[0].1.as_str()),
        ("core.physics", "collision")
    );
    assert_eq!(
        (found[0].2.clone(), found[0].3.clone()),
        (json!("Wall"), json!("group"))
    );
    assert_eq!(found[0].4["size"], json!([4, 1, 0.2]));
    assert_eq!(
        (found[1].4["shape"].clone(), found[1].4["type"].clone()),
        (json!("sphere"), json!("dynamic"))
    );
    assert_eq!(
        (found[2].0.as_str(), found[2].1.as_str()),
        ("core.animation", "clips")
    );
    assert_eq!(
        (found[2].2.clone(), found[2].4["loop"].clone()),
        (json!("Mill"), json!("Spin"))
    );
    let mill = tree.nodes.iter().find(|n| n.path == "Mill/Turn").unwrap();
    let w = place::wanted(&tree, "level.gfscene")
        .into_iter()
        .find(|w| w.node == "Mill/Turn")
        .unwrap();
    let (el, props, _) = place::resolve_section(&reg, &w).unwrap();
    let up = tree.nodes.iter().find(|p| p.path == "Mill");
    let spec = place::element_spec("level", &w, &el, &props, Some(mill), up, Json::Null);
    assert_eq!(
        spec["parentAsset"],
        json!("models/mill.glb"),
        "a collision mesh or an animation reads its parent's model"
    );
    assert_eq!(spec["parentArchetype"], Json::Null);
    // The shapes take the group's place: its position and turn, then their own.
    let low = tree.nodes.iter().find(|n| n.path == "Wall/Low").unwrap();
    assert!(
        (low.world.position[0] - 10.0).abs() < 1e-9 && (low.world.position[1] - 0.5).abs() < 1e-9
    );
}

#[test]
fn the_code_of_a_collision_under_a_group_gives_its_kit_the_parent_as_dev_builder_does() {
    use gripforge_scene_format::code::code;
    let api = json!({ "id": "core.physics", "expose": [
        { "id": "core.physics.collision", "for": ["scene"], "at": "node", "nodeType": "collision", "nodeTypes": ["collision"],
          "props": { "size": { "type": "f64x3", "default": [1, 1, 1] } },
          "create": "core.physics.elementCreate", "remove": "core.physics.elementRemove" }
    ] });
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    let project = json!({ "id": "p", "requires": ["game.engine", "core.physics"],
        "installed": [{ "id": "game.engine", "config": { "startScene": "level" } }, { "id": "core.physics" }],
        "data": { "scenes": [{ "id": "level", "role": "level", "file": "scenes/level.gfscene" }] } });
    let scene = json!({ "requires": ["core.physics"], "nodes": [
        { "name": "Wall", "type": "group", "children": [
            { "name": "Low", "type": "collision", "collision": { "size": [4, 1, 0.2] } }
        ] }
    ] });
    let list = [("scenes/level.gfscene", scene)];
    let out = code(&project, &files(&list), &reg, "test").unwrap();
    let level = &out
        .files
        .iter()
        .find(|(p, _)| p.ends_with("level.rs"))
        .unwrap()
        .1;
    assert!(
        level.contains(r#"ty: Some("collision")"#)
            && level.contains(r#"parent_node: Some("Wall")"#)
            && level.contains(r#"parent_type: Some("group")"#),
        "the collision acts on its parent group, as dev.builder tells core.physics:\n{level}"
    );
}

#[test]
fn a_watch_node_of_core_physics_is_checked_with_a_fix_and_read_by_its_kit_as_its_parents() {
    use gripforge_scene_format::place;
    // The real declaration of core.physics (its `watch` node type, docs/design/core-physics-surveillances.md).
    let api: Json =
        serde_json::from_str(include_str!("../../../kits/core_physics/kit.api.json")).unwrap();
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    assert!(
        reg.node_types.contains_key("watch"),
        "core.physics declares the node type watch"
    );
    let scene = json!({ "requires": ["core.physics"], "nodes": [
        { "name": "Door", "type": "shape", "shape": { "shape": "box" }, "position": [0, 1, -8], "children": [
            { "name": "Near", "type": "watch", "watch": { "kind": "around", "radius": 3, "tags": ["hero"], "radiu": 2 } }
        ] }
    ] });
    let list = [("level.gfscene", scene)];
    let tree = resolve("level.gfscene", &reg, &files(&list));
    let typo = tree
        .issues
        .iter()
        .find(|i| i.code == "section_field_unknown")
        .expect("the typo is said");
    assert_eq!(typo.field, "kits.core.physics.radiu");
    assert!(
        typo.fix.contains("radius"),
        "the fix names the right fields: {}",
        typo.fix
    );
    let w = place::wanted(&tree, "level.gfscene")
        .into_iter()
        .find(|w| w.node == "Door/Near")
        .expect("the watch node is a section of core.physics");
    let (el, props, _) = place::resolve_section(&reg, &w).unwrap();
    assert_eq!(
        (w.kit.as_str(), el.name.as_str()),
        ("core.physics", "watch")
    );
    let node = tree.nodes.iter().find(|n| n.path == "Door/Near");
    let up = tree.nodes.iter().find(|n| n.path == "Door");
    let spec = place::element_spec("level", &w, &el, &props, node, up, Json::Null);
    assert_eq!(
        (spec["parentNode"].clone(), spec["props"]["radius"].clone()),
        (json!("Door"), json!(3))
    );
    assert_eq!(spec["props"]["tags"], json!(["hero"]));
}

#[test]
fn a_shape_under_a_node_that_has_an_entity_hangs_under_it() {
    // A crate holds its states on a group (an element) and is drawn by its child shape; a ball is a shape with a
    // shape under it; a lamp under a plain group follows nobody.
    let scene = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Crate", "id": "crate", "position": [-6, 0.4, 0],
          "entity": { "states": { "crate": { "values": ["intact", "broken"] } } },
          "children": [
            { "name": "Box", "type": "shape", "shape": { "shape": "box", "size": [0.8, 0.8, 0.8] } },
            { "name": "Lid", "type": "group", "children": [
                { "name": "Plank", "type": "shape", "shape": { "shape": "box" }, "position": [0, 0.4, 0] }
            ] }
          ] },
        { "name": "Ball", "id": "ball", "type": "shape", "shape": { "shape": "sphere", "radius": 0.25 },
          "children": [ { "name": "Dot", "type": "shape", "shape": { "shape": "sphere", "radius": 0.05 } } ] },
        { "name": "Hall", "children": [ { "name": "Lamp", "type": "shape", "shape": { "shape": "cone" } } ] }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    let l = lower("s", &t, &Registry::base());
    let parent = |id: &str| {
        l.content["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == json!(id))
            .unwrap_or_else(|| panic!("no instance {id}"))
            .get("parent")
            .cloned()
    };
    assert_eq!(parent("crate"), None);
    assert_eq!(parent("crate_box"), Some(json!("crate")));
    assert_eq!(parent("crate_lid_plank"), Some(json!("crate")));
    assert_eq!(parent("ball"), None);
    assert_eq!(parent("ball_dot"), Some(json!("ball")));
    assert_eq!(parent("hall_lamp"), None);
}

#[test]
fn an_element_keeps_the_place_its_node_asks_for() {
    // A vase holds its states on a group (an element) raised on its pedestal: `kits.game.engine.place: none` keeps the
    // place the file gives it; without it, game.engine's default (ground) is left as it is.
    let scene = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Vase", "id": "vase", "position": [-2, 1.2, 0], "kits": { "game.engine": { "place": "none" } },
          "entity": { "states": { "vase": { "values": ["intact", "broken"] } } } },
        { "name": "Crate", "id": "crate", "position": [-6, 0.4, 0],
          "entity": { "states": { "crate": { "values": ["intact", "broken"] } } } }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    let l = lower("s", &t, &Registry::base());
    let place = |id: &str| {
        l.content["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == json!(id))
            .unwrap_or_else(|| panic!("no instance {id}"))
            .get("place")
            .cloned()
    };
    assert_eq!(place("vase"), Some(json!("none")));
    assert_eq!(place("crate"), None);
}

/// Collision nodes under a node are its collision (a free shape, a composition of shapes): game.engine's automatic
/// solid gives way to them (`solid.shape: nodes`), said when the node also wrote one; a trigger volume (sensor) is
/// a body of its own and leaves the solid alone.
#[test]
fn collision_children_replace_the_automatic_solid_of_their_parent() {
    let api = json!({ "id": "core.physics", "expose": [
        { "id": "core.physics.collision", "for": ["scene"], "at": "node", "nodeType": "collision", "nodeTypes": ["collision"],
          "parentCollision": { "except": { "type": "sensor" } },
          "props": { "shape": { "type": "string", "default": "" }, "type": { "type": "string", "default": "" },
                     "size": { "type": "f64x3", "default": [0, 0, 0] } },
          "create": "core.physics.elementCreate", "remove": "core.physics.elementRemove" }
    ] });
    let mut reg = Registry::base();
    reg.add_kit_api(&api);
    let solid = json!({ "game.engine": { "place": "none", "set": { "solid": { "shape": "box", "size": [3, 1, 1] } } } });
    let scene = json!({ "requires": ["game.engine", "core.physics"], "nodes": [
        // An L made of two boxes: the drawn shape is one bar, its collision the two boxes of the L.
        { "name": "Corner", "id": "corner", "type": "shape", "shape": { "shape": "box", "size": [3, 1, 1] }, "kits": solid, "children": [
            { "name": "Long", "type": "collision", "collision": { "shape": "box", "size": [3, 1, 1] } },
            { "name": "Short", "type": "collision", "position": [-1, 0, 1.5], "collision": { "shape": "box", "size": [1, 1, 2] } }
        ] },
        { "name": "Bare", "id": "bare", "type": "shape", "shape": { "shape": "box" }, "children": [
            { "name": "Hit", "type": "collision", "collision": { "shape": "sphere" } }
        ] },
        { "name": "Gate", "id": "gate", "type": "shape", "shape": { "shape": "box" }, "kits": solid, "children": [
            { "name": "Zone", "type": "collision", "collision": { "type": "sensor", "size": [2, 2, 2] } }
        ] },
        { "name": "Plain", "id": "plain", "type": "shape", "shape": { "shape": "box" }, "kits": solid }
    ] });
    let list = [("level.gfscene", scene)];
    let tree = resolve("level.gfscene", &reg, &files(&list));
    let l = lower("level", &tree, &reg);
    let solid_of = |id: &str| {
        l.content["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .map(|i| i["set"]["solid"].clone())
            .unwrap()
    };
    assert_eq!(solid_of("corner"), json!({ "shape": "nodes" }));
    assert_eq!(solid_of("bare"), json!({ "shape": "nodes" }));
    assert_eq!(
        solid_of("gate"),
        json!({ "shape": "box", "size": [3, 1, 1] }),
        "a sensor leaves the solid"
    );
    assert_eq!(
        solid_of("plain"),
        json!({ "shape": "box", "size": [3, 1, 1] })
    );
    let replaced: Vec<&str> = l
        .issues
        .iter()
        .filter(|i| i.code == "solid_replaced")
        .map(|i| i.node.as_str())
        .collect();
    assert_eq!(replaced, ["Corner"], "{:?}", l.issues);
}
