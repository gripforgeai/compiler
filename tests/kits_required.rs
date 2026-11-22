//! The kits are fixed at the start (decision of Damien, module-kits `docs/chargement-des-kits.md`): `project.gripforge`
//! lists EVERY kit of the game, the ones its kits need included, and the BUILD refuses a project where a kit needs a
//! kit the list does not have, with the one `kit_required` error the gateways and dev.builder give
//! (`gripforge_scene_format::kits::check_required`, one copy, never written again here). And the parity with the
//! development mode: the same project gives the same lock and the same refusal by dev.builder and by the compiler.
//!
//!   cargo test --test kits_required

use gripforge_compiler::build::{build, BuildOptions, BuildResult};
use gripforge_compiler::cli::main as cli;
use gripforge_compiler::core::{DEFAULT_TARGET, DEV_KITS, GAME_MODULE};
use gripforge_compiler::project::{kit_catalog, project_files};
use gripforge_scene_format::{kits, project, Registry};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

fn room_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/room")
}

fn tmp() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "gfc-req-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

fn read(f: &Path) -> Json {
    serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap()
}

fn write(f: &Path, v: &Json) {
    std::fs::write(f, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

/// A copy of the room (its kits beside it), its project changed by `edit`.
fn room_with(edit: impl FnOnce(&mut Json)) -> PathBuf {
    let d = tmp().join("room");
    copy_dir(&room_dir(), &d);
    let f = d.join("project.gripforge");
    let mut p = read(&f);
    edit(&mut p);
    write(&f, &p);
    d
}

/// The kit.api.json of `folder` in the room copy `d`, changed by `edit`.
fn edit_kit(d: &Path, folder: &str, edit: impl FnOnce(&mut Json)) {
    let f = d.join("kits").join(folder).join("kit.api.json");
    let mut k = read(&f);
    edit(&mut k);
    write(&f, &k);
}

/// The project without the kit `id` (installed and requires).
fn without(p: &mut Json, id: &str) {
    p["installed"]
        .as_array_mut()
        .unwrap()
        .retain(|k| k["id"] != id);
    if let Some(r) = p["requires"].as_array_mut() {
        r.retain(|k| k != id);
    }
}

fn built(d: &Path, check: bool, target: Option<&str>) -> BuildResult {
    build(&BuildOptions {
        project: Some(d.to_path_buf()),
        out: Some(d.join(".build")),
        check,
        target: target.map(String::from),
        ..Default::default()
    })
}

fn required(r: &BuildResult) -> Vec<String> {
    r.issues
        .iter()
        .filter(|i| i["code"] == "kit_required")
        .map(|i| i["message"].as_str().unwrap().to_string())
        .collect()
}

const ENGINE_NEEDS_SCENE: &str = "kit_required: game.engine a besoin de core.scene (cible bevy-web) — ajoute-le dans \"installed\" du project.gripforge";

/// The room lists every kit its kits need: it builds, and core.scene (game.engine needs it on the Bevy players) is in
/// the lock of the built game.
#[test]
fn the_room_lists_every_kit_its_kits_need_and_builds() {
    let d = room_with(|_| {});
    let r = built(&d, false, None);
    assert!(r.ok, "{:#?}", r.issues);
    assert!(required(&r).is_empty());
    let game = read(&d.join(".build/game.json"));
    let order: Vec<&str> = game["lock"]["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(order.contains(&"core.scene"), "{order:?}");
}

/// Criterion 1: game.engine without the kit it needs on the target: the build is refused with the one kit_required
/// message, naming both kits and the target, at --check too, and nothing is written.
#[test]
fn a_kit_needed_and_not_listed_refuses_the_build_with_kit_required() {
    let d = room_with(|p| without(p, "core.scene"));
    for check in [true, false] {
        let r = built(&d, check, None);
        assert!(!r.ok, "check={check}");
        assert_eq!(required(&r), vec![ENGINE_NEEDS_SCENE.to_string()]);
        let i = r.issues.iter().find(|i| i["code"] == "kit_required").unwrap();
        assert_eq!(i["level"], "error");
        assert!(i["fix"].as_str().unwrap().contains("core.scene"));
    }
    assert!(!d.join(".build").exists(), "a refused build writes nothing");
    // The command line: exit 1, the message on the error stream.
    let (mut out, mut err) = (String::new(), String::new());
    let code = cli(
        &[d.to_string_lossy().into_owned()],
        &mut |s| out.push_str(s),
        &mut |s| err.push_str(s),
    );
    assert_eq!(code, 1);
    assert!(
        format!("{out}{err}").contains(ENGINE_NEEDS_SCENE),
        "{out}\n{err}"
    );
}

/// Criterion 2: two lacks are both said, at once.
#[test]
fn every_lack_is_said_at_once() {
    let d = room_with(|p| without(p, "core.scene"));
    edit_kit(&d, "game_engine", |k| {
        k["dependencies"].as_array_mut().unwrap().push(json!({
            "kit": "core.render", "range": "*", "targets": ["bevy", "bevy-web"], "reason": "the drawing"
        }));
    });
    let r = built(&d, true, None);
    let m = required(&r);
    assert_eq!(m.len(), 2, "{m:?}");
    assert!(m.iter().any(|x| x.contains("game.engine a besoin de core.scene (cible bevy-web)")));
    assert!(m.iter().any(|x| x.contains("game.engine a besoin de core.render (cible bevy-web)")));
}

/// Criterion 3: a recommendation absent blocks nothing and is never added to the built game.
#[test]
fn a_recommendation_absent_blocks_nothing_and_is_never_added() {
    let d = room_with(|_| {});
    edit_kit(&d, "game_engine", |k| {
        k["dependencies"].as_array_mut().unwrap().push(json!({
            "kit": "core.input.pointer_lock", "range": "*", "recommended": true, "reason": "the pointer"
        }));
    });
    let r = built(&d, false, None);
    assert!(r.ok, "{:#?}", r.issues);
    let game = read(&d.join(".build/game.json"));
    assert!(!game.to_string().contains("core.input.pointer_lock"));
}

/// The target is the gateway's name: a dependency of another target is not needed (`--target godot`), the Bevy
/// players need it (`bevy`, `bevy-web`, the default).
#[test]
fn the_target_says_which_dependencies_are_needed() {
    let d = room_with(|p| without(p, "core.scene"));
    assert_eq!(DEFAULT_TARGET, "bevy-web");
    assert!(built(&d, true, Some("godot")).ok);
    assert_eq!(
        required(&built(&d, true, Some("bevy"))),
        vec![ENGINE_NEEDS_SCENE.replace("bevy-web", "bevy")]
    );
    let (mut out, mut err) = (String::new(), String::new());
    let code = cli(
        &[d.to_string_lossy().into_owned(), "--check".into(), "--target".into(), "godot".into()],
        &mut |s| out.push_str(s),
        &mut |s| err.push_str(s),
    );
    assert_eq!(code, 0, "{out}\n{err}");
}

/// What dev.builder makes of a project without a build: the project packed by the common core (`project::pack`, the
/// function devGame calls through core/scene-format-wasm), its lock, and the check of its kits the gateway runs before
/// loading them (`kits::check_json`, the function module-kits calls through `gf_kits_check`), for the web target.
fn development_mode(d: &Path) -> (Vec<String>, Option<String>) {
    let p = read(&d.join("project.gripforge"));
    let files = project_files(&p, d).unwrap();
    let catalog = kit_catalog(Some(&d.join("kits"))).unwrap();
    let mut reg = Registry::base();
    for api in &catalog {
        reg.add_kit_api(api);
    }
    let version = |id: &str| {
        catalog
            .iter()
            .find(|a| a["id"] == id)
            .and_then(|a| a["version"].as_str())
            .map(String::from)
    };
    let game = project::pack_with(&p, &|f: &str| files.get(f).cloned(), &reg, &version, true)
        .map(|(g, _)| g)
        .unwrap();
    let order: Vec<String> = game["lock"]["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let apis: serde_json::Map<String, Json> = catalog
        .iter()
        .map(|a| (a["id"].as_str().unwrap().to_string(), a.clone()))
        .collect();
    let checked = kits::check_json(&json!({
        "installed": kits::installed_ids(&game["project"]),
        "apis": apis,
        "target": "bevy-web",
    }));
    (order, checked["message"].as_str().map(String::from))
}

/// Criterion 8, the parity: the same project gives the same lock (the built game's is dev.builder's without the
/// development kits, with the game's module last) and the same refusal, message for message.
#[test]
fn the_build_gives_the_lock_and_the_refusal_of_the_development_mode() {
    // Accepted: the same lock.
    let d = room_with(|_| {});
    let (dev_order, dev_refusal) = development_mode(&d);
    assert_eq!(dev_refusal, None);
    let r = built(&d, false, None);
    assert!(r.ok, "{:#?}", r.issues);
    let game = read(&d.join(".build/game.json"));
    let mut expected: Vec<String> = dev_order
        .iter()
        .filter(|k| !DEV_KITS.contains(&k.as_str()))
        .cloned()
        .collect();
    expected.push(GAME_MODULE.to_string());
    let built_order: Vec<String> = game["lock"]["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(built_order, expected);
    for k in &expected[..expected.len() - 1] {
        assert_eq!(
            game["lock"]["kits"][k]["version"],
            json!(read_version(&d, k)),
            "{k}"
        );
    }

    // Refused: the same message, both lacks, in the same order.
    let d = room_with(|p| without(p, "core.scene"));
    edit_kit(&d, "game_engine", |k| {
        k["dependencies"].as_array_mut().unwrap().push(json!({
            "kit": "core.render", "range": "*", "targets": ["bevy-web"], "reason": "the drawing"
        }));
    });
    let (_, dev_refusal) = development_mode(&d);
    let dev_refusal = dev_refusal.expect("dev.builder refuses it");
    let r = built(&d, true, None);
    assert!(!r.ok);
    assert_eq!(required(&r).join("\n"), dev_refusal);
}

fn read_version(d: &Path, id: &str) -> String {
    kit_catalog(Some(&d.join("kits")))
        .unwrap()
        .into_iter()
        .find(|a| a["id"] == id)
        .and_then(|a| a["version"].as_str().map(String::from))
        .unwrap()
}
