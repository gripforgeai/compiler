//! The game's code declared by the project (`sources` of project.gripforge, decision of gripforgeai/gripforge#420): each
//! file added to the game's module game.main by path, never written; the module of the game it declares gets every hook;
//! no file taken by convention (an `index.rs` beside the project is not read); clear errors for a file that is missing,
//! outside the project, listed twice, badly named, or that hides a file of its own.
//!
//!   cargo test --test sources          UPDATE_GOLDEN=1 cargo test --test sources   (rewrites tests/golden/room-sources/)

use gripforge_compiler::build::{build, BuildOptions, BuildResult};
use gripforge_compiler::cli::main as cli;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn room_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/room")
}

fn tmp() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "gfc-src-{}-{}",
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

fn write(file: &Path, text: &str) {
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// A copy of the room (its kits beside it) whose project declares `sources`.
fn room_with(sources: Json) -> PathBuf {
    let d = tmp().join("room");
    copy_dir(&room_dir(), &d);
    let f = d.join("project.gripforge");
    let mut p: Json = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    p["sources"] = sources;
    std::fs::write(&f, serde_json::to_string_pretty(&p).unwrap()).unwrap();
    d
}

/// The module of the game as a creator writes it: a unit struct on the kit module contract.
const RULES: &str = r#"//! The rules of the room: yours.
use gripforge_kit_guest::{KitModule, Kx};

/// The module of the game: its setup runs before the first scene is entered.
pub struct RoomRules;

impl KitModule for RoomRules {
    fn id(&self) -> &'static str {
        "game.main"
    }
    fn setup(&self, kx: &mut Kx) {
        crate::score::reset(kx);
    }
}
"#;
const SCORE: &str =
    "//! A helper of the rules.\nuse gripforge_kit_guest::Kx;\n\npub fn reset(_kx: &mut Kx) {}\n";

fn with_rules(d: &Path) {
    write(&d.join("rules.rs"), RULES);
    write(&d.join("code/score.rs"), SCORE);
}

fn built(d: &Path) -> BuildResult {
    build(&BuildOptions {
        project: Some(d.to_path_buf()),
        ..Default::default()
    })
}

fn errors(r: &BuildResult) -> Vec<String> {
    r.issues
        .iter()
        .filter(|i| i["level"] == "error")
        .map(|i| i["code"].as_str().unwrap_or("").to_string())
        .collect()
}

fn sha(f: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(f).unwrap()))
}

fn golden(name: &str, actual: &str) {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/room-sources")
        .join(name);
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "golden file missing: {} (UPDATE_GOLDEN=1 cargo test --test sources writes it)",
            file.display()
        )
    });
    assert_eq!(expected, actual, "{name} differs from its golden file");
}

/// `--check` of the command line: its exit code and its error output.
fn check_cli(d: &Path) -> (i32, String) {
    let (mut out, mut err) = (String::new(), String::new());
    let exit = cli(
        &[d.to_string_lossy().into_owned(), "--check".into()],
        &mut |s| out.push_str(s),
        &mut |s| err.push_str(s),
    );
    (exit, err)
}

#[test]
fn the_declared_sources_are_modules_of_game_main_taken_by_path_never_written() {
    let d = room_with(json!(["rules.rs", "code/score.rs"]));
    with_rules(&d);
    let before = (
        sha(&d.join("rules.rs")),
        sha(&d.join("code/score.rs")),
        sha(&d.join("project.gripforge")),
    );
    for _ in 0..2 {
        let r = built(&d);
        assert!(r.ok, "{:?}", r.issues);
        assert!(errors(&r).is_empty());
        assert_eq!(
            (
                sha(&d.join("rules.rs")),
                sha(&d.join("code/score.rs")),
                sha(&d.join("project.gripforge"))
            ),
            before,
            "the creator's files are never written"
        );
        assert_eq!(r.artifacts["gameModule"], json!("crate::rules::RoomRules"));
    }
    let game = d.join(".build/game");
    for f in ["src/lib.rs", "src/module.rs"] {
        golden(
            &f.replace('/', "__"),
            &std::fs::read_to_string(game.join(f)).unwrap(),
        );
    }
    let lib = std::fs::read_to_string(game.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("#[path = \"../../../rules.rs\"]\npub mod rules;"),
        "{lib}"
    );
    assert!(
        lib.contains("#[path = \"../../../code/score.rs\"]\npub mod score;"),
        "{lib}"
    );
    let module = std::fs::read_to_string(game.join("src/module.rs")).unwrap();
    assert!(
        module.contains("const GAME: crate::rules::RoomRules = crate::rules::RoomRules;"),
        "{module}"
    );
    assert!(
        module.contains("fn setup(&self, kx: &mut Kx) {\n        GAME.setup(kx)\n    }"),
        "{module}"
    );
    // The scene is posed, then the game's code gets it; it gets the exit before what was posed is given back.
    let enter = &module[module.find("fn scene_enter").unwrap()..];
    assert!(enter.find("init_scene::enter").unwrap() < enter.find("GAME.scene_enter").unwrap());
    let exit = &module[module.find("fn scene_exit").unwrap()..];
    assert!(exit.find("GAME.scene_exit").unwrap() < exit.find("posed.leave").unwrap());
    // The game the players load does not carry the sources: they are compiled into game.main.
    let game_json: Json =
        serde_json::from_str(&std::fs::read_to_string(d.join(".build/game.json")).unwrap())
            .unwrap();
    assert!(
        game_json["project"].get("sources").is_none(),
        "{}",
        game_json["project"]
    );
    assert!(!d.join(".build/app").exists(), "no Bevy app any more");
}

