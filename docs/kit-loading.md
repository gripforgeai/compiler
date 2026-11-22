# The kits of a game, fixed at the build: `kit_required`

> Français : [chargement-des-kits.md](chargement-des-kits.md). The rule common to every reader of a game (the web and
> native gateways, `dev.builder`) is in module-kits `docs/chargement-des-kits.en.md`; this page says what the compiler
> does with it.

## The rule

- **The build reads `project.gripforge` as `dev.builder` does**: the same code (`gripforge-scene-format`, a crate of this
  repository that module-kits imports pinned), the same resolution (`includes`, variables, `$var`), the same lock. The
  lock of the built game (`.build/game.json`, `lock.order`) is `dev.builder`'s without the development kits
  (`dev.builder`, `scene.builder`), with the game's module `game.main` last.
- **The kits are fixed at the start**, like the libraries of a program: `installed` lists **every** kit of the game,
  the ones its kits need included (their `requires`, their simple and per-target dependencies). A recommendation
  (`recommended`) or an optional kit (`optional`) is never required nor added.
- **A required kit that is not listed fails the BUILD** (and `--check`) with the error `kit_required`, **the same
  message** the gateway and `dev.builder` give at the start: it is the same function
  (`gripforge_scene_format::kits::check_required`, written once, never copied). Every lack is listed at once; nothing
  is written.
- **The target** says which dependencies count: `--target <gateway>` (`bevy-web` by default, the official web player;
  `bevy`, `godot`…). A dependency declared with `targets: ["bevy", "bevy-web"]` is not required for `godot`.
- The built game ships its kits; `gf.kits` and `gf.kits-path` (the gateway's parameters) are for development, tests
  and the test bench, not for the build.

## Example

`game.engine` declares in its `kit.api.json`:

```json
"dependencies": [
  { "kit": "core.scene", "range": "*", "targets": ["bevy", "bevy-web"],
    "reason": "On Bevy, core.scene loads the native part of the scenes." }
]
```

A project that installs `game.engine` without `core.scene`:

```json
{
  "id": "arena",
  "installed": [ { "id": "game.engine" }, { "id": "core.light" } ],
  "data": { "scenes": [ … ] }
}
```

```console
$ gripforge-build arena
error kit_required — project.gripforge: kit_required: game.engine a besoin de core.scene (cible bevy-web) — ajoute-le dans "installed" du project.gripforge. Expected: ajoute core.scene dans "installed" du project.gripforge (docs/dependances.md)
project.gripforge: 1 error(s), 0 warning(s) — nothing generated; fix the project and build again
$ echo $?
1
$ gripforge-build arena --check --target godot     # Godot has its own scenes: core.scene is not required
project.gripforge: checked (0 warning(s)) — …
```

The message is the one the gateways print (written in French by the common function, the same everywhere; tools match
on the code `kit_required`, never on the text). The fix: add the kit to `installed`, never let it be guessed.

```json
"installed": [ { "id": "game.engine" }, { "id": "core.scene" }, { "id": "core.light" } ]
```

`dev.builder` (`?gf.kits=dev.builder`) refuses the same project at the start with the same `kit_required` line.

## The game's code: declared by the project (compiler#10)

The `.gripforge` is the **only entry**: it declares its own external Rust files in `sources`, and the build takes no
file by convention (an `index.rs` beside it is not read). See [sources.md](sources.md).

```json
"sources": ["rules.rs", "code/weapons.rs"]
```

## The parity with `dev.builder`

`tests/kits_required.rs` builds the same project both ways: as `dev.builder` does (the project packed by
`project::pack_with`, the function `devGame` calls through the format's wasm, then `kits::check_json`, the one the web
gateway calls through `gf_kits_check`) and by `gripforge-build`. The same lock (but for the development kits and
`game.main`, as said above), the same refusal, message for message.

| Code | When |
|---|---|
| `kit_required` | a kit of `installed` needs (for the target) a kit or a capability the list does not have; one line per lack |
