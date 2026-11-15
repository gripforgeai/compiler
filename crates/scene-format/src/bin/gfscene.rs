//! `gfscene`: the command-line tool of the project and scene format (docs/ecrire-un-projet.md).
//!
//! ```text
//! gfscene validate <project> [--kits <dir>] [--json]
//! gfscene pack <project> --out <game.json> [--kits <dir>] [--present <present.json>] [--keep-errors]
//! gfscene apply <project> <patches.json> [--kits <dir>] [--dry-run] [--json]
//! gfscene build <project> --out <dir> [--kits <dir>] [--scenes <subdir>] [--json]
//! gfscene bundle <project> --out <name.gripforge>
//! gfscene code <project> --out <dir> [--kits <dir>] [--json]
//! ```
//!
//! `<project>` is a project file, or a folder: `project.gripforge` in it first, else its only `*.gripforge`.
//!
//! `validate` prints one line per issue (or a JSON report with `--json`) and exits 1 on an error; `--kits` reads the
//! kits' kit.api.json (their sections and versions).
//! `pack` writes the game.json today's loaders read, the scene files inside it; `--present` also writes the package the
//! players open: the model slots they bind (`{ "models": { "<project path>": "assets/<project path>" } }`) and each
//! model file copied under `assets/` beside present.json (the players read a slot's file there only);
//! `--keep-errors` writes the game.json in spite of the errors of the scene files (printed, exit 1), to prove that the
//! kits refuse such a scene at load.
//! `apply` applies a file of patches
//! (the debugging editor's `gripforge.editor-patches/1`, commands of the site's scene patch) to the scene files with the smallest diff, then validates the project.
//! `build` writes the published form (`project.gripforge`, one resolved scene file `<name>.gfscene` per scene, `build.json` for
//! asset.preload); `code` writes the Rust that poses each scene (`init_scene.rs`, `init_scene/<scene>.rs`,
//! docs/design/build-gripforge.md), a broken reference refusing it; `bundle` writes the project as one file (scenes inline, sub-scenes carried). Only an error (a
//! required kit missing) makes a command fail: warnings are printed and the output is written.

use gripforge_scene_format::{build, code, issue::has_errors, ops, project, Issue, Registry};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn read_json(p: &Path) -> Result<Json, String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: not JSON ({e})", p.display()))
}

/// The kit.api.json of every kit under `dir` (`kits/<kit>/kit.api.json`), by kit id.
fn kit_apis(dir: Option<&str>) -> Vec<Json> {
    let Some(dir) = dir else { return Vec::new() };
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if let Ok(api) = read_json(&e.path().join("kit.api.json")) {
            out.push(api);
        }
    }
    out
}

/// The project file a path names: the file itself, or in a folder `project.gripforge`, else its only `*.gripforge`.
fn project_file(p: &Path) -> Result<PathBuf, String> {
    if !p.is_dir() {
        return Ok(p.to_path_buf());
    }
    let default = p.join("project.gripforge");
    if default.is_file() {
        return Ok(default);
    }
    let found: Vec<PathBuf> = std::fs::read_dir(p)
        .map_err(|e| format!("{}: {e}", p.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|f| {
            f.extension().is_some_and(|x| x == "gripforge")
                && f.file_stem().is_some_and(|s| !s.is_empty())
        })
        .collect();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!("no project.gripforge in {}", p.display())),
        _ => Err(format!(
            "several *.gripforge in {} and no project.gripforge: name the file",
            p.display()
        )),
    }
}

/// The project path of the asset with library id `id`: `library/<id>/<id>.*`, else the only file of
/// `library/<id>/` that is not its passport.
fn library_asset(root: &Path, id: &str) -> Option<String> {
    let dir = root.join("library").join(id);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n != "passport.json")
        .collect();
    names.sort();
    let pick = names
        .iter()
        .find(|n| n.split('.').next() == Some(id))
        .or(if names.len() == 1 {
            names.first()
        } else {
            None
        })?;
    Some(format!("library/{id}/{pick}"))
}

fn write_all(out: &Path, list: &[(String, Json)]) -> Result<(), String> {
    for (p, f) in list {
        let dest = out.join(p);
        std::fs::create_dir_all(dest.parent().unwrap_or(out)).map_err(|e| e.to_string())?;
        std::fs::write(&dest, pretty(f)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn registry(apis: &[Json]) -> Registry {
    let mut r = Registry::base();
    for a in apis {
        r.add_kit_api(a);
    }
    r
}

fn report(issues: &[Issue], as_json: bool) {
    if as_json {
        let list: Vec<Json> = issues.iter().map(Issue::to_json).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "ok": !has_errors(issues), "issues": list }))
                .unwrap_or_default()
        );
    } else {
        for i in issues {
            println!("{}", i.line());
        }
        let errors = issues.iter().filter(|i| i.is_error()).count();
        println!("{} error(s), {} warning(s)", errors, issues.len() - errors);
    }
}

