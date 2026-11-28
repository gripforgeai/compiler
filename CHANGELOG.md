# Changes

## [Unreleased] (0.1.2)

- **A path of a project may start from the root** (`#kits/…`, `#tools/…`, `#core/…`, `#tests/…`; decision of Damien,
  28/11, module-kits: no `../../../`): `crates/scene-format/src/path.rs` `ROOT_ALIASES` (the table of the kits
  repository's package.json `imports`), `root_alias`, `root_alias_error` and `on_disk` (the one rule of a reader of a
  disk: the nearest folder at or above the project whose package.json maps `#<name>/*` in `imports`, to its target, else
  the project's folder). An include (`includes[].file`) or a scene file written with an alias starts from the root
  whatever the file that names it; the resolution keeps the alias in the paths it asks for (`missing`), a relative path
  inside a file reached by an alias stays under it, and an unknown alias or a climb above it is `include_missing` (the
  hint names the aliases); a project path (`project_path_error`: a sub-scene, an asset) accepts a known alias.
  `gripforge-build` reads them from the root (`project::included_files`, `scene_files`). A project without an alias
  resolves byte for byte as before.

- **The kits a game starts with, checked once for everyone** (`crates/scene-format/src/kits.rs`, decision of Damien
  « chargement des kits au démarrage », module-kits `docs/dependances.md`): `kits::check_required(installed, apis,
  target)` gives one `kit_required` error per kit a listed kit needs and the project does not list (its `requires`,
  its dependencies for the target; never a recommendation nor an optional one; `anyOf` met by one of its kits; a
  capability met by a kit that `provides` it or by the host, `runtime.core`); `kits::required_message` makes the one
  message that names every lack. The web gateway (through module-kits `core/scene-format-wasm`), the native gateway,
  dev.builder and gripforge-build call it; nothing changes yet for a user of `gripforge-build`.

- **The format of a project has one copy, here** (`crates/scene-format`, crate `gripforge-scene-format`; same decision):
  it is no longer a copy of module-kits `core/scene-format`, which imports it by git at a pinned commit (dev.builder,
  game.engine and the wasm of the development mode call it). `tools/sync-scene-format` and
  `crates/scene-format/scene-format.json` are gone; a change to the format is made here first (its pull request), then
  module-kits raises its pin. The JSON schemas (`schema/`) leave the crate: the kits repository keeps and assembles them
  and checks the syntax schema against the registry (the test moved there). The fixtures the crate's tests read
  (`tests/base-room/configs/includes*.gripforge`, `kits/core_physics/kit.api.json`, `tests/parity`) are now this
  repository's own. Nothing changes for a user of `gripforge-build`.

- **The digest of the game's code has one copy** (`crates/sources-digest`, crate `gripforge-sources-digest`; decision of
  Damien, 21/11: the code common to the compiler and dev.builder lives in this public repository, module-kits imports
  it by git at a pinned commit): `src/sources_digest.rs` and `tests/sources-digest.json` are gone, the generated crate
  gets the crate's file (`SOURCE`), the common core of the format re-exports the crate (`sources_digest`). The digest
  values do not change; the comment at the top of the generated `src/sources_digest.rs` does.

- **The embedded SDK follows the ABI by contract** (module-kits#1865): `sdk/` is taken again from module-kits `67a42985` and gains `gripforge-contracts` (`tools/sync-sdk` copies `core/contracts`). A `game.main` built by the previous SDK had no section `gripforge.contracts` and the host refused it (`contract_missing`); the CI now builds the `--dev` module to wasm and checks the section.

- **The game's code in the development mode** (decision of Damien, voie B of module-kits
  `docs/design/sources-du-jeu.md`): `gripforge-build --dev` writes `.build/dev/game`, the module of the game's code
  only (every hook, `scene_enter` and `scene_exit` included, to the module of the game; no `init_scene`: dev.builder
  poses the scenes), which the CLI compiles and the development mode loads like any kit module, on the web too. Both
  module forms keep the digest of the files they compile (`src/sources_digest.rs`, the common core's function (copied from module-kits),
  computed at compile time; the custom section `gripforge.sources` of the wasm, the symbol `GRIPFORGE_SOURCES` of the
  native library): the development mode refuses a module of other files (`game_main_stale`) or none
  (`game_main_missing`). New code: `dev_without_sources`.

- `crates/scene-format` is now an unchanged copy of `core/scene-format` of the kits repository (`tools/sync-scene-format`,
  revision in `crates/scene-format/scene-format.json`, see `PROVENANCE.md`): dev.builder, devGame, the validator of
  the kits and gripforge-build read a `project.gripforge` with the same code. It brings `includes`, the variables of
  nodes, `$var`, `data.scenes[].enabled`, `$schema`, the scenes' write-back, and drops what the kits repository
  retired: the `gfscene` tool and its `.gfscene` schema (a game is one `project.gripforge`).
- A project's included `.gripforge` files are read beside it, as the development mode reads them (the resolution called
  again with the files it asks for); the strict rules, the pack and the generated code read the project resolved.
- `--resolve`: the project resolved and the tolerant validator's issues as JSON (`ok`, `project`, `resolveIssues`,
  `issues`), nothing written, nothing promoted. The parity test (`tests/parity.rs`, cases copied from the kits
  repository) compares it with the kits repository's reference reader.
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
