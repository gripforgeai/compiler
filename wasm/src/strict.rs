//! What a build checks beyond the tolerant validator of core/scene-format: what the dev mode lets pass with a default
//! but a compiled game must not guess. Generic: it reads the kits' declarations (their kit.api.json), never a kit.
//!
//! - `prop_out_of_range`: a property of a kit's section outside the `min` / `max` its declaration gives;
//! - `scene_empty`: a scene with nothing in it (no node, no section, no file, no content of game.engine);
//! - `scene_id_duplicate`: two scenes of the project with the same id;
//! - `scene_unknown`: a reference to a scene the project does not have (`next` of a scene, `load_scene` of a binding);
//! - the versions of the kits installed without one, taken from the catalog, so that a `requires` with a version
//!   range is checked (`kit_version_mismatch` of the core) as the lock of the built game will hold them.

use gripforge_scene_format::build::{inline_label, resolve_scene};
use gripforge_scene_format::Registry;
use serde_json::{json, Map, Value as Json};

/// The keys of a scene of the project that hold game.engine's own content (a scene may be only that).
const CONTENT_KEYS: &[&str] = &["instances", "entries", "zones", "rules", "hud", "ui"];

/// The project with the version of every installed kit that names none, from the catalog.
pub fn with_versions(project: &Json, kits: &[Json]) -> Json {
    let mut p = project.clone();
    for k in p
        .get_mut("installed")
        .and_then(Json::as_array_mut)
        .into_iter()
        .flatten()
    {
        if k.get("version").is_some() {
            continue;
        }
        let id = k.get("id").and_then(Json::as_str).unwrap_or("");
        if let Some(v) = kits
            .iter()
            .find(|a| a.get("id").and_then(Json::as_str) == Some(id))
            .and_then(|a| a.get("version"))
        {
            k["version"] = v.clone();
        }
    }
    p
}

