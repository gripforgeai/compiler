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
use crate::entities::Query;
use crate::host::{Host, Kx};
use crate::js::js_round;
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

/// `hudPlayer(ctx)`: the player this machine shows on its HUD — here the first player.
pub fn hud_player(host: &Host) -> Option<String> {
    host.entities
        .first(&Query::tag("player"))
        .map(|e| e.id.clone())
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

/// `roundTo(v, digits)`.
pub fn round_to(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    js_round(v * k) / k
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

/// `createThrottle(period).tick(dt)` over the accumulator a kit keeps in its own state (reset: `acc = period`).
pub fn throttle_tick(acc: &mut f64, period: f64, dt: f64) -> bool {
    *acc += dt;
    if *acc + 1e-9 < period {
        return false;
    }
    *acc -= period;
    if *acc > period {
        *acc = period;
    }
    true
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

/// [`service`] for an answer that is only read (`Kx::call_with`): `f` reads it where it is — the answer of another
/// kit module is not rebuilt; `f` copies (`Val::to_json`) only what the caller keeps.
pub fn service_with<R>(
    kx: &mut Kx,
    capability: &str,
    method: &str,
    args: &[Json],
    f: impl FnOnce(crate::host::Val) -> R,
) -> Option<R> {
    if !kx.host.has_service(capability) {
        return None;
    }
    kx.call_with(capability, method, args, f).ok()
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

/// The edge of a world: the Rust side of `kits/world-edge.ts` (same pieces, same order, same rule).
pub mod world_edge {
    use crate::cfg::cfg_number;
    use crate::host::Kx;
    use crate::js::{num, str_of, truthy};
    use serde_json::{json, Value as Json};

    pub const BARRIER_THICKNESS: f64 = 0.5;

    #[derive(Clone, Debug, PartialEq)]
    pub struct WorldEdge {
        pub apron: f64,
        /// `rail`, `wall` or `none`.
        pub barrier: String,
        pub height: f64,
        pub recover_below: f64,
    }

    /// `readEdge(conf)`.
    pub fn read_edge(conf: &Json) -> WorldEdge {
        let empty = json!({});
        let e = conf.get("edge").filter(|e| e.is_object()).unwrap_or(&empty);
        let barrier = match str_of(e.get("barrier")) {
            Some(b @ ("rail" | "wall" | "none")) => b,
            _ => "rail",
        };
        WorldEdge {
            apron: cfg_number(e, "apron", 40.0, 0.0, 400.0),
            barrier: barrier.to_string(),
            height: cfg_number(e, "height", 1.2, 0.3, 20.0),
            recover_below: cfg_number(e, "recoverBelow", 12.0, 1.0, 1000.0),
        }
    }

    pub struct EdgePiece {
        pub kind: &'static str,
        pub tags: Vec<String>,
        pub center: [f64; 3],
        pub size: [f64; 3],
    }

    /// `edgePieces(half, edge, hidden)`: the apron (south, north, west, east), then the barrier (x−, z−, x+, z+).
    pub fn edge_pieces(half: f64, edge: &WorldEdge, hidden: bool) -> Vec<EdgePiece> {
        let mut out = Vec::new();
        let (a, e) = (edge.apron, half * 2.0);
        let tag = |base: &[&str], hide: bool| -> Vec<String> {
            let mut t: Vec<String> = if hide {
                vec!["hidden".to_string()]
            } else {
                Vec::new()
            };
            t.extend(base.iter().map(|s| s.to_string()));
            t
        };
        if a > 0.0 {
            for side in [-1.0, 1.0] {
                out.push(EdgePiece {
                    kind: "ground",
                    tags: tag(&["ground", "apron"], hidden),
                    center: [0.0, -0.25, side * (half + a / 2.0)],
                    size: [e + 2.0 * a, 0.5, a],
                });
            }
            for side in [-1.0, 1.0] {
                out.push(EdgePiece {
                    kind: "ground",
                    tags: tag(&["ground", "apron"], hidden),
                    center: [side * (half + a / 2.0), -0.25, 0.0],
                    size: [a, 0.5, e],
                });
            }
        }
        if edge.barrier != "none" {
            let (t, h) = (BARRIER_THICKNESS, edge.height);
            let style = format!("edge:{}", edge.barrier);
            for side in [-1.0, 1.0] {
                for axis in [0, 2] {
                    let mut center = [0.0, h / 2.0, 0.0];
                    center[axis] = side * (half - t / 2.0);
                    out.push(EdgePiece {
                        kind: "boundary",
                        tags: tag(&["boundary", &style], hidden || edge.barrier == "wall"),
                        center,
                        size: if axis == 0 { [t, h, e] } else { [e, h, t] },
                    });
                }
            }
        }
        out
    }

    /// `edgeActors(ctx)`: the player, then the vehicles, then the NPCs, each once; not a rider.
    pub fn edge_actors(kx: &Kx) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        if let Some(p) = kx.host.entities.player() {
            seen.push(p.id.clone());
        }
        for tag in ["vehicle", "npc"] {
            for e in kx.host.entities.by_tag(tag) {
                if !seen.contains(&e.id) {
                    seen.push(e.id.clone());
                }
            }
        }
        seen.retain(|id| {
            !kx.host
                .entities
                .get(id)
                .is_some_and(|e| truthy(e.refs.get("vehicle")))
        });
        seen
    }

    /// `lostCause(p, half, edge)`: `fell`, `outside` or ``.
    pub fn lost_cause(x: f64, y: f64, z: f64, half: f64, edge: &WorldEdge) -> &'static str {
        if !(y >= -edge.recover_below) {
            return "fell";
        }
        let limit = half + edge.apron;
        if x.abs() > limit || z.abs() > limit {
            return "outside";
        }
        ""
    }

    /// `putBack(ctx, id, at, yaw)`.
    pub fn put_back(kx: &mut Kx, id: &str, at: [f64; 3], yaw: f64) {
        let p = json!([num(at[0]), num(at[1]), num(at[2])]);
        let vehicle = kx
            .host
            .entities
            .get(id)
            .is_some_and(|e| e.kind == "vehicle");
        if kx.host.has_service("vehicle.drive") && vehicle {
            let _ = kx.call("vehicle.drive", "teleport", &[json!(id), p, num(yaw)]);
            return;
        }
        // A body core.locomotion drives (the player, an NPC capsule) goes back through it: its physics body would write
        // the old position back, and the kit resets its fall. The answer is `true` only for an entity it drives.
        if kx.host.has_service("core.locomotion")
            && kx
                .call(
                    "core.locomotion",
                    "teleport",
                    &[json!(id), p.clone(), num(yaw)],
                )
                .is_ok_and(|done| done == Json::Bool(true))
        {
            return;
        }
        kx.host.entities.set_transform(
            id,
            &json!({ "x": num(at[0]), "y": num(at[1]), "z": num(at[2]), "yaw": num(yaw) }),
        );
        if let Some(mut body) = kx.host.physics.body(id) {
            body.p = at;
            body.v = [0.0, 0.0, 0.0];
            body.w = [0.0, 0.0, 0.0];
            kx.host.physics.set_body(&body);
        }
        if kx.host.physics.has_character(id) {
            kx.host.physics.set_character_position(id, at);
        }
    }
}
