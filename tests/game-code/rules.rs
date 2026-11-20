//! The game's code of the example room, as a creator writes it: declared in `sources` of project.gripforge
//! (`"sources": ["rules.rs"]`), compiled into the game's module game.main, native and wasm (the CI job `game`), and
//! played by the acceptance (tests/accept/fps-shooter). docs/sources.md says when the host calls each hook.

use gripforge_kit_guest::{KitModule, Kx};
use serde_json::{json, Value as Json};

/// The module of the game: a unit struct, stateless like every kit module (what it keeps lives in the host: here the
/// state of game.engine, `game_code.*`).
pub struct GameRules;

impl KitModule for GameRules {
    fn id(&self) -> &'static str {
        "game.main"
    }

    /// At the mount, after the setup of every kit of the lock, before any `ready` and before the first scene.
    fn setup(&self, kx: &mut Kx) {
        let entered = kx.get_var("game_code.scene").is_some_and(|s| !s.is_null());
        kx.set_var("game_code.setup_before_scene", json!(!entered));
        kx.set_num("game_code.ticks", 0.0);
    }

    /// At each tick of the simulation.
    fn update(&self, kx: &mut Kx, _dt: f64) {
        let n = kx
            .get_var("game_code.ticks")
            .and_then(Json::as_f64)
            .unwrap_or(0.0);
        kx.set_num("game_code.ticks", n + 1.0);
    }

    /// Once game.main posed the scene.
    fn scene_enter(&self, kx: &mut Kx, scene: &Json) {
        kx.set_var("game_code.scene", scene["id"].clone());
    }
}