#[test]
fn an_index_rs_beside_the_project_is_not_taken_by_convention() {
    let d = room_with(json!(null));
    let f = d.join("project.gripforge");
    let mut p: Json = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    p.as_object_mut().unwrap().remove("sources");
    std::fs::write(&f, p.to_string()).unwrap();
    write(&d.join("index.rs"), RULES);
    let r = built(&d);
    assert!(r.ok, "{:?}", r.issues);
    let lib = std::fs::read_to_string(d.join(".build/game/src/lib.rs")).unwrap();
    assert!(!lib.contains("index"), "{lib}");
    assert!(!d.join(".build/app").exists());
    let w: Vec<&Json> = r
        .issues
        .iter()
        .filter(|i| i["code"] == "source_undeclared")
        .collect();
    assert_eq!(w.len(), 1, "{:?}", r.issues);
    assert_eq!(w[0]["level"], "warning");
    assert!(
        w[0]["fix"].as_str().unwrap().contains("\"index.rs\""),
        "{}",
        w[0]
    );
}

#[test]
fn a_declared_source_that_cannot_be_compiled_is_refused_clearly_at_check_and_build() {
    let outside = tmp().join("elsewhere.rs");
    write(&outside, RULES);
    // (the sources, files to write, the code expected)
    type Case = (Json, Vec<(&'static str, &'static str)>, &'static str);
    let cases: Vec<Case> = vec![
        (json!(["rules.rs", "missing.rs"]), vec![("rules.rs", RULES)], "source_missing"),
        (json!(["../elsewhere.rs"]), vec![], "source_outside"),
        (json!(["rules.rs", "rules.rs"]), vec![("rules.rs", RULES)], "source_duplicate"),
        (json!(["rules.rs", "./rules.rs"]), vec![("rules.rs", RULES)], "source_duplicate"),
        (json!(["rules.rs", "a/rules.rs"]), vec![("rules.rs", RULES), ("a/rules.rs", SCORE)], "source_duplicate"),
        (json!(["rules.txt"]), vec![("rules.txt", RULES)], "source_not_rust"),
        (json!("rules.rs"), vec![("rules.rs", RULES)], "sources_invalid"),
        (json!(["lib.rs"]), vec![("lib.rs", RULES)], "source_name_invalid"),
        (json!(["module.rs"]), vec![("module.rs", RULES)], "source_name_invalid"),
        (json!(["2d.rs"]), vec![("2d.rs", RULES)], "source_name_invalid"),
        (json!(["rules.rs"]), vec![("rules.rs", "mod helpers;\npub struct R;\nimpl gripforge_kit_guest::KitModule for R { fn id(&self) -> &'static str { \"game.main\" } fn setup(&self, _: &mut gripforge_kit_guest::Kx) {} }\n")], "source_mod_undeclared"),
        (json!(["code/score.rs"]), vec![("code/score.rs", SCORE)], "sources_module_missing"),
        (json!(["rules.rs"]), vec![("rules.rs", "use gripforge_kit_guest::*;\nstruct Hidden;\nimpl KitModule for Hidden { fn id(&self) -> &'static str { \"game.main\" } fn setup(&self, _: &mut Kx) {} }\n")], "sources_module_missing"),
        (json!(["rules.rs", "other.rs"]), vec![("rules.rs", RULES), ("other.rs", "use gripforge_kit_guest::*;\npub struct Other;\nimpl KitModule for Other { fn id(&self) -> &'static str { \"game.main\" } fn setup(&self, _: &mut Kx) {} }\n")], "sources_module_ambiguous"),
    ];
    for (sources, files, code) in cases {
        let d = room_with(sources.clone());
        for (f, text) in files {
            write(&d.join(f), text);
        }
        let r = built(&d);
        assert!(!r.ok, "{code} ({sources}): refused");
        assert_eq!(errors(&r), [code], "{sources}: {:?}", r.issues);
        let i = r.issues.iter().find(|i| i["code"] == code).unwrap();
        assert!(i["message"].is_string() && i["fix"].is_string(), "{i}");
        assert!(!d.join(".build").exists(), "{code}: nothing written");
        let (exit, err) = check_cli(&d);
        assert_eq!(exit, 1, "{code}: --check refuses too");
        assert!(err.contains(code), "{err}");
    }
    // The ambiguity names both modules.
    let d = room_with(json!(["rules.rs", "other.rs"]));
    write(&d.join("rules.rs"), RULES);
    write(&d.join("other.rs"), "use gripforge_kit_guest::*;\npub struct Other;\nimpl KitModule for Other { fn id(&self) -> &'static str { \"game.main\" } fn setup(&self, _: &mut Kx) {} }\n");
    let m = built(&d)
        .issues
        .iter()
        .find(|i| i["code"] == "sources_module_ambiguous")
        .unwrap()["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        m.contains("crate::rules::RoomRules") && m.contains("crate::other::Other"),
        "{m}"
    );
}

/// A link inside the project that leads out of it is outside.
#[cfg(unix)]
#[test]
fn a_link_out_of_the_project_is_outside() {
    let outside = tmp().join("elsewhere.rs");
    write(&outside, RULES);
    let d = room_with(json!(["rules.rs"]));
    std::os::unix::fs::symlink(&outside, d.join("rules.rs")).unwrap();
    let r = built(&d);
    assert_eq!(errors(&r), ["source_outside"], "{:?}", r.issues);
}
