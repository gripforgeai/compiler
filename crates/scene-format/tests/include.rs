//! A project that includes another `.gripforge` (`includes`: `file`, `as`, `variables`, `hide`, `overrides`), the
//! variables of a node (`<id>.<name>`), the read of a variable in a property (`{ "$var" }`: `visible`, a node's
//! variable, `data.scenes[].enabled`), and every code of the format (docs/design/banc-de-demo-gripforge.md § 5).

use gripforge_scene_format::include::{resolve_project, resolve_project_with};
use gripforge_scene_format::project::{pack, validate_project};
use gripforge_scene_format::{Issue, Registry};
use serde_json::{json, Value as Json};

fn files<'a>(list: &'a [(&'a str, Json)]) -> impl Fn(&str) -> Option<Json> + 'a {
    move |p| list.iter().find(|(k, _)| *k == p).map(|(_, v)| v.clone())
}

fn codes(issues: &[Issue]) -> Vec<&'static str> {
    issues.iter().map(|i| i.code).collect()
}

/// The room shared by the demos: two scenes (one inline, one in its file), a pillar shown by a variable, a crate.
fn salle() -> Json {
    json!({
        "id": "salle",
        "requires": ["game.engine"],
        "variables": { "pilier": true, "annexe": true },
        "installed": [
            { "id": "game.engine", "config": { "startScene": "salle_room", "tick": { "hz": 30 } } },
            { "id": "core.light", "config": { "shadows": { "cascades": 1, "size": 1024 } } }
        ],
        "data": {
            "scenes": [
                { "id": "salle_room", "role": "level", "requires": ["game.engine"], "nodes": [
                    { "name": "Pilier", "id": "pilier", "type": "shape",
                      "variables": { "shown": { "$var": "pilier" } },
                      "visible": { "$var": "pilier.shown" } },
                    { "name": "Props", "type": "group", "children": [
                        { "name": "Caisse", "id": "caisse", "type": "shape" }
                    ] },
                    { "name": "Sun", "id": "sun", "type": "light", "light": { "intensity": 1.0 } }
                ] },
                { "id": "salle_maison", "role": "level", "file": "scenes/maison.json" },
                { "id": "salle_annexe", "role": "level", "enabled": { "$var": "annexe" }, "requires": ["game.engine"], "nodes": [] }
            ],
            "archetypes": [{ "id": "box" }]
        }
    })
}

fn maison() -> Json {
    json!({ "requires": ["game.engine"], "nodes": [{ "name": "Mur", "id": "mur", "type": "shape" }] })
}

fn master(entry: Json) -> Json {
    json!({
        "id": "core_light_demo",
        "requires": ["core.light"],
        "includes": [entry],
        "installed": [{ "id": "core.light", "config": { "shadows": { "cascades": 3 } } }],
        "data": { "stations": [{ "id": "light-sun" }] }
    })
}

fn disk() -> Vec<(&'static str, Json)> {
    vec![
        ("../base/salle.gripforge", salle()),
        ("../base/scenes/maison.json", maison()),
    ]
}

fn scene<'a>(p: &'a Json, id: &str) -> Option<&'a Json> {
    p.pointer("/data/scenes")?
        .as_array()?
        .iter()
        .find(|s| s["id"] == json!(id))
}

fn node<'a>(scene: &'a Json, name: &str) -> &'a Json {
    fn find<'a>(nodes: &'a Json, name: &str) -> Option<&'a Json> {
        nodes.as_array()?.iter().find_map(|n| {
            if n["name"] == json!(name) {
                Some(n)
            } else {
                find(&n["children"], name)
            }
        })
    }
    find(&scene["nodes"], name).expect("node")
}

