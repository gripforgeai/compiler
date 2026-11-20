//! The project file (`<name>.gripforge`): its validation (its own `requires`, its scenes and their files, every scene
//! checked down to its sub-scenes) and its packing into the `game.json` today's loaders read, the scene files carried
//! in `data.scene_files` for game.engine to lower (crate::expand).

use crate::expand::{installed_of, SCENE_FILES};
use crate::issue::{has_errors, Issue};
use crate::lower::lower;
use crate::registry::Registry;
use crate::requires;
use crate::tree::{check_bindings, resolve, Files};
use serde_json::{json, Map, Value as Json};
use std::cell::RefCell;

/// The keys of a project file (the `project` block of a game.json, plus `requires` and `run`); `$schema` is for the
/// editors only (the path of gripforge.full.schema.json, docs/ecrire-un-projet.md), kept and never read; `sources`
/// lists the Rust files of the game's code (crate::sources), compiled into game.main, not packed into the game.
const PROJECT_KEYS: &[&str] = &[
    "$schema",
    "sources",
    "id",
    "name",
    "requires",
    "installed",
    "bindings",
    "variables",
    "data",
    "preset",
    "schema",
    "run",
    "meta",
];
/// What a scene of the project keeps (its flow, game.engine's) besides `file`.
const FLOW_KEYS: &[&str] = &[
    "id",
    "title",
    "kind",
    "role",
    "next",
    "loading",
    "background",
    "unload",
    "transition",
    "below",
    "file",
];

/// Check a whole project: the file itself, its required kits, each scene file and what game.engine will get of it.
pub fn validate_project(project: &Json, files: Files, reg: &Registry) -> Vec<Issue> {
    // The sub-scene files a project in one file carries (`data.scene_files`) come before the disk.
    let carried = crate::build::carried(project);
    let all = |p: &str| carried.get(p).cloned().or_else(|| files(p));
    let files: Files = &all;
    let mut out = Vec::new();
    let Some(o) = project.as_object() else {
        return vec![Issue::warning("file_invalid", "the project file is not a JSON object", "write the project as { \"id\": ..., \"requires\": [...], \"installed\": [...], \"data\": { \"scenes\": [...] } }")];
    };
    for k in o.keys().filter(|k| !PROJECT_KEYS.contains(&k.as_str())) {
        out.push(
            Issue::warning(
                "field_unknown",
                format!("the project has an unknown key {k}: it is kept and not read"),
                format!("remove {k}, or move it under meta"),
            )
            .at("", k),
        );
    }
    if o.get("id")
        .and_then(Json::as_str)
        .is_none_or(|s| !crate::path::is_engine_id(s))
    {
        out.push(
            Issue::warning(
                "project_id_invalid",
                "the project has no valid id",
                "write \"id\": a lowercase id (letters, digits, _), like \"arena\"",
            )
            .at("", "id"),
        );
    }
    out.extend(crate::sources::check(project));
    let installed = installed_of(project);
    let reqs = requires::read(o.get("requires"), &mut out);
    out.extend(requires::check(&reqs, &installed));
    // The project level of each kit, as the kit describes it (its configSchema).
    for k in o
        .get("installed")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(id), Some(c)) = (k.get("id").and_then(Json::as_str), k.get("config")) {
            out.extend(crate::registry::check_config(reg, id, c));
        }
    }
    let scenes = project
        .pointer("/data/scenes")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    if scenes.is_empty() {
        out.push(Issue::warning("scenes_missing", "the project has no scene", "list the scenes in data.scenes: [{ \"id\": \"level\", \"role\": \"level\", \"file\": \"scenes/level.gfscene\" }]").at("", "data.scenes"));
    }
    let start = project
        .pointer("/installed")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .find(|k| k.get("id") == Some(&json!("game.engine")))
        .and_then(|k| k.pointer("/config/startScene"))
        .and_then(Json::as_str);
    if let Some(s) = start.filter(|s| !scenes.iter().any(|x| x.get("id") == Some(&json!(s)))) {
        out.push(
            Issue::warning(
                "start_scene_unknown",
                format!("the start scene {s} is not in data.scenes"),
                "set game.engine's config.startScene to the id of a listed scene",
            )
            .at("", "installed.game.engine.config.startScene"),
        );
    }
    for (i, s) in scenes.iter().enumerate() {
        let id = s.get("id").and_then(Json::as_str).unwrap_or("");
        let at = format!("data.scenes[{i}]");
        if !crate::path::is_engine_id(id) {
            out.push(
                Issue::warning(
                    "scene_id_invalid",
                    format!("{at} has no valid id"),
                    "give the scene a lowercase id (letters, digits, _)",
                )
                .at("", &at),
            );
            continue;
        }
        let Some(file) = s.get("file").and_then(Json::as_str) else {
            // A scene written inline in the project is checked as its file would be.
            if let Some(doc) = crate::build::inline_doc(s) {
                let label = crate::build::inline_label(id);
                let tree = crate::tree::resolve_doc(&label, &doc, reg, files);
                out.extend(check_tree(id, &label, tree, reg, Some(&installed)));
            }
            continue;
        };
        for k in s
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
            .filter(|k| !FLOW_KEYS.contains(&k.as_str()))
        {
            out.push(
                Issue::warning(
                    "scene_content_twice",
                    format!("the scene {id} has {k} in the project and a file: the file wins"),
                    format!("move {k} into {file}"),
                )
                .at("", &format!("{at}.{k}")),
            );
        }
        out.extend(validate_scene(id, file, files, reg, Some(&installed)));
    }
    out
}

