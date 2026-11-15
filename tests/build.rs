//! The command on the example room (examples/room: its project.gripforge and the kit.api.json of its kits, frozen):
//! what it generates against the golden files, the messages of a broken project, the reading errors, the command line
//! and its exit codes, the JSON report.
//!
//!   cargo test --test build            UPDATE_GOLDEN=1 cargo test --test build   (rewrites tests/golden/)

use gripforge_compiler::build::{build, BuildOptions};
use gripforge_compiler::cli::{main as cli, parse};
use gripforge_compiler::project::{
    find_kits_dir, kit_catalog, project_file, read_json, scene_files,
};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

fn room_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/room")
}

fn room() -> Json {
    serde_json::from_str(&std::fs::read_to_string(room_dir().join("project.gripforge")).unwrap())
        .unwrap()
}

fn tmp() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "gfc-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A project folder holding `project` and the example room's kits.
fn project_dir(project: &Json) -> PathBuf {
    let d = tmp();
    std::fs::write(d.join("project.gripforge"), project.to_string()).unwrap();
    d
}

fn kits() -> PathBuf {
    room_dir().join("kits")
}

fn scene<'a>(p: &'a mut Json, id: &str) -> &'a mut Json {
    p["data"]["scenes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["id"] == id)
        .unwrap()
}

fn node<'a>(s: &'a mut Json, name: &str) -> &'a mut Json {
    s["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|n| n["name"] == name)
        .unwrap()
}

fn run(args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (String::new(), String::new());
    let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let code = cli(
        &argv,
        &mut |s| {
            out.push_str(s);
            out.push('\n');
        },
        &mut |s| {
            err.push_str(s);
            err.push('\n');
        },
    );
    (code, out, err)
}

/// A line diff, `-` expected `+` actual (only the differing lines, with their numbers).
fn diff(expected: &str, actual: &str) -> String {
    let (a, b): (Vec<_>, Vec<_>) = (expected.lines().collect(), actual.lines().collect());
    let mut out = Vec::new();
    for k in 0..a.len().max(b.len()) {
        if a.get(k) != b.get(k) {
            if let Some(x) = a.get(k) {
                out.push(format!("- {:>4} | {x}", k + 1));
            }
            if let Some(x) = b.get(k) {
                out.push(format!("+ {:>4} | {x}", k + 1));
            }
        }
    }
    out.join("\n")
}

fn golden(name: &str, actual: &str) {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "golden file missing: {} (UPDATE_GOLDEN=1 cargo test writes it)",
            file.display()
        )
    });
    let d = diff(&expected, actual);
    assert!(d.is_empty(), "{name} differs from its golden file (- expected, + generated):\n{d}\n\nIf the change is wanted: UPDATE_GOLDEN=1 cargo test, and review the diff of tests/golden/ in the PR.");
}

#[test]
fn the_room_generates_the_golden_files() {
    // A copy of the room, built into its own .build/: the paths of the kits' accessors are the same everywhere.
    let room = tmp().join("room");
    copy_dir(&room_dir(), &room);
    let out = room.join(".build");
    let r = build(&BuildOptions {
        project: Some(room),
        ..Default::default()
    });
    assert!(r.ok, "{:?}", r.issues);
    for f in [
        "game.json",
        "game/Cargo.toml",
        "game/kit.api.json",
        "game/module.json",
        "game/src/lib.rs",
        "game/src/index.rs",
        "game/src/init_scene.rs",
        "game/src/init_scene/menu.rs",
        "game/src/init_scene/level.rs",
    ] {
        golden(
            &format!("room/{}", f.replace('/', "__")),
            &std::fs::read_to_string(out.join(f)).unwrap(),
        );
    }
    let report: Json =
        serde_json::from_str(&std::fs::read_to_string(out.join("build.json")).unwrap()).unwrap();
    assert_eq!(report["schema"], "gripforge.build-report/1");
    assert_eq!(report["ok"], true);
}

fn broken() -> Json {
    let mut p = room();
    scene(&mut p, "level")["kits"]["core.light"]["brightness"] = json!(2);
    scene(&mut p, "level")["kits"]["core.light"]["exposure"] = json!(-1);
    node(scene(&mut p, "level"), "Sun")["light"]["intensity"] = json!("strong");
    node(scene(&mut p, "level"), "Shelf")["scene"] = json!("scenes/cupboard.gfscene");
    scene(&mut p, "menu")["on"][0]["do"][0]["load_scene"]["scene"] = json!("credits");
    p["installed"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "world.nowhere", "version": "1.0.0" }));
    p["data"]["scenes"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "void", "nodes": [] }));
    p
}

