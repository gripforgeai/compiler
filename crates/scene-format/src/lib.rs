//! gripforge-scene-format: the format of a GripForge project (`<name>.gripforge`) and of the scenes it holds, JSON,
//! owned by the game (no technical version in them): read with tolerance, checked by a validator whose
//! every issue says what to fix, lowered into the documents game.engine already compiles. The conception is in
//! docs/dev-builder.md; the guide for whoever writes a project (a person or an AI) is docs/ecrire-un-projet.md.
//!
//! - [`tree`]: a scene read and resolved (sub-scenes and overrides, transforms), checked against the registries.
//! - [`registry`]: the node types (`node_types.json`) and the kits' sections (`expose` entries `for: ["scene"]`).
//! - [`lower`]: a resolved scene as game.engine's instances and entries, world.zones' zones and the script of its bindings.
//! - [`expand`]: the project's scenes that name a `file`, lowered for game.engine (`data.scene_files`).
//! - [`build`]: a scene of the project resolved, inline or from a carried sub-scene (`resolve_scene`).
//! - [`states`]: the states of an element (`entity.states`), its table state → reflection (`reflect`) and its bound values (`entity.bind`).
//! - [`place`]: what a scene asks of the kits when it is entered (lights, cameras, the kits' sections and their order),
//!   one source for dev.builder at run time and for the Rust code the build writes ([`code`]).
//! - [`code`]: the Rust code that poses each scene ahead of time (`init_scene.rs`), over the same resolution.
//! - [`include`]: the project resolved first: its included `.gripforge` files, its nodes' variables, its `$var`.
//! - [`project`]: the project file checked as a whole, and packed into the `game.json` envelope of the development mode.
//! - [`ops`]: the edit commands of the site's scene patch (`insert`, `remove`, `transform`, `replace`, `update`) and
//!   the debugging editor's file of patches, applied to the project with the smallest diff ([`jsontext`]).
//! - [`verbs`]: the verbs of a rule or a binding that game.engine could not compile (its whole rule left out), said first.
//! - [`writeback`]: one command written back where its scene lives (inline in the project or its file), with the
//!   revision of a text and the lines that moved (dev.builder `applyToProject`, the CLI, the MCP).
//!
//! Who uses it: the compiler (gripforge-build, this repository), and in the kits repository (gripforgeai/module-kits,
//! which imports this crate by git at a pinned commit: one copy, here, decision of 21/11) game.engine (lowering the
//! scenes of a project), the dev.builder kit (the development mode, the project read at run time) and the web page of
//! the development mode (its wasm, core/scene-format-wasm). The JSON schema of the format (gripforge.schema.json,
//! assembled with the kits' sections into gripforge.full.schema.json) lives in the kits repository, which checks it
//! against [`registry`] at the pinned commit.
//! No host, no kit, no engine: JSON in, JSON out.

pub mod build;
pub mod code;
pub mod expand;
pub mod include;
pub mod issue;
pub mod jsontext;
pub mod lower;
pub mod math;
pub mod ops;
pub mod path;
pub mod place;
pub mod project;
pub mod registry;
pub mod requires;
pub mod sources;
/// The digest of the game's code: the crate `gripforge-sources-digest` of the compiler's public repository (its one
/// copy, decision of Damien 21/11), re-exported so the readers of the format name it here.
pub use gripforge_sources_digest as sources_digest;
pub mod states;
pub mod tree;
pub mod verbs;
pub mod writeback;

pub use issue::{Issue, Level};
pub use registry::Registry;
