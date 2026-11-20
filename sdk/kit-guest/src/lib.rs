//! gripforge-kit-guest — the kit side of the kit logic modules (docs/kit-modules.md).
//!
//! A kit of `kits/<kit>/logic` compiles unchanged into its own wasm: this crate gives it the same
//! module paths (`crate::host::{Kx, KitModule}`, `crate::entities::Pos`, `crate::bus::GameEvent`…) through the kit
//! crate's root (`pub use gripforge_kit_guest::*`), where `Host` is a façade whose fields and methods call the core
//! (`kithost.world@1`, `kithost.state@1`… of the `kithost.core` module) through the calls between modules.
//!
//! The files that hold no state of the host are this crate's and the core compiles them too (`#[path]` from the
//! kit host): the JS helpers, configs, schemas, rng, expressions, the kit contract (`KitModule`), the entity, event,
//! zone, mission and port types. One definition for both sides; the rest of this crate are façades.

pub mod cfg;
/// The kit side of the debugger exposure contract `debug.expose@1` (docs/exposer-au-debogueur.md).
pub mod debug_expose;
pub mod debug_pipe;
pub mod expose;
pub mod expr;
pub mod js;
pub mod kept;
pub mod kit;
pub mod mailbox;
/// What a kit does with its elements out of their contexts of game.engine's stack (docs/hors-contexte.md).
pub mod out_of_context;
/// The kit side of the passport reader of `expose` (docs/exposer-au-passeport.md).
pub mod passport_expose;
pub mod published;
pub mod ray_chain;
pub mod rng;
/// The kit side of the scene reader of `expose`: the elements a scene file drives (docs/design/dev-builder-sections.md).
pub mod scene_expose;
pub mod schema;
/// The kit side of core.settings' variables: a kit reads its settings where core.settings holds them.
pub mod settings_vars;
pub mod shared;
pub mod wire;

pub mod bus;
pub mod columns;
pub mod direct;
mod engine_entry;
pub mod entities;
mod event_fixed;
pub mod events;
mod fixed_read;
pub mod host;
mod hot;
pub mod iso;
pub mod kit_time;
pub mod kx_typed;
#[cfg(feature = "common-memory")]
pub mod mem;
pub mod motion;
/// The native form of the common memory (`kithost.native@1`, docs/kit-modules-native.md).
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
pub mod native;
pub mod ports;
#[cfg(feature = "entry-probe")]
pub mod probe;
pub mod project;
pub mod state;
pub mod svc;
pub mod tick;
pub mod timers;
pub mod triggers;
pub mod typed;
pub mod uses;
pub mod val;
pub mod zone;

/// The tracer of the interceptable hooks: in the game.engine module, the runs go to the host's tracer.
pub mod hook_trace;
mod kx_engine;
mod kx_hooks;
mod kx_ports;
mod kx_world;

pub use bus::GameEvent;
pub use gripforge_abi as kitcall;

