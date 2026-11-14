# @gripforge/compiler

**`gripforge project build`: compile a GripForge game.**

The compiler reads a GripForge project (`project.gripforge`), checks it strictly with the same core as the GripForge
kits, writes one Rust `init_scene.rs` per scene that calls the kits through their typed façades, and compiles the
game's own module **native** and **wasm**. The kits are not linked into the game: they stay modules the player loads.
What a compiled game does is what the development mode (`dev.builder`) shows.

## Install

The compiler is downloaded from the [GripForge](https://gripforge.ai) site, which lists every version. Each version is
one archive that npm installs by its URL:

```sh
npm install -g https://<download link of the version>/compiler-0.1.0.tgz
gripforge-build --version
```

It needs Node.js 20 or newer. To compile (not only to check), it also needs Rust (`rustup`), with the target
`wasm32-unknown-unknown` and the component `rust-src` for the web build, and the GripForge kits SDK (below).

## Use

```sh
gripforge-build path/to/my-game                      # check, generate, compile native and wasm into my-game/.build/
gripforge-build path/to/my-game --check --json       # the strict check only, a JSON report for a tool or an AI
gripforge-build path/to/my-game --target native --sdk path/to/module-kits
```

| Option | What it does |
|---|---|
| `<project>` | `project.gripforge`, or its folder (`project.gripforge`, else the only `*.gripforge`) |
| `--out <dir>` | where the build goes (default `.build/` beside the project) |
| `--kits <dir>` | the catalog of the kits (`<kit>/kit.api.json`); default the `kits/` of a kits SDK checkout above the project, else the catalog the package carries |
| `--sdk <dir>` | a kits SDK checkout (crates `core/kit-guest`, `core/kit-facade`, the wasm recipe); default the one above the project |
| `--sdk-rev <commit or branch>` | without a checkout: the revision of the kits SDK the game's crate takes by git (default: the one this compiler is made for) |
| `--target native\|wasm\|all` | what is compiled (default `all`); wasm needs `--sdk` (its recipe) |
| `--no-compile` | writes `game.json` and the crate, no cargo |
| `--check` | the strict check only, nothing written |
| `--lenient` | the warnings of the tolerant validator stay warnings (like the development mode) |
| `--jobs <n>`, `--target-dir <dir>` | cargo `-j` (default 4, always at low priority) and its build folder, to keep between builds |
| `--json` | the report `gripforge.build-report/1` on the standard output |

Exit code: `0` built (or checked), `1` the project has errors (nothing is built), `2` wrong command line.

From JavaScript (the GripForge CLI and MCP call it this way):

```js
import { build } from '@gripforge/compiler';
const result = await build({ project: 'my-game', targets: ['native'], sdk: '../module-kits' });
if (!result.ok) for (const issue of result.issues) console.error(issue.code, issue.message, issue.fix);
```

## What it writes

| File | Content |
|---|---|
| `.build/game.json` | what the player loads: the project packed, its scenes lowered for `game.engine` at build time (no scene file read at run time), `dev.builder` left out, the game's module `game.main` last in the lock |
| `.build/game/` | the crate of the game's module: `Cargo.toml`, `kit.api.json`, `module.json`, `src/lib.rs`, `src/index.rs`, `src/init_scene.rs` and `src/init_scene/<scene>.rs` |
| `.build/native/` | `libgfkit_game_main.so` (`.dll`, `.dylib`) and its `kit-modules-native.json`: put them with the kits' modules |
| `.build/web/` | `game.main.wasm`, relocatable for the kits' common memory |
| `.build/build.json` | the report: issues, steps and their times, files produced |

Never edit `.build/` by hand: change `project.gripforge` and build again.

## Errors

A build accepts no guess: every warning of the tolerant validator becomes an error (`promoted: true`), except the nodes
that are only read (`sound_not_placed`, `hud_not_placed`, `light_not_placed`, `camera_not_placed`). Each problem names
**the file, the scene, the element, the property, what is wrong and what is expected**:

```text
error section_field_unknown — project.gripforge › scene level › property kits.core.light.brightness: core.light.ambience has no property brightness: … Expected: use one of exposure, preset, seconds, tonemapping
error kit_unknown — project.gripforge › property installed: the kit world.nowhere is not in the catalog of kits … Expected: check the id …
```

The codes are those of the project format, plus the build's:

| Code | When |
|---|---|
| `kit_unknown` | an installed or required kit has no `kit.api.json` in the catalog |
| `kits_incompatible` | the kits or the SDK are not those this compiler is made for (another `kit.api.json` schema, another project format, a checkout missing what the build uses): use the version of the compiler made for them |
| `file_not_json`, `project_missing`, `kits_missing` | the project or the catalog cannot be read, with the faulty line |
| `generator_missing`, `generator_failed` | the core has no generator, or it refuses the project |
| `sdk_missing` | the crates of the SDK cannot be found (give `--sdk` or `--sdk-rev`) |
| `rust_build_failed` | the generated code does not compile: a bug of the generator or of a kit's façade, to report |
| `wasm_recipe_missing`, `cargo_missing` | the wasm recipe (a local `--sdk`) or cargo are missing |

## The kits SDK and compatibility

The validator, the packing and the generator of `init_scene.rs` are not in this repository: they are the common core of
the GripForge kits (`core/scene-format` of [gripforgeai/module-kits](https://github.com/gripforgeai/module-kits)). The
compiler takes it at one fixed revision, written in [`sdk.json`](sdk.json), and compiles it into the wasm the package
carries (`dist/gripforge_compiler.wasm`, no WASI, no wasm-bindgen). The package also carries the kits' catalog of that
revision (`dist/kits.json`) and what it needs of the kits (`dist/sdk.json`): before a build it checks the kits and the
SDK it is given, and refuses those it was not made for with `kits_incompatible`.

Following the kits: change `rev` in `sdk.json`, run `npm run sdk` and `npm test`; when the generated code changes on
purpose (a kit renamed an event, a property…), `UPDATE_GOLDEN=1 npm test` rewrites the golden files and the diff of
`test/golden/` is reviewed in the pull request. The workflow **Compatibility with the latest kits** builds the
`dev.builder` room of the latest kits on each pull request and each night, and checks with the kits' own test that the
compiled game reaches the same state as the development mode.

## Develop

```sh
npm run sdk              # the kits SDK at the revision of sdk.json, in .sdk/module-kits (git; read access needed)
npm run sdk -- --link ../module-kits   # or use a checkout you already have
npm test                 # unit and golden tests (builds the wasm core first if missing); no network, no AI call
npm run coverage         # line coverage of lib/, bin/ and the release script: fails under 90 %
npm run coverage:rust    # line coverage of the Rust core (llvm-tools): fails under 90 %
npm run lint             # JavaScript; cargo fmt --check and cargo clippy -D warnings for wasm/
```

| Part | Role |
|---|---|
| `wasm/` | the Rust core: strict check, packing, call of the generator, compiled to `dist/gripforge_compiler.wasm` |
| `lib/core.mjs` | loads the wasm, hands it a JSON request |
| `lib/project.mjs` | reads the project, the scene files it still names, the catalog of the kits |
| `lib/compat.mjs` | what the compiler needs of the kits, `kits_incompatible` |
| `lib/build.mjs` | the command as a function (`build()`) |
| `lib/crate.mjs`, `lib/cargo.mjs` | the crate of the game's module, cargo native and wasm |
| `lib/report.mjs`, `lib/cli.mjs`, `bin/` | the lines and the JSON report, the command line |
| `test/` | one suite per step (reading, check, generation, crate, compilation, messages, command line, compatibility, release) and the golden tests (`test/golden/`: the crate and code of the room, the messages of a broken project) |

The tests replace cargo by a fake that records its arguments; `GF_COMPILER_CARGO=1 npm test` compiles for real. The CI
also compiles an example project (the `dev.builder` room of the SDK) native and wasm on each pull request.

## Releases

A tag `vX.Y.Z` (the version of `package.json`, on `main`) releases that version: the CI checks it, packs it (`npm pack`)
and puts it on the GripForge download storage. Nothing is published to the npm registry. A version is never rewritten.

```text
compiler/<version>/compiler-<version>.tgz    the package (npm install <its url>)
compiler/<version>/SHA256SUMS                its sha256
compiler/<version>/release.json              the entry of this version (as in versions.json)
compiler/versions.json                       every version, newest first
compiler/latest.json                         the newest version's entry, with "schema": "gripforge.compiler-latest/1"
```

### `versions.json`

```json
{
 "schema": "gripforge.compiler-versions/1",
 "updated": "2026-11-20T10:00:00.000Z",
 "latest": "0.1.0",
 "versions": [
  {
   "version": "0.1.0",
   "tag": "v0.1.0",
   "date": "2026-11-20T09:58:12.000Z",
   "commit": "<commit of the tag>",
   "source": "https://github.com/gripforgeai/compiler/tree/v0.1.0",
   "notes": "<the message of the annotated tag, else the section of CHANGELOG.md, else empty>",
   "sdk": { "repository": "https://github.com/gripforgeai/module-kits", "rev": "<kits SDK revision>" },
   "files": [
    { "name": "compiler-0.1.0.tgz", "path": "compiler/0.1.0/compiler-0.1.0.tgz", "size": 245760, "sha256": "<hex>", "url": "<public link>" },
    { "name": "SHA256SUMS", "path": "compiler/0.1.0/SHA256SUMS", "size": 85, "sha256": "<hex>", "url": "<public link>" }
   ],
   "install": "npm install <public link of the archive>"
  }
 ]
}
```

| Field | Meaning |
|---|---|
| `schema` | `gripforge.compiler-versions/1`; a breaking change of this format changes the number |
| `updated` | when the file was last written (ISO 8601, UTC) |
| `latest` | the highest version (numeric order), the one a download page offers first |
| `versions[]` | one entry per version, highest first |
| `version`, `tag`, `commit`, `source` | the version, its git tag, its commit and its source tree |
| `date` | the date of the tag (ISO 8601, UTC) |
| `notes` | the release notes, Markdown |
| `sdk` | the kits SDK revision the version is made for |
| `files[]` | `name`, `path` (the key in the storage), `size` in bytes, `sha256`, and `url` when the public download address is configured |
| `install` | the install command, when `url` is known |

## License

MIT
