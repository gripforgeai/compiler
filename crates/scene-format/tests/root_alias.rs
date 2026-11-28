//! The root aliases of a path in a project (`#kits/…`, `#tools/…`, `#core/…`, `#tests/…`): the same table as the
//! `imports` of the kits repository's package.json, read from the root of the repository or of the project instead of
//! climbing (`../../../tests/…`). The format keeps the alias in the paths it asks the reader for (`missing`), and the
//! reader maps it (`path::on_disk` on a disk, the root URL in a page): one rule for gripforge-build, dev.builder and
//! devGame.

use gripforge_scene_format::include::{resolve_project, resolve_with_files};
use gripforge_scene_format::path::{
    on_disk, project_path_error, root_alias, root_alias_error, ROOT_ALIASES,
};
use serde_json::{json, Map, Value as Json};

fn files<'a>(list: &'a [(&'a str, Json)]) -> impl Fn(&str) -> Option<Json> + 'a {
    move |p| list.iter().find(|(k, _)| *k == p).map(|(_, v)| v.clone())
}

fn salle() -> Json {
    json!({
        "id": "salle",
        "requires": ["game.engine"],
        "variables": { "pilier": true },
        "installed": [{ "id": "game.engine", "config": { "startScene": "salle_room" } }],
        "data": { "scenes": [
            { "id": "salle_room", "role": "level", "requires": ["game.engine"], "nodes": [
                { "name": "Pilier", "id": "pilier", "type": "shape", "visible": { "$var": "pilier" } }
            ] },
            { "id": "salle_maison", "role": "level", "file": "../scenes/maison.json" }
        ] }
    })
}

fn maison() -> Json {
    json!({ "requires": ["game.engine"], "nodes": [{ "name": "Mur", "id": "mur", "type": "shape" }] })
}

fn master(file: &str) -> Json {
    json!({ "id": "demo", "requires": ["game.engine"], "includes": [{ "file": file, "as": "banc" }] })
}

#[test]
fn the_table_is_the_one_of_the_package_json_imports() {
    assert_eq!(ROOT_ALIASES, &["kits", "tools", "core", "tests"]);
    assert_eq!(
        root_alias("#tests/base-room/a.gripforge"),
        Some(("tests", "base-room/a.gripforge"))
    );
    assert_eq!(root_alias("../tests/a.gripforge"), None);
    assert_eq!(root_alias_error("#kits/a/b.json"), None);
    for bad in [
        "#nope/a.gripforge",
        "#tests",
        "#tests/",
        "#tests/../a.gripforge",
        "#tests//a",
        "#tests/a\\b",
    ] {
        assert!(root_alias_error(bad).is_some(), "{bad} is refused");
    }
}

/// An include written with an alias resolves to the same project as the same include written relative, and the
/// files the reader is asked for keep the alias (it maps them from its root, never from the project's folder).
#[test]
fn an_include_from_the_root_resolves_as_the_relative_one() {
    let rel = [
        ("../../../tests/salles/banc.gripforge", salle()),
        ("../../../tests/scenes/maison.json", maison()),
    ];
    let ali = [
        ("#tests/salles/banc.gripforge", salle()),
        ("#tests/scenes/maison.json", maison()),
    ];
    let (a, ia) = resolve_project(
        &master("../../../tests/salles/banc.gripforge"),
        &files(&rel),
    );
    let (b, ib) = resolve_project(&master("#tests/salles/banc.gripforge"), &files(&ali));
    assert!(ia.is_empty(), "{ia:?}");
    assert!(ib.is_empty(), "{ib:?}");
    assert_eq!(a, b);
    assert_eq!(
        b["data"]["scene_files"]["banc/../scenes/maison.json"],
        maison()
    );
    let asked = resolve_with_files(&master("#tests/salles/banc.gripforge"), &Map::new());
    assert_eq!(asked["missing"], json!(["#tests/salles/banc.gripforge"]));
}

#[test]
fn an_unknown_alias_or_a_climb_above_the_alias_is_an_error_and_nothing_is_read() {
    for file in ["#nope/banc.gripforge", "#tests/../kits/banc.gripforge"] {
        let asked = std::cell::RefCell::new(Vec::new());
        let read = |p: &str| {
            asked.borrow_mut().push(p.to_string());
            None
        };
        let (_, issues) = resolve_project(&master(file), &read);
        assert!(
            issues
                .iter()
                .any(|i| i.code == "include_missing" && i.message.contains("root alias")),
            "{file}: {issues:?}"
        );
        assert!(
            asked.borrow().is_empty(),
            "{file}: nothing read, asked {:?}",
            asked.borrow()
        );
    }
    // A relative path inside a file reached by an alias cannot climb above the alias either.
    let list = [(
        "#tests/banc.gripforge",
        json!({ "includes": [{ "file": "../../kits/x.gripforge" }] }),
    )];
    let (_, issues) = resolve_project(&master("#tests/banc.gripforge"), &files(&list));
    assert!(
        issues
            .iter()
            .any(|i| i.code == "include_missing" && i.message.contains("root alias")),
        "{issues:?}"
    );
}

#[test]
fn a_project_path_may_start_with_a_known_alias() {
    assert_eq!(project_path_error("#tests/base-room/assets/a.glb"), None);
    assert!(project_path_error("#nope/a.glb").is_some());
    assert!(project_path_error("#tests/../a.glb").is_some());
}

/// On a disk: from the nearest folder above the project that has a package.json declaring the alias in `imports`
/// (its target), else from the project's folder.
#[test]
fn on_a_disk_the_alias_is_read_from_the_root_of_the_repository() {
    let base = std::env::temp_dir().join(format!("gf-root-alias-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("kits/a/demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        base.join("package.json"),
        r##"{ "imports": { "#kits/*": "./kits/*", "#tests/*": "./tests/*" } }"##,
    )
    .unwrap();
    // A package.json without `imports` on the way is passed over.
    std::fs::write(base.join("kits/a/package.json"), r#"{ "name": "a" }"#).unwrap();
    assert_eq!(
        on_disk(&project, "#tests/salles/banc.gripforge"),
        base.join("tests/salles/banc.gripforge")
    );
    assert_eq!(
        on_disk(&project, "scenes/x.json"),
        project.join("scenes/x.json")
    );
    assert_eq!(on_disk(&project, "../x.json"), project.join("../x.json"));
    // No package.json declares `#core/*`: from the project's folder.
    assert_eq!(
        on_disk(&project, "#core/x.json"),
        project.join("core/x.json")
    );
    std::fs::remove_dir_all(&base).unwrap();
}
