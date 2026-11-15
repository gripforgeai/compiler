//! The façade of the kit host a kit module sees (`Host`, `Kx`): the same fields and methods as
//! kits/game_engine/host/src/host.rs, each a call to the core module (`kithost.*@1`) or a local value.
//!
//! Local to the module (no call): the kit's id, config (read again when the core says it changed), the project, its
//! rng (forked by the core at mount, then the module's), its state (`put` / `data`), and the clock of the hook that
//! runs (tick, frame, elapsed, fixed dt: given by the core on every entry, `Ctx`).
//!
//! The hooks of the kit (`kithost.kit@1`) are run by [`entry_peer`]; [`export_kit!`] exports them.

pub use crate::kit::{Issue, KitModule};
pub use crate::kx_world::SAVE_SCHEMA;
use crate::rng::Rng;
pub use crate::val::Val;
#[cfg(all(feature = "common-memory", target_arch = "wasm32"))]
use gripforge_abi::calls::guest;
use gripforge_abi::{self as kc, kit, Ctx, Reader, Writer};
use serde_json::{json, Value as Json};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

/// What the module keeps of its own (the `KitSlot` of the core, but here).
pub struct KitSlot {
    pub id: String,
    pub config: Json,
    pub rng: Rng,
    pub data: Option<Box<dyn Any>>,
}

/// `host.kits[i]`: a kit module sees only its own slot, whatever the index (`kx.host.kits[kx.kit]`).
pub struct Kits {
    pub slot: KitSlot,
}

impl std::ops::Index<usize> for Kits {
    type Output = KitSlot;
    fn index(&self, _: usize) -> &KitSlot {
        &self.slot
    }
}
impl std::ops::IndexMut<usize> for Kits {
    fn index_mut(&mut self, _: usize) -> &mut KitSlot {
        &mut self.slot
    }
}

/// The façade of `Host`. Its fields are façades too (each call reaches the core), named and typed as the core's.
pub struct Host {
    pub entities: crate::entities::Entities,
    pub state: crate::state::GameState,
    pub bus: crate::bus::Bus,
    pub triggers: crate::triggers::Triggers,
    pub timers: crate::timers::Timers,
    pub player_scopes: crate::entities::PlayerScopes,
    pub physics: crate::ports::PhysicsProxy,
    pub input: crate::ports::InputPort,
    pub assets: crate::ports::AssetsProxy,
    pub stores: crate::ports::SaveStores,
    pub platform: crate::ports::Platform,
    pub audio: crate::ports::AudioProxy,
    pub display: crate::ports::DisplayProxy,
    pub pointer: crate::ports::PointerProxy,
    pub kits: Kits,
    /// The project as mounted (mods applied): sent once at mount.
    pub project: crate::project::Project,
    pub fixed_dt: f64,
    pub tick: u64,
    pub elapsed: f64,
    pub config_gen: u32,
    /// `host.lock_hash`: asked of the core on `clone()` (src/kx_world.rs).
    pub lock_hash: crate::kx_world::LockHash,
    /// The context of the hook that runs (given by the core on every entry).
    pub ctx: Ctx,
    /// Capability → served, valid for `ctx.services_gen` (`has_service` asks the core once per generation).
    pub services_cache: RefCell<(u32, Vec<(String, bool)>)>,
    /// Capability → the columns its provider publishes (`Kx::published`), valid for `ctx.services_gen`.
    #[allow(
        clippy::type_complexity,
        reason = "lint of a newer clippy than the source; copied as is (docs/sources.md)"
    )]
    pub published_cache: RefCell<(u32, Vec<(String, Vec<String>)>)>,
    /// What the per-tick hook that runs said with `Kx::rest` (told to the core when it returns).
    pub(crate) rest: Option<u32>,
    /// The first entry was made (the target and the language of the platform are read then).
    started: bool,
    /// `ports::keeps()` when this instance last dropped what its ports kept.
    port_keeps: u64,
}

/// What can wake a kit that rests (`Kx::rest`): the same as the kit host's (`gripforge-kithost` `host::wake`).
pub mod wake {
    pub const INPUT: u32 = gripforge_abi::fast::WAKE_INPUT;
    pub const STATE: u32 = gripforge_abi::fast::WAKE_STATE;
    pub const WORLD: u32 = gripforge_abi::fast::WAKE_WORLD;
    pub const STRUCTURE: u32 = gripforge_abi::fast::WAKE_STRUCTURE;
    pub const ALL: u32 = INPUT | STATE | WORLD;
}

