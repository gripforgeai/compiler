//! The function numbers of `kithost.world@1`, `kithost.state@1` and `kithost.services@1` (schemas:
//! interfaces/{world,state,services}.gfi). Stable: a number is never reused nor retyped within a major. An entity travels
//! as a tagged `value` (an object of its fields, see the `.gfi` files). The numbers of the retired `kithost.zones@1`
//! (768..=1023) and `kithost.objectives@1` are never given again.

/// `kithost.world@1`: the entities (`host.entities`), spawn, the player scopes, the animation helpers of `Kx`.
pub mod world {
    const B: u32 = crate::WORLD.base;
    pub const HAS: u32 = B | 0x01;
    pub const GET: u32 = B | 0x02;
    pub const COMPONENT: u32 = B | 0x03;
    pub const TRANSFORM: u32 = B | 0x04;
    pub const SET: u32 = B | 0x05;
    pub const TAG: u32 = B | 0x06;
    pub const UNTAG: u32 = B | 0x07;
    pub const SET_TRANSFORM: u32 = B | 0x08;
    pub const PUT: u32 = B | 0x09;
    pub const ALL: u32 = B | 0x0a;
    pub const QUERY: u32 = B | 0x0b;
    pub const FIRST: u32 = B | 0x0c;
    pub const SERIALIZE: u32 = B | 0x0d;
    pub const NEXT_ID: u32 = B | 0x0e;
    pub const QUERY_IDS: u32 = B | 0x0f;

    pub const CREATE: u32 = B | 0x20;
    pub const DESTROY: u32 = B | 0x21;
    pub const RESTORE: u32 = B | 0x22;
    pub const RESOLVE_POSITION: u32 = B | 0x23;
    pub const SPAWN: u32 = B | 0x24;
    pub const SPAWN_TRY: u32 = B | 0x25;
    pub const SPAWN_KINDS: u32 = B | 0x26;
    pub const DESPAWN_BY_TAG: u32 = B | 0x27;
    pub const RESOLVE_ENTITY_REF: u32 = B | 0x28;
    pub const ACTING_PLAYER: u32 = B | 0x29;
    pub const SCOPE_PUSH: u32 = B | 0x2a;
    pub const SCOPE_POP: u32 = B | 0x2b;
    pub const REGISTER_FACTORY: u32 = B | 0x2c;

    pub const ANIM_OVERRIDE: u32 = B | 0x30;
    pub const SET_ANIM: u32 = B | 0x31;
    pub const PLAY_ANIM_ONCE: u32 = B | 0x32;
    pub const CANCEL_ANIM: u32 = B | 0x33;
    /// 1.6 — the animation layers.
    pub const PLAY_ANIM_LAYERED: u32 = B | 0x34;
    pub const ACTION_MOVEMENT: u32 = B | 0x35;
    pub const SET_ACTION_LAYERS: u32 = B | 0x36;
    // RETIRED (#655): `decide_action_movement` 0x37 (the action a character decided: that is the controller kit's, not the core's); its number stays taken.
    pub const ACTION_LAYER_MOVEMENT: u32 = B | 0x38;

    /// 1.1 — the scope the direct link `w_scope_pop` popped when its id is too long for the registers.
    pub const SCOPE_POPPED: u32 = B | 0x40;
    /// 1.7 — authoritative ownership of the entity's pose by a native engine node.
    pub const IS_HELD: u32 = B | 0x41;
}

/// `kithost.state@2`: the game variables (2.0: no phase).
pub mod state {
    const B: u32 = crate::STATE.base;
    pub const GET: u32 = B | 0x02;
    pub const SET_VAR: u32 = B | 0x03;
    pub const INC: u32 = B | 0x04;
    pub const SNAPSHOT: u32 = B | 0x05;
    pub const DEFINE: u32 = B | 0x06;
    pub const BIND: u32 = B | 0x07;
    pub const GUARD: u32 = B | 0x08;
    pub const SET_GUARDED: u32 = B | 0x0a;
    pub const INC_GUARDED: u32 = B | 0x0b;
    pub const SET: u32 = B | 0x0c;
    pub const VALUES: u32 = B | 0x0d;
    pub const VARS: u32 = B | 0x0e;
    pub const DRAIN_BINDS: u32 = B | 0x10;
}

/// `kithost.services@1`: the services, the bus, the timers, the triggers, the host itself.
pub mod services {
    const B: u32 = crate::SERVICES.base;
    pub const KIT_CONFIG: u32 = B | 0x01;
    pub const LOG: u32 = B | 0x02;
    /// The project as it is now (game.engine reactions edit `installed[].config`): read again with the config.
    pub const PROJECT: u32 = B | 0x03;

    pub const HAS_SERVICE: u32 = B | 0x10;
    pub const INSTALLED: u32 = B | 0x11;
    pub const CALL: u32 = B | 0x12;
    pub const CALL_PREVIOUS: u32 = B | 0x13;
    pub const INVOKE: u32 = B | 0x14;
    pub const DEFAULT_DATA: u32 = B | 0x15;
    pub const DATA: u32 = B | 0x16;
    pub const DECIDE: u32 = B | 0x17;
    pub const RUNTIMES: u32 = B | 0x18;
    pub const SAVE: u32 = B | 0x19;
    pub const LOAD: u32 = B | 0x1a;
    pub const KIT_IDS: u32 = B | 0x1b;
    pub const LOCK_HASH: u32 = B | 0x1c;

