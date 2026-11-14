//! The `gfscene` tool on a project folder: the project file found (`project.gripforge` first, else the only
//! `*.gripforge`), `build` writing the published form with a library asset resolved by its folder, `bundle` writing
//! one file, and `validate` passing on warnings only.

use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gfscene-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(p: &Path, v: &Json) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn read(p: &Path) -> Json {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn gfscene(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_gfscene"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr),
    )
}

/// A project named arena.gripforge, one scene in its file, a crate in the library.
fn project_folder(name: &str) -> PathBuf {
    let d = dir(name);
    write(
        &d.join("arena.gripforge"),
        &json!({ "id": "arena", "requires": ["game.engine"], "installed": [{ "id": "game.engine", "version": "1.0.0" }],
            "data": { "scenes": [{ "id": "level", "file": "scenes/level.gfscene" }] } }),
    );
    write(
        &d.join("scenes/level.gfscene"),
        &json!({ "requires": ["game.engine"], "nodes": [
            { "name": "Crate", "type": "model", "model": { "asset": "crate" }, "position": "nowhere" }
        ] }),
    );
    write(&d.join("library/crate/passport.json"), &json!({}));
    std::fs::write(d.join("library/crate/crate.glb"), b"glb").unwrap();
    d
}

#[test]
fn the_project_file_of_a_folder_is_found_and_built() {
    let d = project_folder("build");
    let src = d.to_str().unwrap();
    let (code, out) = gfscene(&["validate", src]);
    assert_eq!(code, 0, "warnings only: {out}");
    assert!(out.contains("transform_invalid"), "{out}");
    let pub_dir = d.join("out");
    let (code, out) = gfscene(&["build", src, "--out", pub_dir.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    let level = read(&pub_dir.join("scenes/level.gfscene"));
    assert_eq!(
        level["nodes"][0]["model"]["asset"],
        json!("library/crate/crate.glb")
    );
    assert_eq!(
        read(&pub_dir.join("project.gripforge"))["data"]["scenes"][0]["file"],
        json!("scenes/level.gfscene")
    );
    assert_eq!(
        read(&pub_dir.join("build.json"))["scenes"][0]["assets"],
        json!(["library/crate/crate.glb"])
    );
    // The published folder is a project too: project.gripforge is found in it.
    let (code, out) = gfscene(&["validate", pub_dir.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    let one = d.join("one/project.gripforge");
    let (code, out) = gfscene(&["bundle", src, "--out", one.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(read(&one)["data"]["scenes"][0].get("nodes").is_some());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn project_gripforge_comes_first_and_two_other_names_are_refused() {
    let d = project_folder("names");
    write(
        &d.join("other.gripforge"),
        &json!({ "id": "other", "data": { "scenes": [] } }),
    );
    let (code, out) = gfscene(&["validate", d.to_str().unwrap()]);
    assert_eq!(code, 2, "two *.gripforge, no project.gripforge: {out}");
    assert!(out.contains("several"), "{out}");
    write(
        &d.join("project.gripforge"),
        &json!({ "id": "main", "data": { "scenes": [] } }),
    );
    let (code, out) = gfscene(&["validate", d.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("scenes_missing"),
        "project.gripforge was read: {out}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn pack_present_writes_a_package_the_players_open() {
    // The players read a model slot's file under the folder of present.json, at a path that starts with `assets/`
    // (core/present-abi safe_path; player/native replay.rs package_definition): pack lays the model files out so.
    let d = dir("present");
    write(
        &d.join("project.gripforge"),
        &json!({ "id": "room", "requires": ["game.engine"], "installed": [{ "id": "game.engine", "version": "1.0.0" }],
            "data": { "scenes": [
                { "id": "level", "file": "scenes/level.gfscene" },
                { "id": "yard", "nodes": [{ "name": "Rock", "type": "model", "model": { "asset": "models/rock.glb" } }] }
            ] } }),
    );
    write(
        &d.join("scenes/level.gfscene"),
        &json!({ "requires": ["game.engine"], "nodes": [
            { "name": "Crate", "type": "model", "model": { "asset": "models/crate.glb" } }
        ] }),
    );
    std::fs::create_dir_all(d.join("models")).unwrap();
    std::fs::write(d.join("models/crate.glb"), b"glb-crate").unwrap();
    std::fs::write(d.join("models/rock.glb"), b"glb-rock").unwrap();
    let pkg = d.join("package");
    std::fs::create_dir_all(&pkg).unwrap();
    let present = pkg.join("present.json");
    let (code, out) = gfscene(&[
        "pack",
        d.to_str().unwrap(),
        "--out",
        pkg.join("game.json").to_str().unwrap(),
        "--present",
        present.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}");
    let models = read(&present)["models"].clone();
    assert_eq!(
        models,
        json!({ "models/crate.glb": "assets/models/crate.glb", "models/rock.glb": "assets/models/rock.glb" }),
        "the slot is the project path, the file a package path under assets/ (inline scenes too)"
    );
    assert_eq!(
        std::fs::read(pkg.join("assets/models/crate.glb")).unwrap(),
        b"glb-crate"
    );
    assert_eq!(
        std::fs::read(pkg.join("assets/models/rock.glb")).unwrap(),
        b"glb-rock"
    );
    let _ = std::fs::remove_dir_all(&d);
}
