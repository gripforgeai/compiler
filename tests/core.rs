//! The core of the compiler (`core::run`): the ops, the strict rules, on the example room and its kits.

use gripforge_compiler::core::{module_name, run, GAME_MODULE};
use serde_json::{json, Value as Json};
use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/room"))
}

fn kits() -> Vec<Json> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root().join("kits")).unwrap().flatten() {
        if let Ok(t) = std::fs::read_to_string(e.path().join("kit.api.json")) {
            out.push(serde_json::from_str(&t).unwrap());
        }
    }
    out
}

fn room() -> Json {
    let t = std::fs::read_to_string(root().join("project.gripforge")).unwrap();
    serde_json::from_str(&t).unwrap()
}

fn req(op: &str, project: Json) -> Json {
    json!({ "op": op, "project": project, "files": {}, "kits": kits() })
}

fn codes(a: &Json) -> Vec<String> {
    a["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["code"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_room_checks_packs_and_generates() {
    let c = run(&req("check", room()));
    assert_eq!(c["ok"], true, "{c}");
    let p = run(&req("pack", room()));
    assert_eq!(
        p["game"]["lock"]["order"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        GAME_MODULE
    );
    let g = run(&req("code", room()));
    assert_eq!(g["entry"], "init_scene::enter");
    assert_eq!(g["files"].as_array().unwrap().len(), 3);
}

#[test]
fn the_strict_rules_refuse_what_the_dev_mode_lets_pass() {
    let mut p = room();
    let scenes = p["data"]["scenes"].as_array_mut().unwrap();
    scenes[1]["kits"]["core.light"]["exposure"] = json!(-1);
    scenes[0]["next"] = json!("credits");
    scenes.push(json!({ "id": "menu", "nodes": [{ "name": "A", "type": "group" }] }));
    scenes.push(json!({ "id": "void" }));
    let a = run(&req("check", p));
    let c = codes(&a);
    for want in [
        "prop_out_of_range",
        "scene_unknown",
        "scene_id_duplicate",
        "scene_empty",
    ] {
        assert!(c.iter().any(|x| x == want), "{want} in {c:?}");
    }
    assert_eq!(a["ok"], false);
}

#[test]
fn lenient_keeps_the_warnings() {
    let mut p = room();
    p["data"]["scenes"][1]["kits"]["core.light"]["brightness"] = json!(2);
    let mut r = req("check", p);
    r["lenient"] = json!(true);
    let a = run(&r);
    assert_eq!(a["ok"], true);
    assert_eq!(a["warnings"], 1);
}

#[test]
fn the_generator_refusal_is_an_error_of_the_build() {
    let mut p = room();
    p["data"]["scene_files"] = json!({});
    let mut r = req("code", p);
    r["lenient"] = json!(true);
    let a = run(&r);
    assert_eq!(a["ok"], false);
    assert!(codes(&a).contains(&"scene_file_missing".to_string()));
}

#[test]
fn unknown_ops_and_the_version() {
    assert_eq!(
        run(&json!({ "op": "fly" }))["issues"][0]["code"],
        "op_unknown"
    );
    assert_eq!(run(&json!({ "op": "version" }))["gameModule"], GAME_MODULE);
    assert_eq!(module_name("level-2"), "level_2");
    assert_eq!(module_name("2nd"), "scene_2nd");
    assert_eq!(module_name(""), "scene_");
}

#[test]
fn a_kit_outside_the_catalog_installed_or_required() {
    let mut p = room();
    p["installed"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "world.nowhere" }));
    p["requires"]
        .as_array_mut()
        .unwrap()
        .push(json!("world.elsewhere"));
    let a = run(&req("check", p));
    let unknown: Vec<&Json> = a["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["code"] == "kit_unknown")
        .collect();
    assert_eq!(unknown.len(), 2, "{a}");
    assert_eq!(unknown[1]["field"], "requires");
}

#[test]
fn pack_refuses_a_kit_whose_version_nobody_knows() {
    let ks: Vec<Json> = kits()
        .into_iter()
        .map(|mut k| {
            if k["id"] == "core.camera" {
                k.as_object_mut().unwrap().remove("version");
            }
            k
        })
        .collect();
    let a = run(&json!({ "op": "pack", "project": room(), "files": {}, "kits": ks }));
    assert_eq!(a["ok"], false);
    assert!(
        codes(&a).contains(&"kit_version_unknown".to_string()),
        "{a}"
    );
    let mut empty = room();
    empty["data"]["scenes"] = json!([]);
    assert_eq!(run(&req("pack", empty.clone()))["ok"], false);
    assert_eq!(run(&req("code", empty))["ok"], false);
}

#[test]
fn the_build_rules_in_nodes_and_with_named_elements() {
    let kit = json!({ "id": "test.bounded", "version": "1.0.0", "expose": [
        { "id": "test.bounded.zone", "for": ["scene"], "at": "node", "props": { "size": { "type": "f64", "default": 1, "min": 1, "max": 10 }, "depth": { "type": "f64", "default": 0, "max": 5 } } },
        { "id": "test.bounded.other", "for": ["scene"], "at": "node", "props": {} } ] });
    let mut p = room();
    p["installed"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "test.bounded", "version": "1.0.0" }));
    let level = &mut p["data"]["scenes"][1];
    level["requires"]
        .as_array_mut()
        .unwrap()
        .push(json!("test.bounded"));
    level["nodes"][0]["kits"] =
        json!({ "test.bounded": { "element": "zone", "size": 11, "depth": 9 } });
    level["nodes"][0]["on"] = json!([{ "when": "event", "event": "x", "do": [{ "load_scene": { "scene": "credits" } }] }]);
    p["data"]["scenes"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "bare", "nodes": [] }));
    p["data"]["scenes"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "nodes": [] }));
    let mut ks = kits();
    ks.push(kit);
    let a = run(&json!({ "op": "check", "project": p, "files": {}, "kits": ks }));
    let msgs: Vec<String> = a["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| format!("{} {} {}", i["code"], i["node"], i["message"]))
        .collect();
    assert!(
        msgs.iter().any(|m| m.contains("between 1 and 10")),
        "{msgs:?}"
    );
    assert!(msgs.iter().any(|m| m.contains("at most 5")), "{msgs:?}");
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("\"scene_unknown\" \"Spawn\"")),
        "{msgs:?}"
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("scene_empty") && m.contains("bare")),
        "{msgs:?}"
    );
}
