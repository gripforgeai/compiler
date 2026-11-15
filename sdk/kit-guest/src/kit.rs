//! The kit module contract (`KitModule`) — shared word for word by the core (src/host.rs) and by the kit logic
//! modules compiled one per wasm (the file is the kit façade's, the core includes it): a kit's source compiles against
//! either without a change.

use crate::bus::GameEvent;
use crate::host::Kx;
use serde_json::Value as Json;

/// A problem found by a module's `validate` (`{ path, message }`, as the TypeScript issues).
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

impl Issue {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Issue {
        Issue {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// One kit ported to Rust. A module is stateless: what it keeps lives in the host (`Kx::put` / `Kx::data`), so
/// any verb, handler or service of any kit may call back into any other — re-entrancy is free, as in TypeScript.
#[allow(unused_variables)]
pub trait KitModule {
    /// The kit id (`world.stages`).
    fn id(&self) -> &'static str;
    /// The capabilities this module serves to others (`ctx.services`), registered after `setup`.
    fn services(&self) -> &'static [&'static str] {
        &[]
    }
    /// The capabilities served under this config (`services: cond ? {…} : {…}` of a web runtime whose config turns
    /// a service on); `services()` by default.
    fn services_for(&self, config: &Json) -> &'static [&'static str] {
        self.services()
    }
    /// What the module cannot run in this project (export-time compatibility).
    fn validate(&self, project: &Json, config: &Json) -> Vec<Issue> {
        Vec::new()
    }
    fn setup(&self, kx: &mut Kx);
    fn ready(&self, kx: &mut Kx) {}
    fn update(&self, kx: &mut Kx, dt: f64) {}
    fn late_update(&self, kx: &mut Kx, dt: f64) {}
    /// `updateIdle(dt)`: upkeep while the simulation is held or paused (`Host::step_idle`). Never advances
    /// gameplay.
    fn update_idle(&self, kx: &mut Kx, dt: f64) {}
    fn teardown(&self, kx: &mut Kx) {}
    /// A bus subscription made with `Kx::on(pattern, tag)`.
    fn on_event(&self, kx: &mut Kx, tag: &str, ev: &GameEvent) {}
    /// A timer made with `Kx::after` / `Kx::every`.
    fn on_timer(&self, kx: &mut Kx, tag: &str, data: &Json) {}
    /// A trigger action registered with `Kx::register_action`.
    fn action(&self, kx: &mut Kx, verb: &str, args: &Json, ev: Option<&GameEvent>) {}
    /// A trigger condition registered with `Kx::register_condition`.
    fn condition(&self, kx: &mut Kx, verb: &str, args: &Json, ev: Option<&GameEvent>) -> bool {
        false
    }
    /// A spawn factory registered with `Kx::register_factory`: returns the new entity id.
    fn spawn(&self, kx: &mut Kx, spec: &Json) -> Option<String> {
        None
    }
    /// A service method (`services.get(capability)[method](...args)`).
    fn call(
        &self,
        kx: &mut Kx,
        capability: &str,
        method: &str,
        args: &[Json],
    ) -> Result<Json, String> {
        Err(format!(
            "{capability}.{method} is not served by {}",
            self.id()
        ))
    }
    /// A service method called each frame with typed arguments (docs/kit-modules.md §7.13): the provider reads
    /// `args` where the caller holds them and answers a scalar. `None`: this method is not typed — the façade calls
    /// [`KitModule::call`] with the JSON values of `args`, as before. A provider that types a method answers
    /// exactly what `call` answers for it (`Ret::to_json` of the answer is `call`'s value: the parity checks it).
    fn call_typed(
        &self,
        kx: &mut Kx,
        capability: &str,
        method: &str,
        args: &[crate::typed::Arg],
    ) -> Option<Result<crate::typed::Ret, String>> {
        None
    }
    /// The kit's save section (`None`: nothing saved).
    fn save(&self, kx: &mut Kx) -> Option<Json> {
        None
    }
    fn load(&self, kx: &mut Kx, data: &Json) {}
    /// game.engine: a scene is entered, after its instances spawned (`scene` is the `SceneView` of the contract).
    /// A kit without this hook is global.
    fn scene_enter(&self, kx: &mut Kx, scene: &Json) {}
    /// game.engine: a scene is left, before the engine destroys its instances (the view carries `next`).
    fn scene_exit(&self, kx: &mut Kx, scene: &Json) {}
    /// An archetype component claimed with `game.engine.content.registerComponent(name, kit)`: builds what it means
    /// on the entity of a freshly spawned instance (`instance` is its `InstanceView`).
    fn component(&self, kx: &mut Kx, name: &str, entity: &str, value: &Json, instance: &Json) {}
    /// A state path bound with `Kx::bind_var(path, tag)` changed (`state.bind`): `None` is `undefined`.
    fn on_var(
        &self,
        kx: &mut Kx,
        tag: &str,
        path: &str,
        value: Option<&Json>,
        prev: Option<&Json>,
    ) {
    }
    /// A function this kit handed to another kit (`Kx::callback(name)`), called back with `Kx::invoke`.
    fn callback(&self, kx: &mut Kx, name: &str, args: &[Json]) -> Result<Json, String> {
        Err(format!("{} has no callback {name}", self.id()))
    }
    /// `game.engine.contract@1` (kit module ABI 1.2, docs/modules-de-kit.md): whether this module exports the contract the host
    /// asks of `game.engine` without knowing it. Only the `game.engine` module answers true; the three functions below
    /// are read only then.
    fn engine_contract(&self) -> bool {
        false
    }
    /// game.engine 1.2 and 1.3: the project with its mods applied, then the inheritance of the registries (`extends`)
    /// resolved. Asked once, at `Host::mount`, before anything reads the project.
    fn engine_prepare_project(&self, project: &Json) -> Json {
        project.clone()
    }
    /// The `kit.api.json` of the kits of the lock, by kit id (`HostOptions::kit_apis`): given once, at `Host::mount`,
    /// before `engine_prepare_project` (what game.engine reads of the other kits: verbs, hooks, moddable keys…).
    fn engine_kit_apis(&self, apis: &Json) {}
    /// The deferred operations of a mod on a data collection (`Host::default_data`).
    fn engine_collection_ops(&self, list: &mut Vec<Json>, ops: &Json) {}
    /// Whether no tap listens to `hook` now (`game.engine.hooks`): `Host::decide` then answers without the full path.
    /// False when it cannot tell.
    fn engine_untapped(&self, kx: &mut Kx, hook: &str) -> bool {
        false
    }
}