fn run(args: &[String]) -> Result<bool, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let input = args
        .get(1)
        .ok_or("usage: gfscene validate|pack|apply|build|bundle|code <file> ...")?;
    let path = project_file(Path::new(input))?;
    let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let files = |p: &str| read_json(&root.join(p)).ok();
    let apis = kit_apis(arg(args, "--kits").as_deref());
    let reg = registry(&apis);
    match cmd {
        "validate" => {
            let issues = project::validate_project(&read_json(&path)?, &files, &reg);
            report(&issues, args.iter().any(|a| a == "--json"));
            Ok(!has_errors(&issues))
        }
        "pack" => {
            let out = PathBuf::from(arg(args, "--out").ok_or("pack needs --out <game.json>")?);
            let version = |id: &str| {
                apis.iter()
                    .find(|a| a.get("id") == Some(&json!(id)))
                    .and_then(|a| a.get("version")?.as_str().map(str::to_string))
            };
            let keep = args.iter().any(|a| a == "--keep-errors");
            let proj = read_json(&path)?;
            match project::pack_with(&proj, &files, &reg, &version, keep) {
                Ok((game, issues)) => {
                    std::fs::write(&out, pretty(&game)).map_err(|e| e.to_string())?;
                    println!("{} written", out.display());
                    if let Some(present) = arg(args, "--present") {
                        // A package the players open: each model file copied under `assets/` beside
                        // present.json, the slot (the project path) bound to it.
                        let dir = Path::new(&present).parent().unwrap_or(Path::new("."));
                        let mut models = serde_json::Map::new();
                        for m in project::model_files(&proj, &files, &reg) {
                            let to = project::package_path(&m);
                            let (src, dst) = (root.join(&m), dir.join(&to));
                            if src != dst {
                                if let Some(parent) = dst.parent() {
                                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                                }
                                if let Err(e) = std::fs::copy(&src, &dst) {
                                    eprintln!("warning: {m}: not copied into the package ({e}): its slot stays unbound");
                                    continue;
                                }
                            }
                            models.insert(m, json!(to));
                        }
                        let n = models.len();
                        std::fs::write(&present, pretty(&json!({ "models": models })))
                            .map_err(|e| e.to_string())?;
                        println!("{present} written ({n} model slot(s))");
                    }
                    if !issues.is_empty() {
                        report(&issues, args.iter().any(|a| a == "--json"));
                    }
                    Ok(!has_errors(&issues))
                }
                Err(issues) => {
                    report(&issues, args.iter().any(|a| a == "--json"));
                    Ok(false)
                }
            }
        }
        "apply" => {
            let patches = read_json(&PathBuf::from(
                args.get(2).ok_or("apply needs <patches.json>")?,
            ))?;
            let proj = read_json(&path)?;
            let text = |p: &str| std::fs::read_to_string(root.join(p)).ok();
            let (changed, mut issues) = ops::apply_patches(&patches, &proj, &text);
            if !args.iter().any(|a| a == "--dry-run") {
                for (p, t) in &changed {
                    std::fs::write(root.join(p), t).map_err(|e| e.to_string())?;
                }
                issues.extend(project::validate_project(&proj, &files, &reg));
            }
            println!(
                "{} file(s) changed{}",
                changed.len(),
                if args.iter().any(|a| a == "--dry-run") {
                    " (dry run, nothing written)"
                } else {
                    ""
                }
            );
            report(&issues, args.iter().any(|a| a == "--json"));
            Ok(!has_errors(&issues))
        }
        "build" => {
            let out = PathBuf::from(arg(args, "--out").ok_or("build needs --out <dir>")?);
            let asset = |id: &str| library_asset(&root, id);
            let dir = arg(args, "--scenes").unwrap_or_else(|| "scenes".into());
            match build::build(&read_json(&path)?, &files, &reg, &asset, &dir) {
                Ok(b) => {
                    write_all(&out, &b.files)?;
                    write_all(&out, &[("project.gripforge".to_string(), b.project)])?;
                    report(&b.issues, args.iter().any(|a| a == "--json"));
                    println!(
                        "project.gripforge and {} file(s) written in {}",
                        b.files.len(),
                        out.display()
                    );
                    Ok(true)
                }
                Err(issues) => {
                    report(&issues, args.iter().any(|a| a == "--json"));
                    Ok(false)
                }
            }
        }
        "code" => {
            let out = PathBuf::from(arg(args, "--out").ok_or("code needs --out <dir>")?);
            let origin = path.file_name().map_or("project.gripforge".into(), |f| {
                f.to_string_lossy().to_string()
            });
            match code::code(&read_json(&path)?, &files, &reg, &origin) {
                Ok(g) => {
                    for (p, text) in &g.files {
                        let dest = out.join(p);
                        std::fs::create_dir_all(dest.parent().unwrap_or(&out))
                            .map_err(|e| e.to_string())?;
                        std::fs::write(&dest, text).map_err(|e| e.to_string())?;
                    }
                    report(&g.issues, args.iter().any(|a| a == "--json"));
                    println!(
                        "{} file(s) of Rust written in {}",
                        g.files.len(),
                        out.display()
                    );
                    Ok(true)
                }
                Err(issues) => {
                    report(&issues, args.iter().any(|a| a == "--json"));
                    Ok(false)
                }
            }
        }
        "bundle" => {
            let out = PathBuf::from(arg(args, "--out").ok_or("bundle needs --out <file>")?);
            let (one, issues) = build::bundle(&read_json(&path)?, &files);
            if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&out, pretty(&one)).map_err(|e| e.to_string())?;
            report(&issues, args.iter().any(|a| a == "--json"));
            println!("{} written", out.display());
            Ok(true)
        }
        _ => Err("usage: gfscene validate|pack|apply|build|bundle|code <file> ...".into()),
    }
}

fn pretty(v: &Json) -> String {
    gripforge_scene_format::jsontext::to_text(v)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("gfscene: {e}");
            ExitCode::from(2)
        }
    }
}
