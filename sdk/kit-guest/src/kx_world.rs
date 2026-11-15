//! The methods of `Kx` and `Host` on the world, state, bus, timers, triggers and services
//! (owner: world agent): the same names and signatures as kits/game_engine/host/src/host.rs (and the `impl Host`
//! of its triggers.rs / objectives.rs), each one call to the core (`kithost.world@1`, `kithost.state@1`,
//! `kithost.services@1`) or a local answer.
//!
//! A call that may run another kit (a service, a guard, a binding, a factory, an action…) drops what the façades
//! read before (`Host::reads_stale`): the world may have changed meanwhile.

use crate::bus::GameEvent;
use crate::host::{call_core, Host, Kx};
use crate::js::to_js_string;
use crate::state::call;
use gripforge_abi::{services as fs, state as fst, world as fw, Reader, Writer};
use serde_json::{json, Value as Json};
use std::cell::RefCell;

pub const SAVE_SCHEMA: &str = "gripforge.save/1";

/// `host.lock_hash` (a field of the core's `Host`): asked of the core on `clone()`.
#[derive(Default)]
pub struct LockHash {}

impl LockHash {
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> String {
        Reader::new(&call(crate::func!(fs::LOCK_HASH), |_| {}))
            .string()
            .unwrap_or_default()
    }
}

fn opt_event(w: &mut Writer, ev: Option<&GameEvent>) {
    match ev {
        Some(e) => w.value(&e.to_json()),
        None => w.value(&Json::Null),
    };
}

fn value_of(bytes: &[u8]) -> Json {
    Reader::new(bytes).value().unwrap_or(Json::Null)
}

fn opt_string(bytes: &[u8]) -> Option<String> {
    Reader::new(bytes)
        .opt_str()
        .ok()
        .flatten()
        .map(str::to_string)
}

/// A call whose `failed` is the service's `Err` (`call_service`, `invoke`, `load`…).
fn call_result(func: crate::uses::Func, args: impl FnOnce(&mut Writer)) -> Result<Vec<u8>, String> {
    let mut w = crate::host::writer();
    args(&mut w);
    call_core(func, &w).map_err(|(_, msg)| msg)
}

