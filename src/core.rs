//! The core of `gripforge project build`: the common core of the kits (core/scene-format, its validator, its
//! packing and its generator of `init_scene.rs`) called directly, plus the strict rules of a build.
//!
//! One entry, [`run`], JSON in and JSON out (the reading of the disk is in [`crate::project`]):
//!
//! - `{ "op": "check", project, files, kits, lenient? }`: the project checked by the common core (core/scene-format
//!   `validate_project`, the same validator as dev.builder and the MCP), then **strictly**: a build accepts no
//!   guess, so every warning of the tolerant validator is an error here (`promoted: true`), but for the few that only
//!   say a node is read and not played ([`LENIENT`]); every installed or required kit must have its kit.api.json in
//!   the catalog (`kit_unknown`: no accessor can be generated without it). Each issue says the file, the scene, the
//!   element (node path), the property (field), what is wrong and what is expected.
//! - `{ "op": "resolve", project, files, kits }`: the project resolved and the tolerant validator's issues, nothing
//!   promoted ([`resolve`]): what the development mode of the kits repository makes of the same project.
//! - `{ "op": "pack", ... }`: the check, then the game.json of the built game: the project packed (core/scene-format
//!   `pack_with`) and its scenes lowered for game.engine at build time (`expand_project`: no scene JSON resolved at
//!   run time), the development kits ([`DEV_KITS`]) left out, the game's own module ([`GAME_MODULE`], the compiled
//!   `init_scene.rs` + `index.rs`) put last in the lock; with the scenes `{ id, module }` the crate is written for.
//!
//! `files`: the scene files the project names (project path → parsed JSON), `kits`: the kit.api.json of the catalog.
//! Nothing here knows a kit: the kits speak through their kit.api.json.

use gripforge_scene_format::expand::{expand_project, installed_of};
use gripforge_scene_format::issue::{Issue, Level};
use gripforge_scene_format::{project, requires, Registry};
use serde_json::{json, Map, Value as Json};

use crate::strict;

/// The id of the game's own module in a built game: the compiled `index.rs` and its `init_scene.rs`.
pub const GAME_MODULE: &str = "game.main";
/// The kits of the development mode, never shipped in a built game (the compiled code poses the scenes).
pub const DEV_KITS: &[&str] = &["scene.builder", "dev.builder"];
/// The warnings a build keeps as warnings: a node read and shown, that no installed kit plays yet.
pub const LENIENT: &[&str] = &[
    "sound_not_placed",
    "hud_not_placed",
    "light_not_placed",
    "camera_not_placed",
];
/// The version of this core (the package's).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One request, answered.
pub fn run(req: &Json) -> Json {
    match req.get("op").and_then(Json::as_str).unwrap_or("") {
        "check" => {
            let (issues, scenes) = check(req);
            report(issues, json!({ "scenes": scenes }))
        }
        "pack" => pack(req),
        "resolve" => resolve(req),
        "code" => code(req),
        "version" => json!({ "ok": true, "version": VERSION, "gameModule": GAME_MODULE }),
        other => json!({ "ok": false, "issues": [{ "level": "error", "code": "op_unknown",
            "message": format!("unknown op {other:?}"), "fix": "op is check, pack, code, resolve or version" }] }),
    }
}

fn kits_of(req: &Json) -> Vec<Json> {
    req.get("kits")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default()
}

fn registry(kits: &[Json]) -> Registry {
    let mut reg = Registry::base();
    for api in kits {
        reg.add_kit_api(api);
    }
    reg
}

/// One issue located for an AI: the core's JSON plus the scene it belongs to and whether the build promoted it.
fn located(i: &Issue, scenes: &Map<String, Json>, promoted: bool) -> Json {
    let mut o = i.to_json();
    if let Some(s) = scenes.get(&i.file) {
        o["scene"] = s.clone();
    }
    if promoted {
        o["level"] = json!("error");
        o["promoted"] = json!(true);
    }
    o
}

/// The scenes of the project, `{ id, file, module }` (file: the scene file, `#<id>` inline), in order.
fn scene_list(project: &Json) -> Vec<Json> {
    let mut out = Vec::new();
    for s in project
        .pointer("/data/scenes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = s.get("id").and_then(Json::as_str) else {
            continue;
        };
        let file = s
            .get("file")
            .and_then(Json::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| gripforge_scene_format::build::inline_label(id));
        out.push(json!({ "id": id, "file": file, "module": module_name(id) }));
    }
    out
}

/// The Rust module name of a scene id (`level-2` → `level_2`, `2nd` → `scene_2nd`).
pub fn module_name(id: &str) -> String {
    let mut s: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        s = format!("scene_{s}");
    }
    s
}

