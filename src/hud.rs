//! The asset HUD a project builds itself (its npm interface, `webui/`, built by Vite into `<project>/.build/hud/<id>/`:
//! module-kits `tools/project-ui`): a slot of core.webui's config `hud` in the built game.json that names an asset
//! (`"asset": "<id>@<range>"`) and no folder of its own (`file`) gets that folder, written relative to the game.json,
//! when `<project>/.build/hud/<id>/passport.json` exists. The same rule, at parity, as the build of a project in
//! module-kits (`tools/kit-modules/game-hud.mjs`, `gameHud`: the project's own asset before every other source) and
//! as dev.builder on the page (`kits/dev_builder/present/web/dev-hud.mjs`, `projectHudFiles`: `file:
//! ".build/hud/<id>/"` relative to project.gripforge, the same folder). npm never runs here: the CLI (or module-kits
//! `build.mjs --project`) builds `webui/` before.

use serde_json::Value as Json;
use std::path::{Component, Path, PathBuf};

/// Where a project's npm interface is built, under the project's folder (module-kits `tools/project-ui/project.mjs`).
pub const HUD_BUILT: &str = ".build/hud";

/// Gives each such slot of `game` (changed in place) its built folder, relative to `out` (the folder of game.json);
/// answers the slots given, `(key, file)`.
pub fn own_hud_files(project_dir: &Path, out: &Path, game: &mut Json) -> Vec<(String, String)> {
    let mut given = Vec::new();
    let Some(installed) = game.pointer_mut("/project/installed") else {
        return given;
    };
    let Some(webui) = installed
        .as_array_mut()
        .into_iter()
        .flatten()
        .find(|k| k["id"] == "core.webui" && k["enabled"] != false)
    else {
        return given;
    };
    let Some(hud) = webui
        .pointer_mut("/config/hud")
        .and_then(Json::as_object_mut)
    else {
        return given;
    };
    for (key, slot) in hud.iter_mut() {
        let Some(asset) = slot.get("asset").and_then(Json::as_str) else {
            continue;
        };
        if slot.get("file").is_some_and(|f| !f.is_null() && f != false) {
            continue;
        }
        let id = asset.split('@').next().unwrap_or(asset).to_string();
        let built = project_dir.join(HUD_BUILT).join(&id);
        if !built.join("passport.json").is_file() {
            continue;
        }
        let file = format!("{}/", relative(&built, out));
        slot["file"] = Json::String(file.clone());
        given.push((key.clone(), file));
    }
    given
}

/// `target` relative to `base` (both absolute), with `/` separators.
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

    fn project(name: &str, built: bool) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gfc-hud-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        if built {
            let a = d.join(HUD_BUILT).join("demo.npm.colors");
            std::fs::create_dir_all(&a).unwrap();
            std::fs::write(
                a.join("passport.json"),
                r#"{"id":"demo.npm.colors","version":"1.0.0"}"#,
            )
            .unwrap();
        }
        d
    }

    fn game() -> Json {
        json!({ "project": { "installed": [{ "id": "game.engine" }, { "id": "core.webui", "config": { "hud": {
            "demo.npm/colors": { "asset": "demo.npm.colors@^1" },
            "demo.npm/own": { "asset": "demo.npm.colors@^1", "file": "own/" },
            "demo.npm/none": { "asset": "demo.npm.none@^1" }
        } } }] } })
    }

    #[test]
    fn a_slot_naming_the_projects_built_asset_gets_its_folder_relative_to_game_json() {
        let d = project("built", true);
        let mut g = game();
        let given = own_hud_files(&d, &d.join(".build"), &mut g);
        assert_eq!(
            given,
            vec![(
                "demo.npm/colors".to_string(),
                "hud/demo.npm.colors/".to_string()
            )]
        );
        let hud = &g["project"]["installed"][1]["config"]["hud"];
        assert_eq!(
            hud["demo.npm/colors"]["file"], "hud/demo.npm.colors/",
            "the same folder dev.builder gives: .build/hud/<id>/ of the project"
        );
        assert_eq!(
            hud["demo.npm/own"]["file"], "own/",
            "a slot with its own folder is left as it is"
        );
        assert!(
            hud["demo.npm/none"].get("file").is_none(),
            "an asset the project did not build is left to the kits"
        );
    }

    #[test]
    fn nothing_built_nothing_given_and_another_out_folder_is_followed() {
        let d = project("none", false);
        let mut g = game();
        assert!(own_hud_files(&d, &d.join(".build"), &mut g).is_empty());
        let d = project("out", true);
        let mut g = game();
        let given = own_hud_files(&d, &d.join("dist/web"), &mut g);
        assert_eq!(given[0].1, "../../.build/hud/demo.npm.colors/");
    }
}
