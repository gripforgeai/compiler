//! gripforge-build reads a file included from the root (`#tests/…`, a root alias of the format) where the kits
//! repository's readers read it: under the folder its package.json `imports` names, never beside the project.

use gripforge_compiler::project::project_files;
use serde_json::json;

#[test]
fn an_include_from_the_root_is_read_from_the_root_of_the_repository() {
    let base = std::env::temp_dir().join(format!("gf-build-root-alias-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let demo = base.join("kits/a/demo");
    std::fs::create_dir_all(&demo).unwrap();
    std::fs::create_dir_all(base.join("tests/salles/scenes")).unwrap();
    std::fs::write(
        base.join("package.json"),
        r##"{ "imports": { "#tests/*": "./tests/*" } }"##,
    )
    .unwrap();
    let salle = json!({ "id": "salle", "requires": ["game.engine"], "data": { "scenes": [
        { "id": "salle_room", "role": "level", "file": "scenes/room.json" } ] } });
    let room =
        json!({ "requires": ["game.engine"], "nodes": [{ "name": "Mur", "type": "shape" }] });
    std::fs::write(base.join("tests/salles/banc.gripforge"), salle.to_string()).unwrap();
    std::fs::write(base.join("tests/salles/scenes/room.json"), room.to_string()).unwrap();
    let project = json!({ "id": "demo", "requires": ["game.engine"],
        "includes": [{ "file": "#tests/salles/banc.gripforge", "as": "banc" }] });
    let files = project_files(&project, &demo).unwrap();
    assert_eq!(files.get("#tests/salles/banc.gripforge"), Some(&salle));
    assert_eq!(files.get("#tests/salles/scenes/room.json"), Some(&room));
    std::fs::remove_dir_all(&base).unwrap();
}
