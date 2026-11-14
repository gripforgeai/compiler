# Provenance

The code of this repository comes from the kits SDK repository `gripforgeai/module-kits`, folder `tools/compiler`,
at commit `837cb5e19dbed150b4437e464ecdecbf93fc8511` (branch `feat/gripforge-compiler`). It was moved here without its
history, then adapted to live on its own:

- the Rust core (`wasm/`) is its own Cargo workspace and reads the common core of the kits (`core/scene-format`: the
  validator, the packing and the generator of `init_scene.rs`) from the kits SDK at the revision of `sdk.json`,
  never copied (`npm run sdk`);
- the tests read the kits SDK from `.sdk/module-kits` instead of the folders around them;
- `lib/compat.mjs` (the `kits_incompatible` check), `scripts/sdk.mjs`, `scripts/release.mjs`, `scripts/lint.mjs` and
  the workflows are new.

`tools/compiler` was removed from module-kits in the same change; what belongs to the kits (the dev.builder room as one
`project.gripforge`, the end-to-end test `kits/dev_builder/tests/compiled.rs`) stays there.
