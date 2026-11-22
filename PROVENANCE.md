# Provenance

- `crates/scene-format` (the common core of the project format: reading, resolution of `includes` / node variables /
  `$var`, validator, order of posing, references, lowering, packing, write-back, the generator of `init_scene.rs`) is
  this repository's own since 21/11 (the code common to the compiler and dev.builder lives here): moved from
  `core/scene-format` of the kits SDK repository `gripforgeai/module-kits` (`src/`, `tests/`), whose last copy here
  was taken at module-kits `849c4b80619a8e6f7b194b0712e0c07542f7bca4`; the kits repository imports it by git at a pinned
  commit and no longer holds the crate. Its JSON schemas (`schema/`) stayed in the kits repository (assembled with the
  kits' sections), which checks the syntax schema against this crate's registry. The fixtures its tests read outside
  the crate (`tests/base-room/configs/includes*.gripforge`, `kits/core_physics/kit.api.json`) and the parity cases
  (`tests/parity`, from `tools/parity/cases` of the kits repository) were copied at the same revision and are now this
  repository's fixtures. History of the former copy: first moved from the kits repository at
  `0c88723ab65b96093b0c5d0a9458855c187e6fa2`, taken again by `tools/sync-scene-format` (retired) until `849c4b80`.
- `crates/sources-digest` (the digest of the game's code) is this repository's own, the one copy of the function: moved
  from `core/scene-format/src/sources_digest.rs` and `core/scene-format/tests/sources-digest.json` of the kits
  repository (by way of this repository's former copies `src/sources_digest.rs` and `tests/sources-digest.json`), its
  header rewritten. The kits repository imports it by git at a pinned commit (decision of 21/11: the common code lives
  here); `crates/scene-format` re-exports it.
- `src/core.rs` and `src/strict.rs` come from the same repository, `tools/compiler/wasm/src` (commit
  `837cb5e19dbed150b4437e464ecdecbf93fc8511`), without their wasm surface; the rest of `src/` rewrites in Rust the
  JavaScript of `tools/compiler` (`lib/`), and `tests/` its tests (golden files kept, see the pull request).
- `examples/room` is the dev.builder room of the kits SDK (one `project.gripforge`) with a frozen copy of the
  `kit.api.json` of the kits at the same commit. The accessor of each kit (`kits/<kit>/access/rust/`) is written from that
  frozen `kit.api.json` by the generator of the kits SDK (`tools/kit-api/langs/rust_facade.mjs`, branch
  `feat/kit-accessors-published`), as the kit's package ships it. `kits/core_scene` (its `kit.api.json` and
  `access/rust/`, unchanged) was taken later, at module-kits `b9a1d631e4619b93a84ee9442b3828de6aff21c1`: game.engine needs
  core.scene on the Bevy players, and the project lists every kit its kits need (`kit_required`, docs/kit-loading.md).
- `sdk/` (the public part of the kits SDK: `abi/`, `kit-guest/`, `kit-access/`, `contracts/` and `rust-toolchain.toml`) is copied by
  `tools/sync-sdk` from `gripforgeai/module-kits` (`core/abi`, `core/kit-guest`, `core/kit-access`, `core/contracts`,
  `rust-toolchain.toml`) at the commit of `sdk/sdk.json` (`67a42985d32fc477b34069a9842a7a987eee4a72` at this writing), unchanged but for
  `kit-access/Cargo.toml`, which loses its `kithost` feature (the kit host's side, `gripforge-kit-api`, stays in the
  kits repository). `sdk/Cargo.toml` (their workspace), `sdk/README.md` and `sdk/sdk.json` are this repository's own.
  `tools/sync-sdk <module-kits checkout> --check` says whether `sdk/` is still exactly that copy.
