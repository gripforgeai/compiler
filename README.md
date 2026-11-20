# gripforge-build

**`gripforge project build`: generate the Rust of a GripForge game.**

`gripforge-build` reads a GripForge project (`project.gripforge`) and the `kit.api.json` of the kits installed in it,
checks it strictly with the common core of the project format, and **generates** the Rust of the game: one
`init_scene.rs` per scene that calls the kits through their typed accessors, and the crate of the game's module around
them, with **the game's code that `project.gripforge` declares in `sources`** added to that module. It compiles nothing:
the game's own build does. What the generated game does is what the development mode (`dev.builder`)
shows.

## Install

`gripforge-build` is one native binary, with no runtime to install. Download the archive of your platform from the
[latest release](https://github.com/gripforgeai/compiler/releases/latest) (the GripForge page
[gripforge.ai/compiler](https://gripforge.ai/compiler) lists them too), check it against `SHA256SUMS`, unpack it and put
`gripforge-build` in your `PATH`:

| Platform | Archive |
|---|---|
| Linux x64 | `gripforge-build-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `gripforge-build-X.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x64 | `gripforge-build-X.Y.Z-x86_64-pc-windows-msvc.zip` |
| macOS x64 (Intel) | `gripforge-build-X.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS arm64 (Apple silicon) | `gripforge-build-X.Y.Z-aarch64-apple-darwin.tar.gz` |

```sh
v=0.1.0; t=x86_64-unknown-linux-gnu        # your version and platform
curl -LO https://github.com/gripforgeai/compiler/releases/download/v$v/gripforge-build-$v-$t.tar.gz
curl -LO https://github.com/gripforgeai/compiler/releases/download/v$v/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS   # macOS: shasum -a 256 -c SHA256SUMS
tar -xzf gripforge-build-$v-$t.tar.gz && ./gripforge-build-$v-$t/gripforge-build --version
```

On Windows, unpack the `.zip` and run `gripforge-build.exe --version`.

## Use

```sh
gripforge-build path/to/my-game              # check, then generate the Rust of the game into my-game/.build/
gripforge-build path/to/my-game --check      # the strict check only, nothing written
gripforge-build path/to/my-game --dev        # the module of the game's code only, for the development mode (.build/dev/game)
gripforge-build path/to/my-game --resolve    # the project resolved (includes, variables, $var) and the tolerant issues, JSON
gripforge-build path/to/my-game --json       # the report gripforge.build-report/1 on the standard output (for a tool or an AI)
gripforge-build path/to/my-game --compress-textures   # also a KTX2 copy of each GLB, by the public texture route (below)
gripforge-build path/to/demo --kits-path path/to/module-kits/kits   # a demo of the kits repository, kits read from the sources
```

| Option | What it does |
|---|---|
| `<project>` | `project.gripforge`, or its folder (`project.gripforge`, else the only `*.gripforge`); default the current folder |
| `--out <dir>` | where the generated files go (default `.build/` beside the project) |
| `--kits-path <dir>` | take the kits (`<kit>/kit.api.json` and `<kit>/access/rust/`) from this folder instead of the kits downloaded into the project: the `kits/` of the sources of the kits, for their demos (below); default: found in the project |
| `--sdk <dir>` | a local SDK: the generated crate takes its SDK crates (`gripforge-kit-guest`, `gripforge-kit-access`) by path, from this repository's `sdk/` (crates at its top) or a checkout of the kits repository (under `core/`) |
| `--sdk-rev <tag, commit or branch>` | without `--sdk`: the revision of this repository whose `sdk/` the generated crate takes by git (default the compiler's tag, `v<version>`) |
| `--check` | the strict check only |
| `--dev` | the crate of the game's code for the development mode of the kits (`dev.builder` plays `project.gripforge` and poses the scenes): `<out>/dev/game`, the files of `sources` only, no `init_scene`, no `game.json`; refused without `sources` (`dev_without_sources`). [docs/sources.md](docs/sources.md#in-the-development-mode) |
| `--resolve` | the project resolved by the common core (its included `.gripforge` files read beside it, the variables of its nodes, its `$var`) and the tolerant validator's issues, as JSON (`ok`, `project`, `resolveIssues`, `issues`); nothing written, nothing promoted |
| `--lenient` | the warnings of the tolerant validator stay warnings (like the development mode) |
| `--json` | the JSON report instead of lines |
| `--compress-textures` | sends the GLB without a compressed copy to the public texture route (see below); off by default |
| `--no-compress-textures` | no texture compression (the default; cancels an earlier `--compress-textures`) |
| `--compress-textures-url <url>` | the route (default `https://gripforge.ai/api/v1/public/textures/compress`) |
| `--version`, `--help` | the version, the usage |

Exit code: `0` generated (or checked), `1` the project has errors (nothing is generated), `2` wrong command line.

**It compiles nothing.** It generates Rust; the game's own build compiles it (`cargo build` in `.build/game`, native or
`wasm32-unknown-unknown`, like any kit module).

## The game's code: declared in `project.gripforge`

**`project.gripforge` is the only entry of a game: it declares its own Rust files, in `sources`.** No file is taken by
convention (no `index.rs`, no `src/`). Each declared file is added to the game's module `game.main` (`.build/game`), by
path, never written: the same code native and in the official web player (WebGPU, WebGL2).

```json
"sources": ["rules.rs", "code/weapons.rs"]
```

```
my-game/
  project.gripforge   the scenes, the kits, their settings, and "sources"
  rules.rs            yours, declared: crate::rules (one source declares the module of the game)
  code/weapons.rs     yours, declared: crate::weapons
  .build/             generated by gripforge-build, never versioned
```

- One source declares **the module of the game**: `pub struct MyGame;` with `impl KitModule for MyGame`
  (`gripforge_kit_guest`). The generated `module.rs` hands it every hook of game.main: `setup` (at the mount, **before
  the first scene**), `ready`, `update` at the tick, `on_event`, `scene_enter` once the scene is posed, …
- Paths are relative to the project file, with `/`, each once, inside the project. A missing file, a path outside the
  project, a file listed twice, a bad module name, a `mod x;` toward an undeclared file, no or several modules of the
  game: an error that says which and what to write, at `--check` too. A `.rs` beside the project that `sources` does
  not list: the warning `source_undeclared` (not compiled).
- The rule, every error code and **when each hook runs** (before the first scene, the development mode):
  [docs/sources.md](docs/sources.md).

```sh
gripforge-build my-game --sdk path/to/module-kits
cargo build --manifest-path my-game/.build/game/Cargo.toml                                    # game.main, native
cargo build --release --target wasm32-unknown-unknown --manifest-path my-game/.build/game/Cargo.toml   # and wasm
```

The game plays in the GripForge players, which load `game.main` beside the kits (on the web, the CLI builds the
relocatable wasm and `.build/web/kit-modules.json`). `tests/accept/fps-shooter/run.sh <module-kits checkout>` is the
acceptance on the template `fps-shooter`: its scenes in `project.gripforge`, a declared source, `game.main` built, the
game played headless, the sha256 of the source the same before and after. Heavy (Bevy and the kits): a build machine,
not the CI.

## The kits of a project

The compiler reads the `kit.api.json` of each kit the project uses, **from the project itself**: the kits are
downloaded into the project by the GripForge CLI (`gripforge game-attach <project folder>`), at the version
`project.gripforge` declares. It looks, in this order:

1. `--kits-path <dir>`;
2. `<project folder>/kits/<kit>/kit.api.json`;
3. `<project folder>/.gripforge/kits/<kit>/kit.api.json`;
4. a `kits/` folder above the project (a workspace holding several games).

The folder of a kit can have any name: the compiler reads the id and the version in its `kit.api.json`. It never
downloads anything. A kit the project installs or requires that is not there is an error that says which one and how to
install it:

```text
error kit_unknown — project.gripforge › property installed: the kit world.nowhere version 1.0.0 is not in the project: no kit.api.json for it among the kits of the project, so no accessor to call it. Expected: install the kits of the project (gripforge game-attach <project folder>), or give the folder of the kits with --kits-path <dir> (<kit>/kit.api.json); …
```

A `kit.api.json` of another schema than `gripforge.kit-api/1` is refused with `kits_incompatible`.

[`examples/room`](examples/room) is a project laid out this way: `project.gripforge` and its `kits/` (each with its
`kit.api.json` and its `accessor/`).

### The accessors of the kits

The generated code calls each kit through its **Rust accessor**: a crate the kit ships **in its own folder**, beside its
`kit.api.json` (`<kit>/access/rust/Cargo.toml` and `src/lib.rs`), named `gripforge-kit-access-<kit>` (`core.light` →
`gripforge-kit-access-core-light`) at **the version of the kit**. It is the Rust accessor of the kit, in the kit's
package like its `kit.api.json`: no registry, nothing downloaded by the compiler, and the compiler never writes a accessor.

For each kit of the built game (`dev.builder` is not one), the crate depends on the accessor found in the folder the
kit was read from, by a path relative to `.build/game`, at the exact version of the kit:

```toml
gripforge-kit-access-core-light = { path = "../../kits/core_light/access/rust", version = "=1.0.0" }

[patch.crates-io]
gripforge-kit-access = { git = "https://github.com/gripforgeai/compiler", tag = "v0.1.1" }  # or the --sdk path
```

The accessors ask for the common part (`gripforge-kit-access`: `Host`, `Spec`, `Posed`) by version only; the
`[patch.crates-io]` of the generated crate makes it the SDK's, the same crate the game itself uses (`--sdk`,
`--sdk-rev`). A kit without its accessor (`access_missing`) or with a accessor of another name or version
(`access_mismatch`) stops the build before anything is written (and fails `--check`), and says what to do.

