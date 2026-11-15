# Provenance

- `crates/scene-format` (the common core of the project format: reading, validator, order of posing, references,
  lowering, packing, the generator of `init_scene.rs` and the `gfscene` tool) was moved, unchanged but for its
  `Cargo.toml` (no workspace inheritance), from the kits SDK repository `gripforgeai/module-kits`, folder
  `core/scene-format`, at commit `0c88723ab65b96093b0c5d0a9458855c187e6fa2`. It now lives here; the kits SDK takes it
  from this repository.
  Since then, `src/registry.rs`, `src/tree.rs`, `src/tree/values.rs`, `src/place.rs` and `tests/lower.rs` were taken
  again, unchanged, from the same folder at commit `6da0bb9650c1fa6221fff448b9a52f05e0c1acad` (node types declared by
  the kits, `nodeType`).
- `src/core.rs` and `src/strict.rs` come from the same repository, `tools/compiler/wasm/src` (commit
  `837cb5e19dbed150b4437e464ecdecbf93fc8511`), without their wasm surface; the rest of `src/` rewrites in Rust the
  JavaScript of `tools/compiler` (`lib/`), and `tests/` its tests (golden files kept, see the pull request).
- `examples/room` is the dev.builder room of the kits SDK (one `project.gripforge`) with a frozen copy of the
  `kit.api.json` of the kits at the same commit. The accessor of each kit (`kits/<kit>/access/rust/`) is written from that
  frozen `kit.api.json` by the generator of the kits SDK (`tools/kit-api/langs/rust_facade.mjs`, branch
  `feat/kit-accessors-published`), as the kit's package ships it.
