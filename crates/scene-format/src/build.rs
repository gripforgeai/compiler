//! The scenes of a project as the project holds them (docs/dev-builder.md): written inline in `data.scenes[].nodes`,
//! the sub-scenes they instance carried in `data.scene_files` by their path (until their target form). What
//! dev.builder (reading the project at run time) and the compiler (gripforge-build, its own copy of this crate) share
//! to resolve a scene: validated, sub-scenes and overrides applied, one tree per scene.

use crate::expand::SCENE_FILES;
use crate::registry::Registry;
use crate::tree::{resolve, resolve_doc, Files, Tree};
use serde_json::{Map, Value as Json};

/// The keys of a scene's content, held inline by a scene of the project (and by a carried sub-scene).
pub const SCENE_FILE_KEYS: &[&str] = &["requires", "nodes", "kits", "on", "meta"];

/// The scene content written inline in a scene of the project (it has `nodes` and no `file`), or `None`.
pub fn inline_doc(scene: &Json) -> Option<Json> {
    let o = scene.as_object()?;
    if o.contains_key("file") || !o.contains_key("nodes") {
        return None;
    }
    let mut doc = Map::new();
    for k in SCENE_FILE_KEYS {
        if let Some(v) = o.get(*k) {
            doc.insert((*k).to_string(), v.clone());
        }
    }
    Some(Json::Object(doc))
}

/// The name an inline scene carries in the issues, in place of a path (`#annex`).
pub fn inline_label(id: &str) -> String {
    format!("#{id}")
}

/// The sub-scenes carried in the project itself (`data.scene_files`, by their path).
pub fn carried(project: &Json) -> Map<String, Json> {
    project
        .pointer(&format!("/data/{SCENE_FILES}"))
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default()
}

/// A scene of the project resolved, inline or from the path it names, with the name its issues carry (`None`: neither).
pub fn resolve_scene(scene: &Json, reg: &Registry, files: Files) -> Option<(String, Tree)> {
    if let Some(f) = scene.get("file").and_then(Json::as_str) {
        return Some((f.to_string(), resolve(f, reg, files)));
    }
    let id = scene.get("id").and_then(Json::as_str).unwrap_or("");
    let doc = inline_doc(scene)?;
    let label = inline_label(id);
    Some((label.clone(), resolve_doc(&label, &doc, reg, files)))
}