impl Host {
    fn new(ctx: Ctx, id: &str, project: crate::project::Project, config: Json, rng: Rng) -> Host {
        // A mount: the zone may have been laid out again for this game (its schema): read its directory again.
        crate::zone::refresh();
        Host {
            entities: Default::default(),
            state: Default::default(),
            bus: Default::default(),
            triggers: Default::default(),
            timers: Default::default(),
            player_scopes: Default::default(),
            physics: Default::default(),
            input: Default::default(),
            assets: Default::default(),
            stores: Default::default(),
            platform: Default::default(),
            audio: Default::default(),
            display: Default::default(),
            pointer: Default::default(),
            kits: Kits {
                slot: KitSlot {
                    id: id.to_string(),
                    config,
                    rng,
                    data: None,
                },
            },
            project,
            fixed_dt: ctx.fixed_dt,
            tick: ctx.tick,
            elapsed: ctx.elapsed,
            config_gen: ctx.config_gen,
            lock_hash: Default::default(),
            ctx,
            services_cache: RefCell::new((0, Vec::new())),
            published_cache: RefCell::new((0, Vec::new())),
            rest: None,
            started: false,
            port_keeps: 0,
        }
    }

    /// A new entry of the core: the clock, and what the module cached that the core says changed.
    pub(crate) fn enter(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.tick = ctx.tick;
        self.elapsed = ctx.elapsed;
        self.fixed_dt = ctx.fixed_dt;
        if ctx.config_gen != self.config_gen {
            self.config_gen = ctx.config_gen;
            self.reread_project();
        }
        // What the façades read in an earlier entry may have changed (entities, state, objectives): the
        // moment moves, and each drops what it read when it is next read — nothing is walked on an entry.
        moved();
        // The ports: the copies they kept are dropped, when any kept one since this instance last dropped them.
        if !self.started {
            self.started = true;
            self.platform.enter();
        }
        let kept = crate::ports::keeps();
        if kept != self.port_keeps {
            self.port_keeps = kept;
            self.enter_ports();
        }
    }

    /// The config of this kit and the copy of the project, read again (a config generation moved, or this module wrote).
    pub(crate) fn reread_project(&mut self) {
        let bytes = call_core_ok(crate::func!(KIT_CONFIG), &Writer::new());
        self.kits.slot.config = Reader::new(&bytes).value().unwrap_or(json!({}));
        let bytes = call_core_ok(crate::func!(KIT_PROJECT), &Writer::new()); // game.engine edits installed[].config
        if let Ok(p) = crate::project::Project::read(&mut Reader::new(&bytes)) {
            self.project = p;
        }
    }

    pub fn log(&mut self, level: &str, msg: &str) {
        let mut w = writer();
        w.str(level).str(msg);
        call_core_ok(crate::func!(HOST_LOG), &w);
    }
}

/// `kithost.services@1` numbers this file uses (the rest: src/kx_world.rs).
pub const KIT_CONFIG: u32 = kc::SERVICES.base | 1;
pub const HOST_LOG: u32 = kc::SERVICES.base | 2;
pub const KIT_PROJECT: u32 = kc::SERVICES.base | 3;

/// What a kit module sees: the host façade and which kit slot it is (the `KitContext` of TypeScript).
pub struct Kx<'a> {
    pub host: &'a mut Host,
    pub kit: usize,
}