/// Check one scene file: its nodes, sub-scenes, sections, bindings and required kits (`installed`: the project's
/// kits, `None` to skip the check of installed kits).
pub fn validate_scene(
    id: &str,
    file: &str,
    files: Files,
    reg: &Registry,
    installed: Option<&[(String, Option<String>)]>,
) -> Vec<Issue> {
    check_tree(id, file, resolve(file, reg, files), reg, installed)
}

/// The issues of a resolved scene (`file`: its path, or the label of an inline scene).
fn check_tree(
    id: &str,
    file: &str,
    tree: crate::tree::Tree,
    reg: &Registry,
    installed: Option<&[(String, Option<String>)]>,
) -> Vec<Issue> {
    let mut out = tree.issues.clone();
    if let Some(inst) = installed {
        out.extend(
            requires::check(&tree.requires, inst)
                .into_iter()
                .map(|i| i.in_file(file)),
        );
    }
    let required = |kit: &str| tree.requires.iter().any(|r| r.id == kit);
    for (kit, _) in &tree.kits {
        if !required(kit) {
            out.push(not_required(kit, file, "", &format!("kits.{kit}")));
        }
    }
    for n in &tree.nodes {
        for (kit, _) in n
            .raw
            .get("kits")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            if !required(kit) {
                out.push(not_required(kit, &n.file, &n.path, &format!("kits.{kit}")));
            }
        }
    }
    let scene_when = ["entered", "left", "event"].map(String::from);
    for i in check_bindings(&Json::Array(tree.on.clone()), &scene_when, "scene") {
        out.push(i.in_file(file).at("", "on"));
    }
    out.extend(
        lower(id, &tree, reg)
            .issues
            .into_iter()
            .filter(|i| i.code != "binding_invalid"),
    );
    out
}

fn not_required(kit: &str, file: &str, node: &str, field: &str) -> Issue {
    Issue::warning("section_not_required", format!("the section {kit} is used but {kit} is not in the requires of the file"), format!("add \"{kit}\" to the requires of {file}, so the build and the tools know the scene needs it")).in_file(file).at(node, field)
}

/// Pack a project into the `game.json` today's loaders read: `{ format_version, project_id, seed, fixed_hz, project,
/// lock, physics }`, every scene file (sub-scenes included) carried in `project.data.scene_files`. `version` gives the
/// version of a kit whose `installed` entry has none (its kit.api.json). Refused with the issues when there are errors.
pub fn pack(
    project: &Json,
    files: Files,
    reg: &Registry,
    version: &dyn Fn(&str) -> Option<String>,
) -> Result<Json, Vec<Issue>> {
    pack_with(project, files, reg, version, false).map(|(game, _)| game)
}