/// The contracts the façade reads or writes on behalf of every kit module (docs/decisions/2026-11-20-abi-par-contrat.md
/// §8): what `export_kit!` and `isolated_kit!` declare (`gripforge.contracts`, `gf_contracts`), checked by the host at
/// load. The generated `DECL` of each converted contract the façade touches (§9), with its role. One list for every
/// kit module (the façade is one crate): a kit that never watches a pose still declares `abi.scene.drawn` — a refusal
/// wider than needed, never a wrong read (§8, the pitfall of the unified features).
pub mod contracts {
    use gripforge_abi::contract::{BOTH, READ, WRITE};
    /// `(DECL, role)` of each contract the façade reads or writes.
    pub const USES: &[(&[u8], u8)] = &[
        // The drawn poses, read at the tick (ports/drawn_pose.rs).
        (gripforge_contracts::abi_scene_drawn::DECL, READ),
        // The render yaws, written by the kit that claimed (ports/render_yaw_claim.rs).
        (gripforge_contracts::abi_scene_render_yaw::DECL, BOTH),
        // The frozen view of a body's intentions, read by a kit's registered intention at each physics step
        // (gripforge_kit_common::intent::IntentStep::others).
        (gripforge_contracts::abi_intent_view::DECL, READ),
        // The step block a kit's registered intention reads and answers in at each physics step
        // (gripforge_kit_common::intent::IntentStep).
        (gripforge_contracts::abi_intent_step::DECL, BOTH),
        // The parameters of the base intentions a kit launches or changes (gripforge_kit_common::intent::base).
        (gripforge_contracts::abi_intent_velocity::DECL, WRITE),
        (gripforge_contracts::abi_intent_target::DECL, WRITE),
        (gripforge_contracts::abi_intent_follow::DECL, WRITE),
        (gripforge_contracts::abi_intent_force::DECL, WRITE),
        (gripforge_contracts::abi_intent_jump::DECL, WRITE),
        (gripforge_contracts::abi_intent_burst::DECL, WRITE),
        (gripforge_contracts::abi_intent_set_velocity::DECL, WRITE),
        (gripforge_contracts::abi_intent_turn::DECL, WRITE),
        // The words of the physics a kit writes or reads through the façade (ports/physics_direct.rs, joint_types.rs,
        // vehicle_types.rs, fluid_types.rs).
        (gripforge_contracts::abi_physics_body_state::DECL, BOTH),
        (gripforge_contracts::abi_physics_joint::DECL, WRITE),
        (gripforge_contracts::abi_physics_vehicle::DECL, WRITE),
        (gripforge_contracts::abi_physics_vehicle_wheel::DECL, WRITE),
        (gripforge_contracts::abi_physics_vehicle_input::DECL, WRITE),
        (gripforge_contracts::abi_physics_vehicle_state::DECL, READ),
        (
            gripforge_contracts::abi_physics_vehicle_wheel_state::DECL,
            READ,
        ),
        (gripforge_contracts::abi_physics_buoyancy::DECL, WRITE),
        (gripforge_contracts::abi_physics_buoyancy_cell::DECL, WRITE),
        (gripforge_contracts::abi_physics_fluid::DECL, WRITE),
        (gripforge_contracts::abi_physics_wave::DECL, WRITE),
        // The shape table of the physics, read in place (ai.navigation, the debugger, the vegetation).
        (gripforge_contracts::abi_physics_shape_table::DECL, READ),
        (gripforge_contracts::abi_physics_shapes::DECL, READ),
        // The blocks of active contacts of a kit's groups of sensors: laid by the kit (its TableHead), read in place
        // (gripforge_kit_common::sensors_guest::SensorGroup).
        (gripforge_contracts::abi_sensor_contacts::DECL, BOTH),
        // Lot 5: a value of the fixed-layout blocks (src/fixed.rs of gripforge-abi).
        (gripforge_contracts::abi_mem_fixed_value::DECL, BOTH),
        (gripforge_contracts::abi_mem_fixed_pair::DECL, BOTH),
        (gripforge_contracts::abi_mem_fixed_block::DECL, BOTH),
        // Lot 5: the bytes a callee gives away.
        (gripforge_contracts::abi_mem_bytes::DECL, BOTH),
        // Lot 5: the context of every hook of kithost.kit@1.
        (gripforge_contracts::abi_kit_ctx::DECL, READ),
        // Lot 5: the context of a per-tick hook, by address.
        (gripforge_contracts::abi_fast_hook::DECL, READ),
        // Lot 5: the scalar service calls between kits.
        (gripforge_contracts::abi_fast_scalar::DECL, BOTH),
        (gripforge_contracts::abi_fast_service::DECL, BOTH),
        // Lot 5: the headers of gripforge:calls@1.
        (gripforge_contracts::abi_calls_header::DECL, BOTH),
        (gripforge_contracts::abi_calls_reply::DECL, BOTH),
        // Lot 5: the records of the shared zone, read in place.
        (gripforge_contracts::abi_zone_buffer::DECL, READ),
        (gripforge_contracts::abi_zone_directory::DECL, READ),
        (gripforge_contracts::abi_zone_state::DECL, READ),
        (gripforge_contracts::abi_zone_events::DECL, READ),
        (gripforge_contracts::abi_zone_event::DECL, READ),
        (gripforge_contracts::abi_zone_regions::DECL, READ),
        // Lot 5: the rows of WORLD and their header.
        (gripforge_contracts::abi_engine_entity::DECL, READ),
        (gripforge_contracts::abi_engine_world::DECL, READ),
        // Lot 5: natively: the tables exchanged at load (declared on every target: one section).
        (gripforge_contracts::abi_native_host::DECL, READ),
        (gripforge_contracts::abi_native_kit::DECL, WRITE),
        (gripforge_contracts::abi_native_links::DECL, READ),
        (gripforge_contracts::abi_native_bytes::DECL, BOTH),
        (gripforge_contracts::abi_native_zone_place::DECL, READ),
        (gripforge_contracts::abi_native_svc_provider::DECL, READ),
        // Lot 3: the head of INPUT (the pointer total, the places of the ring, the bindings, the directory; a kit sets
        // the places it lays), the directory of the published regions (published.rs, written by the publisher) and the
        // PointerFollow rows read by `published::follow_now`.
        (gripforge_contracts::abi_zone_input::DECL, BOTH),
        (gripforge_contracts::abi_zone_published::DECL, BOTH),
        (gripforge_contracts::abi_input_pointer::DECL, READ),
        // Lot 5b: the mailboxes (src/mailbox.rs): a box's head and its messages, written and read in place; the
        // directory the host laid in the zone, read to resolve a box.
        (gripforge_contracts::abi_mail_head::DECL, BOTH),
        (gripforge_contracts::abi_mail_message::DECL, BOTH),
        (gripforge_contracts::abi_mail_entry::DECL, READ),
        (gripforge_contracts::abi_mail_table::DECL, READ),
        // Lot 5b: the pipes of the debugger (src/debug_pipe.rs): written by a kit's counters, read by debug.inspector.
        (gripforge_contracts::abi_debug_pipe::DECL, BOTH),
    ];

