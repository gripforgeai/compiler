//! The acceptance of D01 (gripforgeai/gripforge#420): the template fps-shooter of the kits repository, its
//! `bevy/index.rs` laid as is at the root of the project, built by gripforge-build (`.build/app`, `.build/game`), then
//! played headless through the generated crate: the same play as the templates' own proof
//! (module-kits templates/check/bevy/tests/fps_shooter.rs, « from data »), on the game the compiler made. run.sh
//! prepares everything; this file only plays.
//!
//!   GF_ACCEPT_TARGET=<target/debug of the kits> cargo test -- --test-threads=1

use gripforge_app_fps_shooter::bevy;
use gripforge_app_fps_shooter::bevy::prelude::*;
use gripforge_app_fps_shooter::bevy::time::TimeUpdateStrategy;
use gripforge_app_fps_shooter::gfkits_bevy::{Arg, KitGame, KitsWorldExt};
use gripforge_app_fps_shooter::index as fps;
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

/// The generated game, headless: the game.json of the build with its kit libraries and a fixed step, `add_game` of the
/// generated crate (KitsPlugin, then index::FpsShooter).
fn app() -> App {
    let mut game = gripforge_app_fps_shooter::game();
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
    gripforge_app_fps_shooter::add_game(&mut app, game);
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

fn shooting(app: &App) -> fps::Shooting {
    app.world().resource::<fps::Shooting>().clone()
}

fn mouse(app: &mut App, code: &str, down: bool) {
    app.world_mut()
        .non_send_mut::<KitGame>()
        .raw(1, 0, code, if down { 1. } else { 0. }, 0., 0);
}

fn click(app: &mut App) {
    mouse(app, "Mouse0", true);
    run(app, 2);
    mouse(app, "Mouse0", false);
    run(app, 10);
}

fn target(app: &mut App, id: &str) -> Value {
    app.world_mut()
        .with_kits(|g| g.host().entities.component(id, "target").cloned())
        .flatten()
        .unwrap_or_default()
}

#[test]
fn the_built_fps_shooter_plays_as_the_template() {
    let mut app = app();
    run(&mut app, 30);
    let s = shooting(&app);
    assert!(s.problem.is_none(), "{:?}", s.problem);
    assert_eq!(s.targets.len(), 3, "three targets");
    assert_eq!(
        var(&mut app, "fps_shooter.ammo"),
        json!(fps::MAGAZINE),
        "a full magazine"
    );
    assert_eq!(
        var(&mut app, "menu.main.open"),
        json!(true),
        "the title screen is open"
    );
    app.world_mut()
        .kit_call("menu.main", "choose", &[Arg::Str("play")], |_| ())
        .expect("choose play");
    run(&mut app, 5);
    assert_eq!(
        var(&mut app, "menu.main.open"),
        json!(false),
        "Play closed the menu"
    );

    let ahead = s
        .targets
        .iter()
        .find(|id| {
            app.world_mut()
                .with_kits(|g| g.host().entities.transform(id).map(|p| p.x.abs() < 0.1))
                .flatten()
                .unwrap_or(false)
        })
        .cloned()
        .expect("a target ahead");
    click(&mut app);
    assert_eq!(
        target(&mut app, &ahead)["hp"],
        json!(fps::TARGET_HP - 1),
        "the first shot hit"
    );
    click(&mut app);
    let s = shooting(&app);
    assert_eq!(
        (s.shots, s.hits, s.score),
        (2, 2, 1),
        "two shots, two hits, one target down"
    );
    assert_eq!(
        target(&mut app, &ahead)["down"],
        json!(true),
        "the target fell"
    );
    assert_eq!(
        var(&mut app, "fps_shooter.ammo"),
        json!(fps::MAGAZINE - 2),
        "two rounds less"
    );
    assert_eq!(var(&mut app, "fps_shooter.score"), json!(1), "the score");

    // R reloads: reloading for RELOAD_SECONDS, then a full magazine.
    app.world_mut().non_send_mut::<KitGame>().key("KeyR", true);
    run(&mut app, 2);
    app.world_mut().non_send_mut::<KitGame>().key("KeyR", false);
    assert_eq!(
        var(&mut app, "fps_shooter.reloading"),
        json!(true),
        "reloading"
    );
    run(&mut app, (fps::RELOAD_SECONDS * 60.) as u32 + 5);
    assert_eq!(
        var(&mut app, "fps_shooter.reloading"),
        json!(false),
        "reloaded"
    );
    assert_eq!(
        var(&mut app, "fps_shooter.ammo"),
        json!(fps::MAGAZINE),
        "a full magazine again"
    );

    // The target stands up again RISE_SECONDS after its fall.
    run(&mut app, (fps::RISE_SECONDS * 60.) as u32);
    let t = target(&mut app, &ahead);
    assert_eq!(
        (t["down"].clone(), t["hp"].clone()),
        (json!(false), json!(fps::TARGET_HP)),
        "the target stood up"
    );
    println!("fps-shooter built by gripforge-build: {:?}", shooting(&app));
}
