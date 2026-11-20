//! `gripforge project build` as a function: project.gripforge → strict check by the common core → the game.json of the
//! built game (scenes lowered for game.engine, dev.builder left out, the game's module in the lock) → the Rust of each
//! scene by the generator (`init_scene.rs`) → the crate of the game's module (`module.rs`, `lib.rs`, `Cargo.toml`), with
//! the game's code the project declares in `sources` taken by path (crate::sources). Everything lands in `<out>`
//! (default `.build/` beside the project); nothing is compiled here, the creator's files are never written.
//!
//!   .build/game.json        what the players load (no scene file read at run time)
//!   .build/game/            the crate of the game's module, compiled by the game's own build
//!   .build/build.json       the report (gripforge.build-report/1): issues, steps, times, files
//!
//! Never panics for a problem of the project: every problem is an issue of the result (file, scene, element,
//! property, what is expected).

use crate::access::resolve;
use crate::crate_gen::{sdk_of, write_crate, GAME_MODULE};
use crate::json::pretty;
use crate::project::{
    find_kits_dir, kit_folders, project_file, project_files, read_json, BuildError,
};
use serde_json::{json, Map, Value as Json};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// What a build is asked.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// The project file or its folder (default: the current folder).
    pub project: Option<PathBuf>,
    /// The output folder (default `.build/` beside the project).
    pub out: Option<PathBuf>,
    /// The folder of the kits (`<kit>/kit.api.json` and its accessor `<kit>/access/rust`), instead of the kits downloaded
    /// into the project (`kits/`, `.gripforge/kits/`): the kits of the sources, for the demos of the kits repository.
    pub kits_path: Option<PathBuf>,
    /// A kits SDK checkout the generated crate takes its SDK crates from (else git at `sdk_rev`).
    pub sdk: Option<PathBuf>,
    /// The revision of the kits SDK the generated crate takes by git.
    pub sdk_rev: Option<String>,
    /// The tolerant validator's warnings stay warnings (like the development mode).
    pub lenient: bool,
    /// Only the strict check: nothing written.
    pub check: bool,
    /// The module of the game's code for the development mode (`<out>/dev/game`): the game's code only, no scene
    /// posed (dev.builder poses them), no game.json. Refused without `sources`.
    pub dev: bool,
    /// Sends the GLB without a compressed copy to the public texture route (off by default: the build stays offline).
    pub compress_textures: bool,
    /// The route (default [`crate::textures::DEFAULT_URL`]).
    pub compress_textures_url: Option<String>,
}

/// What a build answers.
#[derive(Debug, Clone, Default)]
pub struct BuildResult {
    pub ok: bool,
    pub issues: Vec<Json>,
    pub steps: Vec<Json>,
    pub artifacts: Map<String, Json>,
    pub project_name: Option<String>,
    pub project: Option<PathBuf>,
    pub out: Option<PathBuf>,
    /// The texture compression report (`--compress-textures`).
    pub textures: Option<Json>,
}

fn failed(issues: Vec<Json>, steps: Vec<Json>, file: Option<&Path>) -> BuildResult {
    BuildResult {
        ok: false,
        issues,
        steps,
        project_name: file
            .and_then(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned()),
        project: file.map(Path::to_path_buf),
        ..Default::default()
    }
}

