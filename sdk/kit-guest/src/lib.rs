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

pub mod anim_layers;
pub mod cfg;
/// The kit side of the debugger exposure contract `debug.expose@1` (docs/exposer-au-debogueur.md).
pub mod debug_expose;
pub mod debug_pipe;
pub mod expr;
pub mod js;
pub mod kept;
pub mod kit;
/// What a kit does with its elements out of their contexts of game.engine's stack (docs/hors-contexte.md).
pub mod out_of_context;
/// The kit side of the passport reader of `expose` (docs/exposer-au-passeport.md).
pub mod passport_expose;
pub mod ray_chain;
pub mod rng;
/// The kit side of the scene reader of `expose`: the elements a scene file drives (docs/design/dev-builder-sections.md).
pub mod scene_expose;
pub mod schema;
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
