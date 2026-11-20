//! The one way a kit hands its `expose` entries to a reader (docs/exposer.md, docs/design/noyau-sans-noms-de-kits.md):
//! the entries `for: X` go to the kit that holds the role `role.X`, whoever it is. Without such a kit, [`hand`] returns
//! at once (the kit's `kit.api.json` is not even parsed): every reader is optional, no kit depends on it.
//!
//! Used by: [`crate::debug_expose`], [`crate::scene_expose`], [`crate::passport_expose`] (each a reader of `expose`).

use crate::host::Kx;
use serde_json::{json, Value as Json};

/// Hands the `expose` entries of `api_json` (the kit's own `kit.api.json`) to the holder of `role.<reader>`, as its
/// method `expose(kit, entries)`. With `listed`, only when one entry at least is `for` that reader. Returns whether
/// the reader took them.
pub fn hand(kx: &mut Kx, kit: &str, api_json: &str, reader: &str, listed: bool) -> bool {
    let role = gripforge_abi::roles::expose_reader(reader);
    if !kx.host.has_service(&role) {
        return false;
    }
    let entries = serde_json::from_str::<Json>(api_json)
        .ok()
        .and_then(|api| api.get("expose").cloned())
        .unwrap_or_else(|| json!([]));
    let meant = entries.as_array().is_some_and(|list| {
        list.iter().any(|e| {
            e.get("for")
                .and_then(Json::as_array)
                .is_some_and(|f| f.iter().any(|r| r == reader))
        })
    });
    (!listed || meant) && kx.call(&role, "expose", &[json!(kit), entries]).is_ok()
}
