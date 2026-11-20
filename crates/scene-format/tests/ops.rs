//! The commands of the site's scene patch applied to a scene file's text: the smallest diff (only the changed values
//! move; the order, the layout and the unknown fields stay), a command of the debugging editor (a game.engine id, a
//! world transform) turned into the node's local transform, overrides for a node of an instanced sub-scene, refusals
//! that say why.

#![allow(
    clippy::result_large_err,
    reason = "the refusals of the commands, as the library returns them"
)]

use gripforge_scene_format::math::from_euler_deg;
use gripforge_scene_format::ops::{apply_command, apply_patches};
use serde_json::{json, Value as Json};

const LEVEL: &str = r#"{
  "requires": ["game.engine"],
  "x-note": "kept as written",
  "nodes": [
    { "name": "Sun", "type": "light", "light": { "kind": "directional", "intensity": 3 } },
    {
      "name": "Hall",
      "position": [0, 0, -6],
      "children": [
        { "name": "Crate", "type": "model", "model": { "asset": "models/crate.glb" }, "position": [2, 0, 0] },
        { "name": "Door", "type": "trigger" }
      ]
    },
    { "name": "Shelf", "type": "scene", "scene": "scenes/shelf.gfscene" }
  ]
}
"#;

const SHELF: &str = r#"{ "nodes": [{ "name": "Book2", "type": "shape" }] }"#;

fn read(p: &str) -> Option<String> {
    match p {
        "scenes/level.gfscene" => Some(LEVEL.to_string()),
        "scenes/shelf.gfscene" => Some(SHELF.to_string()),
        _ => None,
    }
}

fn cmd(text: &str, c: Json) -> Result<String, gripforge_scene_format::Issue> {
    apply_command(text, "scenes/level.gfscene", &c, &read)
}

/// The lines of `b` that are not in `a` at the same place.
fn changed_lines(a: &str, b: &str) -> Vec<String> {
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    lb.iter()
        .enumerate()
        .filter(|(i, l)| la.get(*i) != Some(l))
        .map(|(_, l)| l.to_string())
        .collect()
}

#[test]
fn a_transform_by_path_changes_only_the_line_of_the_node() {
    let out = cmd(LEVEL, json!({ "type": "transform", "id": "Hall/Crate", "transform": { "translation": { "x": 3, "y": 0, "z": 1 } } })).expect("applied");
    assert_eq!(
        changed_lines(LEVEL, &out),
        [
            r#"        { "name": "Crate", "type": "model", "model": { "asset": "models/crate.glb" }, "position": [3, 0, 1] },"#
        ]
    );
    assert_eq!(LEVEL.lines().count(), out.lines().count());
    let v: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(v["x-note"], json!("kept as written"), "unknown fields stay");
}

#[test]
fn a_command_of_the_debugging_editor_lands_as_the_local_transform_of_the_node() {
    // The debugging editor (debug.inspector.edit) names the game.engine instance (hall_crate) and gives its place in the world, a yaw of 90°.
    let q = from_euler_deg([0.0, 90.0, 0.0]);
    let out = cmd(LEVEL, json!({ "type": "transform", "id": "hall_crate", "transform": {
        "translation": { "x": 2.5, "y": 0.4, "z": -5 }, "rotation": { "x": q[0], "y": q[1], "z": q[2], "w": q[3] }, "scale": { "x": 1, "y": 1, "z": 1 } } })).unwrap();
    let v: Json = serde_json::from_str(&out).unwrap();
    let crate_ = &v["nodes"][1]["children"][0];
    assert_eq!(
        crate_["position"],
        json!([2.5, 0.4, 1]),
        "Hall is at z = -6: the world z -5 is 1 in the hall"
    );
    assert_eq!(crate_["rotation"], json!([0, 90, 0]));
    assert!(
        crate_.get("scale").is_none(),
        "an unchanged scale (the default) is not written: {crate_}"
    );
}

#[test]
fn an_update_merges_into_the_node_and_creates_the_section() {
    let out = cmd(
        LEVEL,
        json!({ "type": "update", "id": "Sun", "node": { "light": { "intensity": 0.5 } } }),
    )
    .unwrap();
    assert_eq!(changed_lines(LEVEL, &out).len(), 1);
    let out = cmd(&out, json!({ "type": "update", "id": "Hall/Door", "node": { "kits": { "core.physics": { "body": "static" } } } })).unwrap();
    let v: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["nodes"][0]["light"],
        json!({ "kind": "directional", "intensity": 0.5 }),
        "merged, not replaced"
    );
    assert_eq!(
        v["nodes"][1]["children"][1]["kits"],
        json!({ "core.physics": { "body": "static" } })
    );
    let keys: Vec<&String> = v["nodes"][1].as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["name", "position", "children"],
        "the order of the keys stays"
    );
    assert_eq!(
        cmd(
            LEVEL,
            json!({ "type": "update", "id": "Sun", "node": { "children": [] } })
        )
        .unwrap_err()
        .code,
        "op_invalid"
    );
}

