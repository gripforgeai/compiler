# The game's code: `sources`

**The rule: `project.gripforge` is the only entry of a game.** The Rust files of the game's own code are declared by the
project, in `sources`; the compiler takes no file by convention (no `index.rs`, no `src/` read because it is there).
Decision of gripforgeai/gripforge#420, announced on gripforgeai/compiler#10 (the `index.rs` it took implicitly is gone).

```json
{
  "id": "arena",
  "sources": ["rules.rs", "code/weapons.rs"],
  "installed": [ … ],
  "data": { "scenes": [ … ] }
}
```

- Paths are relative to the folder of the project file, written with `/`, each once, each a `.rs` file inside the
  project (a link that leads out of it is refused).
- `gripforge-build` adds each file to **the game's module `game.main`** (`.build/game`), by path (`#[path]`): never copied,
  never written, the same bytes before and after a build. The name of a file is its module: `rules.rs` is
  `crate::rules`, `code/weapons.rs` is `crate::weapons`. A file uses another one by that path (`use crate::weapons;`).
- `game.main` is a kit module like any other (`gripforge-kit-guest`): native library on desktop, wasm in the **official
  web player** (WebGPU and WebGL2), loaded beside the kits, in their common memory. The same code runs in both; the
  player is never rebuilt for a game. The CLI builds the wasm (`relocatableBuild`, then `.build/web/kit-modules.json`).
- The key is checked by the common core of the format (the same in `dev.builder`), and is not packed into the game the
  players load (`.build/game.json`): the code is compiled, never read at run time.

## The module of the game

One of the sources declares it: a unit struct with the kit module contract.

```rust
use gripforge_kit_guest::{KitModule, Kx};
use serde_json::{json, Value as Json};

pub struct MyGame;

impl KitModule for MyGame {
    fn id(&self) -> &'static str { "game.main" }
    fn setup(&self, kx: &mut Kx) { kx.set_num("my_game.score", 0.0); }
    fn update(&self, kx: &mut Kx, _dt: f64) { /* the rules, at the tick */ }
    fn scene_enter(&self, kx: &mut Kx, scene: &Json) { /* the scene is posed */ }
}
```

The generated `module.rs` hands every hook of `game.main` to it: `setup`, `ready`, `update`, `late_update`,
`update_idle`, `teardown`, `on_event`, `on_timer`, `action`, `condition`, `spawn`, `call`, `call_typed`, `save`, `load`,
`component`, `on_var`, `callback`, `services`, `validate`, and `scene_enter` / `scene_exit` around the posing of the
scene. A module is stateless (what it keeps lives in the host: `Kx::put`, the state of game.engine). Its `id()` is not
read: the module is `game.main`. [`tests/game-code/rules.rs`](../tests/game-code/rules.rs) is a complete example; the CI
builds it into the example room, native and wasm.

## When the game's code runs

What the host of the kits does (module-kits `core/kit-api/src/host/mount.rs`, `tick_loop.rs`;
`kits/game_engine/logic/kit.rs`), and where `game.main` stands: **the last module of the lock** (`.build/game.json`,
`lock.order`).

| Moment | What the game's code gets | What it can do |
|---|---|---|
| Before the player and its host of kits start | nothing: the code is a module the host loads; there is no `main` of the game | nothing. The kits loaded, their order, the project (and the mods game.engine applies to it at the mount) are data, decided before any module runs |
| The mount, **`setup`** | after the `setup` of every kit of the lock (they come first), **before any `ready` and before the first scene** | its subscriptions (`kx.on`), timers, bound variables, hooks and registrations, the state it starts from; call the services of the kits (their `setup` is done) |
| **`ready`** | after every `setup`; game.engine (first in the lock) has entered the start scene at its own `ready`, so the start scene is already there | what needs the first scene's instances |
| **`scene_enter`** | each scene game.engine enters, the start scene included (before `ready` of game.main), once `game.main` has posed it (`init_scene`) | its nodes and the kits' elements of the scene exist |
| **`update(dt)`** | each fixed tick of the simulation, after the kits | the rules of the game; nothing drawn here: the display is the kits' and the engine's, at the render |
| **`scene_exit`** | before what the scene posed is given back | its nodes are still there |

