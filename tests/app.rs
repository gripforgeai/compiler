//! The creator's `index.rs` (decision D01 of gripforgeai/gripforge#420): a Bevy `Plugin` written by the creator at the
//! root of the project, beside `project.gripforge`, with their modules in `src/` (optional). The build writes the Bevy
//! game around it (`.build/app/`), never the file itself: it is taken by path, the same bytes before and after.
//!
//!   cargo test --test app            UPDATE_GOLDEN=1 cargo test --test app   (rewrites tests/golden/room-app/)

use gripforge_compiler::build::{build, BuildOptions};
use gripforge_compiler::cli::main as cli;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn room_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/room")
}

fn tmp() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "gfc-app-{}-{}",
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

/// A copy of the room (its kits beside it), built into its own `.build/`.
fn room() -> PathBuf {
    let d = tmp().join("room");
    copy_dir(&room_dir(), &d);
    d
}

fn write(file: &Path, text: &str) {
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// The rules of the room as a creator writes them: one Bevy plugin, its systems after the kits' step.
const INDEX: &str = r#"//! The rules of the room: yours.
use bevy::prelude::*;
use gfkits_bevy::KitsSet;

/// Adds the rules. Add it after `KitsPlugin`.
pub struct RoomRules;

impl Plugin for RoomRules {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, crate::rules::count.after(KitsSet::Step));
        let _ = crate::templates::door::index::OPEN;
    }
}
"#;

fn with_index(d: &Path) {
    write(&d.join("index.rs"), INDEX);
    write(
        &d.join("src/rules.rs"),
        "pub fn count() {}\n",
    );
    write(
        &d.join("src/templates/mod.rs"),
        "// The templates copied into this game (tools/packs copy): yours to change.\npub mod door;\n",
    );
    write(&d.join("src/templates/door/mod.rs"), "pub mod index;\n");
    write(
        &d.join("src/templates/door/index.rs"),
        "pub const OPEN: bool = true;\n",
    );
}

/// The sha256 of every file the creator owns (`index.rs`, `src/**`, `project.gripforge`), by path.
fn digests(d: &Path) -> Vec<(String, String)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let sum = Sha256::digest(std::fs::read(&p).unwrap());
                out.push((
                    p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                    format!("{sum:x}"),
                ));
            }
        }
    }
    let mut out = Vec::new();
    for f in ["index.rs", "project.gripforge"] {
        let sum = Sha256::digest(std::fs::read(d.join(f)).unwrap());
        out.push((f.to_string(), format!("{sum:x}")));
    }
    walk(d, &d.join("src"), &mut out);
    out
}

fn built(d: &Path) -> gripforge_compiler::build::BuildResult {
    build(&BuildOptions {
        project: Some(d.to_path_buf()),
        ..Default::default()
    })
}

fn codes(r: &gripforge_compiler::build::BuildResult) -> Vec<String> {
    r.issues
        .iter()
        .filter(|i| i["level"] == "error")
        .map(|i| i["code"].as_str().unwrap_or("").to_string())
        .collect()
}

fn golden(name: &str, actual: &str) {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/room-app")
        .join(name);
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "golden file missing: {} (UPDATE_GOLDEN=1 cargo test --test app writes it)",
            file.display()
        )
    });
    assert_eq!(expected, actual, "{name} differs from its golden file");
}

#[test]
fn without_index_rs_the_build_writes_no_app_and_its_module_is_not_an_index() {
    let d = room();
    let r = built(&d);
    assert!(r.ok, "{:?}", r.issues);
    assert!(!d.join(".build/app").exists());
    // The generated module of game.main is not called index.rs any more: index.rs is the creator's.
    assert!(d.join(".build/game/src/module.rs").exists());
    assert!(!d.join(".build/game/src/index.rs").exists());
}

#[test]
fn the_creators_index_rs_is_taken_by_path_never_written() {
    let d = room();
    with_index(&d);
    let before = digests(&d);
    for _ in 0..2 {
        let r = built(&d);
        assert!(r.ok, "{:?}", r.issues);
        assert_eq!(digests(&d), before, "the creator's files are never written");
    }
    let app = d.join(".build/app");
    for f in ["Cargo.toml", "src/lib.rs", "src/main.rs"] {
        golden(
            &f.replace('/', "__"),
            &std::fs::read_to_string(app.join(f)).unwrap(),
        );
    }
    let lib = std::fs::read_to_string(app.join("src/lib.rs")).unwrap();
    assert!(lib.contains("#[path = \"../../../index.rs\"]\npub mod index;"), "{lib}");
    assert!(lib.contains("#[path = \"../../../src/rules.rs\"]\npub mod rules;"), "{lib}");
    assert!(
        lib.contains("#[path = \"../../../src/templates/mod.rs\"]\npub mod templates;"),
        "{lib}"
    );
    let (kits, plugin) = (
        lib.find("KitsPlugin::new(game)").expect("the kits"),
        lib.find("index::RoomRules").expect("the creator's plugin"),
    );
    assert!(kits < plugin, "the creator's plugin comes after KitsPlugin");
    let files: Vec<String> = r_files(&d);
    assert!(files.iter().any(|f| f == "app/src/lib.rs"), "{files:?}");
}