/// Builds (or only checks) a project.
pub fn build(o: &BuildOptions) -> BuildResult {
    let mut steps = Vec::new();
    let file = match project_file(o.project.as_deref()) {
        Ok(f) => f,
        Err(BuildError(i)) => return failed(vec![i], steps, None),
    };
    let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    let t0 = Instant::now();
    let read = (|| -> Result<_, BuildError> {
        let project = read_json(&file, "file_not_json")?;
        let files = project_files(&project, &dir)?;
        let kits_dir = o.kits_path.clone().or_else(|| find_kits_dir(&dir));
        let folders = kit_folders(kits_dir.as_deref())?;
        let kits: Vec<Json> = folders.iter().map(|(_, k)| k.clone()).collect();
        Ok((project, files, kits, folders))
    })();
    let (project, files, kits, folders) = match read {
        Ok(r) => r,
        Err(BuildError(i)) => return failed(vec![i], steps, Some(&file)),
    };
    steps.push(json!({ "name": "read", "ms": t0.elapsed().as_millis() as u64, "kits": kits.len(), "files": files.len() }));
    let incompatible: Vec<Json> = kits.iter().filter(|k| k["schema"] != "gripforge.kit-api/1").map(|k| json!({
        "level": "error", "code": "kits_incompatible", "field": format!("kits.{}.schema", k["id"].as_str().unwrap_or("?")),
        "message": format!("the kit {} describes itself with {}, this compiler reads gripforge.kit-api/1", k["id"].as_str().unwrap_or("(no id)"), k["schema"]),
        "fix": "use kits made for gripforge.kit-api/1, or a version of gripforge-build made for these kits" })).collect();
    if !incompatible.is_empty() {
        return failed(incompatible, steps, Some(&file));
    }

    let t0 = Instant::now();
    let op = if o.check { "check" } else { "code" };
    let answer = crate::core::run(
        &json!({ "op": op, "project": project, "files": files, "kits": kits, "lenient": o.lenient }),
    );
    steps.push(json!({ "name": if o.check { "check" } else { "check+generate" }, "ms": t0.elapsed().as_millis() as u64, "errors": answer["errors"].as_u64().unwrap_or(0) }));
    let issues: Vec<Json> = answer["issues"].as_array().cloned().unwrap_or_default();
    let mut r = failed(issues, steps, Some(&file));
    if answer["ok"] != true {
        return r;
    }
    // The accessor of each kit of the built game: its lock (a build), else the kits the project installs (a check);
    // the development kits are left out of the game, game.main is this crate.
    let ids = |v: Option<&Json>| -> Vec<String> {
        v.and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|k| k.as_str().or_else(|| k["id"].as_str()))
            .filter(|k| *k != GAME_MODULE && !crate::core::DEV_KITS.contains(k))
            .map(String::from)
            .collect()
    };
    let wanted = match answer.pointer("/game/lock/order") {
        Some(order) => ids(Some(order)),
        None => ids(project.get("installed")),
    };
    let accessors = match resolve(&folders, &wanted) {
        Ok(f) => f,
        Err(mut i) => {
            r.issues.append(&mut i);
            return r;
        }
    };
    steps_access(&mut r, &accessors);
    // The game's code the project declares (`sources`): its files there, inside the project, one module of the game,
    // before anything is written.
    let mut beside = Vec::new();
    let code = match crate::sources::game_code(&project, &file, &dir, &mut beside) {
        Ok(c) => c,
        Err(mut i) => {
            r.issues.append(&mut i);
            return r;
        }
    };
    r.issues.extend(beside);
    // `--dev`: the module of the game's code for the development mode; a project without code has nothing to compile.
    if o.dev && code.is_none() {
        r.issues.push(json!({ "level": "error", "code": "dev_without_sources", "field": "sources",
            "message": "--dev builds the module of the game's code, and the project declares no sources",
            "fix": "declare the game's code in sources (docs/sources.md), or build without --dev" }));
        return r;
    }
    r.ok = true;
    if o.check {
        return r;
    }

    let out = crate::project::absolute(&o.out.clone().unwrap_or_else(|| dir.join(".build")));
    let written = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&out)?;
        if o.dev {
            // The development mode plays project.gripforge itself (dev.builder poses the scenes): no game.json, only
            // the crate of the game's code, in `<out>/dev/game` (the CLI compiles it: docs/sources.md).
            let sdk = sdk_of(o.sdk.as_deref(), o.sdk_rev.as_deref());
            let dev_dir = out.join("dev/game");
            let files = write_crate(
                &dev_dir,
                project["id"].as_str().unwrap_or("game"),
                project["version"].as_str().unwrap_or("0.1.0"),
                &sdk,
                &accessors,
                crate::crate_gen::Code {
                    generated: &[],
                    entry: "",
                    game: code.as_ref(),
                    dev: true,
                },
            )?;
            r.artifacts
                .insert("crate".into(), json!(dev_dir.to_string_lossy()));
            r.artifacts.insert("files".into(), json!(files));
            if let Some(c) = &code {
                r.artifacts
                    .insert("gameModule".into(), json!(c.entry_path()));
            }
            return Ok(());
        }
        std::fs::write(
            out.join("game.json"),
            format!("{}\n", pretty(&answer["game"])),
        )?;
        r.artifacts.insert(
            "game".into(),
            json!(out.join("game.json").to_string_lossy()),
        );
        let generated: Vec<(String, String)> = answer["files"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                (
                    f["path"].as_str().unwrap_or("").to_string(),
                    f["text"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        let sdk = sdk_of(o.sdk.as_deref(), o.sdk_rev.as_deref());
        let id = project["id"].as_str().unwrap_or("game");
        let version = project["version"].as_str().unwrap_or("0.1.0");
        let files = write_crate(
            &out.join("game"),
            id,
            version,
            &sdk,
            &accessors,
            crate::crate_gen::Code {
                generated: &generated,
                entry: answer["entry"].as_str().unwrap_or("init_scene::enter"),
                game: code.as_ref(),
                dev: false,
            },
        )?;
        r.artifacts
            .insert("crate".into(), json!(out.join("game").to_string_lossy()));
        r.artifacts.insert("files".into(), json!(files));
        if let Some(c) = &code {
            r.artifacts.insert(
                "sources".into(),
                json!(c
                    .sources
                    .iter()
                    .map(|s| s.file.to_string_lossy())
                    .collect::<Vec<_>>()),
            );
            r.artifacts
                .insert("gameModule".into(), json!(c.entry_path()));
        }
        Ok(())
    })();
    if let Err(e) = written {
        r.ok = false;
        r.issues.push(json!({ "level": "error", "code": "write_failed", "file": out.to_string_lossy(),
            "message": format!("the build cannot be written in {}: {e}", out.display()), "fix": "check the folder given by --out and its rights" }));
        return r;
    }
    if o.compress_textures {
        let t0 = Instant::now();
        let url = o
            .compress_textures_url
            .as_deref()
            .unwrap_or(crate::textures::DEFAULT_URL);
        let (rep, issues) = crate::textures::compress(&dir, &out, url);
        r.steps.push(json!({ "name": "textures", "ms": t0.elapsed().as_millis() as u64, "compressed": rep["compressed"] }));
        r.issues.extend(issues);
        r.textures = Some(rep);
    }
    r.out = Some(out.clone());
    let report = crate::report::report(&r);
    let _ = std::fs::write(out.join("build.json"), format!("{}\n", pretty(&report)));
    r
}

