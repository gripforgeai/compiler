//! The façades of the kits in the generated crate: each kit ships its own in its folder (`<kit>/facade`), the crate
//! depends on it by path at the kit's version; `--kits-path` takes the kits (and so their façades) from another folder
//! (the sources of the kits, for a demo); a kit without its façade, or with a façade of another version, is an error
//! that says what to do.
//!
//!   cargo test --test facades

use gripforge_compiler::build::{build, BuildOptions};
use gripforge_compiler::facades::{crate_name, package_of, relative};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

fn room_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/room")
}

fn tmp() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "gfc-facades-{}-{}",
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
        if e.path().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// A copy of the example room (project and its downloaded kits) in a temporary folder.
fn room_copy() -> PathBuf {
    let d = tmp().join("room");
    copy_dir(&room_dir(), &d);
    d
}

fn cargo_toml(project: &Path) -> String {
    std::fs::read_to_string(project.join(".build/game/Cargo.toml")).unwrap()
}

/// The dependency line of a façade in a Cargo.toml (`<crate> = { path = "...", version = "=..." }`).
fn dep_line<'a>(toml: &'a str, krate: &str) -> Option<&'a str> {
    toml.lines().find(|l| l.starts_with(&format!("{krate} = ")))
}

/// The folder a dependency line's `path` names, resolved from the crate's folder.
fn dep_dir(crate_dir: &Path, line: &str) -> PathBuf {
    let path = line
        .split("path = \"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    crate_dir.join(path).canonicalize().unwrap()
}

#[test]
fn the_names_and_the_manifest_of_a_facade() {
    assert_eq!(crate_name("core.light"), "gripforge-kit-facade-core-light");
    assert_eq!(
        crate_name("locomotion.first_person"),
        "gripforge-kit-facade-locomotion-first-person"
    );
    let toml = "[package]\nname = \"gripforge-kit-facade-core-light\"\nversion = \"1.2.3\" # the kit's\n\n[dependencies]\nname = \"other\"\nversion = \"9\"\n";
    assert_eq!(
        package_of(toml),
        (
            Some("gripforge-kit-facade-core-light".into()),
            Some("1.2.3".into())
        )
    );
    assert_eq!(package_of("[dependencies]\nname = \"x\"\n"), (None, None));
    let base = tmp();
    std::fs::create_dir_all(base.join("a/b")).unwrap();
    std::fs::create_dir_all(base.join("kits/k/facade")).unwrap();
    assert_eq!(
        relative(&base.join("a/b"), &base.join("kits/k/facade")),
        PathBuf::from("../../kits/k/facade")
    );
}

#[test]
fn each_kit_s_facade_is_taken_from_its_folder_at_the_kit_s_version() {
    let p = room_copy();
    let r = build(&BuildOptions {
        project: Some(p.clone()),
        ..Default::default()
    });
    assert!(r.ok, "{:?}", r.issues);
    let toml = cargo_toml(&p);
    let game = p.join(".build/game");
    for (krate, dir, version) in [
        ("gripforge-kit-facade-core-light", "core_light", "1.0.0"),
        ("gripforge-kit-facade-core-camera", "core_camera", "1.1.0"),
        ("gripforge-kit-facade-game-engine", "game_engine", "1.24.1"),
    ] {
        let line = dep_line(&toml, krate).unwrap_or_else(|| panic!("no {krate} in\n{toml}"));
        assert!(
            line.contains(&format!("version = \"={version}\"")),
            "{krate} at the version of its kit: {line}"
        );
        assert_eq!(
            dep_dir(&game, line),
            p.join("kits")
                .join(dir)
                .join("facade")
                .canonicalize()
                .unwrap()
        );
    }
    assert!(
        dep_line(&toml, "gripforge-kit-facade-dev-builder").is_none(),
        "dev.builder is not in the built game: {toml}"
    );
    assert!(
        toml.contains("[patch.crates-io]\ngripforge-kit-facade = "),
        "the façades' gripforge-kit-facade is the SDK's: {toml}"
    );

    // A kit downloaded at another version than its façade: refused, both versions said.
    let api = p.join("kits/core_light/kit.api.json");
    let mut j: Json = serde_json::from_str(&std::fs::read_to_string(&api).unwrap()).unwrap();
    j["version"] = "1.0.1".into();
    std::fs::write(&api, j.to_string()).unwrap();
    let r = build(&BuildOptions {
        project: Some(p.clone()),
        lenient: true,
        ..Default::default()
    });
    let i = r
        .issues
        .iter()
        .find(|i| i["code"] == "facade_mismatch")
        .unwrap_or_else(|| panic!("{:?}", r.issues));
    let m = i["message"].as_str().unwrap();
    assert!(m.contains("1.0.1") && m.contains("version 1.0.0"), "{m}");
    assert_eq!(i["field"], "kits.core.light.facade");
}

#[test]
fn kits_path_takes_the_kits_and_their_facades_from_that_folder() {
    // A project with no kits of its own; the kits (as in the sources of the kits repository) elsewhere.
    let p = tmp().join("game");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::copy(
        room_dir().join("project.gripforge"),
        p.join("project.gripforge"),
    )
    .unwrap();
    let sources = tmp().join("module-kits/kits");
    copy_dir(&room_dir().join("kits"), &sources);
    let r = build(&BuildOptions {
        project: Some(p.clone()),
        kits_path: Some(sources.clone()),
        ..Default::default()
    });
    assert!(r.ok, "{:?}", r.issues);
    let toml = cargo_toml(&p);
    let line = dep_line(&toml, "gripforge-kit-facade-core-light").unwrap();
    assert_eq!(
        dep_dir(&p.join(".build/game"), line),
        sources.join("core_light/facade").canonicalize().unwrap(),
        "{line}"
    );
    assert!(
        line.starts_with("gripforge-kit-facade-core-light = { path = \"../../../"),
        "a local path: {line}"
    );
}

#[test]
fn a_kit_without_its_facade_says_how_to_get_it() {
    let p = room_copy();
    std::fs::remove_dir_all(p.join("kits/core_camera/facade")).unwrap();
    let r = build(&BuildOptions {
        project: Some(p.clone()),
        ..Default::default()
    });
    assert!(!r.ok);
    let i = r
        .issues
        .iter()
        .find(|i| i["code"] == "facade_missing")
        .unwrap_or_else(|| panic!("{:?}", r.issues));
    let m = i["message"].as_str().unwrap();
    let fix = i["fix"].as_str().unwrap();
    assert!(
        m.contains("core.camera version 1.1.0 has no Rust façade"),
        "{m}"
    );
    assert!(
        fix.contains("gripforge game-attach") && fix.contains("--kits-path"),
        "{fix}"
    );
    assert!(
        !p.join(".build/game").exists(),
        "nothing generated without the façades"
    );
    // The strict check says it too: an AI fixes the kits before asking for the build.
    let r = build(&BuildOptions {
        project: Some(p),
        check: true,
        ..Default::default()
    });
    assert!(!r.ok && r.issues.iter().any(|i| i["code"] == "facade_missing"));
}
