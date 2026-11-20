//! The acceptance of the game's code declared in `sources` of project.gripforge (gripforgeai/gripforge#420): the
//! template fps-shooter of the kits repository, `rules.rs` (tests/game-code) declared as its code, built by
//! gripforge-build (`.build/game.json`, `.build/game` with rules.rs), then played headless through the Bevy gateway of
//! the kits, game.main loaded from its native library beside the kits' own. run.sh prepares everything; this file only
//! plays.
//!
//!   GF_ACCEPT_TARGET=<target/debug of the kits> GF_ACCEPT_KITS=<kits> cargo test -- --test-threads=1

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gfkits_bevy::KitsWorldExt;
use gripforge_kit_common::tests::{Kit, modules_dir_in};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DT: f64 = 1.0 / 60.0;

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The folder of kit libraries of the game: each kit of the lock from the kits' sources (built before by run.sh), the
/// game's module game.main from `.build/game` (built by its own crate), and the physics library of core.physics.
fn modules(game: &Value) -> PathBuf {
    let target = PathBuf::from(std::env::var("GF_ACCEPT_TARGET").expect("GF_ACCEPT_TARGET"));
    let kits_dir = PathBuf::from(std::env::var("GF_ACCEPT_KITS").expect("GF_ACCEPT_KITS"));
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut list = Vec::new();
    for id in game["lock"]["order"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        let (api, module) = if id == "game.main" {
            let d = here.join("game/.build/game");
            (read(&d.join("kit.api.json")), read(&d.join("module.json")))
        } else {
            let d = kits_dir.join(id.replace(['.', '-'], "_"));
            (
                read(&d.join("kit.api.json")),
                read(&d.join("logic/module.json")),
            )
        };
        let version = serde_json::from_str::<Value>(&api).unwrap()["version"]
            .as_str()
            .unwrap()
            .to_string();
        list.push(Kit {
            id: leak(id.to_string()),
            version: leak(version),
            api: leak(api),
            module: leak(module),
        });
    }
    let dir = modules_dir_in(&target, "accept_fps_shooter_modules", &list);
    let lib = format!(
        "{}gfphysics_rapier{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    std::fs::copy(target.join(&lib), dir.join(&lib)).expect("gfphysics_rapier built");
    let file = dir.join("kit-modules-native.json");
    let mut manifest: Value = serde_json::from_str(&read(&file)).unwrap();
    manifest["physics"] = json!({ "library": lib });
    std::fs::write(&file, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    dir
}

/// The built game, headless: the game.json of the build with its kit libraries and a fixed step, then the kits
/// (`KitsPlugin`), game.main among them.
fn app() -> App {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut game: Value = serde_json::from_str(&read(&here.join("game/.build/game.json"))).unwrap();
    game["kitModules"] = json!(modules(&game).to_string_lossy());
    game["fixedDt"] = json!(DT);
    game["physicsThread"] = json!("main");
    let mut app = App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        bevy::transform::TransformPlugin,
        bevy::asset::AssetPlugin::default(),
    ))
    .init_asset::<Mesh>()
    .init_asset::<StandardMaterial>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        DT,
    )));
    app.add_plugins(gfkits_bevy::KitsPlugin::new(game));
    app
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

fn var(app: &mut App, path: &str) -> Value {
    app.world_mut()
        .with_kits(|g| g.host().state.get(path).cloned())
        .flatten()
        .unwrap_or_default()
}

#[test]
fn the_declared_game_code_runs_in_game_main_from_before_the_first_scene() {
    let mut app = app();
    run(&mut app, 30);
    assert_eq!(
        var(&mut app, "game_code.setup_before_scene"),
        json!(true),
        "the setup of the game's code ran before the first scene was entered"
    );
    assert_eq!(
        var(&mut app, "game_code.scene"),
        json!("fps_range"),
        "the start scene was entered, once posed by game.main"
    );
    let ticks = var(&mut app, "game_code.ticks").as_f64().expect("ticks");
    assert!(ticks > 0.0, "the game's code runs at the tick: {ticks}");
    run(&mut app, 60);
    let later = var(&mut app, "game_code.ticks").as_f64().unwrap();
    assert!(later >= ticks + 50.0, "one tick per step: {ticks} -> {later}");
    // The kits of the template play beside it: game.engine spawned the three targets of the scene.
    let targets = app
        .world_mut()
        .with_kits(|g| g.host().entities.by_tag("target").len())
        .unwrap_or(0);
    println!("fps-shooter with its declared code: ticks {later}, targets {targets}");
}