### The SDK of the game's crate (public)

The game's crate compiles against the public part of the kits SDK: `gripforge-kit-guest` (the kit side of the host),
`gripforge-kit-access` (the common part of the accessors) and their contract `gripforge-abi`. They are in this
repository, [`sdk/`](sdk/README.md), copied from the kits repository at the revision of `sdk/sdk.json`
(`tools/sync-sdk` copies them, or checks the copy). By default the crate takes them by git from this repository at the
compiler's tag, so a game builds with nothing but this public repository and the kits downloaded into the project:

```toml
gripforge-kit-guest = { git = "https://github.com/gripforgeai/compiler", tag = "v0.1.1" }
gripforge-kit-access = { git = "https://github.com/gripforgeai/compiler", tag = "v0.1.1", features = ["guest"] }
```

`--sdk <dir>` takes them by path, `--sdk-rev` (or `GRIPFORGE_SDK_REV`) another revision, `GRIPFORGE_SDK_GIT` another
repository. The CI builds the example room this way, native and wasm, without any access to the kits repository.

**Demos of the kits repository**: they build from the sources, not from downloaded kits. `--kits-path <module-kits>/kits`
makes the compiler read every kit, and so its accessor, in the sources (`kits/<kit>/kit.api.json`,
`kits/<kit>/access/rust/`); without it nothing changes (the kits downloaded into the project).

