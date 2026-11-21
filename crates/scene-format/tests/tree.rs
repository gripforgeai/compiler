//! The resolved tree of a scene file: names and paths, tolerance of unknown fields, sub-scenes and their overrides,
//! transforms down the tree, the bindings' checks.

use gripforge_scene_format::{tree::resolve, Registry};
use serde_json::{json, Value as Json};

fn files<'a>(list: &'a [(&'a str, Json)]) -> impl Fn(&str) -> Option<Json> + 'a {
    move |p| list.iter().find(|(k, _)| *k == p).map(|(_, v)| v.clone())
}

fn codes(t: &gripforge_scene_format::tree::Tree) -> Vec<&'static str> {
    t.issues.iter().map(|i| i.code).collect()
}

#[test]
fn nodes_are_named_by_their_path_and_badly_named_nodes_are_still_placed() {
    let scene = json!({ "nodes": [
        { "name": "Hall", "children": [{ "name": "Door", "type": "trigger", "colour": "red" }] },
        { "name": "Hall" },
        { "type": "shape" },
        { "name": "bad/name" }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    let paths: Vec<&str> = t.nodes.iter().map(|n| n.path.as_str()).collect();
    // Tolerant: a repeated, missing or invalid name is replaced by one made from its place, and the node is placed.
    assert_eq!(paths, ["Hall", "Hall/Door", "Hall_2", "node2", "bad_name"]);
    assert_eq!(
        t.nodes[1].raw.get("colour"),
        Some(&json!("red")),
        "an unknown field is kept as written"
    );
    assert_eq!(
        codes(&t),
        [
            "field_unknown",
            "name_duplicate",
            "name_missing",
            "name_invalid"
        ]
    );
    let unknown = &t.issues[0];
    assert_eq!(
        (
            unknown.node.as_str(),
            unknown.field.as_str(),
            unknown.file.as_str()
        ),
        ("Hall/Door", "colour", "s.gfscene")
    );
    assert!(!unknown.fix.is_empty(), "every issue says what to do");
}

#[test]
fn a_child_is_placed_in_the_space_of_its_parent() {
    let scene = json!({ "nodes": [{ "name": "P", "position": [10, 0, 0], "rotation": [0, 90, 0], "scale": 2,
        "children": [{ "name": "C", "position": [1, 0, 0] }] }] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    let c = t.nodes[1].world;
    // A yaw of 90° turns +X into -Z; the parent's scale doubles the offset.
    let want = [10.0, 0.0, -2.0];
    for (got, want) in c.position.iter().zip(want) {
        assert!((got - want).abs() < 1e-9, "{:?}", c.position);
    }
    assert!((c.yaw_only().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn a_sub_scene_is_instanced_with_its_overrides_and_its_requires() {
    let shelf = json!({ "requires": ["core.light"], "nodes": [
        { "name": "Book1", "type": "shape" },
        { "name": "Book2", "type": "shape" },
        { "name": "Lamp", "type": "light", "light": { "intensity": 2 } }
    ] });
    let room = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Shelf", "type": "scene", "scene": "props/shelf.gfscene", "position": [4, 0, 0],
          "overrides": { "Book2": { "visible": false }, "Lamp": { "light": { "intensity": 0.5 } }, "Nope": { "visible": false } } }
    ] });
    let list = [("room.gfscene", room), ("props/shelf.gfscene", shelf)];
    let t = resolve("room.gfscene", &Registry::base(), &files(&list));
    let get = |p: &str| {
        t.nodes
            .iter()
            .find(|n| n.path == p)
            .unwrap_or_else(|| panic!("{p} missing"))
    };
    assert!(get("Shelf/Book1").visible);
    assert!(!get("Shelf/Book2").visible, "the override hides Book2");
    assert_eq!(
        get("Shelf/Lamp").raw["light"]["intensity"],
        json!(0.5),
        "merged key by key"
    );
    assert_eq!(get("Shelf/Lamp").file, "props/shelf.gfscene");
    assert!(
        (get("Shelf/Book1").world.position[0] - 4.0).abs() < 1e-12,
        "placed under the instance node"
    );
    let req: Vec<&str> = t.requires.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(req, ["game.engine", "core.light"]);
    assert_eq!(codes(&t), ["override_orphan"]);
}

#[test]
fn an_outer_override_reaches_through_a_nested_instance() {
    let book =
        json!({ "nodes": [{ "name": "Cover", "type": "shape", "shape": { "color": "#ff0000" } }] });
    let shelf = json!({ "nodes": [{ "name": "B", "type": "scene", "scene": "book.gfscene" }] });
    let room = json!({ "nodes": [{ "name": "S", "type": "scene", "scene": "shelf.gfscene", "overrides": { "B/Cover": { "shape": { "color": "#00ff00" } } } }] });
    let list = [
        ("room.gfscene", room),
        ("shelf.gfscene", shelf),
        ("book.gfscene", book),
    ];
    let t = resolve("room.gfscene", &Registry::base(), &files(&list));
    let cover = t
        .nodes
        .iter()
        .find(|n| n.path == "S/B/Cover")
        .expect("nested node");
    assert_eq!(cover.raw["shape"]["color"], json!("#00ff00"));
    assert!(t.issues.is_empty(), "{:?}", t.issues);
}

#[test]
fn a_scene_that_instances_itself_is_refused_with_its_loop() {
    let a = json!({ "nodes": [{ "name": "B", "type": "scene", "scene": "b.gfscene" }] });
    let b = json!({ "nodes": [{ "name": "A", "type": "scene", "scene": "a.gfscene" }] });
    let list = [("a.gfscene", a), ("b.gfscene", b)];
    let t = resolve("a.gfscene", &Registry::base(), &files(&list));
    assert_eq!(codes(&t), ["scene_cycle"]);
    assert!(
        t.issues[0].message.contains("a.gfscene → b.gfscene"),
        "{}",
        t.issues[0].message
    );
}

#[test]
fn wrong_properties_paths_and_bindings_say_what_to_write() {
    let scene = json!({ "nodes": [
        { "name": "Crate", "type": "model", "model": { "asset": "/home/x/crate.glb" } },
        { "name": "Box", "type": "shape", "shape": { "shape": "cube" } },
        { "name": "Door", "type": "trigger", "on": [{ "when": "interact", "do": [] }] },
        { "name": "Pos", "position": [1, 2] }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    assert_eq!(
        codes(&t),
        [
            "path_invalid",
            "prop_invalid",
            "binding_invalid",
            "transform_invalid"
        ]
    );
    assert!(
        t.issues[1].fix.contains("box, sphere"),
        "{}",
        t.issues[1].fix
    );
    assert!(
        t.issues[2].fix.contains("enter, leave, event"),
        "{}",
        t.issues[2].fix
    );
}

/// Decision of the owner (dev.builder step c): a value written wrong is a warning and its default applies; the scene
/// still loads. Only a missing required kit refuses a scene.
#[test]
fn a_wrong_value_is_a_warning_and_its_default_applies() {
    let scene = json!({ "nodes": [
        { "name": "Box", "type": "shape", "shape": { "shape": "cube", "radius": 0.5 }, "visible": "yes",
          "kits": { "game.engine": "crate", "core.light": { "x": 1 } } }
    ] });
    let list = [("s.gfscene", scene)];
    let t = resolve("s.gfscene", &Registry::base(), &files(&list));
    assert_eq!(
        codes(&t),
        ["field_invalid", "prop_invalid", "section_invalid"]
    );
    assert!(t.issues.iter().all(|i| !i.is_error()), "warnings only");
    let n = &t.nodes[0];
    assert!(n.visible, "visible: the default");
    assert_eq!(
        n.raw["shape"],
        json!({ "radius": 0.5 }),
        "the wrong shape dropped, its default applies"
    );
    assert_eq!(
        n.raw["kits"],
        json!({ "core.light": { "x": 1 } }),
        "the section that is not an object dropped"
    );
}
