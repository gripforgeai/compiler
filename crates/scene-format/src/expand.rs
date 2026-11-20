//! The scenes of a project, given to game.engine as inline scenes. A scene document of `data.scenes` with a `file`
//! gets the content lowered from that sub-scene; the sub-scenes come with the project, in `data.scene_files`
//! (`{ "<project path>": <parsed scene> }`, until their target form), put there by the loader of the project
//! (dev.builder's packing, a gateway): a kit never reads the disk. A project without any `file` is returned untouched, at no cost: the games of
//! today keep working until they are migrated (by hand).

use crate::issue::Issue;
use crate::lower::lower;
use crate::registry::Registry;
use crate::requires;
use serde_json::{json, Map, Value as Json};

/// Where the loader puts the parsed scene files in the project's data.
pub const SCENE_FILES: &str = "scene_files";
/// Where the issues of the scene files are left for game.engine's report.
pub const SCENE_FILE_ISSUES: &str = "scene_file_issues";
/// The keys of a scene document that belong to its content (in the file), not to the flow (in the project).
pub const CONTENT_KEYS: &[&str] = &[
    "entries",
    "instances",
    "world",
    "data",
    "layers",
    "audio",
    "ui",
    "native",
];

/// The installed kits of a project and their versions (`installed[]`, enabled only).
pub fn installed_of(project: &Json) -> Vec<(String, Option<String>)> {
    project
        .get("installed")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter(|k| k.get("enabled") != Some(&Json::Bool(false)))
        .filter_map(|k| {
            Some((
                k.get("id")?.as_str()?.to_string(),
                k.get("version").and_then(Json::as_str).map(str::to_string),
            ))
        })
        .collect()
}

/// The issue in the form of game.engine's report: `{ level, code, ref, message, fix }`, `ref` = `file#node`.
pub fn engine_issue(i: &Issue) -> Json {
    let mut r = i.file.clone();
    if !i.node.is_empty() {
        r = format!("{r}#{}", i.node);
    }
    if !i.field.is_empty() {
        r = format!("{r} {}", i.field);
    }
    json!({ "level": if i.is_error() { "error" } else { "warning" }, "code": i.code, "ref": r, "message": i.message, "fix": i.fix })
}

/// The project with its scene files lowered into `data.scenes` (and their archetypes and scripts added), or `None`
/// when no scene names a file nor is written inline (`nodes`).
pub fn expand_project(project: &Json, reg: &Registry) -> Option<Json> {
    let data = project.get("data")?;
    let scenes = data.get("scenes")?.as_array()?;
    if !scenes.iter().any(|s| {
        s.get("file").is_some_and(Json::is_string) || crate::build::inline_doc(s).is_some()
    }) {
        return None;
    }
    let empty = Map::new();
    let files = data
        .get(SCENE_FILES)
        .and_then(Json::as_object)
        .unwrap_or(&empty);
    let read = |p: &str| files.get(p).cloned();
    let installed = installed_of(project);
    let mut issues: Vec<Issue> = Vec::new();
    let mut archetypes: Vec<Json> = data
        .get("archetypes")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let mut scripts: Vec<Json> = data
        .get("scripts")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out_scenes = Vec::new();
    for doc in scenes {
        let (Some((file, tree)), Some(o)) = (
            crate::build::resolve_scene(doc, reg, &read),
            doc.as_object(),
        ) else {
            out_scenes.push(doc.clone());
            continue;
        };
        let file = file.as_str();
        let id = o.get("id").and_then(Json::as_str).unwrap_or("");
        issues.extend(tree.issues.iter().cloned());
        let missing: Vec<Issue> = requires::check(&tree.requires, &installed)
            .into_iter()
            .map(|i| i.in_file(file))
            .collect();
        let refused = missing.iter().any(|i| i.code == "kit_required_missing");
        issues.extend(missing);
        let mut scene = o.clone();
        scene.remove("file");
        for k in crate::build::SCENE_FILE_KEYS {
            scene.remove(*k);
        }
        if refused {
            // The scene is refused as a whole: nothing of it is placed, the flow entry stays so the game still runs.
            for k in CONTENT_KEYS {
                scene.remove(*k);
            }
            out_scenes.push(Json::Object(scene));
            continue;
        }
        let low = lower(id, &tree, reg);
        issues.extend(low.issues);
        if CONTENT_KEYS.iter().any(|k| scene.contains_key(*k)) {
            issues.push(Issue::warning("scene_content_twice", format!("the scene {id} has content in the project and a file: the file wins"), "keep the content in the file only (instances, entries, data... belong to the scene named by file)").in_file(file));
            for k in CONTENT_KEYS {
                scene.remove(*k);
            }
        }
        scene.extend(low.content);
        for a in low.archetypes {
            if !archetypes.iter().any(|x| x.get("id") == a.get("id")) {
                archetypes.push(a);
            }
        }
        scripts.extend(low.scripts);
        out_scenes.push(Json::Object(scene));
    }
    let mut p = project.clone();
    let d = p["data"].as_object_mut()?;
    d.remove(SCENE_FILES);
    d.insert("scenes".into(), Json::Array(out_scenes));
    d.insert("archetypes".into(), Json::Array(archetypes));
    d.insert("scripts".into(), Json::Array(scripts));
    d.insert(
        SCENE_FILE_ISSUES.into(),
        Json::Array(issues.iter().map(engine_issue).collect()),
    );
    Some(p)
}