/// The strict check: the issues as JSON (errors first kept in the order found) and the scenes.
pub fn check(req: &Json) -> (Vec<Json>, Vec<Json>) {
    let project = req.get("project").cloned().unwrap_or(Json::Null);
    let files = req
        .get("files")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let lenient = req.get("lenient") == Some(&Json::Bool(true));
    let kits = kits_of(req);
    let reg = registry(&kits);
    let read = |p: &str| files.get(p).cloned();
    // The validator resolves the project itself (its issues come with its own); the build's own rules read the
    // project resolved, as the pack and the generator do (core/scene-format crate::include).
    let written = project;
    let project = gripforge_scene_format::include::resolve_project(&written, &read).0;
    let scenes = scene_list(&project);
    let by_file: Map<String, Json> = scenes
        .iter()
        .map(|s| {
            (
                s["file"].as_str().unwrap_or("").to_string(),
                s["id"].clone(),
            )
        })
        .collect();
    let mut out: Vec<Json> = Vec::new();
    // The versions the lock will hold, so that a `requires` range is checked against them.
    let versioned = strict::with_versions(&written, &kits);
    for i in project::validate_project(&versioned, &read, &reg) {
        let promoted = !lenient && i.level == Level::Warning && !LENIENT.contains(&i.code);
        out.push(located(&i, &by_file, promoted));
    }
    if !lenient {
        out.extend(strict::check(&project, &files, &kits, &reg));
    }
    // Every kit the project installs or requires has its kit.api.json: the accessors come from it.
    let known: Vec<&str> = kits
        .iter()
        .filter_map(|a| a.get("id").and_then(Json::as_str))
        .collect();
    let mut named: Vec<(String, &str)> = installed_of(&project)
        .into_iter()
        .map(|(id, _)| (id, "installed"))
        .collect();
    let version_of = |id: &str| {
        project
            .get("installed")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .find(|k| k.get("id").and_then(Json::as_str) == Some(id))
            .and_then(|k| k.get("version").and_then(Json::as_str))
            .map(|v| format!(" version {v}"))
            .unwrap_or_default()
    };
    let mut sink = Vec::new();
    for r in requires::read(project.get("requires"), &mut sink) {
        if !named.iter().any(|(k, _)| *k == r.id) {
            named.push((r.id.clone(), "requires"));
        }
    }
    for (id, field) in named {
        if !known.contains(&id.as_str()) {
            out.push(json!({ "level": "error", "code": "kit_unknown", "field": field,
                "message": format!("the kit {id}{} is not in the project: no kit.api.json for it among the kits of the project, so no accessor to call it", version_of(&id)),
                "fix": format!("install the kits of the project ({}), or give the folder of the kits with --kits-path <dir> (<kit>/kit.api.json); check the id {id} (the kits are named like core.light)", crate::project::INSTALL_KITS) }));
        }
    }
    (out, scenes)
}

fn report(issues: Vec<Json>, mut extra: Json) -> Json {
    let errors = issues.iter().filter(|i| i["level"] == "error").count();
    extra["ok"] = json!(errors == 0);
    extra["errors"] = json!(errors);
    extra["warnings"] = json!(issues.len() - errors);
    extra["issues"] = Json::Array(issues);
    extra
}

/// The project as every reader of the kits repository resolves it, without the build's rules: `project` (resolved by
/// core/scene-format `include::resolve_project`: its included files, its nodes' variables, its `$var`),
/// `resolveIssues` (that resolution's) and `issues` (the tolerant validator, `project::validate_project`, as
/// dev.builder and the MCP see them, never promoted). What the parity test of the kits repository compares with its
/// reference reader (tools/parity there, docs/design/parite-compilateur-dev-builder.md).
pub fn resolve(req: &Json) -> Json {
    let project = req.get("project").cloned().unwrap_or(Json::Null);
    let files = req
        .get("files")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let reg = registry(&kits_of(req));
    let read = |p: &str| files.get(p).cloned();
    let (resolved, resolve_issues) = gripforge_scene_format::include::resolve_project(&project, &read);
    let issues: Vec<Json> = project::validate_project(&project, &read, &reg)
        .iter()
        .map(Issue::to_json)
        .collect();
    let errors = issues.iter().filter(|i| i["level"] == "error").count();
    json!({
        "ok": errors == 0,
        "project": resolved,
        "resolveIssues": resolve_issues.iter().map(Issue::to_json).collect::<Vec<_>>(),
        "issues": issues,
    })
}