impl Kx<'_> {
    pub fn kit_id(&self) -> String {
        self.host.kits.slot.id.clone()
    }
    pub fn config(&self) -> &Json {
        &self.host.kits.slot.config
    }
    pub fn project(&self) -> &Json {
        &self.host.project
    }
    /// Read one top-level field without decoding the rest of the project.
    pub fn project_field(&self, name: &str) -> Option<&Json> {
        self.host.project.get(name)
    }
    /// `ctx.project.data[name]`.
    pub fn data_collection(&self, name: &str) -> Option<&Json> {
        self.host.project.collection(name)
    }
    pub fn rng(&mut self) -> &mut Rng {
        &mut self.host.kits.slot.rng
    }
    pub fn elapsed(&self) -> f64 {
        self.host.ctx.elapsed
    }
    pub fn tick(&self) -> u64 {
        self.host.ctx.tick
    }
    /// `time.frame`: the `step` calls of the host so far.
    pub fn frame(&self) -> u64 {
        self.host.ctx.frame
    }
    pub fn dt(&self) -> f64 {
        self.host.ctx.fixed_dt
    }
    /// The every-N scheduler (src/tick.rs, docs/kit-modules.md « Périodes »): the period of this kit's `system`, as
    /// the game tunes it (game.engine `tick.systems["<kit>.<system>"]`: a number of ticks, or `{ every, offset, hz }`),
    /// else `default`. Resolve it in `setup` (and again when `config_gen` moves), keep it, then each tick
    /// `due(kx.tick())` (the system runs this tick, its time is `dt(kx.dt())`) or `slice(kx.tick()).has(id)` (this
    /// entity's turn: 1/N of them each tick). Combat hit resolution and anything a player feels at once stays
    /// every tick.
    pub fn schedule(&self, system: &str, default: crate::tick::Every) -> crate::tick::Every {
        let kit = self.kit_id();
        crate::tick::resolve(self.project(), &kit, system, default, self.dt())
    }
    /// Keeps the module's state (in this module's memory).
    pub fn put<T: 'static>(&mut self, value: T) {
        self.host.kits.slot.data = Some(Box::new(value));
    }
    /// The module's state (`put` first).
    pub fn data<T: 'static>(&mut self) -> &mut T {
        self.host
            .kits
            .slot
            .data
            .as_mut()
            .and_then(|d| d.downcast_mut::<T>())
            .expect("kit state missing: put() it in setup")
    }
    /// The module's state, read only.
    pub fn host_state<T: 'static>(&self) -> &T {
        self.host
            .kits
            .slot
            .data
            .as_ref()
            .and_then(|d| d.downcast_ref::<T>())
            .expect("kit state missing: put() it in setup")
    }
    /// **This kit rests** (the same as `Kx::rest` of the kit host: see there). Said in a per-tick hook that finds
    /// nothing to do: the core does not run the kit's per-tick hooks again until the kit is entered by anything
    /// else or what `wake` names moves (`host::wake`). Every entry of the per-tick hooks tells the core.
    pub fn rest(&mut self, wake: u32) {
        self.host.rest = Some(wake);
    }
    /// **This kit rests with a clock** (the same as `Kx::rest_counting` of the kit host: see there): `counter` is a
    /// field of the state the kit `put`, moved by the core at each tick it rests as the `update` would have moved it,
    /// until it reaches `limit`. In the common memory only (the core moves the number where it is); elsewhere, or with
    /// a core before `kithost.services@1.2`, the kit does not rest and its `update` runs every tick, as before.
    pub fn rest_counting(&mut self, wake: u32, counter: *mut f64, limit: f64, round6: bool) {
        #[cfg(feature = "common-memory")]
        {
            thread_local! {
                /// The core refused the clock once (a core before 1.2): never asked again.
                static REFUSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
            }
            if !REFUSED.with(std::cell::Cell::get) {
                let mut w = writer();
                w.u64(counter as usize as u64).f64(limit).bool(round6);
                if call_core(crate::func!(kc::services::REST_COUNTING), &w).is_ok() {
                    self.host.rest = Some(wake);
                    return;
                }
                REFUSED.with(|r| r.set(true));
            }
        }
        let _ = (wake, counter, limit, round6);
    }
    pub fn log(&mut self, level: &str, msg: &str) {
        let id = self.kit_id();
        self.host.log(level, &format!("[{id}] {msg}"));
    }
}

// ---- calls to the core -------------------------------------------------------------------------------------------

thread_local! {
    static CALLS: RefCell<u64> = const { RefCell::new(0) };
    /// The moment of this module: it moves each time what its façades read may no longer hold — a new entry of the
    /// core (`Host::enter`), a call that may have run other kits (`Host::reads_stale`). A façade (entities, state,
    /// objectives) remembers the moment it last looked at and drops what it read before when it is read at
    /// another: an entry or a service call walks nothing, and a façade the hook does not read is not touched.
    static MOMENT: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// The moment now (see `MOMENT`).
#[inline]
pub(crate) fn moment() -> u64 {
    MOMENT.with(std::cell::Cell::get)
}

/// What the façades read may no longer hold from here on. Called where no reference they handed out can be alive
/// (`&mut Host`): what they free at their next read is referenced by nothing.
#[inline]
pub(crate) fn moved() {
    MOMENT.with(|m| m.set(m.get().wrapping_add(1)));
}

/// The writer of the arguments of a call to the core: in the common memory, lists and dictionaries go as
/// fixed-layout blocks (docs/kit-modules.md §9.7; the core reads them in place, once, and frees them).
///
/// An isolated kit module (feature `isolated`): they go as blocks that cross, inside the message — the same
/// structures, copied once by the host (crate::iso).
#[inline]
pub fn writer() -> Writer {
    Writer::to_peer(
        cfg!(feature = "common-memory"),
        cfg!(all(feature = "isolated", target_arch = "wasm32")),
    )
}

/// A call to the core: `Err((status, message))` when it did not answer `ok` (`failed` carries the core's message).
pub fn call_core(func: crate::uses::Func, args: &Writer) -> Result<Vec<u8>, (u16, String)> {
    CALLS.with(|c| *c.borrow_mut() += 1);
    // In the common memory: by pointer, straight to the core (crate::mem).
    #[cfg(feature = "common-memory")]
    return crate::mem::call(func.id(), args.bytes());
    // Isolated: across the two memories, the host copies the message once (crate::iso).
    #[cfg(all(
        feature = "isolated",
        not(feature = "common-memory"),
        target_arch = "wasm32"
    ))]
    return crate::iso::call(func.id(), args.bytes());
    // Off wasm (the façade checked or tested natively) there is no core to call.
    #[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
    {
        let _ = args;
        Err((gripforge_abi::calls::Status::Failed as u16, format!("kitcall: core function {:#x} called off wasm — a kit module runs in the common memory or isolated", func.id())))
    }
}