#[test]
fn a_node_of_an_instance_is_changed_by_an_override() {
    let out = cmd(
        LEVEL,
        json!({ "type": "update", "id": "Shelf/Book2", "node": { "visible": false } }),
    )
    .unwrap();
    let v: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["nodes"][2]["overrides"],
        json!({ "Book2": { "visible": false } })
    );
    let by_id = cmd(LEVEL, json!({ "type": "transform", "id": "shelf_book2", "transform": { "translation": { "x": 1, "y": 0, "z": 0 } } })).unwrap();
    assert!(
        by_id.contains(r#""overrides": { "Book2": { "position": [1, 0, 0] } }"#),
        "{by_id}"
    );
    let refused = cmd(LEVEL, json!({ "type": "remove", "id": "Shelf/Book2" })).unwrap_err();
    assert_eq!(refused.code, "op_in_instance");
    assert!(refused.fix.contains("visible"), "{}", refused.fix);
}

#[test]
fn insert_and_remove_keep_the_layout_of_the_list() {
    let out = cmd(LEVEL, json!({ "type": "insert", "index": 1, "node": { "id": "Lamp", "type": "light", "parentId": "Hall" } })).unwrap();
    let v: Json = serde_json::from_str(&out).unwrap();
    let names: Vec<&str> = v["nodes"][1]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Crate", "Lamp", "Door"]);
    assert!(
        out.contains("\n        { \"type\": \"light\", \"name\": \"Lamp\" },\n"),
        "on its own line, with the siblings' indent:\n{out}"
    );
    let back = cmd(&out, json!({ "type": "remove", "id": "Hall/Lamp" })).unwrap();
    assert_eq!(back, LEVEL, "remove undoes insert byte for byte");
    assert_eq!(
        cmd(
            LEVEL,
            json!({ "type": "insert", "node": { "name": "Sun" } })
        )
        .unwrap_err()
        .code,
        "name_duplicate"
    );
    let swapped = cmd(
        LEVEL,
        json!({ "type": "replace", "id": "Hall/Crate", "asset": "models/barrel.glb" }),
    )
    .unwrap();
    assert!(swapped.contains("models/barrel.glb"));
    assert_eq!(cmd(LEVEL, json!({ "type": "replace", "id": "Hall/Crate", "asset": { "assetId": "a1", "revisionId": "r1" } })).unwrap_err().code, "asset_ref_unsupported");
}

#[test]
fn a_file_of_patches_goes_to_the_file_of_its_room_and_refusals_are_reported() {
    let project = json!({ "id": "arena", "data": { "scenes": [{ "id": "level", "file": "scenes/level.gfscene" }] } });
    let patches = json!({ "schema": "gripforge.editor-patches/1", "room": "level", "baseRevision": null, "commands": [
        { "type": "transform", "id": "Hall", "transform": { "translation": { "x": 0, "y": 0, "z": -8 } } },
        { "type": "update", "id": "Nope", "node": { "visible": false } },
        { "type": "update", "file": "scenes/missing.gfscene", "id": "A", "node": { "visible": false } },
        { "type": "fly", "id": "Hall" }
    ] });
    let (changed, issues) = apply_patches(&patches, &project, &read);
    assert_eq!(changed.len(), 1);
    assert!(changed["scenes/level.gfscene"].contains("\"position\": [0, 0, -8]"));
    let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
    assert_eq!(
        codes,
        ["node_not_found", "scene_file_missing", "op_invalid"]
    );
    assert_eq!(
        (issues[0].field.as_str(), issues[0].node.as_str()),
        ("commands[1]", "Nope")
    );
}

#[test]
fn euler_angles_come_back_from_their_quaternion() {
    for e in [[0.0, 90.0, 0.0], [-50.0, 30.0, 0.0], [10.0, -120.0, 35.0]] {
        let back = gripforge_scene_format::math::to_euler_deg(from_euler_deg(e));
        for (a, b) in back.iter().zip(e) {
            assert!((a - b).abs() < 1e-9, "{e:?} -> {back:?}");
        }
    }
}