/// The check, then the game.json of the built game.
pub fn pack(req: &Json) -> Json {
    let (issues, scenes) = check(req);
    if issues.iter().any(|i| i["level"] == "error") {
        return report(issues, json!({ "scenes": scenes }));
    }
    let project = req.get("project").cloned().unwrap_or(Json::Null);
    let files = req
        .get("files")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let kits = kits_of(req);
    let reg = registry(&kits);
    let read = |p: &str| files.get(p).cloned();
    let version = |id: &str| {
        kits.iter()
            .find(|a| a.get("id").and_then(Json::as_str) == Some(id))
            .and_then(|a| a.get("version").and_then(Json::as_str))
            .map(str::to_string)
    };
    let mut game = match project::pack_with(&project, &read, &reg, &version, false) {
        Ok((g, _)) => g,
        Err(errs) => {
            let mut all = issues;
            all.extend(errs.iter().map(|i| located(i, &Map::new(), false)));
            return report(all, json!({ "scenes": scenes }));
        }
    };
    // The scenes lowered for game.engine now, not at run time.
    if let Some(p) = expand_project(&game["project"], &reg) {
        game["project"] = p;
    }
    let dev = |v: &Json| {
        let id = v
            .as_str()
            .or_else(|| v.get("id").and_then(Json::as_str))
            .unwrap_or("");
        DEV_KITS.contains(&id)
    };
    for key in ["installed", "requires"] {
        if let Some(a) = game["project"].get_mut(key).and_then(Json::as_array_mut) {
            a.retain(|v| !dev(v));
        }
    }
    if let Some(a) = game["project"]
        .get_mut("installed")
        .and_then(Json::as_array_mut)
    {
        a.push(json!({ "id": GAME_MODULE, "version": VERSION, "enabled": true }));
    }
    if let Some(a) = game.pointer_mut("/lock/order").and_then(Json::as_array_mut) {
        a.retain(|v| !dev(v));
        a.push(json!(GAME_MODULE));
    }
    if let Some(k) = game.pointer_mut("/lock/kits").and_then(Json::as_object_mut) {
        for d in DEV_KITS {
            k.remove(*d);
        }
        k.insert(
            GAME_MODULE.into(),
            json!({ "version": VERSION, "enabled": true, "dependencies": {} }),
        );
    }
    game["build"] = json!({ "schema": "gripforge.build/1", "compiler": VERSION, "gameModule": GAME_MODULE,
        "scenes": scenes.iter().map(|s| s["id"].clone()).collect::<Vec<_>>() });
    report(issues, json!({ "scenes": scenes, "game": game }))
}

/// The pack, then the Rust of each scene by the generator of core/scene-format (`init_scene.rs`, the same as the
/// templates'): `files` (`[{ path, text }]`, paths under the crate's `src/`) and `entry` (the function each scene
/// module exports, which the crate's `index.rs` calls).
pub fn code(req: &Json) -> Json {
    let mut r = pack(req);
    if r["ok"] != Json::Bool(true) {
        return r;
    }
    match generate(req, &r) {
        Ok((files, entry, warnings)) => {
            r["files"] = files;
            r["entry"] = json!(entry);
            if let Some(a) = r["issues"].as_array_mut() {
                a.extend(warnings);
            }
        }
        Err(issue) => {
            r["ok"] = json!(false);
            r["errors"] = json!(r["errors"].as_u64().unwrap_or(0) + 1);
            if let Some(a) = r["issues"].as_array_mut() {
                a.push(issue);
            }
        }
    }
    r
}

/// The generator's files for the project: core/scene-format `code::code` (the same writer as the
/// templates'), `init_scene.rs` and `init_scene/<scene>.rs`, whose `init_scene::enter(h, id)` the crate's
/// `index.rs` calls. Its warnings join the report; an error of it refuses the build.
fn generate(req: &Json, _packed: &Json) -> Result<(Json, &'static str, Vec<Json>), Json> {
    let project = req.get("project").cloned().unwrap_or(Json::Null);
    let files = req
        .get("files")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let reg = registry(&kits_of(req));
    let read = |p: &str| files.get(p).cloned();
    let origin = req
        .get("origin")
        .and_then(Json::as_str)
        .unwrap_or("project.gripforge");
    let by_file: Map<String, Json> = scene_list(&project)
        .iter()
        .map(|s| {
            (
                s["file"].as_str().unwrap_or("").to_string(),
                s["id"].clone(),
            )
        })
        .collect();
    match gripforge_scene_format::code::code(&project, &read, &reg, origin) {
        Ok(g) => Ok((
            Json::Array(
                g.files
                    .iter()
                    .map(|(path, text)| json!({ "path": path, "text": text }))
                    .collect(),
            ),
            "init_scene::enter",
            g.issues
                .iter()
                .map(|i| located(i, &by_file, false))
                .collect(),
        )),
        Err(issues) => {
            let first = issues.iter().find(|i| i.is_error()).or(issues.first());
            Err(first.map_or_else(
                || json!({ "level": "error", "code": "generator_failed", "message": "the generator refused the project", "fix": "check the project with --check" }),
                |i| located(i, &by_file, true),
            ))
        }
    }
}
