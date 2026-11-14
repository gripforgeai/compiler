//! gripforge-build: `gripforge project build`. Reads a GripForge project (`project.gripforge`) and the
//! `kit.api.json` of the kits installed in it, checks it strictly with the common core of the project format
//! (`gripforge-scene-format`), and generates the Rust of the game: one `init_scene.rs` per scene, `index.rs`, and the
//! crate around them in `.build/`. It compiles nothing: the game's own build compiles the generated crate.

pub mod build;
pub mod cli;
pub mod core;
pub mod crate_gen;
pub mod facades;
pub mod json;
pub mod project;
pub mod report;
pub mod strict;
