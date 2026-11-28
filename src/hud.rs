//! The asset HUD a project brings itself, named by its slot in the built game.json: a slot of core.webui's config `hud`
//! that names an asset (`"asset": "<id>@<range>"`) and no folder of its own (`file`) gets, written relative to the
//! game.json, the first folder of the project that holds that asset (`passport.json`). First what the project builds
//! itself from its npm interface, `<project>/.build/hud/<id>/` (`webui/`, built by Vite before: the CLI, or module-kits
//! `build.mjs --project`; npm never runs here); then the copy the project carries beside the file that declares the
//! slot, `<its folder>/hud/<id>/` (referenced, never copied); else the slot is left to the assets the kits bring. The
//! same rules, at parity, as module-kits' build of a project (`tools/kit-modules/game-hud.mjs`, `gameHud`) and
//! dev.builder on the page (`kits/dev_builder/present/web/dev-hud.mjs`: `hudSlotsOf`, `projectFolders`,
//! `projectHudFiles`).

use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Where a project's npm interface is built, under the project's folder (module-kits `tools/project-ui/project.mjs`).
pub const HUD_BUILT: &str = ".build/hud";

/// The slots of ONE project document that name an asset and no folder, `(key, spec)`, and the files it includes
/// (module-kits dev-hud.mjs `hudSlotsOf`).
pub fn hud_slots_of(doc: &Json) -> (Vec<(String, String)>, Vec<String>) {
    let own = |key: &str, e: &Json| -> Option<(String, String)> {
        let asset = e.get("asset")?.as_str()?;
        let file = e.get("file");
        if file.is_some_and(|f| !f.is_null() && f != false) {
            return None;
        }
        Some((key.to_string(), asset.to_string()))
    };
    if let Some(list) = doc.get("hud").and_then(Json::as_array) {
        let slots = list
            .iter()
            .filter_map(|e| own(e.get("slot").and_then(Json::as_str).unwrap_or(""), e))
            .collect();
        return (slots, Vec::new());
    }
    let webui = doc
        .get("installed")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .find(|k| k["id"] == "core.webui" && k["enabled"] != false);
    let slots = webui
        .and_then(|w| w.pointer("/config/hud"))
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(k, e)| own(k, e))
        .collect();
    let includes = doc
        .get("includes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|i| i.get("file").and_then(Json::as_str).map(String::from))
        .collect();
    (slots, includes)
}

/// The folders of the project where the asset `id` of a slot is looked for, in order, as absolute paths: the build of
/// webui/ under `project_dir`, then the copy beside the declaring file in `from_dir` (module-kits `projectFolders`).
pub fn project_folders(project_dir: &Path, from_dir: &Path, id: &str) -> [PathBuf; 2] {
    [
        project_dir.join(HUD_BUILT).join(id),
        from_dir.join("hud").join(id),
    ]
}

/// The slots of the project file `file` and the files it includes, read from the disk: `(key, spec, folder of the file)`.
fn slots(file: &Path, seen: &mut BTreeSet<PathBuf>, out: &mut Vec<(String, String, PathBuf)>) {
    let file = normalize(file);
    if !seen.insert(file.clone()) {
        return;
    }
    let Ok(doc) = std::fs::read_to_string(&file)
        .map(|t| serde_json::from_str::<Json>(&t).unwrap_or(Json::Null))
    else {
        return;
    };
    let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    let (mine, includes) = hud_slots_of(&doc);
    out.extend(mine.into_iter().map(|(k, s)| (k, s, dir.clone())));
    for inc in includes {
        slots(&dir.join(inc), seen, out);
    }
}

/// Gives each such slot of `game` (changed in place) its folder, relative to `out` (the folder of game.json); answers
/// the slots given, `(key, file)`.
pub fn own_hud_files(project_file: &Path, out: &Path, game: &mut Json) -> Vec<(String, String)> {
    let mut given: Vec<(String, String)> = Vec::new();
    let project_dir = normalize(project_file.parent().unwrap_or(Path::new(".")));
    let mut found = Vec::new();
    slots(project_file, &mut BTreeSet::new(), &mut found);
    let Some(installed) = game
        .pointer_mut("/project/installed")
        .and_then(Json::as_array_mut)
    else {
        return given;
    };
    let Some(hud) = installed
        .iter_mut()
        .find(|k| k["id"] == "core.webui" && k["enabled"] != false)
        .and_then(|w| w.pointer_mut("/config/hud"))
        .and_then(Json::as_object_mut)
    else {
        return given;
    };
    for (key, spec, from_dir) in found {
        let Some(slot) = hud.get_mut(&key).filter(|s| s.is_object()) else {
            continue;
        };
        if slot.get("file").is_some_and(|f| !f.is_null() && f != false)
            || given.iter().any(|(k, _)| *k == key)
        {
            continue;
        }
        let id = spec.split('@').next().unwrap_or(&spec);
        let Some(folder) = project_folders(&project_dir, &from_dir, id)
            .into_iter()
            .find(|f| f.join("passport.json").is_file())
        else {
            continue;
        };
        let file = format!("{}/", relative(&normalize(&folder), &normalize(out)));
        slot["file"] = Json::String(file.clone());
        given.push((key, file));
    }
    given
}