```sh
gripforge-build path/to/demo --kits-path path/to/module-kits/kits --sdk path/to/module-kits
```

## Texture compression (`--compress-textures`)

The compiler only CALLS the public route of GripForge (no key); the compression itself (KTX2 / Basis) is done by the
service, nothing of it is in this repository. A local integration may come later.

During the build, each `*.glb` of the project (hidden folders, `target`, `node_modules` and the output folder left out)
that does not declare `KHR_texture_basisu` is sent as `{ glbBase64 }`; the answer's `glbBase64` is written as
`<out>/textures/<path>.ktx2.glb`. The original is never touched. The copy is also kept by the sha256 of the original in
`<out>/textures/.cache/`: a GLB already compressed is not sent again.

The report (`build.json`, `--json`) gets a `textures` object (`gripforge.texture-compress/1`): per file the status, the
file gain (`bytesBefore/After`) and the GPU memory gain (`gpuBytesBefore/After`), and the totals. A network or server
failure, a file over 32 MB or `applied:false` is a warning (`textures_service_failed`, `textures_too_big`,
`textures_not_applied`, `textures_write_failed`): the build goes on with the original GLB.

It is off by default: the build is otherwise offline and deterministic, and sending the files of a project to a service
is a choice to make, not a surprise. HTTP client: `ureq` (rustls).