    /// The façade's contracts ([`USES`]) followed by a kit's own (`export_kit!(…; contracts: …)`), in a constant.
    pub const fn join<const N: usize>(b: &[(&'static [u8], u8)]) -> [(&'static [u8], u8); N] {
        let a = USES;
        let mut out: [(&'static [u8], u8); N] = [(&[], 0); N];
        let mut i = 0;
        while i < a.len() {
            out[i] = a[i];
            i += 1;
        }
        let mut j = 0;
        while j < b.len() {
            out[i + j] = b[j];
            j += 1;
        }
        out
    }
}
#[doc(hidden)]
pub use gripforge_abi;
pub use gripforge_abi::calls;
pub use host::{Host, Issue, KitModule, Kx};

// A kit module is built one of two ways, never a third: for the common memory of the trusted kit modules (feature
// `common-memory`, relocatable, `export_kit!` + `common_memory!`) or isolated (feature `isolated`, `isolated_kit!`).
#[cfg(all(
    target_arch = "wasm32",
    not(any(feature = "common-memory", feature = "isolated"))
))]
compile_error!("gripforge-kit-guest: a kit module is built with the feature `common-memory` (tools/kit-modules/build.mjs, gripforge kit build --mode common) or `isolated` (gripforge kit build)");
#[cfg(all(feature = "common-memory", feature = "isolated"))]
compile_error!("gripforge-kit-guest: `common-memory` and `isolated` are two ways to build a kit module — one of them");

/// An isolated kit module allocates with the size classes of src/alloc.rs; a kit of the common memory (feature
/// `common-memory`) with the core's allocator (`common_memory!`): one allocator for that memory.
#[cfg(all(target_arch = "wasm32", not(feature = "common-memory")))]
#[global_allocator]
static ALLOC: gripforge_abi::alloc::KitAlloc = gripforge_abi::alloc::KitAlloc::new();
