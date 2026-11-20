//! The helpers every ported kit shares — the Rust side of `kits/shared-runtime.ts` and `kits/shared-action.ts`.
//! One module for all batches: a helper a kit needs from those files is added here by the owner of the base (a
//! batch keeps it in its own kit file until then), never in a per-batch `shared_*.rs`.
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "`!(a < b)` is deliberate: true for NaN, like the TypeScript reference the parity traces were recorded from"
)]
#![allow(
    clippy::manual_unwrap_or,
    reason = "false positive of clippy: the match keeps only the values it lists (Some(v @ (a | b)) => v), unwrap_or would keep any value"
)]

pub use crate::cfg::{cfg_bool, cfg_int, cfg_number, cfg_string, cfg_strings, clamp};
use crate::host::{Host, Kx};
use serde_json::Value as Json;

/// `players(ctx)`: the ids of every player entity (tag `player`), in id order.
pub fn players(host: &Host) -> Vec<String> {
    let mut ids: Vec<String> = host
        .entities
        .by_tag("player")
        .iter()
        .map(|e| e.id.clone())
        .collect();
    ids.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    ids
}

/// The player an event or a service call is about: the acting player, else the first player entity.
pub fn current_player(host: &Host) -> Option<String> {
    host.acting_player()
        .or_else(|| host.entities.player().map(|e| e.id.clone()))
}

/// `ctx.entities.player()?.id` from a kit: the player in scope (co-op), else the first player.
pub fn player_id(kx: &Kx) -> Option<String> {
    current_player(kx.host)
}

pub fn is_player(host: &Host, id: &str) -> bool {
    !id.is_empty() && host.entities.get(id).is_some_and(|e| e.has_tag("player"))
}

/// `withPlayer(ctx, player, fn)`: runs `f` with `current_player` answering `player` (when it is one).
pub fn with_player<R>(kx: &mut Kx, player: Option<&str>, f: impl FnOnce(&mut Kx) -> R) -> R {
    match player.filter(|p| is_player(kx.host, p)) {
        None => f(kx),
        Some(p) => {
            kx.host.player_scopes.push(p.to_string());
            let out = f(kx);
            kx.host.player_scopes.pop();
            out
        }
    }
}

/// `posOf(ctx, id)`: the position of an entity.
pub fn pos_of(host: &Host, id: &str) -> Option<[f64; 3]> {
    let t = host.entities.transform(id)?;
    Some([t.x, t.y, t.z])
}

/// `createThrottle(period)`: `tick(dt)` is true once every `period` seconds of accumulated dt, the first tick included.
#[derive(Clone, Debug)]
pub struct Throttle {
    period: f64,
    acc: f64,
}

impl Throttle {
    pub fn new(period: f64) -> Throttle {
        Throttle {
            period,
            acc: period,
        }
    }
    pub fn tick(&mut self, dt: f64) -> bool {
        self.acc += dt;
        if self.acc + 1e-9 < self.period {
            return false;
        }
        self.acc -= self.period;
        if self.acc > self.period {
            self.acc = self.period;
        }
        true
    }
    pub fn reset(&mut self) {
        self.acc = self.period;
    }
}

/// `ctx.services.get(capability)?.method(...args)`: `None` when no mounted kit serves the capability (or the
/// method). Only plain-data methods cross kits this way; a service taking a callback has no equivalent.
/// Moved from `shared_runtime.rs` of the former controller kit (D1, #658).
pub fn service(kx: &mut Kx, capability: &str, method: &str, args: &[Json]) -> Option<Json> {
    if !kx.host.has_service(capability) {
        return None;
    }
    kx.call(capability, method, args).ok()
}

/// `collectionOr(data, name, [])`: the records of a project collection that carry a string `id`, in document order.
pub fn collection(kx: &Kx, name: &str) -> Vec<Json> {
    kx.data_collection(name)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter(|d| d.is_object() && d.get("id").is_some_and(Json::is_string))
        .cloned()
        .collect()
}
