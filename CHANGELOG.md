# Changes

## [0.1.0]

- `gripforge-build`: one native binary. Reads `project.gripforge` and the `kit.api.json` of the kits of the project,
  checks it strictly, generates `game.json`, `init_scene.rs` per scene, `index.rs` and the crate of the game's module.
  Compiles nothing.
- Errors for an AI: stable codes, file › scene › element › property, what is expected; `--check`, `--json`.
- A kit missing from the project says which one (and its version) and how to install the kits.
- The generated crate calls each kit through the façade the kit ships in its folder (`<kit>/facade`,
  `gripforge-kit-facade-<kit>` at the kit's version), by a local path; `facade_missing`, `facade_mismatch`.
- `--kits-path <dir>` (was `--kits`): the kits, and their façades, from another folder (the sources of the kits, for
  their demos).