/// `gripforge-build --resolve`: the project read as a build reads it (its included files, its scene files, the kits'
/// kit.api.json), answered by the core's `resolve` (`{ ok, project, resolveIssues, issues }`, nothing promoted,
/// nothing written), or `{ ok: false, issues }` with the problem of the reading.
pub fn resolved(o: &BuildOptions) -> Json {
    let read = (|| -> Result<_, BuildError> {
        let file = project_file(o.project.as_deref())?;
        let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let project = read_json(&file, "file_not_json")?;
        let files = project_files(&project, &dir)?;
        let kits_dir = o.kits_path.clone().or_else(|| find_kits_dir(&dir));
        let kits: Vec<Json> = kit_folders(kits_dir.as_deref())?
            .into_iter()
            .map(|(_, k)| k)
            .collect();
        Ok((project, files, kits))
    })();
    match read {
        Ok((project, files, kits)) => crate::core::run(
            &json!({ "op": "resolve", "project": project, "files": files, "kits": kits }),
        ),
        Err(BuildError(i)) => json!({ "ok": false, "issues": [i] }),
    }
}

/// The accessors found, in the steps of the report (kit, version, folder).
fn steps_access(r: &mut BuildResult, accessors: &[crate::access::Access]) {
    r.steps.push(json!({ "name": "access", "ms": 0, "access": accessors.iter().map(|f| json!({
        "kit": f.kit, "version": f.version, "crate": f.krate, "path": f.dir.to_string_lossy() })).collect::<Vec<_>>() }));
}