So **yes, the game's code runs before the first scene**: in `setup`. It cannot run before the host of the kits is
started, nor change what the host mounts. `tests/accept/fps-shooter` plays it: `game_code.setup_before_scene` is true.

### In the development mode

The development mode (`dev.builder` in the player, a `project.gripforge` played without a build) compiles nothing: it
**loads the module of the game's code compiled apart**, like any kit module, on the web (the default) and native, and
refuses to play a project that declares `sources` without it. The module is the one of the development mode:

```sh
gripforge-build my-game --dev --sdk path/to/module-kits    # .build/dev/game: the game's code only
```

- `.build/dev/game` holds the files of `sources` (by path, as above) and a `module.rs` that hands **every** hook to the
  module of the game, `scene_enter` and `scene_exit` included: dev.builder poses the scenes, there is no `init_scene`
  and no `game.json`. It is still `game.main`, last of the lock (module-kits `sources::with_game_module`), so the hooks
  run at the same moments as in the table above.
- The module keeps **the digest of the files it compiles** (`src/sources_digest.rs`: the file of
  `crates/sources-digest`, the one copy of the function, written as it is; computed at compile time from the bytes `include_bytes!` reads): the custom section `gripforge.sources` of the wasm,
  the symbol `GRIPFORGE_SOURCES` of the native library. The built module (`.build/game`) carries it too.
- The CLI compiles it (PROPOSAL to the CLI, gripforge#420: the place and the command are its choice): the wasm and
  `.build/dev/web/kit-modules.json` (module-kits `tools/kit-modules/game-module.mjs` is the reference), the native
  library and `.build/dev/native/`.
- The development mode then refuses, with the code first: `game_main_missing` (no compiled module found),
  `game_main_stale` (its digest is not the one of the files now: a file changed since it was compiled),
  `source_missing` (a declared file is not there). The same rule in module-kits (`core/scene-format`
  `sources::dev_module_issue`, `core/gateway-web/dev-game-main.mjs`), with the same function: module-kits imports
  `crates/sources-digest` by git (pinned) and the page calls it through its wasm; the shared values are the crate's
  (`tests/values.json`, `VALUES`).

## Errors

Each says the file, the field (`sources[i]`), what is wrong and what to write; `--check` refuses the same way, and
nothing is written.

| Code | When |
|---|---|
| `sources_invalid` | `sources` is not a list of strings |
| `source_outside` | an absolute path, a drive, a `\`, a `..`, or a link that leads out of the project |
| `source_not_rust` | a file that is not `.rs` |
| `source_duplicate` | the same file twice (also through `./` or a link), or two files whose names make the same module |
| `source_missing` | the file is not there |
| `source_name_invalid` | its name is not a free Rust module name: a keyword, `lib`, `main`, `module`, `init_scene`, `host`, `std`, `core`, `alloc`, `serde_json`, `gripforge_kit_guest`, `gripforge_kit_access`, or not letters, digits and `_` |
| `source_unreadable` | not UTF-8 |
| `source_mod_undeclared` | the file declares `mod x;` (a file the project does not list): declare that file too, or write the module inline |
| `sources_module_missing` | no source declares `pub struct X;` with `impl KitModule for X` |
| `sources_module_ambiguous` | several do (named) |
| `source_undeclared` (warning) | a `.rs` beside the project that `sources` does not list: not compiled |
| `dev_without_sources` | `--dev` on a project that declares no `sources`: nothing to compile |

## See also

- [README](../README.md): the command, its options, the kits of a project.
- [CHANGELOG](../CHANGELOG.md): 0.1.2.
