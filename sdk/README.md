# The kits SDK (public part)

The three crates the crate of a game built by `gripforge-build` compiles against, and nothing else of the kits
repository:

| Folder | Crate | What it is |
|---|---|---|
| `abi/` | `gripforge-abi` | the contract between the kit host, the kit modules and the gateways (tables, layouts, ports, frames) |
| `kit-guest/` | `gripforge-kit-guest` | the kit side of the host (`Host`, `Kx`): a kit module, the game's own included, built native or wasm |
| `kit-access/` | `gripforge-kit-access` | the common part of the kits' Rust accessors (`Host`, `Spec`, `Posed`); feature `guest` |

The accessor of each kit (`gripforge-kit-access-<kit>`) is not here: it ships in the kit's package
(`<kit>/access/rust`).

## Where it comes from

Copied from `gripforgeai/module-kits` (`core/abi`, `core/kit-guest`, `core/kit-access`, `rust-toolchain.toml`) at the
revision of `sdk.json`, unchanged but for one patch: `gripforge-kit-access` has no `kithost` feature (the kit host's
side, which stays in the kits repository). `Cargo.toml` (the workspace), this file and `sdk.json` are this repository's
own. The comments of the copied files point at documents of the kits repository (`docs/…`, `tools/…`); they are not
part of this repository.

The maintainers sync it, or check it before a release, from a checkout of the kits repository:

```sh
tools/sync-sdk path/to/module-kits --rev <commit>   # copy, and write the revision in sdk.json
tools/sync-sdk path/to/module-kits --check          # sdk/ is exactly the copy at the revision of sdk.json
```

## How a game takes it

`gripforge-build` writes, by default, the git dependency on this repository at the compiler's tag:

```toml
gripforge-kit-guest = { git = "https://github.com/gripforgeai/compiler", tag = "v0.1.0" }
```

`--sdk <dir>` takes it by path instead (this folder, or a checkout of the kits repository); `--sdk-rev <tag, commit or
branch>` another revision of this repository.