/// `p` absolute, without `.` nor `..` (lexically: the folders may not exist yet).
fn normalize(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// `target` relative to `base` (both normalized), with `/` separators.
fn relative(target: &Path, base: &Path) -> String {
    let t: Vec<Component> = target.components().collect();
    let b: Vec<Component> = base.components().collect();
    let common = t.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out: PathBuf = std::iter::repeat_n("..", b.len() - common).collect();
    for c in &t[common..] {
        out.push(c.as_os_str());
    }
    out.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn asset(dir: &Path, id: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("passport.json"),
            format!(r#"{{"id":"{id}","version":"1.0.0"}}"#),
        )
        .unwrap();
    }

    /// A project with a slot of its own file and one of an included file, both naming an asset without `file`.
    fn project(name: &str) -> (PathBuf, Json) {
        let d = std::env::temp_dir().join(format!("gfc-hud-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("parts")).unwrap();
        let hud = json!({ "demo.npm/colors": { "asset": "demo.npm.colors@^1" }, "demo.npm/own": { "asset": "demo.npm.colors@^1", "file": "own/" }, "demo.npm/none": { "asset": "demo.npm.none@^1" } });
        let doc = json!({ "id": name, "includes": [{ "file": "parts/hud.gripforge" }], "installed": [{ "id": "core.webui", "config": { "hud": hud } }] });
        std::fs::write(d.join("project.gripforge"), doc.to_string()).unwrap();
        let part = json!({ "installed": [{ "id": "core.webui", "config": { "hud": { "game/side": { "asset": "demo.npm.side@^1" } } } }] });
        std::fs::write(d.join("parts/hud.gripforge"), part.to_string()).unwrap();
        let mut merged = hud.clone();
        merged["game/side"] = json!({ "asset": "demo.npm.side@^1" });
        (
            d,
            json!({ "project": { "installed": [{ "id": "game.engine" }, { "id": "core.webui", "config": { "hud": merged } }] } }),
        )
    }

    #[test]
    fn the_projects_build_of_webui_then_its_copy_beside_the_declaring_file_relative_to_game_json() {
        let (d, mut g) = project("order");
        asset(&d.join("hud/demo.npm.colors"), "demo.npm.colors");
        asset(&d.join(".build/hud/demo.npm.colors"), "demo.npm.colors");
        asset(&d.join("parts/hud/demo.npm.side"), "demo.npm.side");
        let given = own_hud_files(&d.join("project.gripforge"), &d.join(".build"), &mut g);
        assert_eq!(
            given,
            vec![("demo.npm/colors".to_string(), "hud/demo.npm.colors/".to_string()), ("game/side".to_string(), "../parts/hud/demo.npm.side/".to_string())],
            "the build of webui/ first (.build/hud/<id>/), the copy beside the included file for its slot"
        );
        let hud = &g["project"]["installed"][1]["config"]["hud"];
        assert_eq!(
            hud["demo.npm/own"]["file"], "own/",
            "a slot with its own folder is left as it is"
        );
        assert!(
            hud["demo.npm/none"].get("file").is_none(),
            "an asset the project does not bring is left to the kits"
        );
    }

    #[test]
    fn the_carried_copy_alone_nothing_brought_nothing_given_and_another_out_folder() {
        let (d, mut g) = project("carried");
        asset(&d.join("hud/demo.npm.colors"), "demo.npm.colors");
        let given = own_hud_files(&d.join("project.gripforge"), &d.join("dist/web"), &mut g);
        assert_eq!(
            given,
            vec![(
                "demo.npm/colors".to_string(),
                "../../hud/demo.npm.colors/".to_string()
            )]
        );
        let (d, mut g) = project("none");
        assert!(own_hud_files(&d.join("project.gripforge"), &d.join(".build"), &mut g).is_empty());
    }
}
