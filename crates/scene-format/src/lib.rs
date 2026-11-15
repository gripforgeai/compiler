//! gripforge-scene-format: the format of a GripForge project (`<name>.gripforge`) and of its scenes (`<name>.gfscene`),
//! both JSON, both owned by the game (no technical version in them): read with tolerance, checked by a validator whose
//! every issue says what to fix, lowered into the documents game.engine already compiles. The conception is in
//! docs/dev-builder.md; the guide for whoever writes a project (a person or an AI) is docs/ecrire-un-projet.md.
//!
//! - [`tree`]: a scene file read and resolved (sub-scenes and overrides, transforms), checked against the registries.
//! - [`registry`]: the node types (`node_types.json`) and the kits' sections (`expose` entries `for: ["scene"]`).
//! - [`lower`]: a resolved scene as game.engine's instances, entries, zones and the script of its bindings.
//! - [`expand`]: the project's scenes that name a `file`, lowered for game.engine (`data.scene_files`).
//! - [`build`]: the published form of a project (one resolved file per scene, a manifest for asset.preload), and
//!   the way back into one file (`bundle`).
//! - [`states`]: the states of an element (`entity.states`), its table state → reflection (`reflect`) and its bound values (`entity.bind`).
//! - [`place`]: what a scene asks of the kits when it is entered (lights, cameras, the kits' sections and their order),
//!   one source for dev.builder at run time and for the Rust code `gfscene code` writes.
//! - [`code`]: the Rust code that poses each scene ahead of time (`init_scene.rs`), over the same resolution.
//! - [`project`]: the project file checked as a whole, and packed into the `game.json` today's loaders read.
//! - [`ops`]: the edit commands of the site's scene patch (`insert`, `remove`, `transform`, `replace`, `update`) and
//!   the debugging editor's file of patches, applied to the files with the smallest diff ([`jsontext`]).
//!
//! Who uses it: game.engine (kits/game_engine/logic, reading the scene files of a project), the dev.builder kit, and
//! the `gfscene` tool (`validate`, `pack`, `apply`, `build`, `bundle`). No host, no kit, no engine: JSON in, JSON out.

pub mod build;
pub mod code;
pub mod expand;
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
pub mod states;
pub mod tree;

pub use issue::{Issue, Level};
pub use registry::Registry;