/// The issues of the build's own rules, as JSON (errors).
pub fn check(
    project: &Json,
    files: &Map<String, Json>,
    kits: &[Json],
    reg: &Registry,
) -> Vec<Json> {
    let mut out = Vec::new();
    let scenes: Vec<&Json> = project
        .pointer("/data/scenes")
        .and_then(Json::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let ids: Vec<&str> = scenes
        .iter()
        .filter_map(|s| s.get("id").and_then(Json::as_str))
        .collect();
    for (i, id) in ids.iter().enumerate() {
        if ids[..i].contains(id) {
            out.push(error(
                "scene_id_duplicate",
                &inline_label(id),
                id,
                "",
                "",
                format!("two scenes of the project have the id {id}"),
                format!("give each scene its own id (rename the second {id})"),
            ));
        }
    }
    let carried = gripforge_scene_format::build::carried(project);
    let read = |p: &str| carried.get(p).cloned().or_else(|| files.get(p).cloned());
    for s in &scenes {
        let Some(id) = s.get("id").and_then(Json::as_str) else {
            continue;
        };
        let label = s
            .get("file")
            .and_then(Json::as_str)
            .map_or_else(|| inline_label(id), str::to_string);
        if let Some(next) = s.get("next").and_then(Json::as_str) {
            if !ids.contains(&next) {
                out.push(unknown_scene(&label, id, "", "next", next, &ids));
            }
        }
        let content = CONTENT_KEYS.iter().any(|k| {
            s.get(*k)
                .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
        });
        let Some((_, tree)) = resolve_scene(s, reg, &read) else {
            if !content {
                out.push(error(
                    "scene_empty",
                    &label,
                    id,
                    "",
                    "",
                    format!("the scene {id} has nothing in it: no nodes, no file, no content"),
                    "write its nodes (\"nodes\": [ ... ]) or remove the scene",
                ));
            }
            continue;
        };
        if tree.nodes.is_empty() && tree.kits.is_empty() && !content {
            out.push(error(
                "scene_empty",
                &label,
                id,
                "",
                "nodes",
                format!("the scene {id} has nothing in it: no nodes, no section of a kit"),
                "write its nodes (\"nodes\": [ ... ]) or remove the scene",
            ));
        }
        bindings(
            &Json::Array(tree.on.clone()),
            &label,
            id,
            "",
            &ids,
            &mut out,
        );
        for (kit, w) in &tree.kits {
            bounds(kits, kit, w, "scene", &label, id, "", &mut out);
        }
        for n in &tree.nodes {
            bindings(
                n.raw.get("on").unwrap_or(&Json::Null),
                &label,
                id,
                &n.path,
                &ids,
                &mut out,
            );
            for (kit, w) in n
                .raw
                .get("kits")
                .and_then(Json::as_object)
                .into_iter()
                .flatten()
            {
                bounds(kits, kit, w, "node", &label, id, &n.path, &mut out);
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments, reason = "one issue, located")]
fn error(
    code: &str,
    file: &str,
    scene: &str,
    node: &str,
    field: &str,
    message: String,
    fix: impl Into<String>,
) -> Json {
    let mut o = json!({ "level": "error", "code": code, "file": file, "scene": scene, "message": message, "fix": fix.into() });
    if !node.is_empty() {
        o["node"] = json!(node);
    }
    if !field.is_empty() {
        o["field"] = json!(field);
    }
    o
}

fn unknown_scene(
    file: &str,
    scene: &str,
    node: &str,
    field: &str,
    target: &str,
    ids: &[&str],
) -> Json {
    error(
        "scene_unknown",
        file,
        scene,
        node,
        field,
        format!("{target} is not a scene of the project"),
        format!("name one of the scenes: {}", ids.join(", ")),
    )
}

/// The `load_scene` of a list of bindings (`on`), anywhere inside it.
fn bindings(on: &Json, file: &str, scene: &str, node: &str, ids: &[&str], out: &mut Vec<Json>) {
    let mut stack = vec![(on, "on".to_string())];
    while let Some((v, at)) = stack.pop() {
        match v {
            Json::Array(a) => {
                stack.extend(a.iter().enumerate().map(|(i, x)| (x, format!("{at}[{i}]"))))
            }
            Json::Object(o) => {
                if let Some(t) = o
                    .get("load_scene")
                    .and_then(|l| l.get("scene"))
                    .and_then(Json::as_str)
                {
                    if !ids.contains(&t) {
                        out.push(unknown_scene(
                            file,
                            scene,
                            node,
                            &format!("{at}.load_scene.scene"),
                            t,
                            ids,
                        ));
                    }
                }
                stack.extend(o.iter().map(|(k, x)| (x, format!("{at}.{k}"))));
            }
            _ => {}
        }
    }
}

/// The numbers of a written section outside the `min` / `max` of the element its kit declares there.
#[allow(clippy::too_many_arguments, reason = "one section, located")]
fn bounds(
    kits: &[Json],
    kit: &str,
    written: &Json,
    at: &str,
    file: &str,
    scene: &str,
    node: &str,
    out: &mut Vec<Json>,
) {
    let Some(w) = written.as_object() else { return };
    let Some(api) = kits
        .iter()
        .find(|a| a.get("id").and_then(Json::as_str) == Some(kit))
    else {
        return;
    };
    let elements: Vec<&Json> = api
        .get("expose")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter(|e| gripforge_scene_format::registry::for_scene(e))
        .filter(|e| e.get("at").and_then(Json::as_str).unwrap_or("node") == at)
        .collect();
    let named = w.get("element").and_then(Json::as_str);
    let el = match named {
        Some(n) => elements.iter().find(|e| {
            e.get("id")
                .and_then(Json::as_str)
                .is_some_and(|id| id.rsplit('.').next() == Some(n))
        }),
        None => elements.first(),
    };
    let Some(props) = el.and_then(|e| e.get("props")).and_then(Json::as_object) else {
        return;
    };
    for (k, v) in w {
        let (Some(x), Some(f)) = (v.as_f64(), props.get(k)) else {
            continue;
        };
        let (min, max) = (
            f.get("min").and_then(Json::as_f64),
            f.get("max").and_then(Json::as_f64),
        );
        if min.is_some_and(|m| x < m) || max.is_some_and(|m| x > m) {
            let range = match (min, max) {
                (Some(a), Some(b)) => format!("between {a} and {b}"),
                (Some(a), None) => format!("at least {a}"),
                (None, Some(b)) => format!("at most {b}"),
                _ => unreachable!("one bound is crossed"),
            };
            out.push(error(
                "prop_out_of_range",
                file,
                scene,
                node,
                &format!("kits.{kit}.{k}"),
                format!("{kit} takes {k} {range}, {x} is written"),
                format!("write {k} as a number {range}"),
            ));
        }
    }
}
