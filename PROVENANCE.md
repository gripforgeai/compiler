# Provenance

- `crates/scene-format` (the common core of the project format: reading, validator, order of posing, references,
  lowering, packing, the generator of `init_scene.rs` and the `gfscene` tool) was moved, unchanged but for its
  `Cargo.toml` (no workspace inheritance), from the kits SDK repository `gripforgeai/module-kits`, folder
  `core/scene-format`, at commit `0c88723ab65b96093b0c5d0a9458855c187e6fa2`. It now lives here; the kits SDK takes it
  from this repository.
- `src/core.rs` and `src/strict.rs` come from the same repository, `tools/compiler/wasm/src` (commit
  `837cb5e19dbed150b4437e464ecdecbf93fc8511`), without their wasm surface; the rest of `src/` rewrites in Rust the
  JavaScript of `tools/compiler` (`lib/`), and `tests/` its tests (golden files kept, see the pull request).
- `examples/room` is the dev.builder room of the kits SDK (one `project.gripforge`) with a frozen copy of the
  `kit.api.json` of the kits at the same commit. The accessor of each kit (`kits/<kit>/access/rust/`) is written from that
  frozen `kit.api.json` by the generator of the kits SDK (`tools/kit-api/langs/rust_facade.mjs`, branch
  `feat/kit-accessors-published`), as the kit's package ships it.
- `sdk/` (the public part of the kits SDK: `abi/`, `kit-guest/`, `kit-access/` and `rust-toolchain.toml`) is copied by
  `tools/sync-sdk` from `gripforgeai/module-kits` (`core/abi`, `core/kit-guest`, `core/kit-access`,
  `rust-toolchain.toml`) at the commit of `sdk/sdk.json` (`65ffb2254a0881a1b0eb8199e8a3f5d7038ce1d0`), unchanged but for
  `kit-access/Cargo.toml`, which loses its `kithost` feature (the kit host's side, `gripforge-kit-api`, stays in the
  kits repository). `sdk/Cargo.toml` (their workspace), `sdk/README.md` and `sdk/sdk.json` are this repository's own.
  `tools/sync-sdk <module-kits checkout> --check` says whether `sdk/` is still exactly that copy.