fn r_files(d: &Path) -> Vec<String> {
    let report: Json =
        serde_json::from_str(&std::fs::read_to_string(d.join(".build/build.json")).unwrap())
            .unwrap();
    report["artifacts"]["app"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str().map(|s| format!("app/{s}")))
        .collect()
}

#[test]
fn an_index_rs_without_one_plugin_is_refused_at_check_and_build() {
    let cases: [(&str, &str); 3] = [
        ("//! nothing here\npub fn rules() {}\n", "index_plugin_missing"),
        (
            "use bevy::prelude::*;\npub struct A;\nimpl Plugin for A { fn build(&self, _: &mut App) {} }\npub struct B;\nimpl Plugin for B { fn build(&self, _: &mut App) {} }\n",
            "index_plugin_ambiguous",
        ),
        (
            "use bevy::prelude::*;\nstruct Hidden;\nimpl Plugin for Hidden { fn build(&self, _: &mut App) {} }\n",
            "index_plugin_missing",
        ),
    ];
    for (text, code) in cases {
        let d = room();
        write(&d.join("index.rs"), text);
        let r = built(&d);
        assert!(!r.ok, "{code}: refused");
        assert_eq!(codes(&r), [code], "{:?}", r.issues);
        let i = &r.issues[0];
        assert!(i["message"].is_string() && i["fix"].is_string(), "{i}");
        assert!(i["file"].as_str().unwrap().ends_with("index.rs"), "{i}");
        assert!(!d.join(".build").exists(), "nothing written");
        let (mut out, mut err) = (String::new(), String::new());
        let exit = cli(
            &[d.to_string_lossy().into_owned(), "--check".into()],
            &mut |s| out.push_str(s),
            &mut |s| err.push_str(s),
        );
        assert_eq!(exit, 1, "{code}: --check refuses too");
        assert!(err.contains(code), "{err}");
    }
    let two = &cases[1];
    let d = room();
    write(&d.join("index.rs"), two.0);
    let r = built(&d);
    let m = r.issues[0]["message"].as_str().unwrap();
    assert!(m.contains('A') && m.contains('B'), "names the candidates: {m}");
}

#[test]
fn a_crate_root_or_an_index_module_in_src_is_refused() {
    for f in ["src/lib.rs", "src/main.rs", "src/index.rs", "src/2d.rs"] {
        let d = room();
        with_index(&d);
        write(&d.join(f), "\n");
        let r = built(&d);
        assert_eq!(codes(&r), ["src_module_invalid"], "{f}: {:?}", r.issues);
        assert!(r.issues[0]["file"].as_str().unwrap().ends_with(f), "{:?}", r.issues);
    }
}

#[test]
fn the_gateway_comes_from_the_sdk_checkout_else_by_git_with_a_warning() {
    let d = room();
    with_index(&d);
    let r = built(&d);
    assert!(r.ok);
    let toml = std::fs::read_to_string(d.join(".build/app/Cargo.toml")).unwrap();
    assert!(
        toml.contains("gfkits-bevy-native = { git = \"https://github.com/gripforgeai/module-kits\", rev = "),
        "{toml}"
    );
    assert!(
        r.issues
            .iter()
            .any(|i| i["code"] == "gateway_by_git" && i["level"] == "warning"),
        "{:?}",
        r.issues
    );
    // A checkout of the kits repository: the gateway by path, no warning.
    let sdk = tmp();
    write(
        &sdk.join("gateways/gfkits-bevy-native/native/Cargo.toml"),
        "[package]\nname = \"gfkits-bevy-native\"\n",
    );
    write(&sdk.join("core/kit-guest/Cargo.toml"), "[package]\n");
    let r = build(&BuildOptions {
        project: Some(d.clone()),
        sdk: Some(sdk.clone()),
        ..Default::default()
    });
    assert!(r.ok, "{:?}", r.issues);
    let toml = std::fs::read_to_string(d.join(".build/app/Cargo.toml")).unwrap();
    assert!(
        toml.contains(&format!(
            "gfkits-bevy-native = {{ path = {:?}",
            sdk.join("gateways/gfkits-bevy-native/native").to_string_lossy()
        )),
        "{toml}"
    );
    assert!(!r.issues.iter().any(|i| i["code"] == "gateway_by_git"));
}
