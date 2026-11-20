# Provenance

- `crates/scene-format` (the common core of the project format: reading, resolution of `includes` / node variables /
  `$var`, validator, order of posing, references, lowering, packing, write-back, the generator of `init_scene.rs`) is a
  copy, unchanged, of `core/scene-format` of the kits SDK repository `gripforgeai/module-kits` (`src/`, `tests/`,
  `schema/`), made by `tools/sync-scene-format` at the revision of `crates/scene-format/scene-format.json`. Only its
  `Cargo.toml` (no workspace inheritance) and `scene-format.json` are this repository's own. The same tool copies the
  fixtures its tests read outside the crate, at the same paths from the root (`tests/base-room/configs/includes*.gripforge`,
  `kits/core_physics/kit.api.json`), and the parity cases with their expected resolution (`tools/parity/cases` of the
  kits repository → `tests/parity`). `tools/sync-scene-format <module-kits checkout> --check` says whether they are still
  exactly that copy; the CI of the kits repository checks it against this repository's `main` (tools/parity there).
  History: first moved from the kits repository at `0c88723ab65b96093b0c5d0a9458855c187e6fa2`, partly taken again at
  `6da0bb9650c1fa6221fff448b9a52f05e0c1acad` and `108031bf9f6ca9a5ea811a55058c3600b2c5227d` and `b2b670825edc57305de74b1c83b001be35d91d22` (`$schema`) with local changes; the
  local changes and the `gfscene` tool are gone since the copy became whole again (the kits repository retired
  `gfscene` and `.gfscene` in `9cbb3a5d9`).
- `src/sources_digest.rs` and `tests/sources-digest.json` are copies, unchanged, of `core/scene-format/src/sources_digest.rs`
  and `core/scene-format/tests/sources-digest.json` of the kits repository at commit
  `7a06355a907d3ea15018c3f3d7fdd184082e67d0` (branch `feat/devbuilder-game-main`, module-kits#1862): the digest of the
  game's code. They leave for `crates/scene-format` at the next `tools/sync-scene-format` once that branch is merged
  (the revision of `scene-format.json`, `6b433125a`, does not hold them yet).
- `src/core.rs` and `src/strict.rs` come from the same repository, `tools/compiler/wasm/src` (commit
  `837cb5e19dbed150b4437e464ecdecbf93fc8511`), without their wasm surface; the rest of `src/` rewrites in Rust the
  JavaScript of `tools/compiler` (`lib/`), and `tests/` its tests (golden files kept, see the pull request).
- `examples/room` is the dev.builder room of the kits SDK (one `project.gripforge`) with a frozen copy of the
  `kit.api.json` of the kits at the same commit. The accessor of each kit (`kits/<kit>/access/rust/`) is written from that
  frozen `kit.api.json` by the generator of the kits SDK (`tools/kit-api/langs/rust_facade.mjs`, branch
  `feat/kit-accessors-published`), as the kit's package ships it.
- `sdk/` (the public part of the kits SDK: `abi/`, `kit-guest/`, `kit-access/` and `rust-toolchain.toml`) is copied by
  `tools/sync-sdk` from `gripforgeai/module-kits` (`core/abi`, `core/kit-guest`, `core/kit-access`,
  `rust-toolchain.toml`) at the commit of `sdk/sdk.json` (`e6d744cb9160f723f0936f0e4880a3b55265ac86` at this writing), unchanged but for
  `kit-access/Cargo.toml`, which loses its `kithost` feature (the kit host's side, `gripforge-kit-api`, stays in the
  kits repository). `sdk/Cargo.toml` (their workspace), `sdk/README.md` and `sdk/sdk.json` are this repository's own.
  `tools/sync-sdk <module-kits checkout> --check` says whether `sdk/` is still exactly that copy.
