//! One command written back where its scene lives (writeback.rs): inline in the project file, inline in the project of
//! a packed game.json, or in the sub-scene file the project names; only the changed line moves, the text read again
//! gives the node at its new place, the same command again changes nothing, the revision follows the text.

#![allow(
    clippy::result_large_err,
    reason = "the refusals of the commands, as the library returns them"
)]

use gripforge_scene_format::build::resolve_scene;
use gripforge_scene_format::writeback::{apply_in_project, line_diff, revision};
use gripforge_scene_format::Registry;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

const PROJECT: &str = r#"{
  "id": "hall",
  "x-tool": "kept",
  "data": {
    "scenes": [
      {
        "id": "hall",
        "nodes": [
          { "name": "Floor", "type": "shape", "shape": { "shape": "plane" } },
          { "name": "Crate", "type": "group", "position": [1, 0.5, -2], "children": [
            { "name": "Solid", "type": "collision", "collision": { "type": "dynamic" } }
          ] }
        ]
      },
      { "id": "yard", "file": "scenes/yard.gfscene" }
    ]
  }
}
"#;

const YARD: &str = r#"{
  "nodes": [
    { "name": "Barrel", "type": "shape", "position": [0, 0, 0] }
  ]
}
"#;

fn texts(project: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("project.gripforge".to_string(), project.to_string()),
        ("scenes/yard.gfscene".to_string(), YARD.to_string()),
    ])
}

fn move_to(id: &str, x: f64, y: f64, z: f64) -> Json {
    json!({ "type": "transform", "id": id, "transform": { "translation": { "x": x, "y": y, "z": z } } })
}

/// The world position of `path` in the scene `id` of a project text, read again from scratch.
fn position(project: &str, id: &str, path: &str) -> [f64; 3] {
    let doc: Json = serde_json::from_str(project).unwrap();
    let scenes = doc
        .pointer("/data/scenes")
        .or_else(|| doc.pointer("/project/data/scenes"))
        .unwrap();
    let scene = scenes
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap();
    let (_, tree) = resolve_scene(scene, &Registry::base(), &|_: &str| None).unwrap();
    tree.nodes
        .iter()
        .find(|n| n.path == path)
        .unwrap()
        .world
        .position
}

#[test]
fn an_inline_scene_is_rewritten_on_one_line_and_read_back_identical() {
    let w = apply_in_project(
        &texts(PROJECT),
        "project.gripforge",
        "hall",
        &move_to("Crate", 3.0, 0.5, -4.25),
    )
    .unwrap();
    assert_eq!(w.file, "project.gripforge");
    let d = line_diff(&w.file, &w.before, &w.after).expect("a change");
    assert_eq!(d["before"].as_array().unwrap().len(), 1, "{d}");
    assert!(
        d["after"][0]
            .as_str()
            .unwrap()
            .contains(r#""name": "Crate", "type": "group", "position": [3, 0.5, -4.25]"#),
        "{d}"
    );
    assert!(w.after.contains("\"x-tool\": \"kept\""));
    assert_eq!(position(&w.after, "hall", "Crate"), [3.0, 0.5, -4.25]);
    // The same command on the new text: nothing to change.
    let again = apply_in_project(
        &texts(&w.after),
        "project.gripforge",
        "hall",
        &move_to("Crate", 3.0, 0.5, -4.25),
    )
    .unwrap();
    assert_eq!(again.after, w.after);
    assert!(line_diff("p", &again.before, &again.after).is_none());
    assert_ne!(revision(&w.before), revision(&w.after));
    assert_eq!(revision(PROJECT), revision(PROJECT));
}

#[test]
fn a_scene_in_its_own_file_is_rewritten_there_and_the_project_stays() {
    let w = apply_in_project(
        &texts(PROJECT),
        "project.gripforge",
        "yard",
        &move_to("Barrel", 0.0, 0.0, 2.0),
    )
    .unwrap();
    assert_eq!(w.file, "scenes/yard.gfscene");
    assert_eq!(w.before, YARD);
    let doc: Json = serde_json::from_str(&w.after).unwrap();
    assert_eq!(doc["nodes"][0]["position"], json!([0, 0, 2]));
    assert_eq!(line_diff(&w.file, &w.before, &w.after).unwrap()["line"], 3);
}

#[test]
fn the_project_of_a_packed_game_is_found_too() {
    let packed = format!(
        "{{\n  \"seed\": 7,\n  \"project\": {}}}\n",
        PROJECT.trim_end()
    );
    let t = BTreeMap::from([("game.json".to_string(), packed.clone())]);
    let w = apply_in_project(&t, "game.json", "hall", &move_to("Crate", -1.0, 0.5, 0.0)).unwrap();
    assert_eq!(position(&w.after, "hall", "Crate"), [-1.0, 0.5, 0.0]);
    assert_eq!(
        line_diff(&w.file, &packed, &w.after).unwrap()["before"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn what_is_missing_is_refused_with_its_fix() {
    let none = BTreeMap::new();
    let e = apply_in_project(
        &none,
        "project.gripforge",
        "hall",
        &move_to("Crate", 0.0, 0.0, 0.0),
    )
    .unwrap_err();
    assert_eq!(e.code, "project_text_missing");
    let e = apply_in_project(
        &texts(PROJECT),
        "project.gripforge",
        "cellar",
        &move_to("Crate", 0.0, 0.0, 0.0),
    )
    .unwrap_err();
    assert_eq!(e.code, "scene_unknown");
    let only = BTreeMap::from([("project.gripforge".to_string(), PROJECT.to_string())]);
    let e = apply_in_project(
        &only,
        "project.gripforge",
        "yard",
        &move_to("Barrel", 0.0, 0.0, 0.0),
    )
    .unwrap_err();
    assert_eq!(e.code, "scene_file_missing");
}
