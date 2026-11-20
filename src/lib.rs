//! gripforge-build: `gripforge project build`. Reads a GripForge project (`project.gripforge`) and the
//! `kit.api.json` of the kits installed in it, checks it strictly with the common core of the project format
//! (`gripforge-scene-format`), and generates the Rust of the game: one `init_scene.rs` per scene and the crate of the
//! game's module around them in `.build/`, with the game's code the project declares in `sources`. It
//! compiles nothing: the game's own build compiles the generated crates.

pub mod access;
pub mod build;
pub mod cli;
pub mod core;
pub mod crate_gen;
pub mod json;
pub mod project;
pub mod report;
pub mod sources;
pub mod strict;
pub mod textures;
