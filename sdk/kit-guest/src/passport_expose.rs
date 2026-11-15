//! The kit side of the passport reader of `expose` (docs/exposer-au-passeport.md, docs/exposer.md).
//!
//! A kit that owns fields of the asset passport declares them as `expose` entries for `passport` in its
//! `kit.api.json`, and hands them to asset.meta at `ready()` with [`expose`]. Without asset.meta, [`expose`] returns at
//! once (the kit's `kit.api.json` is not even parsed) and the kit keeps the defaults of its fields: the passport is
//! optional, no kit depends on it.
//!
//! Used by: every kit module that lists `expose` entries for `passport`, at `ready()`. What asset.meta does with them
//! lives in `kits/asset_meta/logic` (service `asset.meta`, method `expose`); this file only knows that call.

use crate::host::Kx;
use serde_json::{json, Value as Json};

/// The service of the passport: nothing is called when it is not served.
pub const PASSPORT: &str = "asset.meta";

/// Hands the `expose` entries of `api_json` (the kit's own `kit.api.json`, usually `include_str!("../kit.api.json")`)
/// to asset.meta, when it is loaded and the kit has an entry for `passport`; asset.meta keeps those and skips the
/// others. Returns whether asset.meta took them.
///
/// Call it once at `ready()` (every kit is mounted then). The JSON is parsed only when asset.meta is there.
pub fn expose(kx: &mut Kx, kit: &str, api_json: &str) -> bool {
    if kit == PASSPORT || !kx.host.has_service(PASSPORT) {
        return false;
    }
    let entries = serde_json::from_str::<Json>(api_json)
        .ok()
        .and_then(|api| api.get("expose").cloned())
        .unwrap_or_else(|| json!([]));
    let for_passport = entries.as_array().is_some_and(|list| {
        list.iter().any(|e| {
            e.get("for")
                .and_then(Json::as_array)
                .is_some_and(|f| f.iter().any(|r| r == "passport"))
        })
    });
    for_passport && kx.call(PASSPORT, "expose", &[json!(kit), entries]).is_ok()
}
