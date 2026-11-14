//! The strict check (`core::run` op `check`) on the example room: clean as it is; each way to break it is refused
//! with one error that says the file, the scene, the element, the property and what is expected.

use gripforge_compiler::core::run;
use serde_json::{json, Value as Json};
use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/room"))
}

fn kits() -> Vec<Json> {
    gripforge_compiler::project::kit_catalog(Some(&root().join("kits"))).unwrap()
}

fn room() -> Json {
    serde_json::from_str(&std::fs::read_to_string(root().join("project.gripforge")).unwrap())
        .unwrap()
}

fn scene<'a>(p: &'a mut Json, id: &str) -> &'a mut Json {
    p["data"]["scenes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["id"] == id)
        .unwrap()
}

fn node<'a>(s: &'a mut Json, name: &str) -> &'a mut Json {
    s["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|n| n["name"] == name)
        .unwrap()
}

fn check(p: Json) -> Json {
    run(&json!({ "op": "check", "project": p, "files": {}, "kits": kits() }))
}

type Break = fn(&mut Json);
/// (what, how the room is broken, code, scene, node, field contains).
type Case = (
    &'static str,
    Break,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    &'static str,
);

/// (what, how the room is broken, code, scene, node, field contains).
const CASES: &[Case] = &[
    (
        "an unknown property of a kit section",
        |p| scene(p, "level")["kits"]["core.light"]["brightness"] = json!(2),
        "section_field_unknown",
        Some("level"),
        None,
        "kits.core.light.brightness",
    ),
    (
        "a value of the wrong type in a node",
        |p| node(scene(p, "level"), "Sun")["light"]["intensity"] = json!("strong"),
        "prop_invalid",
        Some("level"),
        Some("Sun"),
        "light.intensity",
    ),
    (
        "a value out of its bounds",
        |p| scene(p, "level")["kits"]["core.light"]["exposure"] = json!(-1),
        "prop_out_of_range",
        Some("level"),
        None,
        "kits.core.light.exposure",
    ),
    (
        "a sub-scene that is not there",
        |p| node(scene(p, "level"), "Shelf")["scene"] = json!("scenes/cupboard.gfscene"),
        "scene_file_missing",
        Some("level"),
        Some("Shelf"),
        "",
    ),
    (
        "a binding that loads a scene that does not exist",
        |p| scene(p, "menu")["on"][0]["do"][0]["load_scene"]["scene"] = json!("credits"),
        "scene_unknown",
        Some("menu"),
        None,
        "on[0].do[0].load_scene.scene",
    ),
    (
        "the next scene does not exist",
        |p| scene(p, "menu")["next"] = json!("credits"),
        "scene_unknown",
        Some("menu"),
        None,
        "next",
    ),
    (
        "a sub-scene that instances itself",
        |p| {
            p["data"]["scene_files"]["scenes/shelf.gfscene"]["nodes"]
                .as_array_mut()
                .unwrap()
                .push(json!({ "name": "Again", "type": "scene", "scene": "scenes/shelf.gfscene" }))
        },
        "scene_cycle",
        None,
        None,
        "",
    ),
    (
        "a kit required by a scene and not installed",
        |p| {
            scene(p, "menu")["requires"]
                .as_array_mut()
                .unwrap()
                .push(json!("core.physics"))
        },
        "kit_required_missing",
        Some("menu"),
        None,
        "",
    ),
    (
        "a kit not in the project",
        |p| {
            p["installed"]
                .as_array_mut()
                .unwrap()
                .push(json!({ "id": "world.nowhere" }))
        },
        "kit_unknown",
        None,
        None,
        "installed",
    ),
    (
        "a kit in an incompatible version",
        |p| {
            scene(p, "menu")["requires"] =
                json!(["game.engine", { "id": "core.light", "version": ">=9.0.0" }, "core.camera"])
        },
        "kit_version_mismatch",
        Some("menu"),
        None,
        "",
    ),
    (
        "an empty scene",
        |p| {
            p["data"]["scenes"]
                .as_array_mut()
                .unwrap()
                .push(json!({ "id": "void", "nodes": [] }))
        },
        "scene_empty",
        Some("void"),
        None,
        "",
    ),
    (
        "two nodes with the same name",
        |p| {
            let m = scene(p, "menu");
            let first = m["nodes"][0].clone();
            m["nodes"].as_array_mut().unwrap().push(first);
        },
        "name_duplicate",
        Some("menu"),
        None,
        "",
    ),
    (
        "two scenes with the same id",
        |p| {
            let m = scene(p, "menu").clone();
            p["data"]["scenes"].as_array_mut().unwrap().push(m);
        },
        "scene_id_duplicate",
        Some("menu"),
        None,
        "",
    ),
    (
        "a project without scenes",
        |p| p["data"]["scenes"] = json!([]),
        "scenes_missing",
        None,
        None,
        "",
    ),
    (
        "a node of an unknown type",
        |p| {
            scene(p, "menu")["nodes"]
                .as_array_mut()
                .unwrap()
                .push(json!({ "name": "Blob", "type": "blob" }))
        },
        "node_type_unknown",
        Some("menu"),
        Some("Blob"),
        "",
    ),
];

#[test]
fn the_room_checks_clean_with_its_scenes_and_modules() {
    let a = check(room());
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(
        a["scenes"],
        json!([{ "id": "menu", "file": "#menu", "module": "menu" }, { "id": "level", "file": "#level", "module": "level" }])
    );
}

#[test]
fn each_way_to_break_the_room_is_refused_with_a_located_error() {
    for (what, break_it, code, sc, nd, field) in CASES {
        let mut p = room();
        break_it(&mut p);
        let a = check(p);
        assert_eq!(a["ok"], false, "{what}: the build should be refused");
        let found = a["issues"].as_array().unwrap().iter().find(|i| {
            i["level"] == "error"
                && i["code"] == *code
                && sc.is_none_or(|s| i["scene"] == s)
                && nd.is_none_or(|n| i["node"] == n)
                && i["field"].as_str().unwrap_or("").contains(field)
        });
        let i = found.unwrap_or_else(|| panic!("{what}: no error {code} in {}", a["issues"]));
        assert!(
            i["message"].is_string() && i["fix"].is_string(),
            "{what}: every error says what is wrong and what is expected"
        );
    }
}

#[test]
fn lenient_keeps_the_warnings_and_a_promoted_warning_says_so() {
    let mut p = room();
    scene(&mut p, "level")["kits"]["core.light"]["brightness"] = json!(2);
    let strict = check(p.clone());
    assert_eq!(strict["issues"][0]["promoted"], true);
    let a =
        run(&json!({ "op": "check", "project": p, "files": {}, "kits": kits(), "lenient": true }));
    assert_eq!(a["ok"], true, "{a}");
}

#[test]
fn the_generator_writes_one_module_per_scene_parents_first() {
    let a = run(&json!({ "op": "code", "project": room(), "files": {}, "kits": kits() }));
    assert_eq!(a["ok"], true);
    let paths: Vec<&str> = a["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["init_scene.rs", "init_scene/menu.rs", "init_scene/level.rs"]
    );
    let level = a["files"][2]["text"].as_str().unwrap();
    let (hall, krate) = (
        level.find("\"Hall\"").unwrap(),
        level.find("\"Hall/Crate\"").unwrap(),
    );
    assert!(hall < krate, "the parent is posed before its child");
    assert!(
        level.contains("exposure: 1.2"),
        "the values are typed in the code"
    );
}
