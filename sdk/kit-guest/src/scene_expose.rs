//! The kit side of the scene reader of `expose` (docs/design/dev-builder-sections.md, docs/exposer.md).
//!
//! A kit that has something a scene may hold (a light ambience, a collider, a terrain layer, a shower) exposes it as
//! an element: an `expose` entry for `scene` in its `kit.api.json`, with its properties and the four methods of its
//! service that make, change, let go and read it (`create`, `set`, `remove`, `get`). The game calls those methods in
//! code; dev.builder calls the same ones for a scene file; the editor and the debugger the same again. At `ready()`
//! the kit hands its entries to the scene reader (the kit that holds `role.scene`, today dev.builder) with [`expose`];
//! without it, [`expose`] returns at once (the `kit.api.json` is not even parsed): the reader is optional, no kit
//! depends on it.
//!
//! Used by: every kit module that lists `expose` entries for `scene`, at `ready()`. What dev.builder does with them
//! lives in `kits/dev_builder/logic/sections.rs`; this file only knows that call. Modelled on `passport_expose.rs`.

use crate::host::Kx;

/// Hands the `expose` entries of `api_json` (the kit's own `kit.api.json`, usually `include_str!("../kit.api.json")`)
/// to dev.builder, when it is loaded and the kit has an entry for `scene`; dev.builder keeps those and skips the
/// others. Returns whether dev.builder took them.
///
/// Call it once at `ready()`. The JSON is parsed only when dev.builder is there.
pub fn expose(kx: &mut Kx, kit: &str, api_json: &str) -> bool {
    crate::expose::hand(kx, kit, api_json, "scene", true)
}
