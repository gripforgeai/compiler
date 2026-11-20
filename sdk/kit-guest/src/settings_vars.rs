//! The kit side of the settings variables (kits/core_settings/README.md, docs/declarer-un-reglage.md).
//!
//! A kit declares its player settings in the `expose` of its `kit.api.json` (`for: ["settings"]`); the kit that holds
//! the settings role (`role.settings`, today core.settings) holds their values and publishes each one in the game
//! state as `<its id>.value.<setting id>` (`core.settings.value.audio.music.level`). The kit reads them there:
//! [`watch`] at `setup()` binds the variables of its own settings (its `on_var` is told each change, tag [`TAG`]),
//! [`held`] at `ready()` gives the values already there. The role is optional: without it no variable exists, nothing
//! is told, and the kit keeps its own default (its config). The holder never calls the kit. The core names no kit:
//! who holds the role is asked to the host (`runtime.core.roleHolder`, known from the mount on).
//!
//! Used by: every kit module that declares a setting it applies (audio, story, world, render kits…).

use crate::host::Kx;
use serde_json::{json, Value as Json};

/// The tag of the bindings made by [`watch`]: the kit's `on_var` receives it for each of its settings.
pub const TAG: &str = "role.settings.value";

/// The prefix of the settings variables, `<holder id>.value.` (empty without a holder: no variable exists). Asked to
/// the host at `setup` and `ready` only: never per tick.
fn prefix(kx: &mut Kx) -> String {
    kx.call(
        "runtime.core",
        "roleHolder",
        &[json!(gripforge_abi::roles::SETTINGS)],
    )
    .ok()
    .and_then(|v| v.as_str().map(|id| format!("{id}.value.")))
    .unwrap_or_default()
}

/// The setting of a variable path bound by [`watch`] (`None`: not a settings variable): what follows the holder's
/// `.value.`.
pub fn setting_of(path: &str) -> Option<&str> {
    path.split_once(".value.").map(|(_, id)| id)
}

/// The ids of the settings `api_json` declares (the kit's own `kit.api.json`, `include_str!("../kit.api.json")`):
/// its `expose` entries for `settings` that are settings (no `kind`, or `kind: "setting"`), not groups or modes.
pub fn ids(api_json: &str) -> Vec<String> {
    let api = serde_json::from_str::<Json>(api_json).unwrap_or(Json::Null);
    api["expose"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| {
            e["for"]
                .as_array()
                .is_some_and(|f| f.iter().any(|r| r == "settings"))
                && e["kind"].as_str().is_none_or(|k| k == "setting")
        })
        .filter_map(|e| e["id"].as_str().map(String::from))
        .collect()
}

/// At `setup()`: binds the variable of each setting the kit declares. Returns their ids. A change of one, from the
/// menu, a mode, the player's file or the address (`?set=`), reaches the kit's `on_var` with the tag [`TAG`] and the
/// variable's path ([`setting_of`] gives the id back).
pub fn watch(kx: &mut Kx, api_json: &str) -> Vec<String> {
    let ids = ids(api_json);
    let prefix = prefix(kx);
    if !prefix.is_empty() {
        for id in &ids {
            kx.bind_var(&format!("{prefix}{id}"), TAG);
        }
    }
    ids
}

/// At `ready()`: the values the holder of the role already holds for the kit's settings (none without it, or before
/// it published them: the binding of [`watch`] tells them later).
pub fn held(kx: &mut Kx, api_json: &str) -> Vec<(String, Json)> {
    let prefix = prefix(kx);
    if prefix.is_empty() {
        return Vec::new();
    }
    ids(api_json)
        .into_iter()
        .filter_map(|id| {
            let v = kx.get_var(&format!("{prefix}{id}"))?.clone();
            (!v.is_null()).then_some((id, v))
        })
        .collect()
}