    pub const EMIT: u32 = B | 0x20;
    pub const BUS_ON: u32 = B | 0x21;
    pub const BUS_OFF: u32 = B | 0x22;
    pub const AFTER: u32 = B | 0x23;
    pub const EVERY: u32 = B | 0x24;
    pub const TIMER_CANCEL: u32 = B | 0x25;
    pub const DEFER: u32 = B | 0x26;

    pub const REGISTER_ACTION: u32 = B | 0x30;
    pub const REGISTER_CONDITION: u32 = B | 0x31;
    pub const ADD_RULE: u32 = B | 0x32;
    pub const RUN_ACTIONS: u32 = B | 0x33;
    pub const EVALUATE: u32 = B | 0x34;
    pub const TRIGGER_REMOVE: u32 = B | 0x35;
    pub const TRIGGER_FIRING: u32 = B | 0x36;
    pub const ACTION_TYPES: u32 = B | 0x37;
    pub const CONDITION_TYPES: u32 = B | 0x38;

    /// 1.1 — the number of a name (a component, a var path, a capability), for the direct links.
    pub const ATOM: u32 = B | 0x40;
    /// 1.1 — the name of an atom (a provider entered by direct link reads its capability and method once).
    pub const ATOM_NAME: u32 = B | 0x41;
    /// 1.2 — (address u64, limit f64, round6 bool): the clock of the rest the per-tick hook that calls is about to
    /// answer (`Kx::rest_counting`: a number of the kit's state, in the common memory, that the core moves by the
    /// step while the kit rests). A core before 1.2 refuses it: the façade then does not rest.
    pub const REST_COUNTING: u32 = B | 0x42;
    /// 1.3 (`game.engine.contract@1`, the game.engine module) — (kit u32, hook u8, …): runs a scene or component hook of
    /// another mounted kit (`KIT_HOOK_*`).
    pub const KIT_HOOK: u32 = B | 0x43;
    /// 1.3 — (verb string, kit u32, condition bool): a trigger verb answered by that kit (`game.engine.scripts`).
    pub const REGISTER_VERB_FOR: u32 = B | 0x44;
    /// 1.4 — (collection string, docs value): the documents of a data collection of the project, replaced; the kits
    /// read the project again (a mod's live data, the game.engine module).
    pub const PROJECT_DOCS: u32 = B | 0x45;
    /// 1.4 — (kit string, path string, value value): a dotted path of a mounted kit's config, written in the project
    /// and in the kit (a mod's live config, the game.engine module).
    pub const KIT_CONFIG_SET: u32 = B | 0x46;
    /// 1.4 — () → bool: the host's tracer of hooks keeps runs (`hook_trace::on`).
    pub const HOOK_TRACE_ON: u32 = B | 0x47;
    /// 1.4 — (run value): one run of a hook, kept by the host's tracer (`hook_trace::record`).
    pub const HOOK_TRACE_RECORD: u32 = B | 0x48;
    /// 1.5 — (topic string) → bool: whether an event of `topic` would be heard (`Host::has_audience`): a kit module
    /// builds nothing for an event nobody hears. A core before 1.5 refuses it: the façade then answers yes.
    pub const HAS_AUDIENCE: u32 = B | 0x49;
    /// 1.6 — (watch bool, filter string) → value: the figures of each hook the filter keeps (`hook_trace::stats`) run since the host's tracer was watched (calls, time,
    /// refusals, cancellations: `hook_trace::stats`), kept while `watch`, dropped by `false` (the debug.inspector
    /// panel). A core before 1.6 refuses it: the façade then answers no figures.
    pub const HOOK_STATS: u32 = B | 0x4a;
    /// 1.6 — (hook string) → bool: whether a tap may listen to `hook` (`Host::hook_tapped`): a kit module builds and
    /// sends nothing for a hook nobody taps. A core before 1.6 refuses it: the façade then takes the full path.
    pub const HOOK_TAPPED: u32 = B | 0x4b;
    /// 1.6 — (hook string, kit string) → bool: `HOOK_TRACE_ON` for one run of `hook`, declared by `kit`: yes while
    /// runs are kept, or while the hooks are watched and the filter keeps it (`hook_trace::on_for`). A core before 1.6
    /// refuses it: the façade then asks `HOOK_TRACE_ON`.
    pub const HOOK_TRACE_ON_FOR: u32 = B | 0x4c;
    /// 1.7 — (id u64, contexts list<string>): the contexts of game.engine's stack a subscription hears in
    /// (docs/contextes.md): empty is `default`, `all` hears whatever is on top. A core before 1.7 refuses it: the
    /// subscription stays on `default`.
    pub const BUS_CONTEXTS: u32 = B | 0x4d;
    /// 1.7 — (name string): the context on top of game.engine's stack (game.engine sets it); only the subscriptions
    /// of that context (and `all`) are matched by the bus from then on.
    pub const CONTEXT_TOP: u32 = B | 0x4e;
    /// `KIT_HOOK` hooks: (scene value), (scene value), (name string, entity string, value value, instance value).
    pub const KIT_HOOK_SCENE_ENTER: u8 = 0;
    pub const KIT_HOOK_SCENE_EXIT: u8 = 1;
    pub const KIT_HOOK_COMPONENT: u8 = 2;
}
