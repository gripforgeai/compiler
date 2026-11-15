//! The kit side of the scene reader of `expose` (docs/design/dev-builder-sections.md, docs/exposer.md).
//!
//! A kit that has something a scene may hold (a light ambience, a collider, a terrain layer, a shower) exposes it as
//! an element: an `expose` entry for `scene` in its `kit.api.json`, with its properties and the four methods of its
//! service that make, change, let go and read it (`create`, `set`, `remove`, `get`). The game calls those methods in
//! code; dev.builder calls the same ones for a scene file; the editor and the debugger the same again. At `ready()`
//! the kit hands its entries to dev.builder with [`expose`]; without dev.builder, [`expose`] returns at once (the
//! `kit.api.json` is not even parsed): dev.builder is optional, no kit depends on it.
//!
//! Used by: every kit module that lists `expose` entries for `scene`, at `ready()`. What dev.builder does with them
//! lives in `kits/dev_builder/logic/sections.rs`; this file only knows that call. Modelled on `passport_expose.rs`.

use crate::host::Kx;
use serde_json::{json, Value as Json};

/// The service that reads scene files: nothing is called when it is not served.
pub const DEV_BUILDER: &str = "dev.builder";

/// Hands the `expose` entries of `api_json` (the kit's own `kit.api.json`, usually `include_str!("../kit.api.json")`)
/// to dev.builder, when it is loaded and the kit has an entry for `scene`; dev.builder keeps those and skips the
/// others. Returns whether dev.builder took them.
///
/// Call it once at `ready()`. The JSON is parsed only when dev.builder is there.
pub fn expose(kx: &mut Kx, kit: &str, api_json: &str) -> bool {
    if kit == DEV_BUILDER || !kx.host.has_service(DEV_BUILDER) {
        return false;
    }
    let entries = serde_json::from_str::<Json>(api_json)
        .ok()
        .and_then(|api| api.get("expose").cloned())
        .unwrap_or_else(|| json!([]));
    let for_scene = entries.as_array().is_some_and(|list| {
        list.iter().any(|e| {
            e.get("for")
                .and_then(Json::as_array)
                .is_some_and(|f| f.iter().any(|r| r == "scene"))
        })
    });
    for_scene
        && kx
            .call(DEV_BUILDER, "expose", &[json!(kit), entries])
            .is_ok()
}