#[test]
fn the_master_gets_the_sub_file_its_kits_its_scenes_and_its_variables() {
    let list = disk();
    let (p, issues) = resolve_project(
        &master(json!({ "file": "../base/salle.gripforge", "as": "salle" })),
        &files(&list),
    );
    assert!(issues.is_empty(), "{issues:?}");
    // The master keeps its id; requires are the union.
    assert_eq!(p["id"], json!("core_light_demo"));
    let req = p["requires"].as_array().unwrap();
    assert!(req.contains(&json!("game.engine")) && req.contains(&json!("core.light")));
    // installed: the sub-file's order, the master's config merged over.
    let ids: Vec<_> = p["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["id"].clone())
        .collect();
    assert_eq!(ids, vec![json!("game.engine"), json!("core.light")]);
    assert_eq!(
        p["installed"][1]["config"]["shadows"],
        json!({ "cascades": 3, "size": 1024 })
    );
    // The variables of the sub-file are prefixed by `as`, the node's variable is a game variable `<id>.<name>`.
    assert_eq!(p["variables"]["salle.pilier"], json!(true));
    assert_eq!(p["variables"]["pilier.shown"], json!(true));
    // The scene file is carried under the prefix, the scene entry names it.
    assert_eq!(
        scene(&p, "salle_maison").unwrap()["file"],
        json!("salle/scenes/maison.json")
    );
    assert_eq!(
        p["data"]["scene_files"]["salle/scenes/maison.json"],
        maison()
    );
    // data collections merged by id.
    assert_eq!(p["data"]["stations"], json!([{ "id": "light-sun" }]));
    assert_eq!(p["data"]["archetypes"], json!([{ "id": "box" }]));
    assert!(p.get("includes").is_none());
}

#[test]
fn a_variable_of_the_entry_hides_the_pillar_and_a_set_var_brings_it_back_at_render() {
    let list = disk();
    let (p, issues) = resolve_project(
        &master(
            json!({ "file": "../base/salle.gripforge", "as": "salle", "variables": { "pilier": false } }),
        ),
        &files(&list),
    );
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(p["variables"]["salle.pilier"], json!(false));
    assert_eq!(p["variables"]["pilier.shown"], json!(false));
    // `visible` follows the variable through the reflection (a state derived from it), never copied at the tick.
    let pilier = node(scene(&p, "salle_room").unwrap(), "Pilier");
    let states = &pilier["entity"]["states"];
    let (state, decl) = states.as_object().unwrap().iter().next().unwrap();
    assert_eq!(decl, &json!({ "derived": { "var": "pilier.shown" } }));
    let reflect = pilier["reflect"].as_array().unwrap();
    assert!(
        reflect.contains(&json!({ "when": { state.clone(): true }, "set": { "visible": true } }))
    );
    assert!(
        reflect.contains(&json!({ "when": { state.clone(): false }, "set": { "visible": false } }))
    );
    assert!(pilier.get("variables").is_none());
}

#[test]
fn hide_takes_a_scene_out_of_the_flow_and_hides_a_node_and_overrides_patch_it() {
    let list = disk();
    let (p, issues) = resolve_project(
        &master(json!({ "file": "../base/salle.gripforge", "as": "salle",
            "hide": { "scenes": ["salle_maison"], "nodes": ["salle_room:Props/Caisse"] },
            "overrides": { "salle_room:#sun": { "light": { "intensity": 2.5 } } } })),
        &files(&list),
    );
    assert!(issues.is_empty(), "{issues:?}");
    assert!(scene(&p, "salle_maison").is_none());
    let room = scene(&p, "salle_room").unwrap();
    assert_eq!(node(room, "Caisse")["visible"], json!(false));
    assert_eq!(node(room, "Sun")["light"]["intensity"], json!(2.5));
}

#[test]
fn a_scene_disabled_by_a_variable_leaves_the_flow() {
    let list = disk();
    let on = resolve_project(
        &master(json!({ "file": "../base/salle.gripforge", "as": "salle" })),
        &files(&list),
    )
    .0;
    assert!(scene(&on, "salle_annexe").is_some());
    assert!(scene(&on, "salle_annexe").unwrap().get("enabled").is_none());
    let off = resolve_project(
        &master(json!({ "file": "../base/salle.gripforge", "as": "salle", "variables": { "annexe": false } })),
        &files(&list),
    )
    .0;
    assert!(scene(&off, "salle_annexe").is_none());
}

#[test]
fn include_missing() {
    let (_, issues) = resolve_project(
        &master(json!({ "file": "nowhere.gripforge", "as": "x" })),
        &files(&[]),
    );
    assert_eq!(codes(&issues), vec!["include_missing"]);
    assert!(issues[0].is_error());
    assert_eq!(issues[0].file, "nowhere.gripforge");
}

#[test]
fn include_cycle() {
    let list = vec![
        (
            "a.gripforge",
            json!({ "id": "a", "includes": [{ "file": "b.gripforge", "as": "b" }] }),
        ),
        (
            "b.gripforge",
            json!({ "id": "b", "includes": [{ "file": "a.gripforge", "as": "a" }] }),
        ),
    ];
    let (_, issues) = resolve_project(
        &json!({ "id": "m", "includes": [{ "file": "a.gripforge", "as": "a" }] }),
        &files(&list),
    );
    assert!(codes(&issues).contains(&"include_cycle"), "{issues:?}");
}

#[test]
fn the_depth_of_includes_is_bounded() {
    let list = vec![
        (
            "a.gripforge",
            json!({ "id": "a", "includes": [{ "file": "b.gripforge", "as": "b" }] }),
        ),
        ("b.gripforge", json!({ "id": "b" })),
    ];
    let m = json!({ "id": "m", "includes": [{ "file": "a.gripforge", "as": "a" }] });
    assert!(resolve_project_with(&m, &files(&list), 2).1.is_empty());
    assert!(codes(&resolve_project_with(&m, &files(&list), 1).1).contains(&"include_cycle"));
}

#[test]
fn include_conflict_unless_replace() {
    let list = disk();
    let mut m = master(json!({ "file": "../base/salle.gripforge", "as": "salle" }));
    m["data"]["archetypes"] = json!([{ "id": "box", "mass": 2 }]);
    let (_, issues) = resolve_project(&m, &files(&list));
    assert_eq!(codes(&issues), vec!["include_conflict"]);
    m["data"]["archetypes"] = json!([{ "id": "box", "mass": 2, "replace": true }]);
    let (p, issues) = resolve_project(&m, &files(&list));
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(p["data"]["archetypes"], json!([{ "id": "box", "mass": 2 }]));
}

#[test]
fn include_unknown_variable() {
    let list = disk();
    let (_, issues) = resolve_project(
        &master(
            json!({ "file": "../base/salle.gripforge", "as": "salle", "variables": { "colonne": false } }),
        ),
        &files(&list),
    );
    assert_eq!(codes(&issues), vec!["include_unknown_variable"]);
}

#[test]
fn node_variables_without_id() {
    let p = json!({ "id": "m", "data": { "scenes": [{ "id": "s", "nodes": [
        { "name": "A", "type": "group", "variables": { "on": true } }
    ] }] } });
    assert_eq!(
        codes(&resolve_project(&p, &files(&[])).1),
        vec!["node_variables_without_id"]
    );
}

#[test]
fn var_ref_not_allowed_and_var_unknown() {
    let p = json!({ "id": "m", "variables": { "a": 1 }, "data": { "scenes": [{ "id": "s", "nodes": [
        { "name": "A", "id": "a_node", "type": "shape", "position": { "$var": "a" } },
        { "name": "B", "id": "b_node", "type": "shape", "visible": { "$var": "nope" } }
    ] }] } });
    let c = codes(&resolve_project(&p, &files(&[])).1);
    assert!(c.contains(&"var_ref_not_allowed"), "{c:?}");
    assert!(c.contains(&"var_unknown"), "{c:?}");
}

#[test]
fn the_validator_and_the_pack_see_the_resolved_project() {
    let list = disk();
    let m = master(
        json!({ "file": "../base/salle.gripforge", "as": "salle", "variables": { "pilier": false } }),
    );
    let issues = validate_project(&m, &files(&list), &Registry::base());
    assert!(!codes(&issues).contains(&"field_unknown"), "{issues:?}");
    let version = |_: &str| Some("0.1.0".to_string());
    let game = pack(&m, &files(&list), &Registry::base(), &version).expect("packed");
    assert_eq!(game["project"]["variables"]["salle.pilier"], json!(false));
    assert_eq!(game["lock"]["order"], json!(["game.engine", "core.light"]));
    // Resolving twice gives the same project, byte for byte (dev.builder, devGame, the validator and the build agree).
    let once = resolve_project(&m, &files(&list)).0;
    let twice = resolve_project(&once, &files(&list)).0;
    assert_eq!(
        serde_json::to_string(&once).unwrap(),
        serde_json::to_string(&twice).unwrap()
    );
}

#[test]
fn the_variables_of_an_instanced_sub_scene_are_per_instance() {
    let shelf = json!({ "nodes": [
        { "name": "Book", "id": "book", "type": "shape",
          "variables": { "open": false }, "visible": { "$var": "book.open" } }
    ] });
    let list = vec![("scenes/shelf.json", shelf)];
    let p = json!({ "id": "m", "data": { "scenes": [{ "id": "s", "nodes": [
        { "name": "Shelf", "type": "scene", "scene": "scenes/shelf.json" },
        { "name": "Shelf2", "type": "scene", "scene": "scenes/shelf.json" }
    ] }], "scene_files": { "scenes/shelf.json": list[0].1.clone() } } });
    let (r, issues) = resolve_project(&p, &files(&list));
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(r["variables"]["shelf_book.open"], json!(false));
    assert_eq!(r["variables"]["shelf2_book.open"], json!(false));
    assert!(r["variables"].get("book.open").is_none());
    let ov = &r["data"]["scenes"][0]["nodes"][1]["overrides"]["book"];
    assert_eq!(
        ov["entity"]["states"]["var_shelf2_book_open"],
        json!({ "derived": { "var": "shelf2_book.open" } })
    );
    // The shared file keeps no per-instance binding.
    let book = &r["data"]["scene_files"]["scenes/shelf.json"]["nodes"][0];
    assert!(book.get("variables").is_none() && book.get("visible").is_none());
    let again = resolve_project(&r, &files(&list)).0;
    assert_eq!(again, r);
}

#[test]
fn the_formulas_of_an_included_file_read_its_prefixed_variables() {
    let sub = json!({ "id": "sub", "variables": { "coins": 0, "coins_max": 9 }, "data": { "scenes": [{ "id": "z", "on": [
        { "when": "entered", "do": [{ "set_var": { "path": "coins", "value": { "calc": "(var.coins ?? 0) + var.coins_max + var.other + novar.coins" } } }] }
    ] }] } });
    let list = vec![("sub.gripforge", sub)];
    let (r, _) = resolve_project(
        &json!({ "id": "m", "includes": [{ "file": "sub.gripforge", "as": "s" }] }),
        &files(&list),
    );
    let act = &r["data"]["scenes"][0]["on"][0]["do"][0]["set_var"];
    assert_eq!(act["path"], json!("s.coins"));
    assert_eq!(
        act["value"]["calc"],
        json!("(var.s.coins ?? 0) + var.s.coins_max + var.other + novar.coins")
    );
}

#[test]
fn a_reader_that_fetches_gets_the_paths_it_misses() {
    use gripforge_scene_format::include::resolve_with_files;
    let m = master(json!({ "file": "../base/salle.gripforge", "as": "salle" }));
    let first = resolve_with_files(&m, &serde_json::Map::new());
    assert_eq!(first["missing"], json!(["../base/salle.gripforge"]));
    let mut got = serde_json::Map::new();
    got.insert("../base/salle.gripforge".into(), salle());
    let second = resolve_with_files(&m, &got);
    assert_eq!(second["missing"], json!(["../base/scenes/maison.json"]));
    got.insert("../base/scenes/maison.json".into(), maison());
    let done = resolve_with_files(&m, &got);
    assert_eq!(done["missing"], json!([]));
    assert_eq!(done["issues"], json!([]));
    let list = disk();
    assert_eq!(done["project"], resolve_project(&m, &files(&list)).0);
}