/// A call to the core that must answer (a refusal is a bug of the contract: the module stops with the reason).
pub fn call_core_ok(func: crate::uses::Func, args: &Writer) -> Vec<u8> {
    match call_core(func, args) {
        Ok(b) => b,
        Err((status, msg)) => panic!(
            "kitcall: core function {:#x} answered status {status}: {msg}",
            func.id()
        ),
    }
}

/// Calls made to the core by this module so far.
pub fn calls_made() -> u64 {
    CALLS.with(|c| *c.borrow())
}

// ---- the hooks: what the core calls ------------------------------------------------------------------------------

thread_local! {
    static MODULE: RefCell<Option<(Rc<dyn KitModule>, &'static str)>> = const { RefCell::new(None) };
    /// The same module by address (kept alive by `MODULE`): what the short entries call, no count touched.
    static MODULE_PTR: std::cell::Cell<Option<*const dyn KitModule>> = const { std::cell::Cell::new(None) };
    /// The instances of this kit: (host instance, kit slot) → its façade, never moved (boxed, raw).
    static INSTANCES: RefCell<Vec<(u32, u32, *mut Host)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn module() -> Rc<dyn KitModule> {
    MODULE
        .with(|m| m.borrow().as_ref().map(|(m, _)| m.clone()))
        .expect("kitcall: export_kit! not run")
}

thread_local! {
    /// The instance entered last: (host instance, kit slot) → its façade. A kit has one instance in a game, and the
    /// core enters it every tick: no search. Forgotten whenever the table of instances changes (MOUNT, DROP).
    static LAST: std::cell::Cell<(u32, u32, *mut Host)> = const { std::cell::Cell::new((0, 0, std::ptr::null_mut())) };
}

/// The kit of this module by address (kept alive by `MODULE` for the life of the wasm module): what the short
/// entries call, no count touched.
#[inline]
pub(crate) fn module_ptr() -> Option<*const dyn KitModule> {
    MODULE_PTR.with(std::cell::Cell::get)
}

pub(crate) fn instance(inst: u32, kit: u32) -> Option<*mut Host> {
    let (a, b, h) = LAST.with(std::cell::Cell::get);
    if !h.is_null() && a == inst && b == kit {
        return Some(h);
    }
    let found = INSTANCES.with(|i| {
        i.borrow()
            .iter()
            .find(|(a, b, _)| *a == inst && *b == kit)
            .map(|(_, _, h)| *h)
    })?;
    LAST.with(|l| l.set((inst, kit, found)));
    Some(found)
}

fn forget_last() {
    LAST.with(|l| l.set((0, 0, std::ptr::null_mut())));
}

/// The short entry of the per-tick hooks (`gripforge_abi::fast::Tick`, docs/kit-modules.md §9.11): the core
/// calls it through the function table with the context by address — what [`entry_peer`] does for `UPDATE`,
/// `LATE_UPDATE` and `UPDATE_IDLE`, with nothing decoded and no answer to build. Answers 0 when the hook ran;
/// 1 when nothing ran (no such instance, another function): the core then takes [`entry_peer`], which says why.
///
/// # Safety
/// `ctx` is readable for the call (the core's, in the common memory; any valid address in a test).
pub unsafe extern "C" fn tick(func: u32, ctx: *const kc::fast::HookCtx, dt: f64) -> u32 {
    #[cfg(feature = "entry-probe")]
    if func
        & (kc::fast::PROBE_ENTER
            | kc::fast::PROBE_LINK
            | kc::fast::PROBE_SVC
            | kc::fast::PROBE_ALLOC)
        != 0
    {
        return crate::probe::tick(func, ctx);
    }
    if !matches!(func, kit::UPDATE | kit::LATE_UPDATE | kit::UPDATE_IDLE) {
        return 1;
    }
    let ctx: Ctx = (*ctx).into();
    let Some(ptr) = instance(ctx.inst, ctx.kit) else {
        return 1;
    };
    let Some(m) = MODULE_PTR.with(std::cell::Cell::get) else {
        return 1;
    };
    // SAFETY: the module is kept by `MODULE` for the life of this wasm module; the instance: as `entry_peer`.
    let (m, host): (&dyn KitModule, &mut Host) = (&*m, &mut *ptr);
    host.enter(ctx);
    host.rest = None;
    let mut kx = Kx {
        host,
        kit: ctx.kit as usize,
    };
    match func {
        kit::UPDATE => m.update(&mut kx, dt),
        kit::LATE_UPDATE => m.late_update(&mut kx, dt),
        _ => m.update_idle(&mut kx, dt),
    }
    // The kit rests (`Kx::rest`): said to the core with the status, which then does not call these hooks.
    match kx.host.rest.take() {
        Some(wake) if func != kit::UPDATE_IDLE => kc::fast::RESTS | (wake & kc::fast::WAKE_MASK),
        _ => 0,
    }
}

/// The short entries of this kit module as the numbers `INFO` gives the core (`tick`, `svc`): the indices of
/// [`tick`] and of `svc::serve_scalar` in the function table. Only a kit module of the common memory has them —
/// its table is the core's; anywhere else a function of this module cannot be called by another one: 0.
fn fast_entries() -> (u32, u32) {
    #[cfg(all(feature = "common-memory", target_arch = "wasm32"))]
    {
        let tick: kc::fast::Tick = tick;
        let svc: kc::fast::Service = crate::svc::serve_scalar;
        (tick as usize as u32, svc as usize as u32)
    }
    #[cfg(not(all(feature = "common-memory", target_arch = "wasm32")))]
    (0, 0)
}

/// Runs one function of `kithost.kit@1` (the bytes of the call → the bytes of the answer, or `failed` + message):
/// `direct` when the caller is the core of the common memory entering by pointer (`gfx_mem_enter`) — the arguments
/// may carry fixed-layout blocks, and so does the answer.
pub fn entry_from(func: u32, args: &[u8], direct: bool) -> Result<Vec<u8>, String> {
    entry_peer(func, args, direct, false)
}

/// [`entry_from`] for any caller: `direct` as there; `cross` when it is the core entering an isolated kit
/// module across two memories (crate::iso) — lists and dictionaries of the answer are blocks that cross.
pub fn entry_peer(func: u32, args: &[u8], direct: bool, cross: bool) -> Result<Vec<u8>, String> {
    let m = module();
    let mut r = Reader::from(args, direct);
    let mut w = Writer::to_peer(direct, cross);
    if crate::engine_entry::plain(func, &*m, &mut r, &mut w)? {
        return Ok(w.buf);
    }
    match func {
        kit::INFO => {
            let version = MODULE
                .with(|x| x.borrow().as_ref().map(|(_, v)| *v))
                .unwrap_or("0.0.0");
            w.value(&json!({
                "id": m.id(),
                "version": version,
                "services": m.services(),
                "kitcall": kc::ABI,
                "hooks": HOOKS.with(|h| h.get()),
                "events": 1,
                "tick": fast_entries().0,
                "svc": fast_entries().1,
                "engine": m.engine_contract().then_some(kit::ENGINE_CONTRACT),
            }));
            return Ok(w.buf);
        }
        kit::SERVICES_FOR => {
            let _inst = r.u32()?;
            let config = r.value()?;
            w.value(&json!(m.services_for(&config)));
            return Ok(w.buf);
        }
        kit::VALIDATE => {
            let project = r.value()?;
            let config = r.value()?;
            let issues: Vec<Json> = m
                .validate(&project, &config)
                .into_iter()
                .map(|i| json!({"path": i.path, "message": i.message}))
                .collect();
            w.value(&Json::Array(issues));
            return Ok(w.buf);
        }
        kit::DROP => {
            let (inst, kit) = (r.u32()?, r.u32()?);
            let gone = INSTANCES.with(|i| {
                let mut i = i.borrow_mut();
                let at = i.iter().position(|(a, b, _)| *a == inst && *b == kit)?;
                Some(i.remove(at).2)
            });
            forget_last();
            if let Some(h) = gone {
                // SAFETY: made by Box::into_raw at MOUNT, removed from the table above (no hook of it runs: the
                // core drops an instance only from outside its hooks).
                drop(unsafe { Box::from_raw(h) });
            }
            return Ok(w.buf);
        }
        _ => {}
    }
    let ctx = Ctx::read(&mut r)?;
    if func == kit::MOUNT {
        let project = crate::project::Project::read(&mut r)?;
        let config = r.value()?;
        let mut rng = Rng::from_u32(0);
        rng.restore(&r.value()?);
        let host = Box::into_raw(Box::new(Host::new(ctx, m.id(), project, config, rng)));
        forget_last();
        INSTANCES.with(|i| {
            let mut i = i.borrow_mut();
            if let Some(at) = i
                .iter()
                .position(|(a, b, _)| *a == ctx.inst && *b == ctx.kit)
            {
                // SAFETY: as DROP.
                drop(unsafe { Box::from_raw(i.remove(at).2) });
            }
            i.push((ctx.inst, ctx.kit, host));
        });
        return Ok(w.buf);
    }
    let ptr = instance(ctx.inst, ctx.kit).ok_or_else(|| {
        format!(
            "kitcall: {} has no instance {}/{} (MOUNT first)",
            m.id(),
            ctx.inst,
            ctx.kit
        )
    })?;
    // SAFETY: the instance lives until DROP; a nested entry of the same instance (core → this kit → core → this
    // kit) only happens while the outer one waits on a call to the core, which holds no borrow of it.
    let host: &mut Host = unsafe { &mut *ptr };
    host.enter(ctx);
    let mut kx = Kx {
        host,
        kit: ctx.kit as usize,
    };
    let kx = &mut kx;
    match func {
        kit::SETUP => m.setup(kx),
        kit::READY => m.ready(kx),
        kit::TEARDOWN => m.teardown(kx),
        kit::UPDATE | kit::LATE_UPDATE => {
            kx.host.rest = None;
            if func == kit::UPDATE {
                m.update(kx, r.f64()?);
            } else {
                m.late_update(kx, r.f64()?);
            }
            // The kit rests (`Kx::rest`): the answer says so, as the status of the short entry does.
            if let Some(wake) = kx.host.rest.take() {
                w.u32(kc::fast::RESTS | (wake & kc::fast::WAKE_MASK));
            }
        }
        kit::UPDATE_IDLE => m.update_idle(kx, r.f64()?),
        kit::ON_EVENT => {
            let tag = r.string()?;
            let ev = crate::event_fixed::read_event(&mut r)?.ok_or("kitcall: bad event")?;
            m.on_event(kx, &tag, &ev);
        }
        kc::events::KIT_EVENTS => crate::events::drain(&*m, kx, &mut r)?,
        kit::ON_TIMER => {
            let tag = r.string()?;
            m.on_timer(kx, &tag, &r.value()?);
        }
        kit::ACTION | kit::CONDITION => {
            let verb = r.string()?;
            let args = r.value()?;
            let ev = crate::event_fixed::read_event(&mut r)?;
            if func == kit::ACTION {
                m.action(kx, &verb, &args, ev.as_ref());
            } else {
                w.bool(m.condition(kx, &verb, &args, ev.as_ref()));
            }
        }
        kit::SPAWN => {
            let id = m.spawn(kx, &r.value()?);
            w.value(&id.map(Json::String).unwrap_or(Json::Null));
        }
        kit::CALL => {
            let cap = r.string()?;
            let method = r.string()?;
            let args = r.values()?;
            w.value(&m.call(kx, &cap, &method, &args)?);
        }
        kit::SAVE => {
            w.opt_value(m.save(kx).as_ref());
        }
        kit::LOAD => m.load(kx, &r.value()?),
        kit::SCENE_ENTER => m.scene_enter(kx, &r.value()?),
        kit::SCENE_EXIT => m.scene_exit(kx, &r.value()?),
        kit::COMPONENT => {
            let name = r.string()?;
            let entity = r.string()?;
            let value = r.value()?;
            m.component(kx, &name, &entity, &value, &r.value()?);
        }
        kit::ON_VAR => {
            let tag = r.string()?;
            let path = r.string()?;
            let value = r.opt_value()?;
            let prev = r.opt_value()?;
            m.on_var(kx, &tag, &path, value.as_ref(), prev.as_ref());
        }
        kit::CALLBACK => {
            let name = r.string()?;
            let args = r.values()?;
            w.value(&m.callback(kx, &name, &args)?);
        }
        kit::RNG => {
            w.value(&kx.host.kits.slot.rng.state());
        }
        kit::ENGINE_UNTAPPED => crate::engine_entry::untapped(&*m, kx, &mut r, &mut w)?,
        _ => return Err(format!("kitcall: kithost.kit@1 has no function {func}")),
    }
    Ok(w.buf)
}

/// The direct entry of the per-tick hooks (`gfx_kit_hook`, exported by [`export_kit!`]): the core calls it wasm to
/// wasm through its function table, the context as parameters (`gens` = `services_gen | config_gen << 32`), `arg` the
/// `dt` of `update` / `late_update` / `update_idle`. No message, no buffer. Answers 0 when the hook ran, 1 when the
/// instance is unknown, 2 for a hook that has no direct entry (the core then enters by the message).
#[allow(clippy::too_many_arguments)]
pub fn hook_direct(
    func: u32,
    inst: u32,
    kit_slot: u32,
    tick: u64,
    frame: u64,
    elapsed: f64,
    fixed_dt: f64,
    gens: u64,
    arg: f64,
) -> u32 {
    let Some(ptr) = instance(inst, kit_slot) else {
        return 1;
    };
    let m = module();
    // SAFETY: as `entry_peer`.
    let host: &mut Host = unsafe { &mut *ptr };
    host.enter(Ctx {
        inst,
        kit: kit_slot,
        tick,
        frame,
        elapsed,
        fixed_dt,
        services_gen: gens as u32,
        config_gen: (gens >> 32) as u32,
    });
    host.rest = None;
    let mut kx = Kx {
        host,
        kit: kit_slot as usize,
    };
    match func {
        kit::UPDATE | kit::LATE_UPDATE => {
            if func == kit::UPDATE {
                m.update(&mut kx, arg);
            } else {
                m.late_update(&mut kx, arg);
            }
            // The kit rests (`Kx::rest`): said with the status, as the short entry does.
            if let Some(wake) = kx.host.rest.take() {
                return kc::fast::RESTS | (wake & kc::fast::WAKE_MASK);
            }
        }
        kit::UPDATE_IDLE => m.update_idle(&mut kx, arg),
        // A function this kit handed over (the tap of a hook): arguments and answer are records of the queue.
        kit::CALLBACK => return crate::events::serve_callback(&*m, &mut kx, arg.to_bits()),
        _ => return 2,
    }
    0
}

/// Registers the kit and its hooks (`gf_calls_init` of the module: see [`export_kit!`]).
pub fn init(module: Rc<dyn KitModule>, version: &'static str) {
    init_with(module, version, HOOKS_ALL);
}

/// The per-tick hooks a kit implements (`INFO.hooks`): the core calls only those every tick. A bit left out is a
/// promise that the kit keeps the default (does nothing) for that hook.
pub const HOOK_UPDATE: u32 = 1;
pub const HOOK_LATE_UPDATE: u32 = 2;
pub const HOOK_UPDATE_IDLE: u32 = 4;
pub const HOOKS_ALL: u32 = 7;

thread_local! {
    static HOOKS: std::cell::Cell<u32> = const { std::cell::Cell::new(HOOKS_ALL) };
}

/// Registers the kit of this module: what its entries run (`gfx_mem_enter` in the common memory, `gfx_iso_enter`
/// of an isolated kit module).
pub fn init_entries(module: Rc<dyn KitModule>, version: &'static str, hooks: u32) {
    HOOKS.with(|h| h.set(hooks));
    MODULE_PTR.with(|m| m.set(Some(Rc::as_ptr(&module))));
    MODULE.with(|m| *m.borrow_mut() = Some((module, version)));
}

/// [`init`] with the mask of the per-tick hooks the kit implements (`export_kit!(module, version, hooks)`).
pub fn init_with(module: Rc<dyn KitModule>, version: &'static str, hooks: u32) {
    init_entries(module.clone(), version, hooks);
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| keep_panic(info.to_string())));
    // The two probes of the common memory a host may post to a kit module (a kit on a Worker, docs/kit-modules.md §9.4).
    #[cfg(all(feature = "common-memory", target_arch = "wasm32"))]
    {
        guest::expose("mem.zone_words", crate::mem::zone_words);
        guest::expose("mem.column", crate::mem::column);
    }
}