#[test]
fn the_messages_of_a_broken_project_are_the_golden_ones() {
    let d = project_dir(&broken());
    let (code, _, err) = run(&[
        d.to_str().unwrap(),
        "--check",
        "--kits-path",
        kits().to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    golden("broken-project.txt", &err);
    assert!(err.contains("error kit_unknown — project.gripforge › property installed: the kit world.nowhere version 1.0.0 is not in the project"), "{err}");
    assert!(
        err.contains("gripforge game-attach"),
        "the missing kit says how to install the kits: {err}"
    );
}

#[test]
fn check_on_the_room_exits_0_and_writes_nothing() {
    let d = tmp();
    std::fs::write(d.join("project.gripforge"), room().to_string()).unwrap();
    let (code, _, err) = run(&[
        d.to_str().unwrap(),
        "--check",
        "--kits-path",
        kits().to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("checked (0 warning(s))"), "{err}");
    assert!(!d.join(".build").exists());
}

#[test]
fn the_kits_are_found_in_the_project_else_a_clear_error() {
    let (code, _, err) = run(&[room_dir().to_str().unwrap(), "--check"]);
    assert_eq!(code, 0, "kits/ beside the project: {err}");
    let hidden = tmp();
    std::fs::write(hidden.join("project.gripforge"), room().to_string()).unwrap();
    std::fs::create_dir_all(hidden.join(".gripforge")).unwrap();
    copy_dir(&kits(), &hidden.join(".gripforge/kits"));
    assert_eq!(find_kits_dir(&hidden), Some(hidden.join(".gripforge/kits")));
    let none = project_dir(&room());
    let (code, _, err) = run(&[none.to_str().unwrap(), "--check"]);
    assert_eq!(code, 1);
    assert!(
        err.contains("kits_missing") && err.contains("gripforge game-attach"),
        "{err}"
    );
    let (code, _, err) = run(&[
        none.to_str().unwrap(),
        "--check",
        "--kits-path",
        "/no/such/kits",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("does not exist"), "{err}");
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

#[test]
fn a_kit_of_another_schema_is_refused_with_kits_incompatible() {
    let k = tmp();
    std::fs::create_dir_all(k.join("core_light")).unwrap();
    std::fs::write(
        k.join("core_light/kit.api.json"),
        r#"{"id":"core.light","schema":"gripforge.kit-api/0"}"#,
    )
    .unwrap();
    let r = build(&BuildOptions {
        project: Some(project_dir(&room())),
        kits_path: Some(k),
        check: true,
        ..Default::default()
    });
    assert!(!r.ok);
    assert_eq!(r.issues[0]["code"], "kits_incompatible");
}

#[test]
fn check_json_and_a_project_that_cannot_be_read() {
    let (code, out, _) = run(&[room_dir().to_str().unwrap(), "--check", "--json"]);
    assert_eq!(code, 0);
    let j: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(j["schema"], "gripforge.build-report/1");
    assert_eq!(j["errors"], 0);
    let d = tmp();
    std::fs::write(
        d.join("project.gripforge"),
        "{\n \"id\": \"x\",\n \"data\": {\n  \"scenes\": [,]\n }\n}",
    )
    .unwrap();
    let (code, out, _) = run(&[d.to_str().unwrap(), "--json"]);
    assert_eq!(code, 1);
    let j: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(j["issues"][0]["code"], "file_not_json");
    assert!(
        j["issues"][0]["message"]
            .as_str()
            .unwrap()
            .contains("line 4"),
        "{j}"
    );
}

#[test]
fn the_command_line_its_mistakes_and_exit_codes() {
    assert_eq!(
        parse(&[
            "a".into(),
            "--out".into(),
            "o".into(),
            "--check".into(),
            "--lenient".into()
        ])
        .unwrap()
        .out
        .as_deref(),
        Some("o")
    );
    assert!(parse(&["--out".into()])
        .unwrap_err()
        .contains("needs a value"));
    assert!(parse(&["--fly".into()])
        .unwrap_err()
        .contains("unknown option --fly"));
    assert!(parse(&["a".into(), "b".into()])
        .unwrap_err()
        .contains("one project only"));
    let (code, _, err) = run(&["--fly"]);
    assert_eq!(code, 2);
    assert!(err.contains("usage:"));
    assert_eq!(run(&["--help"]).0, 0);
    let (code, out, _) = run(&["--version"]);
    assert_eq!((code, out.trim()), (0, env!("CARGO_PKG_VERSION")));
}

#[test]
fn the_project_file_of_a_path() {
    assert_eq!(
        project_file(Some(&room_dir())).unwrap(),
        room_dir().join("project.gripforge")
    );
    let d = tmp();
    std::fs::write(d.join("arena.gripforge"), "{}").unwrap();
    assert_eq!(project_file(Some(&d)).unwrap(), d.join("arena.gripforge"));
    std::fs::write(d.join("b.gripforge"), "{}").unwrap();
    assert!(project_file(Some(&d)).unwrap_err().0["message"]
        .as_str()
        .unwrap()
        .contains("several .gripforge files (arena.gripforge, b.gripforge)"));
    assert!(project_file(Some(&tmp())).unwrap_err().0["message"]
        .as_str()
        .unwrap()
        .contains("holds no project.gripforge"));
    assert_eq!(
        project_file(Some(&tmp().join("nope"))).unwrap_err().0["code"],
        "project_missing"
    );
}

#[test]
fn the_scene_files_of_an_older_project() {
    let d = tmp();
    std::fs::create_dir_all(d.join("scenes")).unwrap();
    std::fs::write(d.join("scenes/level.gfscene"), r#"{"nodes":[{"name":"Shelf","type":"scene","scene":"scenes/shelf.gfscene"},{"name":"Gone","type":"scene","scene":"scenes/gone.gfscene"}]}"#).unwrap();
    std::fs::write(
        d.join("scenes/shelf.gfscene"),
        r#"{"nodes":[{"name":"Board","type":"shape"}]}"#,
    )
    .unwrap();
    let p = json!({ "id": "old", "data": { "scenes": [{ "id": "level", "file": "scenes/level.gfscene" }, { "id": "lost", "file": "scenes/lost.gfscene" }] } });
    let files = scene_files(&p, &d).unwrap();
    assert_eq!(files.keys().cloned().collect::<Vec<_>>().len(), 2);
    assert!(files.contains_key("scenes/shelf.gfscene"));
    std::fs::write(d.join("scenes/level.gfscene"), "{ \"nodes\": [ }").unwrap();
    let e = scene_files(&p, &d).unwrap_err().0;
    assert_eq!(e["code"], "file_not_json");
    assert!(e["file"].as_str().unwrap().ends_with("level.gfscene"));
    assert_eq!(
        read_json(&room_dir().join("project.gripforge"), "file_not_json").unwrap()["id"],
        "dev_builder_room"
    );
    assert_eq!(kit_catalog(Some(&kits())).unwrap().len(), 4);
}

#[test]
fn the_generated_crate_takes_the_sdk_by_path_or_git() {
    let out = tmp();
    let r = build(&BuildOptions {
        project: Some(room_dir()),
        out: Some(out.clone()),
        sdk_rev: Some("main".into()),
        ..Default::default()
    });
    assert!(r.ok);
    let toml = std::fs::read_to_string(out.join("game/Cargo.toml")).unwrap();
    assert!(toml.contains("branch = \"main\""), "{toml}");
    // By default: the public SDK of this repository (sdk/), at the compiler's tag; never the private kits repository.
    let r = build(&BuildOptions {
        project: Some(room_dir()),
        out: Some(out.clone()),
        ..Default::default()
    });
    assert!(r.ok);
    let toml = std::fs::read_to_string(out.join("game/Cargo.toml")).unwrap();
    assert!(
        toml.contains(&format!(
            "gripforge-kit-guest = {{ git = \"https://github.com/gripforgeai/compiler\", tag = \"v{}\" }}",
            env!("CARGO_PKG_VERSION")
        )),
        "{toml}"
    );
    assert!(!toml.contains("module-kits"), "{toml}");
    for (rev, pin) in [("v0.2.0", "tag"), ("65ffb22", "rev")] {
        let r = build(&BuildOptions {
            project: Some(room_dir()),
            out: Some(out.clone()),
            sdk_rev: Some(rev.into()),
            ..Default::default()
        });
        assert!(r.ok);
        let toml = std::fs::read_to_string(out.join("game/Cargo.toml")).unwrap();
        assert!(toml.contains(&format!("{pin} = \"{rev}\"")), "{toml}");
    }
    // This repository's sdk/: the crates at its top.
    let public = Path::new(env!("CARGO_MANIFEST_DIR")).join("sdk");
    let r = build(&BuildOptions {
        project: Some(room_dir()),
        out: Some(out.clone()),
        sdk: Some(public.clone()),
        ..Default::default()
    });
    assert!(r.ok);
    let toml = std::fs::read_to_string(out.join("game/Cargo.toml")).unwrap();
    assert!(
        toml.contains(&format!(
            "path = \"{}\"",
            public.join("kit-guest").display()
        )),
        "{toml}"
    );
    assert!(out.join("game/rust-toolchain.toml").exists());
    // A checkout of the kits repository: the crates under core/.
    let sdk = tmp();
    std::fs::write(
        sdk.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.99.0\"\n",
    )
    .unwrap();
    let r = build(&BuildOptions {
        project: Some(room_dir()),
        out: Some(out.clone()),
        sdk: Some(sdk.clone()),
        ..Default::default()
    });
    assert!(r.ok);
    let toml = std::fs::read_to_string(out.join("game/Cargo.toml")).unwrap();
    assert!(
        toml.contains(&format!(
            "path = \"{}\"",
            sdk.join("core/kit-guest").display()
        )),
        "{toml}"
    );
    assert!(out.join("game/rust-toolchain.toml").exists());
}
