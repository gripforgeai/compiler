//! The states of an element read, checked and lowered for game.engine (the rules are resolved by game.engine): the door of docs/entites-et-rendu.md.

use super::*;
use crate::lower::lower;
use crate::registry::Registry;
use crate::tree::resolve_doc;
use serde_json::json;

fn door() -> Json {
    json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Door", "id": "hall_door", "position": [4, 0, -2], "rotation": [0, 180, 0],
          "entity": {
            "states": {
              "door": { "values": ["closed", "open", "locked"] },
              "angle": { "default": 0 },
              "night": { "derived": { "var": "night" } },
              "moving": { "derived": { "expr": { "all": [
                  { "var": "speed", "op": ">", "value": 0 }, { "not": { "var": "paused" } } ] } } } },
            "bind": [{ "node": "Hinge/Leaf", "property": "rotation.z", "from": "angle" }] },
          "reflect": [
            { "when": { "door": "open" }, "set": { "Hinge": { "rotation": [0, 95, 0] } },
              "transition": { "duration": 0.6, "ease": "out" } },
            { "when": { "door": "locked" }, "set": { "Hinge/Leaf/Lock": { "material": { "emissive": "#ff3020" } } } } ],
          "children": [
            { "name": "Frame", "type": "shape", "shape": { "shape": "box", "size": [1.2, 2.2, 0.1] } },
            { "name": "Hinge", "position": [-0.45, 0, 0], "children": [
              { "name": "Leaf", "type": "shape", "shape": { "shape": "box", "size": [0.9, 2, 0.05] },
                "children": [ { "name": "Lock", "id": "door_lock", "type": "shape", "shape": { "shape": "box" } } ] } ] } ] }
    ] })
}

fn tree_of(doc: &Json) -> Tree {
    let reg = Registry::base();
    resolve_doc("scenes/a.gfscene", doc, &reg, &|_| None)
}

fn codes(t: &Tree) -> Vec<&'static str> {
    t.issues.iter().map(|i| i.code).collect()
}

#[test]
fn entity_states_and_reflect_are_read_not_unknown() {
    let t = tree_of(&door());
    assert!(codes(&t).is_empty(), "{:?}", t.issues);
    let e = elements(&t);
    assert_eq!(e.len(), 1);
    assert_eq!(t.nodes[e[0]].path, "Door");
}

#[test]
fn the_states_go_to_game_engine_as_written() {
    let n = &tree_of(&door()).nodes[0];
    let Engine { decls, vars, left } = to_engine(declared(n).unwrap());
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(
        decls,
        *declared(n).unwrap(),
        "one writing: game.engine's, unchanged"
    );
    assert_eq!(vars, ["night", "speed", "paused"]);
    let d = defaults(&decls);
    assert_eq!(d["door"], "closed");
    assert_eq!(d["angle"], 0);
    assert!(!d.contains_key("night"), "a derived state has no default");
}

#[test]
fn a_group_with_states_gets_a_light_entity() {
    let t = tree_of(&door());
    let l = lower("a", &t, &Registry::base());
    let inst = l.content["instances"].as_array().unwrap();
    let door = inst
        .iter()
        .find(|i| i["id"] == "hall_door")
        .expect("the door's entity");
    assert_eq!(door["archetype"], "gfscene_hall_door");
    let arch = l
        .archetypes
        .iter()
        .find(|a| a["id"] == "gfscene_hall_door")
        .unwrap();
    assert_eq!(
        arch["states"]["door"],
        json!({ "values": ["closed", "open", "locked"] }),
        "as written: game.engine reads it (its default, the first)"
    );
    assert!(arch.get("asset").is_none(), "no mesh in the entity: {arch}");
    assert!(
        inst.iter().any(|i| i["id"] == "door_lock"),
        "the lock is an instance"
    );
}

#[test]
fn what_is_wrong_is_said_with_its_fix() {
    let mut doc = door();
    let d = &mut doc["nodes"][0];
    d["entity"]["states"]["bad"] = json!({ "values": [] });
    d["entity"]["states"]["lit"] = json!({ "derived": { "clock": "night" } });
    d["reflect"][0]["when"] = json!({ "door": "ajar", "ghost": true });
    d["reflect"][1]["set"]["Nope"] = json!({ "visible": false });
    d["reflect"][1]["set"]["Hinge"] = json!({ "colour": 1 });
    d["entity"]["bind"] = json!([{ "property": "rotation.z", "from": "nope" },
        { "property": "tint", "from": "angle" }, { "node": "Gone", "property": "scale", "from": "angle" }]);
    let t = tree_of(&doc);
    let mut c = codes(&t);
    c.sort_unstable();
    assert_eq!(
        c,
        [
            "bind_invalid",
            "bind_invalid",
            "bind_invalid",
            "reflect_node_unknown",
            "reflect_prop_unknown",
            "reflect_state_unknown",
            "reflect_value_unknown",
            "state_invalid",
            "state_invalid"
        ],
        "{:?}",
        t.issues
    );
    assert!(t
        .issues
        .iter()
        .all(|i| i.level == crate::Level::Warning && !i.fix.is_empty()));
}

#[test]
fn the_old_writing_is_not_read() {
    let doc = json!({ "requires": ["game.engine"], "nodes": [
        { "name": "Door", "states": { "door": { "values": ["closed", "open"] } },
          "variants": [{ "when": { "door": "open" }, "set": { "": { "visible": false } } }],
          "bind": { "rotation.z": "state.door" } } ] });
    let t = tree_of(&doc);
    assert!(
        elements(&t).is_empty(),
        "states / variants are not an element any more"
    );
    assert!(codes(&t).contains(&"field_unknown"), "{:?}", t.issues);
    assert!(
        codes(&t).contains(&"bind_invalid"),
        "a state in the node's bind: {:?}",
        t.issues
    );
}

#[test]
fn a_group_with_states_can_be_used_and_sets_its_own_state() {
    let mut doc = door();
    doc["nodes"][0]["on"] = json!([{ "when": "interact", "if": [{ "state": "door", "eq": "closed" }],
        "do": [{ "set_state": { "door": "open" } }] }]);
    let t = tree_of(&doc);
    assert!(
        codes(&t).is_empty(),
        "interact on a group with states: {:?}",
        t.issues
    );
    let l = lower("a", &t, &Registry::base());
    let r = &l.scripts[0]["rules"][0];
    assert_eq!(r["when"], json!({ "interact": "hall_door" }));
    assert_eq!(
        r["if"],
        json!([{ "state": { "door": "closed" }, "on": "hall_door" }])
    );
    assert_eq!(
        r["then"],
        json!([{ "set_state": { "door": "open" }, "on": "hall_door" }])
    );
    let mut plain = door();
    plain["nodes"][0].as_object_mut().unwrap().remove("entity");
    plain["nodes"][0]["reflect"] = json!([]);
    plain["nodes"][0]["on"] = doc["nodes"][0]["on"].clone();
    assert!(
        codes(&tree_of(&plain)).contains(&"binding_invalid"),
        "a bare group has no entity to use"
    );
}
