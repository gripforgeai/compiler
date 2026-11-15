# Changes

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