/// [`pack`], and with `keep_errors` the game.json written in spite of the errors of the scene files (a missing kit, an
/// invalid section), given back with them: what proves that the kits refuse such a scene at load, cleanly
/// (`gfscene pack --keep-errors`). The errors of the pack itself (a kit version unknown) still refuse it.
pub fn pack_with(
    project: &Json,
    files: Files,
    reg: &Registry,
    version: &dyn Fn(&str) -> Option<String>,
    keep_errors: bool,
) -> Result<(Json, Vec<Issue>), Vec<Issue>> {
    let mut issues = Vec::new();
    let read: RefCell<Vec<(String, Json)>> = RefCell::new(Vec::new());
    let recording = |p: &str| {
        let f = files(p);
        if let Some(f) = &f {
            if !read.borrow().iter().any(|(x, _)| x == p) {
                read.borrow_mut().push((p.to_string(), f.clone()));
            }
        }
        f
    };
    issues.extend(validate_project(project, &recording, reg));
    let mut p = project.clone();
    let mut order = Vec::new();
    let mut kits = Map::new();
    for k in p
        .get_mut("installed")
        .and_then(Json::as_array_mut)
        .into_iter()
        .flatten()
    {
        let Some(id) = k.get("id").and_then(Json::as_str).map(str::to_string) else {
            continue;
        };
        let v = k
            .get("version")
            .and_then(Json::as_str)
            .map(str::to_string)
            .or_else(|| version(&id));
        let Some(v) = v else {
            issues.push(Issue::error("kit_version_unknown", format!("the version of {id} is not known"), format!("write \"version\" in the installed entry of {id}, or pack inside the repository (its kit.api.json)")).at("", "installed"));
            continue;
        };
        k["version"] = json!(v);
        let enabled = k.get("enabled") != Some(&Json::Bool(false));
        if k.get("enabled").is_none() {
            k["enabled"] = json!(true);
        }
        order.push(json!(id));
        kits.insert(
            id,
            json!({ "version": v, "enabled": enabled, "dependencies": {} }),
        );
    }
    let packing = issues.iter().any(|i| i.code == "kit_version_unknown");
    if has_errors(&issues) && (!keep_errors || packing) {
        return Err(issues);
    }
    let id = p.get("id").cloned().unwrap_or(json!("game"));
    let run = p
        .as_object_mut()
        .and_then(|o| o.remove("run"))
        .unwrap_or(json!({}));
    // The game's code is compiled (game.main), never read at run time.
    if let Some(o) = p.as_object_mut() {
        o.remove(crate::sources::KEY);
    }
    if p.get("schema").is_none() {
        p["schema"] = json!("gripforge.gameproject/1");
    }
    // The files the project carries (one-file form) and the ones read from the disk.
    let mut scene_files = crate::build::carried(project);
    for (k, v) in read.into_inner() {
        scene_files.insert(k, v);
    }
    if p.get("data").is_none() {
        p["data"] = json!({});
    }
    p["data"][SCENE_FILES] = Json::Object(scene_files);
    Ok((
        json!({
            "format_version": 1,
            "project_id": id,
            "seed": run.get("seed").cloned().unwrap_or(json!(7)),
            "fixed_hz": run.get("fixedHz").cloned().unwrap_or(json!(60)),
            "project": p,
            "lock": { "schema": "gripforge.gamekits.lock/1", "project": id, "order": order, "kits": kits },
            "physics": run.get("physics").cloned().unwrap_or(json!("none")),
        }),
        issues,
    ))
}

/// The model files the scenes of a project name (`model.asset` of their model nodes, sub-scenes and overrides
/// included, scenes in files and inline), each once, in order: what a loader binds as model slots (a model node is
/// lowered into an archetype whose `asset.slot` is that path; `gfscene pack --present` writes them in the
/// `present.json` the players read, each with its [`package_path`]).
pub fn model_files(project: &Json, files: Files, reg: &Registry) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let scenes = project
        .get("data")
        .and_then(|d| d.get("scenes"))
        .and_then(Json::as_array);
    for scene in scenes.into_iter().flatten() {
        let Some((_, tree)) = crate::build::resolve_scene(scene, reg, files) else {
            continue;
        };
        for n in tree.nodes {
            let asset = n
                .raw
                .get("model")
                .and_then(|m| m.get("asset"))
                .and_then(Json::as_str);
            if let Some(a) = asset.filter(|a| n.ty == "model" && !a.is_empty()) {
                if !out.iter().any(|x| x == a) {
                    out.push(a.to_string());
                }
            }
        }
    }
    out
}

/// Where a file of the project goes in a package the players open: under `assets/` beside `present.json` (the players
/// read a slot's file there, and only there: core/present-abi `safe_path`). A path already under `assets/` stays.
pub fn package_path(project_path: &str) -> String {
    if project_path.starts_with("assets/") {
        project_path.to_string()
    } else {
        format!("assets/{project_path}")
    }
}