## What it writes

| File | Content |
|---|---|
| `.build/game.json` | what the player loads: the project packed, its scenes lowered for `game.engine` (no scene file read at run time), `dev.builder` left out, the game's module `game.main` last in the lock |
| `.build/game/` | the crate of the game's module: `Cargo.toml`, `kit.api.json`, `module.json`, `src/lib.rs`, `src/module.rs` (hands the hooks to the module of the game when `sources` declares one), the files of `sources` by path from `src/lib.rs`, `src/init_scene.rs` and `src/init_scene/<scene>.rs` |
| `.build/build.json` | the report: issues, steps and their times, files written (and `textures` with `--compress-textures`) |
| `.build/textures/` | only with `--compress-textures`: `<path>.ktx2.glb`, the compressed copy of each GLB (the originals are never touched) |

Never edit `.build/` by hand: change `project.gripforge` and generate again.

## Errors

A build accepts no guess: every warning of the tolerant validator becomes an error (`promoted: true`), except the nodes
that are only read (`sound_not_placed`, `hud_not_placed`, `light_not_placed`, `camera_not_placed`). Each problem names
**the file, the scene, the element, the property, what is wrong and what is expected**:

```text
error section_field_unknown — project.gripforge › scene level › property kits.core.light.brightness: core.light.ambience has no property brightness: … Expected: use one of exposure, preset, seconds, tonemapping
error prop_out_of_range — project.gripforge › scene level › property kits.core.light.exposure: … Expected: …
```

`$schema` at the head of a project or of a scene file (the path of the published schema, for the editors) is accepted:
kept as written, never read, never an error.