thread_local! {
    /// `runtimes()` for a generation of the served capabilities (the `&'static str` are leaked once per generation).
    #[allow(clippy::type_complexity, reason = "lint of a newer clippy than the source; copied as is (docs/sources.md)")]
    static RUNTIMES: RefCell<(u32, Vec<(String, Vec<&'static str>)>)> = const { RefCell::new((0, Vec::new())) };
}

impl Host {
    // ─── services ───────────────────────────────────────────────────────────

    /// Moves when a service is added or removed, or when the configuration is reloaded (the core's
    /// `Host::services_epoch`): what a kit that caches the answer of `has_service` keys it by.
    pub fn services_epoch(&self) -> (u32, u32) {
        (self.config_gen, self.ctx.services_gen)
    }

    /// Asked once per capability and generation of the served capabilities (`ctx.services_gen`).
    pub fn has_service(&self, capability: &str) -> bool {
        if capability == "runtime.core" {
            return true;
        }
        let gen = self.ctx.services_gen;
        {
            let mut c = self.services_cache.borrow_mut();
            if c.0 != gen {
                *c = (gen, Vec::new());
            }
            if let Some((_, b)) = c.1.iter().find(|(k, _)| k == capability) {
                return *b;
            }
        }
        let b = crate::direct::has_service(capability).unwrap_or_else(|| {
            call(crate::func!(fs::HAS_SERVICE), |w| {
                w.str(capability);
            })
            .first()
                == Some(&1)
        });
        self.services_cache
            .borrow_mut()
            .1
            .push((capability.to_string(), b));
        b
    }

    /// game.engine 1.3 `ctx.services.installed(capability)`.
    pub fn installed_capability(&self, capability: &str) -> bool {
        if self.has_service(capability) {
            return true;
        }
        call(crate::func!(fs::INSTALLED), |w| {
            w.str(capability);
        })
        .first()
            == Some(&1)
    }

    /// Calls a service method of a mounted kit.
    pub fn call_service(
        &mut self,
        capability: &str,
        method: &str,
        args: &[Json],
    ) -> Result<Json, String> {
        if let Some(answer) = self.untapped_json(capability, method, args) {
            return Ok(answer);
        }
        // What the façades read is dropped AFTER the call (the world may have changed); nothing to drop before:
        // no reference handed out lives across a `&mut self`, and a hook of this kit entered meanwhile drops on entry.
        // A capability another kit module serves, when this kit is linked to it: a call of that module, wasm to
        // wasm (crate::svc); else the core carries the call.
        if let Some(out) = crate::svc::call(capability, method, args) {
            self.reads_stale();
            return out;
        }
        let out = call_result(crate::func!(fs::CALL), |w| {
            w.str(capability).str(method).values(args);
        });
        self.reads_stale();
        Ok(value_of(&out?))
    }

    /// [`Host::call_service`] for a caller that reads the answer where it is (`Kx::call_with`): the answer of a
    /// kit module of the common memory is read in the block it was given in; any other answer as its value.
    pub fn call_service_with<R>(
        &mut self,
        capability: &str,
        method: &str,
        args: &[Json],
        f: impl FnOnce(crate::val::Val) -> R,
    ) -> Result<R, String> {
        if let Some(answer) = self.untapped_json(capability, method, args) {
            return Ok(f(crate::val::Val::Json(&answer)));
        }
        if let Some(out) = crate::svc::call_seen(capability, method, args) {
            self.reads_stale();
            return out.map(|answer| f(answer.val()));
        }
        let out = call_result(crate::func!(fs::CALL), |w| {
            w.str(capability).str(method).values(args);
        });
        self.reads_stale();
        let v = value_of(&out?);
        Ok(f(crate::val::Val::Json(&v)))
    }

    /// [`Host::call_service_with`] with typed arguments (docs/kit-modules.md §7.13): to a kit module of the common
    /// memory linked to this one, scalar arguments are lent where the caller holds them — no value built; else
    /// their JSON values, as before.
    pub fn call_service_args_with<R>(
        &mut self,
        capability: &str,
        method: &str,
        args: &[crate::typed::Arg],
        f: impl FnOnce(crate::val::Val) -> R,
    ) -> Result<R, String> {
        if let Some(answer) = self.untapped_args(capability, method, args) {
            return Ok(f(crate::val::Val::Json(&answer)));
        }
        if let Some(out) = crate::svc::call_seen_args(capability, method, args) {
            self.reads_stale();
            return out.map(|answer| f(answer.val()));
        }
        self.call_service_with(capability, method, &crate::typed::args_json(args), f)
    }

    /// `runtime.core.defaultData(name, docs)`.
    pub fn default_data(&mut self, name: &str, docs: &Json) {
        call(crate::func!(fs::DEFAULT_DATA), |w| {
            w.str(name).value(docs);
        });
        // The core may have written the defaults, with the mods' operations, into the project's data (game.engine
        // 1.3): the module's copy of the project follows (`kx.data_collection` reads it). Once per collection.
        let bytes = call(crate::func!(fs::PROJECT), |_| {});
        if let Ok(p) = crate::project::Project::read(&mut gripforge_abi::Reader::new(&bytes)) {
            self.project = p;
        }
    }

    /// `runtime.core.data(name)`.
    pub fn data(&self, name: &str) -> Json {
        value_of(&call(crate::func!(fs::DATA), |w| {
            w.str(name);
        }))
    }

    pub fn set_guarded(&mut self, path: &str, value: Json) {
        self.reads_stale();
        call(crate::func!(fst::SET_GUARDED), |w| {
            w.str(path).value(&value);
        });
        self.reads_stale();
    }

    pub fn inc_guarded(&mut self, path: &str, delta: f64) -> f64 {
        self.reads_stale();
        let bytes = call(crate::func!(fst::INC_GUARDED), |w| {
            w.str(path).f64(delta);
        });
        self.reads_stale();
        Reader::new(&bytes).f64().unwrap_or(0.0)
    }

    /// 1.3 `spawn.try(spec)` → `{ ok, id, cancelled, by, reason }`.
    pub fn spawn_try(&mut self, spec: &Json) -> Json {
        self.reads_stale();
        let v = value_of(&call(crate::func!(fw::SPAWN_TRY), |w| {
            w.value(spec);
        }));
        self.reads_stale();
        v
    }

    /// Calls back a function a kit handed over (`{ "$callback": … }`).
    pub fn invoke(&mut self, callback: &Json, args: &[Json]) -> Result<Json, String> {
        self.reads_stale();
        let out = call_result(crate::func!(fs::INVOKE), |w| {
            w.value(callback).values(args);
        });
        self.reads_stale();
        Ok(value_of(&out?))
    }

    /// `runtime.core.runtimes()`: the mounted kits in mount order, each with the capabilities it serves.
    pub fn runtimes(&self) -> Vec<(String, Vec<&'static str>)> {
        let gen = self.ctx.services_gen;
        if let Some(list) = RUNTIMES.with(|r| (r.borrow().0 == gen).then(|| r.borrow().1.clone())) {
            return list;
        }
        let v = value_of(&call(crate::func!(fs::RUNTIMES), |_| {}));
        let list: Vec<(String, Vec<&'static str>)> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let caps = e[1]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Json::as_str)
                    .map(|s| &*Box::leak(s.to_string().into_boxed_str()))
                    .collect();
                (to_js_string(e.get(0)), caps)
            })
            .collect();
        RUNTIMES.with(|r| *r.borrow_mut() = (gen, list.clone()));
        list
    }

    pub fn kit_ids(&self) -> Vec<String> {
        Reader::new(&call(crate::func!(fs::KIT_IDS), |_| {}))
            .strs()
            .unwrap_or_default()
    }

    /// `gripforge.save/1` of the whole host (every kit's `save`, this one's too).
    pub fn save(&mut self) -> Json {
        self.reads_stale();
        let v = value_of(&call(crate::func!(fs::SAVE), |_| {}));
        self.reads_stale();
        v
    }

    pub fn load(&mut self, save: &Json) -> Result<(), String> {
        self.reads_stale();
        let out = call_result(crate::func!(fs::LOAD), |w| {
            w.value(save);
        });
        self.reads_stale();
        out.map(|_| ())
    }

    /// `step` calls so far (`time.frame`).
    pub fn frames(&self) -> u64 {
        self.ctx.frame
    }

    pub fn project_id(&self) -> String {
        to_js_string(self.project.get("id"))
    }

    // ─── bus ────────────────────────────────────────────────────────────────

    pub fn emit_from(&mut self, topic: &str, payload: Json, source: &str, entity: Option<&str>) {
        call(crate::func!(fs::EMIT), |w| {
            // Typed: the payload as a block every module reads in place (docs/kit-modules.md §10.17).
            w.str(topic)
                .value_crossing(&payload)
                .str(source)
                .opt_str(entity);
        });
    }

    /// [`Host::emit_from`] of a payload given as typed fields (docs/kit-modules.md §7.13): the payload's block is
    /// written from the fields where the kit holds them — no JSON value made.
    pub fn emit_fields_from(
        &mut self,
        topic: &str,
        fields: &[(&str, crate::typed::Arg)],
        source: &str,
        entity: Option<&str>,
    ) {
        call(crate::func!(fs::EMIT), |w| {
            w.str(topic)
                .value_crossing_src(&crate::typed::Arg::Dict(fields), || {
                    crate::typed::dict_json(fields)
                })
                .str(source)
                .opt_str(entity);
        });
    }

    /// Whether an event of `topic` is heard ([`crate::events::has_audience`]).
    pub fn has_audience(&self, topic: &str) -> bool {
        crate::events::has_audience(topic)
    }

    /// `host.emit(topic, payload)`: from the runtime.
    pub fn emit(&mut self, topic: &str, payload: Json) {
        self.emit_from(topic, payload, "runtime", None);
    }

    /// The innermost player scope that is still a player.
    pub fn acting_player(&self) -> Option<String> {
        crate::direct::acting_player()
            .unwrap_or_else(|| opt_string(&call(crate::func!(fw::ACTING_PLAYER), |_| {})))
    }

    /// Tells the kits the changes of the state paths they bound.
    pub fn drain_binds(&mut self) {
        self.reads_stale();
        call(crate::func!(fst::DRAIN_BINDS), |_| {});
        self.reads_stale();
    }

    // ─── entities & spawn ───────────────────────────────────────────────────

    pub fn create_entity(
        &mut self,
        kind: &str,
        tags: &[String],
        components: Option<&Json>,
        owner: &str,
    ) -> String {
        self.create_entity_with(kind, tags, components, owner, None)
    }

    pub fn create_entity_with(
        &mut self,
        kind: &str,
        tags: &[String],
        components: Option<&Json>,
        owner: &str,
        refs: Option<&Json>,
    ) -> String {
        self.entities.enter();
        let bytes = call(crate::func!(fw::CREATE), |w| {
            w.str(kind)
                .strs(tags)
                .opt_value(components)
                .str(owner)
                .opt_value(refs);
        });
        Reader::new(&bytes).string().unwrap_or_default()
    }

    pub fn destroy_entity(&mut self, id: &str) {
        self.entities.enter();
        call(crate::func!(fw::DESTROY), |w| {
            w.str(id);
        });
    }

    /// `entities.restore(snapshot)`: false when its id is taken.
    pub fn restore_entity(&mut self, s: &Json) -> bool {
        self.entities.enter();
        call(crate::func!(fw::RESTORE), |w| {
            w.value(s);
        })
        .first()
            == Some(&1)
    }

    /// `spawn.resolvePosition(at)`.
    pub fn resolve_position(&mut self, at: Option<&Json>) -> [f64; 3] {
        let bytes = call(crate::func!(fw::RESOLVE_POSITION), |w| {
            w.opt_value(at);
        });
        let mut r = Reader::new(&bytes);
        let mut f = || r.f64().unwrap_or(0.0);
        [f(), f(), f()]
    }

    pub fn spawn_kinds(&self) -> Vec<String> {
        Reader::new(&call(crate::func!(fw::SPAWN_KINDS), |_| {}))
            .strs()
            .unwrap_or_default()
    }

    /// `spawn.spawn(spec)`: the kit factory of the kind (another kit may run), else a bare entity.
    pub fn spawn(&mut self, spec: &Json) -> String {
        self.reads_stale();
        let bytes = call(crate::func!(fw::SPAWN), |w| {
            w.value(spec);
        });
        self.reads_stale();
        Reader::new(&bytes).string().unwrap_or_default()
    }

    pub fn despawn_by_tag(&mut self, tag: &str) -> usize {
        self.entities.enter();
        let bytes = call(crate::func!(fw::DESPAWN_BY_TAG), |w| {
            w.str(tag);
        });
        Reader::new(&bytes).u32().unwrap_or(0) as usize
    }

    /// `resolveEntityRef`: `player` (or nothing), an entity id, or `{ tag }`.
    pub fn resolve_entity_ref(&self, r: Option<&Json>) -> Option<String> {
        opt_string(&call(crate::func!(fw::RESOLVE_ENTITY_REF), |w| {
            w.opt_value(r);
        }))
    }

    // ─── triggers ────────────────────────────────────────────────

    pub fn evaluate(&mut self, cond: &Json, ev: Option<&GameEvent>) -> bool {
        self.reads_stale();
        let b = call(crate::func!(fs::EVALUATE), |w| {
            w.value(cond);
            opt_event(w, ev);
        })
        .first()
            == Some(&1);
        self.reads_stale();
        b
    }

    pub fn run_actions(&mut self, actions: &[Json], ev: Option<&GameEvent>) {
        self.reads_stale();
        call(crate::func!(fs::RUN_ACTIONS), |w| {
            w.values(actions);
            opt_event(w, ev);
        });
        self.reads_stale();
    }
}

impl Kx<'_> {
    /// game.engine 1.3 `ctx.services.installed(capability)`.
    pub fn installed(&self, capability: &str) -> bool {
        self.host.installed_capability(capability)
    }

    /// Emits with this kit as the source.
    pub fn emit(&mut self, topic: &str, payload: Json) {
        let id = self.kit_id();
        self.host.emit_from(topic, payload, &id, None);
    }
    pub fn emit_entity(&mut self, topic: &str, payload: Json, entity: &str) {
        let id = self.kit_id();
        self.host.emit_from(topic, payload, &id, Some(entity));
    }

    /// Subscribes `on_event(tag)` to a topic pattern; returns the subscription id (for `Host::bus.off`).
    pub fn on(&mut self, pattern: &str, tag: &str) -> u64 {
        self.on_with(pattern, tag, 0.0, false)
    }
    pub fn on_with(&mut self, pattern: &str, tag: &str, priority: f64, once: bool) -> u64 {
        let id = self.kit_id();
        let sub = crate::bus::Subscriber::Kit {
            kit: self.kit,
            tag: tag.to_string(),
        };
        self.host.bus.on(pattern, sub, priority, once, Some(&id))
    }

    fn timer(&mut self, func: crate::uses::Func, seconds: f64, tag: &str, data: Json) -> u64 {
        let bytes = call(func, |w| {
            w.f64(seconds).str(tag).value(&data);
        });
        Reader::new(&bytes).u64().unwrap_or(0)
    }
    pub fn after(&mut self, seconds: f64, tag: &str, data: Json) -> u64 {
        self.timer(crate::func!(fs::AFTER), seconds, tag, data)
    }
    pub fn every(&mut self, seconds: f64, tag: &str, data: Json) -> u64 {
        self.timer(crate::func!(fs::EVERY), seconds, tag, data)
    }

    pub fn define_var(
        &mut self,
        path: &str,
        kind: &str,
        label: &str,
        format: Option<&str>,
        default: Option<Json>,
    ) {
        self.host.state.wrote();
        call(crate::func!(fst::DEFINE), |w| {
            w.str(path)
                .str(kind)
                .str(label)
                .opt_str(format)
                .opt_value(default.as_ref());
        });
    }
    /// Every game variable (`ctx.state.snapshot()`).
    pub fn state_snapshot(&self) -> Json {
        self.host.state.snapshot()
    }
    pub fn get_var(&self, path: &str) -> Option<&Json> {
        self.host.state.get(path)
    }
    /// `ctx.state.set`: a guarded path goes through its guard (another kit may run), then the bindings are told.
    pub fn set_var(&mut self, path: &str, value: Json) {
        // A number, a boolean or null: a direct link (the same `set_var` in the core); else the bridge.
        if !crate::direct::set_var(path, &value) {
            call(crate::func!(fst::SET_VAR), |w| {
                w.str(path).value(&value);
            });
        }
        self.host.reads_stale();
    }
    pub fn set_num(&mut self, path: &str, value: f64) {
        self.set_var(path, crate::js::num(value));
    }
    /// 1.3 `ctx.state.guard(path, fn)`.
    pub fn guard_var(&mut self, path: &str, callback: Json) {
        call(crate::func!(fst::GUARD), |w| {
            w.str(path).value(&callback);
        });
    }
    /// 1.3 `ctx.spawn.try(spec)`.
    pub fn spawn_try(&mut self, spec: &Json) -> Json {
        self.host.spawn_try(spec)
    }
    /// 1.3 `decide(ctx.services, hook, payload)`.
    pub fn decide(&mut self, hook: &str, payload: Json) -> Json {
        self.host.decide(hook, payload)
    }
    /// `ctx.state.bind(path, fn)`: `KitModule::on_var(tag, …)` is told every change of the path.
    pub fn bind_var(&mut self, path: &str, tag: &str) {
        call(crate::func!(fst::BIND), |w| {
            w.str(path).str(tag);
        });
    }
    pub fn runtimes(&self) -> Vec<(String, Vec<&'static str>)> {
        self.host.runtimes()
    }

    pub fn register_action(&mut self, verb: &str) {
        let kit = self.kit;
        self.host.triggers.register_action(verb, kit);
    }
    pub fn register_condition(&mut self, verb: &str) {
        let kit = self.kit;
        self.host.triggers.register_condition(verb, kit);
    }
    pub fn register_factory(&mut self, kind: &str) {
        call(crate::func!(fw::REGISTER_FACTORY), |w| {
            w.str(kind);
        });
    }
    pub fn spawn_kinds(&self) -> Vec<String> {
        self.host.spawn_kinds()
    }
    /// Adds a trigger rule owned by this kit.
    pub fn add_rule(&mut self, rule: &Json) -> String {
        let bytes = call(crate::func!(fs::ADD_RULE), |w| {
            w.value(rule);
        });
        Reader::new(&bytes).string().unwrap_or_default()
    }
    pub fn run_actions(&mut self, actions: &[Json], ev: Option<&GameEvent>) {
        self.host.run_actions(actions, ev);
    }
    pub fn evaluate(&mut self, cond: &Json, ev: Option<&GameEvent>) -> bool {
        self.host.evaluate(cond, ev)
    }
    pub fn create_entity(
        &mut self,
        kind: &str,
        tags: &[String],
        components: Option<&Json>,
    ) -> String {
        let id = self.kit_id();
        self.host.create_entity(kind, tags, components, &id)
    }
    /// Whether the kit that serves `capability` here publishes every column of `columns` (the core's
    /// `Kx::published`, docs/kit-modules.md §10.13): asked of the provider (its method `published`) once per
    /// capability and generation of the served capabilities.
    pub fn published(&mut self, capability: &str, columns: &[&str]) -> bool {
        if !self.host.has_service(capability) {
            return false;
        }
        let gen = self.host.ctx.services_gen;
        let known = {
            let mut c = self.host.published_cache.borrow_mut();
            if c.0 != gen {
                *c = (gen, Vec::new());
            }
            c.1.iter()
                .find(|(k, _)| k == capability)
                .map(|(_, l)| columns.iter().all(|n| l.iter().any(|h| h == n)))
        };
        if let Some(b) = known {
            return b;
        }
        let list: Vec<String> = match self.host.call_service(capability, "published", &[]) {
            Ok(Json::Array(a)) => a
                .into_iter()
                .filter_map(|v| {
                    if let Json::String(s) = v {
                        Some(s)
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let b = columns.iter().all(|n| list.iter().any(|h| h == n));
        self.host
            .published_cache
            .borrow_mut()
            .1
            .push((capability.to_string(), list));
        b
    }
    pub fn call(&mut self, capability: &str, method: &str, args: &[Json]) -> Result<Json, String> {
        self.host.call_service(capability, method, args)
    }
    /// [`Kx::call`] for a caller that only reads the answer: `f` reads it where it is (`Val`) and keeps what it
    /// copies — the answer of another kit module of the common memory is not rebuilt (src/val.rs).
    pub fn call_with<R>(
        &mut self,
        capability: &str,
        method: &str,
        args: &[Json],
        f: impl FnOnce(crate::val::Val) -> R,
    ) -> Result<R, String> {
        self.host.call_service_with(capability, method, args, f)
    }

    // ─── animation ──────────────────────────────────────────────────────────

    /// `animationOverride(ctx, id)`: the timed action that owns the pose of the entity, while it lasts.
    pub fn anim_override(&self, entity: &str) -> Option<Json> {
        Reader::new(&call(crate::func!(fw::ANIM_OVERRIDE), |w| {
            w.str(entity);
        }))
        .opt_value()
        .ok()
        .flatten()
    }
    /// `setEntityAnimation(ctx, id, anim)`.
    pub fn set_anim(&mut self, entity: &str, clip: &str) {
        self.host.entities.enter();
        if crate::direct::set_anim(entity, clip) {
            return;
        }
        call(crate::func!(fw::SET_ANIM), |w| {
            w.str(entity).str(clip);
        });
    }
    /// `playEntityAnimation(ctx, id, anim, duration)`.
    pub fn play_anim_once(&mut self, entity: &str, clip: &str, duration: f64) {
        self.host.entities.enter();
        call(crate::func!(fw::PLAY_ANIM_ONCE), |w| {
            w.str(entity).str(clip).f64(duration);
        });
    }
    /// `playEntityAnimation(ctx, id, anim, duration, layer)` (`kithost.world@1.6`).
    pub fn play_anim_once_layered(
        &mut self,
        entity: &str,
        clip: &str,
        duration: f64,
        layer: Option<&Json>,
    ) {
        self.host.entities.enter();
        call(crate::func!(fw::PLAY_ANIM_LAYERED), |w| {
            w.str(entity).str(clip).f64(duration).opt_value(layer);
        });
    }
    /// `actionMovement(ctx, id)`: (movement, scale) of the running action.
    pub fn action_movement(&self, entity: &str) -> (String, f64) {
        let bytes = call(crate::func!(fw::ACTION_MOVEMENT), |w| {
            w.str(entity);
        });
        let mut r = Reader::new(&bytes);
        match (r.string(), r.f64()) {
            (Ok(m), Ok(s)) => (m, s),
            _ => ("free".into(), 1.0),
        }
    }
    /// `setEntityActionLayers(ctx, id, table)`.
    pub fn set_action_layers(&mut self, entity: &str, table: Option<&Json>) {
        self.host.entities.enter();
        call(crate::func!(fw::SET_ACTION_LAYERS), |w| {
            w.str(entity).opt_value(table);
        });
    }
    /// What the layer of an action does to the movement of an entity (`free`, `slow`, `lock`, `root`).
    pub fn action_layer_movement(&self, entity: &str, clip: &str, layer: Option<&Json>) -> String {
        let bytes = call(crate::func!(fw::ACTION_LAYER_MOVEMENT), |w| {
            w.str(entity).str(clip).opt_value(layer);
        });
        Reader::new(&bytes)
            .string()
            .unwrap_or_else(|_| "free".into())
    }
    /// `cancelEntityAnimation(ctx, id, expected)`.
    pub fn cancel_anim(&mut self, entity: &str, expected: &str) {
        self.host.entities.enter();
        call(crate::func!(fw::CANCEL_ANIM), |w| {
            w.str(entity).str(expected);
        });
    }

    /// A function of this kit, as data another kit can keep and call (`Kx::invoke`).
    pub fn callback(&self, name: &str) -> Json {
        json!({ "$callback": { "kit": self.kit_id(), "name": name } })
    }
    /// Calls a function another kit handed over with `callback`.
    pub fn invoke(&mut self, callback: &Json, args: &[Json]) -> Result<Json, String> {
        self.host.invoke(callback, args)
    }
    /// Calls the provider this kit replaced for a capability.
    pub fn call_previous(
        &mut self,
        capability: &str,
        method: &str,
        args: &[Json],
    ) -> Result<Json, String> {
        self.host.reads_stale();
        let out = call_result(crate::func!(fs::CALL_PREVIOUS), |w| {
            w.str(capability).str(method).values(args);
        });
        self.host.reads_stale();
        Ok(value_of(&out?))
    }

    /// `runtime.core.defer`: queued until the tick ends when a tick runs, else now (this kit's `on_timer` may run).
    pub fn defer(&mut self, tag: &str, data: Json) {
        self.host.reads_stale();
        call(crate::func!(fs::DEFER), |w| {
            w.str(tag).value(&data);
        });
        self.host.reads_stale();
    }
}

/// The typed values of the façade fill fixed-layout blocks directly (gripforge-kitcall `fixed::Source`).
impl gripforge_abi::fixed::Source for crate::typed::Arg<'_> {
    fn node(&self) -> gripforge_abi::fixed::Node<'_, Self> {
        use crate::typed::Arg;
        use gripforge_abi::fixed::Node;
        match *self {
            Arg::Null => Node::Null,
            Arg::Bool(b) => Node::Bool(b),
            Arg::Num(n) => Node::Num(n),
            Arg::F64(n) => Node::Float(n),
            Arg::Int(i) => Node::Int(i),
            Arg::Str(s) => Node::Text(s),
            Arg::Vec3(v) => Node::Vec3(v),
            Arg::List(l) => Node::List(l),
            Arg::Dict(d) => Node::Dict(d),
            Arg::Value(v) => Node::Json(v),
        }
    }
}
