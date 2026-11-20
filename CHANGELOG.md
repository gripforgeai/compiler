# Changes

## [Unreleased] (0.1.2)

- `$schema` (the path of the published schema, for the editors) is accepted at the head of a project and of a scene
  file: `check` and the build pass, the key is kept as written and never read (decision D03 of
  gripforgeai/gripforge#420). Taken from module-kits (`core/scene-format`, see `PROVENANCE.md`).
- **The game's code is declared by the project** (decision of gripforgeai/gripforge#420, announced on
  gripforgeai/compiler#10): `"sources": ["rules.rs", …]` in `project.gripforge`, paths relative to the project file.
  The build adds each file to the game's module `game.main` (`.build/game`, taken by path, never copied nor written:
  `crate::<name of the file>`), so the same code runs native and in the official web player (WebGPU, WebGL2). One
  source declares the module of the game (`pub struct X;` with `impl KitModule for X`); the generated `module.rs` hands
  it every hook of game.main (`setup`, `ready`, `update`, `on_event`, `scene_enter` once the scene is posed, …). No file
  is taken by convention any more: the `index.rs` beside the project (a Bevy plugin, compiler#10) and `src/` are not
  read, and `.build/app/` (the Bevy game around it) is gone: the game plays in the GripForge players, which load
  game.main beside the kits. New codes: `source_missing`, `source_outside`, `source_duplicate`, `source_not_rust`,
  `sources_invalid` (the format, by the common core), `source_name_invalid`, `source_unreadable`,
  `source_mod_undeclared`, `sources_module_missing`, `sources_module_ambiguous`, and the warning `source_undeclared`
  (a `.rs` beside the project that `sources` does not list). Gone: `index_plugin_missing`, `index_plugin_ambiguous`,
  `src_module_invalid`, `gateway_by_git`. The rule and when each hook runs: [docs/sources.md](docs/sources.md).
- The generated module of `game.main` is `src/module.rs` (was `src/index.rs`).
- `tests/accept/fps-shooter/run.sh`: the template fps-shooter (its scenes in `project.gripforge`) built with a declared
  source, `game.main` compiled with it and played headless (by hand, on a build machine).

## [0.1.1]

- The generated crate takes the public part of the kits SDK (`gripforge-abi`, `gripforge-kit-guest`,
  `gripforge-kit-access`) from this repository, `sdk/` (copied from module-kits by `tools/sync-sdk`, revision in
  `sdk/sdk.json`, see `PROVENANCE.md`), by git at the compiler's tag (`tag = "v0.1.1"`): a game builds without any
  access to the kits repository. `--sdk <dir>` takes it by path (this repository's `sdk/` or a kits checkout);
  `--sdk-rev` or `GRIPFORGE_SDK_REV` another tag, commit or branch. The CI generates the example room and builds it,
  native and wasm, on `sdk/` alone.
- The spec of an element of a node passes what dev.builder passes: `type`, `asset`, `archetype`, `entity` and, of its
  parent node, `parentNode`, `parentType`, `parentEntity`, `parentAsset`, `parentArchetype`: a collision or an
  animation under its parent works built. Typed nodes (collision, animation) reach the generated code.
- A model named by its Library id reaches the generated code by that id.
- The header of the generated scenes says to run `gripforge-build <project>`.

## [0.1.0]

- `gripforge-build`: one native binary. Reads `project.gripforge` and the `kit.api.json` of the kits of the project,
  checks it strictly, generates `game.json`, `init_scene.rs` per scene, `index.rs` and the crate of the game's module.
  Compiles nothing.
- The spec of an element of a node passes what dev.builder passes: `type`, `asset`, `archetype`, `entity` and, of its
  parent node, `parentNode`, `parentType`, `parentEntity`, `parentAsset`, `parentArchetype` (fields of the accessors'
  `Spec`, the literal ends with `..Default::default()`): a collision or an animation under its parent works built.
- Errors for an AI: stable codes, file › scene › element › property, what is expected; `--check`, `--json`.
- A kit missing from the project says which one (and its version) and how to install the kits.
- The generated crate calls each kit through the accessor the kit ships in its folder (`<kit>/access/rust`,
  `gripforge-kit-access-<kit>` at the kit's version), by a local path; `access_missing`, `access_mismatch`.
- `--kits-path <dir>` (was `--kits`): the kits, and their accessors, from another folder (the sources of the kits, for
  their demos).
- `--compress-textures` / `--no-compress-textures` / `--compress-textures-url`: the GLB without `KHR_texture_basisu`
  are sent to the public texture route, the compressed copy is written in `<out>/textures/` (cache by sha256), the
  gains are in the report (`textures`). A failure is a warning, the build goes on. Off by default. New dependencies:
  `ureq` (rustls), `sha2`, `base64`.

- The generated crate takes the kits SDK of module-kits at `108031bf9f6ca9a5ea811a55058c3600b2c5227d` (accessors in the
  kits' folders, `Spec` with the node's kind and its parent); `--sdk-rev` or `GRIPFORGE_SDK_REV` change it. The header of the generated scenes says to run
  `gripforge-build <project>`.