| Code | When |
|---|---|
| `kit_unknown` | an installed or required kit has no `kit.api.json` in the project |
| `kits_missing` | the project has no kits folder, or `--kits-path` names none |
| `access_missing` | a kit of the built game has no `accessor/Cargo.toml` in its folder |
| `access_mismatch` | the accessor in a kit's folder is not `gripforge-kit-access-<kit>` at the kit's version |
| `kits_incompatible` | a `kit.api.json` of another schema |
| `file_not_json`, `project_missing` | the project or a file cannot be read, with the line and column |
| `generator_missing`, `generator_failed` | the generator refuses the project |
| `write_failed` | the output folder cannot be written |
| `source_missing`, `source_outside`, `source_duplicate`, `source_not_rust`, `sources_invalid`, `source_name_invalid`, `source_unreadable`, `source_mod_undeclared` | a file of `sources` that cannot be a module of game.main ([the table](docs/sources.md#errors)) |
| `sources_module_missing`, `sources_module_ambiguous` | no source declares the module of the game (`pub struct X;` + `impl KitModule for X`), or several |
| `source_undeclared` (warning) | a `.rs` beside the project that `sources` does not list: not compiled |
| the codes of the project format | `section_field_unknown`, `prop_invalid`, `prop_out_of_range`, `scene_unknown`, `scene_file_missing`, `scene_cycle`, `scene_empty`, `kit_required_missing`, `kit_version_mismatch`, `name_duplicate`, `node_type_unknown`… |

## Develop

```sh
cargo test --workspace                 # unit, golden and example-room tests; no network
UPDATE_GOLDEN=1 cargo test --test build   # rewrites tests/golden/ when the generated code changes on purpose
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
```

| Part | Role |
|---|---|
| `crates/scene-format` | the common core of the project format: tolerant reading, validator, sub-scenes, lowering for game.engine, packing, the generator of `init_scene.rs` (`code.rs`): an unchanged copy of `core/scene-format` of the kits repository (`tools/sync-scene-format <module-kits checkout> [--check]`, revision in `crates/scene-format/scene-format.json`) |
| `src/core.rs`, `src/strict.rs` | the core of a build: the strict rules, the packing of the built game, the call of the generator |
| `src/project.rs` | reads the project, the scene files it still names, the kits of the project |
| `src/crate_gen.rs` | the crate of the game's module |
| `src/sources.rs` | the game's code of `sources`: the files on the disk, the module of the game |
| `src/build.rs`, `src/cli.rs`, `src/report.rs`, `src/json.rs` | the command, its lines and its JSON report |
| `tests/` | `check.rs` (each way to break the room, one located error), `build.rs` (golden files, messages of a broken project, reading, command line), `core.rs` (the ops and the strict rules), `sources.rs` (the game's code of `sources`), `game-code/rules.rs` (an example built by the CI), `accept/fps-shooter` (the acceptance, by hand) |
| `examples/room` | the example project and the `kit.api.json` of its kits, frozen |

## Releases

A tag `vX.Y.Z` (the version of `Cargo.toml`, on `main`) releases that version. The workflow builds `gripforge-build` on
a runner of each OS (Linux x64 and arm64, Windows x64, macOS x64 and arm64), runs each binary on its own runner
(`--version`, then `--check` on the example room), and creates the GitHub release with the `GITHUB_TOKEN` alone: no
secret, no registry. An existing release stops the job: a version is never rewritten.

| File | Content |
|---|---|
| `gripforge-build-X.Y.Z-<target>.tar.gz` / `.zip` | the binary, `LICENSE`, `README.md`, one archive per platform |
| `SHA256SUMS` | the sha256 of the archives and of `release.json` |
| `release.json` | what the release holds (below) |

The notes are the message of the annotated tag (else the version's section of `CHANGELOG.md`), then the sums. Tools and
download pages list the versions with the public releases API (`GET https://api.github.com/repos/gripforgeai/compiler/releases`,
newest first; `…/releases/latest` for the newest) and read the `release.json` asset of a release for the details.

### `release.json`

```json
{
 "schema": "gripforge.compiler-release/2",
 "version": "0.1.0",
 "tag": "v0.1.0",
 "date": "2026-11-20T09:58:12Z",
 "commit": "<commit of the tag>",
 "source": "https://github.com/gripforgeai/compiler/tree/v0.1.0",
 "binary": "gripforge-build",
 "platforms": [
  {
   "platform": "linux-x64", "os": "linux", "arch": "x64", "target": "x86_64-unknown-linux-gnu",
   "name": "gripforge-build-0.1.0-x86_64-unknown-linux-gnu.tar.gz",
   "url": "https://github.com/gripforgeai/compiler/releases/download/v0.1.0/gripforge-build-0.1.0-x86_64-unknown-linux-gnu.tar.gz",
   "size": 2951234,
   "sha256": "<hex>"
  }
 ]
}
```

| Field | Meaning |
|---|---|
| `schema` | `gripforge.compiler-release/2`; a breaking change of this format changes the number |
| `version`, `tag`, `commit`, `source` | the version, its git tag, its commit and its source tree |
| `date` | the date of the tag's commit (ISO 8601, UTC) |
| `binary` | the name of the command in each archive (`gripforge-build`, `gripforge-build.exe` on Windows) |
| `platforms[]` | one entry per platform, five in all: `linux-x64`, `linux-arm64`, `windows-x64`, `macos-x64`, `macos-arm64` |
| `platforms[].name`, `url`, `size`, `sha256` | the archive: file name, download URL, size in bytes, sha256 |
| `platforms[].os`, `arch`, `target` | the platform (`linux`, `windows`, `macos`; `x64`, `arm64`) and its Rust target triple |

## License

MIT
