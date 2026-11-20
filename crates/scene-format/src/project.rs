//! The project file (`<name>.gripforge`): its validation (its own `requires`, its scenes, every scene checked down to
//! its sub-scenes) and its packing into the `game.json` envelope of the development mode (the kits' lock beside the
//! project), the sub-scenes carried in `data.scene_files` for game.engine to lower (crate::expand).

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
/// The project is resolved first (crate::include: its includes, its nodes' variables, its `$var`).
pub fn validate_project(project: &Json, files: Files, reg: &Registry) -> Vec<Issue> {
    let (resolved, mut out) = crate::include::resolve_project(project, files);
    out.extend(validate_resolved(&resolved, files, reg));
    out
}

fn validate_resolved(project: &Json, files: Files, reg: &Registry) -> Vec<Issue> {
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
    let mut physics_nodes: Vec<(String, String, String)> = Vec::new();
    let scenes = project
        .pointer("/data/scenes")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    if scenes.is_empty() {
        out.push(Issue::warning("scenes_missing", "the project has no scene", "list the scenes in data.scenes: [{ \"id\": \"level\", \"role\": \"level\", \"requires\": [\"game.engine\"], \"nodes\": [] }]").at("", "data.scenes"));
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
                physics_users(&tree, reg, &mut physics_nodes);
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
        let tree = resolve(file, reg, files);
        physics_users(&tree, reg, &mut physics_nodes);
        out.extend(check_tree(id, file, tree, reg, Some(&installed)));
    }
    out.extend(physics_not_run(project, &installed, &physics_nodes));
    out
}

/// The kits whose sections need the physics running (`needsRun: "physics"` of their scene entries).
fn needs_physics(reg: &crate::Registry) -> impl Iterator<Item = &String> {
    reg.sections
        .iter()
        .filter(|(_, s)| s.needs_run.iter().any(|r| r == "physics"))
        .map(|(k, _)| k)
}

/// The nodes of a scene that carry the section of a kit that needs the physics running (a collision, a body...):
/// `(file, node path, kit)`.
fn physics_users(
    tree: &crate::tree::Tree,
    reg: &crate::Registry,
    into: &mut Vec<(String, String, String)>,
) {
    for n in &tree.nodes {
        let Some(kits) = n.raw.get("kits").and_then(Json::as_object) else {
            continue;
        };
        if let Some(kit) = needs_physics(reg).find(|k| kits.contains_key(k.as_str())) {
            into.push((n.file.clone(), n.path.clone(), kit.clone()));
        }
    }
}

/// A kit whose sections need the physics running is installed and nodes use it, but the project says no physics runs
/// (`run.physics`, `none` by default): the game loads, and no node makes a body nor tells anything. One warning, with
/// the line to write.
fn physics_not_run(
    project: &Json,
    installed: &[(String, Option<String>)],
    users: &[(String, String, String)],
) -> Option<Issue> {
    let (file, node, kit) = users.first()?;
    if !installed.iter().any(|(k, _)| k == kit) {
        return None;
    }
    let run = project
        .pointer("/run/physics")
        .and_then(Json::as_str)
        .unwrap_or("none");
    if matches!(run, "rapier" | "engine") {
        return None;
    }
    let more = match users.len() {
        1 => String::new(),
        n => format!(" (and {} more)", n - 1),
    };
    Some(
        Issue::warning(
            "physics_not_run",
            format!("{node} carries a {kit} section{more} but no physics runs (run.physics is \"{run}\"): no body is made and nothing is told"),
            "write \"run\": { \"physics\": \"rapier\" } in the project file, or remove the collision nodes",
        )
        .in_file(file)
        .at(node, &format!("kits.{kit}")),
    )
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

/// Pack a project into the `game.json` envelope of the development mode (what dev.builder and game.engine are given
/// when the game runs without a build): `{ format_version, project_id, seed, fixed_hz, project, lock, physics }`, every
/// sub-scene the scenes instance carried in `project.data.scene_files`. `version` gives the
/// version of a kit whose `installed` entry has none (its kit.api.json). Refused with the issues when there are errors.
pub fn pack(
    project: &Json,
    files: Files,
    reg: &Registry,
    version: &dyn Fn(&str) -> Option<String>,
) -> Result<Json, Vec<Issue>> {
    pack_with(project, files, reg, version, false).map(|(game, _)| game)
}

/// [`pack`], and with `keep_errors` the game.json made in spite of the errors of the scenes (a missing kit, an invalid
/// section), given back with them: what proves that the kits refuse such a scene at load, cleanly (the room `errors`
/// of dev.builder's tests). The errors of the pack itself (a kit version unknown) still refuse it.
pub fn pack_with(
    project: &Json,
    files: Files,
    reg: &Registry,
    version: &dyn Fn(&str) -> Option<String>,
    keep_errors: bool,
) -> Result<(Json, Vec<Issue>), Vec<Issue>> {
    // Resolved once with the plain reader: the included files are not sub-scenes to carry.
    let (resolved, mut issues) = crate::include::resolve_project(project, files);
    let project = &resolved;
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
    issues.extend(validate_resolved(project, &recording, reg));
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
    // The sub-scenes the project carries and the ones read beside it.
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
            // The action tick: `run.fixedHz`, else game.engine `tick.hz`, else its default (30 Hz).
            "fixed_hz": run.get("fixedHz").cloned().unwrap_or_else(|| json!(engine_tick_hz(&p))),
            "project": p,
            "lock": { "schema": "gripforge.gamekits.lock/1", "project": id, "order": order, "kits": kits },
            "physics": run.get("physics").cloned().unwrap_or(json!("none")),
        }),
        issues,
    ))
}

/// game.engine `tick.hz` of a project, else the default action tick (30 Hz, never the physics step).
fn engine_tick_hz(project: &Json) -> f64 {
    project
        .get("installed")
        .and_then(Json::as_array)
        .and_then(|l| {
            l.iter()
                .find(|k| k.get("id").and_then(Json::as_str) == Some("game.engine"))
        })
        .and_then(|k| k.pointer("/config/tick/hz"))
        .and_then(Json::as_f64)
        .filter(|h| h.is_finite() && *h > 0.0)
        .unwrap_or(30.0)
}
