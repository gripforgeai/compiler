//! The pure types and traits of the ports a kit host plugs in (physics, the shape table, assets): one file
//! with the kit façade (`gripforge-kit-guest/src/ports/port_types.rs`), compiled once here so that the core
//! (`gripforge-kithost`) and the physics module (`gfkit-core-physics-rapier`) — two separate libraries — speak the
//! same `PhysicsPort` (docs/kit-modules.md §7.10).

#[path = "../../../kit-guest/src/js.rs"]
#[allow(dead_code)]
mod js;
#[path = "../../../kit-guest/src/ports/port_types.rs"]
mod port_types;
pub use port_types::*;
#[path = "../../../kit-guest/src/ports/character_types.rs"]
mod character_types;
pub use character_types::*;
#[path = "../../../kit-guest/src/ports/shape_types.rs"]
mod shape_types;
pub use shape_types::*;
#[path = "../../../kit-guest/src/ports/joint_types.rs"]
mod joint_types;
pub use joint_types::*;
#[path = "../../../kit-guest/src/ports/vehicle_types.rs"]
mod vehicle_types;
pub use vehicle_types::*;
#[path = "../../../kit-guest/src/ports/fluid_types.rs"]
mod fluid_types;
pub use fluid_types::*;
/// The entries of a physics module that is a library of its own (signatures and records shared by the module and
/// the core).
pub mod abi;
mod abi_records;
/// The turn of a moving body read in place in the shape table: what the reflection of a rolling body draws.
pub mod body_turns;
/// The link of a module that runs asynchronously on a thread of its own: orders, tickets, published state
/// (docs/kit-modules.md §7.17).
pub mod link;
/// The mailboxes between modules: boxes opened by each module, a directory hosted at start, messages read in
/// place (docs/decisions/2026-11-19-canaux-directs.md).
pub mod mailbox;
/// The physics over a link: its orders, its published state, and the physics' side (`serve_live`, `serve_loop`).
pub mod physics_link;
/// The register of the revision keys: one slot per owner of a part of an entity, the same number on every Worker.
pub mod revision_keys;
/// The engine's tree as a kit module reaches it (`kithost.scene@1`): find, read and write by handle, create, adopt.
pub mod scene;
/// The shape table a physics module publishes (ABI 2.6) and its reader: the colliders read in place.
pub mod shape_table;
/// The writer's side of the shape table: what a physics keeps to publish it.
pub mod shape_writer;
pub use scene::*;