thread_local! {
    /// The message of this module's last panic (a kit module is built `panic = "abort"`: the trap `unreachable`
    /// says nothing). The host reads it through `gfx_last_panic` when a call traps, and names it in the error.
    static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

#[cfg(target_arch = "wasm32")]
fn keep_panic(msg: String) {
    let _ = LAST_PANIC.try_with(|p| {
        if let Ok(mut p) = p.try_borrow_mut() {
            *p = msg;
        }
    });
}

/// `gfx_last_panic`: the address and the length of the message of the last panic (`ptr << 32 | len`, 0: none).
pub fn last_panic() -> u64 {
    LAST_PANIC.with(|p| {
        let p = p.borrow();
        if p.is_empty() {
            0
        } else {
            ((p.as_ptr() as usize as u64) << 32) | p.len() as u64
        }
    })
}

/// The contracts of a kit module, in its custom section `gripforge.calls` (checked by the host at link).
/// (A kit module of the common memory; an isolated one has its own list: `crate::iso::CONTRACTS`.)
/// The `consumes` lines here are the widest list (every interface a kit may call): tools/kit-modules/build.mjs
/// replaces them in each house kit by what its binary really calls (the markers of [`crate::uses`],
/// tools/kit-modules/contracts.mjs `sealKit`) — a kit that never touches the physics does not consume it. A kit built
/// by `gripforge kit build` keeps this list until that tool seals its binaries the same way.
pub const CONTRACTS: &str = "provides kithost.kit@1.3.0\nconsumes kithost.world@^1.5\nconsumes kithost.state@^2.0\nconsumes kithost.physics@^1.5\nconsumes kithost.services@^1.1\nconsumes kithost.input@^2.0\nconsumes kithost.assets@^1.0\nconsumes kithost.platform@^1.0\nconsumes kithost.zone@^1.0\nconsumes kithost.scene@^1.0\n";

/// Makes a crate a kit module of the common memory, with [`common_memory!`] beside it (always both):
/// `export_kit!(kits::fx_hit_flash::module, "1.2.0");` — the `gf_*` exports of the
/// calls contract, the hooks of `kithost.kit@1`, and the contracts section.
#[macro_export]
macro_rules! export_kit {
    ($module:path, $version:expr) => {
        $crate::export_kit!($module, $version, $crate::host::HOOKS_ALL);
    };
    ($module:path, $version:expr, $hooks:expr) => {
        $crate::export_kit!($module, $version, $hooks, || {});
    };
    // `$links`: run once at the module's start — the direct links to the providers of the capabilities this kit
    // calls (`service_links!`, docs/kit-modules.md §7).
    ($module:path, $version:expr, $hooks:expr, $links:expr) => {
        $crate::gripforge_abi::calls::export_calls!();
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn gf_calls_init() {
            $crate::host::init_with($module(), $version, $hooks);
            ($links)();
        }
        /// The same start for a native library of the kit (`common_memory!`, `kithost.native@1`).
        #[cfg(not(target_arch = "wasm32"))]
        #[no_mangle]
        pub extern "C" fn gfx_native_start() {
            $crate::host::init_with($module(), $version, $hooks);
            ($links)();
        }
        /// A service of this kit called by another kit module, by direct link ($crate::svc).
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn gfx_svc_call(capability: u32, method: u32, words: u32) -> u64 {
            $crate::svc::serve(capability, method, words)
        }
        /// The message of this module's last panic, for the host's error ($crate::host::last_panic).
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn gfx_last_panic() -> u64 {
            $crate::host::last_panic()
        }
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn gfx_svc_id() -> u32 {
            $crate::svc::id()
        }
        // The structures of the contract as this build laid them out (checked by the tool and by the host).
        $crate::kitcall::structs_section!();
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn gfx_kit_hook(
            func: u32,
            inst: u32,
            kit: u32,
            tick: u64,
            frame: u64,
            elapsed: f64,
            fixed_dt: f64,
            gens: u64,
            arg: f64,
        ) -> u32 {
            $crate::host::hook_direct(func, inst, kit, tick, frame, elapsed, fixed_dt, gens, arg)
        }
        #[cfg(target_arch = "wasm32")]
        #[link_section = "gripforge.calls"]
        #[used]
        static GF_CONTRACTS: [u8; $crate::host::CONTRACTS.len()] = {
            let s = $crate::host::CONTRACTS.as_bytes();
            let mut out = [0u8; $crate::host::CONTRACTS.len()];
            let mut i = 0;
            while i < s.len() {
                out[i] = s[i];
                i += 1;
            }
            out
        };
    };
}
