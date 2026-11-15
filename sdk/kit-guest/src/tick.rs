//! The logic tick of a game (game.engine `tick`, docs/game-engine-contract.md « Boucle de tick et boucle de rendu »):
//! shared word for word by the core (src/host.rs includes this file) and by the kit logic modules.
//!
//! - [`TickClock`]: the accumulator of the tick loop. The host hands it the wall time of a frame (any rate); it answers
//!   how many fixed ticks are due, bounded (`maxCatchUp`: a slow frame never spirals), and the fraction of a tick the
//!   presentation is between the last two published ticks (`alpha`). An external clock (the network's) may scale its
//!   rate and shift it by whole or partial ticks ([`TickClock::set_rate`], [`TickClock::shift`]).
//! - [`Every`] / [`Slice`]: the every-N scheduler of the kits. A system declares a period in ticks (`every`, 1 = each
//!   tick) and a phase; systems with the same period are spread across ticks by their phase (load balancing). A
//!   [`Slice`] spreads the ENTITIES of a system: each tick processes the 1/N whose key matches.
//! - [`resolve`]: the period of a system as the game tunes it (`game.engine` config `tick.systems["kit.system"]`).
//!
//! The kits use it through `Kx::schedule(system, every)` (then `due(kx.tick())`, `slice(kx.tick()).has(id)`), the
//! same on the core and in a kit module.

use serde_json::Value as Json;

/// What a tick is compared with (a frame of exactly one tick runs it).
pub const TICK_EPS: f64 = 1e-9;
/// The default logic rate of a game (game.engine `tick.hz`): the game maker's choice, any rate in
/// [`MIN_TICK_HZ`]..[`MAX_TICK_HZ`] (a strategy game 10-20, an action game 30, a twitch shooter 60).
pub const DEFAULT_TICK_HZ: f64 = 30.0;
pub const MIN_TICK_HZ: f64 = 5.0;
pub const MAX_TICK_HZ: f64 = 120.0;

/// A rate as the host takes it: bounded to `MIN_TICK_HZ..=MAX_TICK_HZ`, the default when not a number.
pub fn valid_hz(hz: f64) -> f64 {
    if hz.is_finite() && hz > 0.0 {
        hz.clamp(MIN_TICK_HZ, MAX_TICK_HZ)
    } else {
        DEFAULT_TICK_HZ
    }
}
/// The wall time one frame may hand the clock at most, in seconds (a tab back from the background).
pub const MAX_FRAME: f64 = 0.25;

/// The accumulator of the tick loop. Owns no game state: the host asks it how many ticks are due.
#[derive(Clone, Debug)]
pub struct TickClock {
    acc: f64,
    /// Wall time scale (1 = real time): the network's drift correction runs the clock a little faster or slower.
    rate: f64,
    /// Seconds of frame time dropped by the catch-up bound since the start (diagnostics).
    pub dropped: f64,
}

impl Default for TickClock {
    fn default() -> TickClock {
        TickClock {
            acc: 0.0,
            rate: 1.0,
            dropped: 0.0,
        }
    }
}

impl TickClock {
    /// Adds the wall time of a frame (clamped to `[0, MAX_FRAME]`, scaled by the rate) and answers the ticks due,
    /// at most `max_ticks`: past it, the rest of the frame's time is dropped (only the part of a tick stays, so the
    /// presentation keeps its phase).
    pub fn advance(&mut self, frame_dt: f64, fixed_dt: f64, max_ticks: u32) -> u32 {
        let wall = if frame_dt.is_finite() {
            frame_dt.clamp(0.0, MAX_FRAME)
        } else {
            0.0
        };
        self.acc += wall * self.rate;
        if !(fixed_dt.is_finite() && fixed_dt > 0.0) {
            return 0;
        }
        let mut ticks = 0u32;
        while self.acc >= fixed_dt - TICK_EPS {
            if ticks >= max_ticks {
                let kept = self.acc % fixed_dt;
                self.dropped += self.acc - kept;
                self.acc = kept;
                break;
            }
            ticks += 1;
            self.acc -= fixed_dt;
        }
        ticks
    }

    /// The fraction of a tick between the last two ticks the presentation draws (0..1).
    pub fn alpha(&self, fixed_dt: f64) -> f64 {
        if fixed_dt > 0.0 {
            (self.acc / fixed_dt).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The time accumulated toward the next tick, in seconds.
    pub fn accumulated(&self) -> f64 {
        self.acc
    }

    /// The wall time scale (1 = real time).
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// The network's drift correction: the clock runs at `rate` × real time, bounded to ±10 % (a client a little
    /// behind the server's tick speeds up until it is aligned, never a visible jump).
    pub fn set_rate(&mut self, rate: f64) {
        self.rate = if rate.is_finite() {
            rate.clamp(0.9, 1.1)
        } else {
            1.0
        };
    }

    /// Moves the clock by `ticks` ticks (negative: holds it back), once: what an external clock asks when it is far
    /// from this one. Bounded to `[-1, MAX_FRAME / fixed_dt]` ticks per call, and the accumulator never goes below 0.
    pub fn shift(&mut self, ticks: f64, fixed_dt: f64) {
        if !(ticks.is_finite() && fixed_dt > 0.0) {
            return;
        }
        let t = ticks.clamp(-1.0, MAX_FRAME / fixed_dt);
        self.acc = (self.acc + t * fixed_dt).max(0.0);
    }

    /// The tick changes length (a live change of `tick.hz`, at a tick boundary): the part of a tick accumulated is
    /// kept as a part of the new tick, so the presentation goes on from the same fraction (no visual jump); the game
    /// time this moves is less than one tick, once.
    pub fn rescale(&mut self, old_dt: f64, new_dt: f64) {
        if old_dt > 0.0 && new_dt > 0.0 {
            self.acc = (self.acc / old_dt).clamp(0.0, 1.0) * new_dt;
        }
    }

    /// Back to no time accumulated (a new match, a load).
    pub fn reset(&mut self) {
        self.acc = 0.0;
    }
}

/// The period of a system: it runs on the ticks where `(tick + offset) % every == 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Every {
    pub every: u64,
    pub offset: u64,
}

impl Default for Every {
    fn default() -> Every {
        Every {
            every: 1,
            offset: 0,
        }
    }
}

impl Every {
    /// Each `every` ticks (0 reads as 1), phase 0.
    pub fn new(every: u64) -> Every {
        Every {
            every: every.max(1),
            offset: 0,
        }
    }

    /// Each `every` ticks, the phase spread by `name` (systems of the same period land on different ticks).
    pub fn spread(name: &str, every: u64) -> Every {
        let every = every.max(1);
        Every {
            every,
            offset: id_key(name) % every,
        }
    }

    /// The period for a rate in Hz at the tick `fixed_dt`: `max(1, round(1 / (hz · dt)))` (a rate above the tick
    /// rate runs each tick).
    pub fn hz(hz: f64, fixed_dt: f64) -> Every {
        if !(hz.is_finite() && hz > 0.0 && fixed_dt > 0.0) {
            return Every::new(1);
        }
        Every::new(crate::js::js_round(1.0 / (hz * fixed_dt)).max(1.0) as u64)
    }

    /// Whether the system runs at `tick`.
    pub fn due(&self, tick: u64) -> bool {
        self.every <= 1 || (tick.wrapping_add(self.offset)).is_multiple_of(self.every)
    }

    /// The time a run stands for: `every` ticks, in seconds at the tick `fixed_dt` (`kx.dt()`): periods are counted
    /// in ticks, this is their length in seconds at the game's rate.
    pub fn dt(&self, fixed_dt: f64) -> f64 {
        self.every.max(1) as f64 * fixed_dt
    }

    /// The period in seconds at the rate `hz` (the docs' helper: `Every::new(3).seconds(30.0)` = 0.1 s).
    pub fn seconds(&self, hz: f64) -> f64 {
        self.every.max(1) as f64 / valid_hz(hz)
    }

    /// The entities this tick processes: each entity once every `every` ticks, a different 1/N each tick.
    pub fn slice(&self, tick: u64) -> Slice {
        let every = self.every.max(1);
        Slice {
            every,
            base: tick.wrapping_add(self.offset) % every,
        }
    }
}

/// The share of entities one tick of a spread system processes: the keys `k` with `(tick + offset + k) % every == 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slice {
    pub every: u64,
    base: u64,
}

impl Slice {
    /// Whether the entity of numeric key `key` is in this tick's share.
    pub fn has_key(&self, key: u64) -> bool {
        self.every <= 1 || (self.base + key % self.every).is_multiple_of(self.every)
    }

    /// Whether the entity `id` is in this tick's share (its key: [`id_key`]).
    pub fn has(&self, id: &str) -> bool {
        self.every <= 1 || self.has_key(id_key(id))
    }
}

/// The key that spreads an id: the number it ends with (`e42` → 42: consecutive entities land on consecutive ticks),
/// else a stable hash of the text (FNV-1a).
pub fn id_key(id: &str) -> u64 {
    let digits = id.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits > 0 && digits <= 18 {
        if let Ok(n) = id[id.len() - digits..].parse::<u64>() {
            return n;
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The config of game.engine in a project (`installed[{ id: "game.engine", config }]`), if any.
pub fn engine_config(project: &Json) -> Option<&Json> {
    project
        .get("installed")?
        .as_array()?
        .iter()
        .find(|k| k.get("id").and_then(Json::as_str) == Some("game.engine"))?
        .get("config")
}

/// The fixed tick of a project in seconds: `1 / game.engine.tick.hz` (default [`DEFAULT_TICK_HZ`], bounded to
/// [`MIN_TICK_HZ`]..[`MAX_TICK_HZ`]).
pub fn tick_dt_of(project: &Json) -> f64 {
    let hz = engine_config(project)
        .and_then(|c| c.get("tick"))
        .and_then(|t| t.get("hz"))
        .and_then(Json::as_f64)
        .map_or(DEFAULT_TICK_HZ, valid_hz);
    1.0 / hz
}

/// The tuned periods of a project (`game.engine.tick.systems`), as `[name, every]` (what the debug panel lists).
pub fn tuned_systems(project: &Json, fixed_dt: f64) -> Vec<(String, u64)> {
    let Some(map) = engine_config(project)
        .and_then(|c| c.get("tick"))
        .and_then(|t| t.get("systems"))
        .and_then(Json::as_object)
    else {
        return Vec::new();
    };
    map.keys()
        .filter_map(|k| {
            let (kit, system) = k.rsplit_once('.')?;
            Some((
                k.clone(),
                resolve(project, kit, system, Every::new(1), fixed_dt).every,
            ))
        })
        .collect()
}

/// The catch-up bound of a project (`game.engine.tick.maxCatchUp`): `u32::MAX` (none) unless the game sets a number
/// of ticks (1..64). 0 or absent: no cap, every due tick is played (a frame hands the clock [`MAX_FRAME`] at most).
pub fn max_catch_up_of(project: &Json) -> u32 {
    engine_config(project)
        .and_then(|c| c.get("tick"))
        .and_then(|t| t.get("maxCatchUp"))
        .and_then(Json::as_f64)
        .filter(|n| n.is_finite() && *n >= 1.0)
        .map_or(u32::MAX, |n| n.clamp(1.0, 64.0) as u32)
}

/// The loops of a game as `game.engine.tick` describes them (the hosts plug each on the callback of their engine).
#[derive(Clone, Debug, PartialEq)]
pub struct Loops {
    /// `independent` (default: the TickClock runs on real time) or `engine-physics` (counted from the physics loop of the engine).
    pub source: String,
    /// The physics step the host sets in Hz: `tick.physicsHz`, else `tick.hz` when the game sets it and `syncPhysics`
    /// is on; None: the engine setting is left alone.
    pub physics_hz: Option<f64>,
    /// Render interpolation between two ticks.
    pub interpolate: bool,
    /// Where the action loop runs (`game.engine.action.thread`): `main` (default: on the engine's main thread, in
    /// its callbacks) or `own` (a thread of its own on a computer, a Web Worker on the web; the host falls back to
    /// `main` with its reason when it cannot).
    pub thread: String,
}

/// Resolve the requested physics rate without allocating loop descriptions on a frame path.
pub fn physics_hz_of(project: &Json) -> Option<f64> {
    physics_hz(project, None)
}

/// Resolve syncPhysics against the actual action tick, including an embedder's fixedDt override.
/// An explicit physicsHz still takes priority; without a configured tick keep the engine default.
pub fn physics_hz_at_tick(project: &Json, tick_hz: f64) -> Option<f64> {
    physics_hz(project, Some(tick_hz))
}

fn physics_hz(project: &Json, actual_tick_hz: Option<f64>) -> Option<f64> {
    let tick = engine_config(project).and_then(|c| c.get("tick"));
    let get = |k: &str| tick.and_then(|t| t.get(k));
    let explicit = get("physicsHz")
        .and_then(Json::as_f64)
        .filter(|h| h.is_finite() && *h >= 1.0)
        .map(|h| h.clamp(1.0, 240.0));
    let sync = get("syncPhysics").and_then(Json::as_bool).unwrap_or(true);
    let tick_hz = get("hz")
        .and_then(Json::as_f64)
        .map(|hz| valid_hz(actual_tick_hz.unwrap_or(hz)));
    explicit.or(if sync { tick_hz } else { None })
}

/// The loops of a project (`game.engine.tick.{source, physicsHz, syncPhysics, interpolate}`).
pub fn loops_of(project: &Json) -> Loops {
    let tick = engine_config(project).and_then(|c| c.get("tick"));
    let get = |k: &str| tick.and_then(|t| t.get(k));
    let source = match get("source").and_then(Json::as_str) {
        Some("engine-physics") => "engine-physics",
        _ => "independent",
    };
    Loops {
        source: source.to_string(),
        physics_hz: physics_hz_of(project),
        interpolate: get("interpolate").and_then(Json::as_bool).unwrap_or(true),
        thread: match engine_config(project)
            .and_then(|c| c.get("action"))
            .and_then(|a| a.get("thread"))
            .and_then(Json::as_str)
        {
            Some("own") => "own",
            _ => "main",
        }
        .to_string(),
    }
}

/// The period of `kit.system` as the game tunes it: `game.engine.tick.systems["kit.system"]` — a number (`every`) or
/// `{ every?, offset?, hz? }` —, else `default`. The phase is spread by the name unless the game sets `offset`.
pub fn resolve(project: &Json, kit: &str, system: &str, default: Every, fixed_dt: f64) -> Every {
    let key = format!("{kit}.{system}");
    let tuned = engine_config(project)
        .and_then(|c| c.get("tick"))
        .and_then(|t| t.get("systems"))
        .and_then(|s| s.get(key.as_str()));
    let spread = |every: u64| Every::spread(&key, every);
    match tuned {
        Some(Json::Number(n)) => spread(n.as_f64().unwrap_or(1.0).clamp(1.0, 600.0) as u64),
        Some(Json::Object(o)) => {
            let mut e = match (
                o.get("every").and_then(Json::as_f64),
                o.get("hz").and_then(Json::as_f64),
            ) {
                (Some(n), _) => spread(n.clamp(1.0, 600.0) as u64),
                (None, Some(hz)) => spread(Every::hz(hz, fixed_dt).every),
                (None, None) => default,
            };
            if let Some(off) = o.get("offset").and_then(Json::as_f64) {
                e.offset = (off.max(0.0) as u64) % e.every.max(1);
            }
            e
        }
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DT: f64 = 1.0 / 30.0;

    #[test]
    fn clock_runs_the_ticks_of_the_wall_time() {
        let mut c = TickClock::default();
        // 60 frames of 1/60 s: 30 ticks of 1/30 s, one every other frame.
        let ticks: Vec<u32> = (0..60).map(|_| c.advance(1.0 / 60.0, DT, 4)).collect();
        assert_eq!(ticks.iter().sum::<u32>(), 30);
        assert!(ticks.iter().all(|&t| t <= 1));
        // 144 Hz display: still 30 ticks a second.
        let mut c = TickClock::default();
        assert_eq!(
            (0..144).map(|_| c.advance(1.0 / 144.0, DT, 4)).sum::<u32>(),
            30
        );
    }

    #[test]
    fn clock_alpha_is_the_part_of_a_tick() {
        let mut c = TickClock::default();
        c.advance(DT * 0.25, DT, 4);
        assert!((c.alpha(DT) - 0.25).abs() < 1e-9);
        assert_eq!(c.advance(DT * 0.75, DT, 4), 1);
        assert!(c.alpha(DT) < 1e-6);
    }

    #[test]
    fn clock_bounds_the_catch_up() {
        let mut c = TickClock::default();
        // A 200 ms hitch: 6 ticks due, 4 run, the rest dropped, the phase kept.
        assert_eq!(c.advance(0.2 + DT * 0.5, DT, 4), 4);
        assert!((c.alpha(DT) - 0.5).abs() < 1e-6);
        assert!(c.dropped > 2.0 * DT - 1e-6);
        // Never more than MAX_FRAME of wall time a frame.
        let mut c = TickClock::default();
        assert_eq!(c.advance(10.0, DT, 100), (MAX_FRAME / DT) as u32);
        // Then real time again: one tick a tick.
        assert_eq!(c.advance(DT, DT, 4), 1);
    }

    #[test]
    fn clock_follows_an_external_clock() {
        let mut c = TickClock::default();
        c.set_rate(1.05);
        assert_eq!(
            (0..600).map(|_| c.advance(1.0 / 60.0, DT, 4)).sum::<u32>(),
            315
        );
        c.set_rate(5.0);
        assert_eq!(c.rate(), 1.1);
        // A live change of rate keeps the fraction of a tick (the presentation does not jump).
        let mut c = TickClock::default();
        c.advance(DT * 0.5, DT, 4);
        c.rescale(DT, DT / 2.0);
        assert!((c.alpha(DT / 2.0) - 0.5).abs() < 1e-9);
        let mut c = TickClock::default();
        c.shift(1.0, DT);
        assert_eq!(c.advance(0.0, DT, 4), 1);
        c.shift(-3.0, DT);
        assert_eq!(c.accumulated(), 0.0);
    }

    #[test]
    fn every_spreads_systems_and_entities() {
        let e = Every::new(3);
        assert_eq!((0..9).filter(|t| e.due(*t)).count(), 3);
        // Two systems of the same period with different phases never run on the same tick.
        let a = Every {
            every: 2,
            offset: 0,
        };
        let b = Every {
            every: 2,
            offset: 1,
        };
        assert!((0..10).all(|t| a.due(t) != b.due(t)));
        // Each entity once every 4 ticks, a quarter each tick.
        let e = Every::new(4);
        for id in ["e1", "e2", "e7", "npc_crow", "boss"] {
            assert_eq!((0..8).filter(|t| e.slice(*t).has(id)).count(), 2, "{id}");
        }
        assert_eq!((0..100u64).filter(|k| e.slice(5).has_key(*k)).count(), 25);
        // The formula npc.core always used: (tick + key % period) % period == 0.
        for tick in 0..12u64 {
            for k in 0..12u64 {
                assert_eq!(
                    Every::new(3).slice(tick).has_key(k),
                    (tick + k % 3) % 3 == 0
                );
            }
        }
        assert_eq!(Every::hz(10.0, DT).every, 3);
        assert_eq!(Every::hz(10.0, 1.0 / 60.0).every, 6);
        assert_eq!(Every::hz(120.0, DT).every, 1);
        assert_eq!(Every::new(1).dt(DT), DT);
    }

    #[test]
    fn resolve_reads_the_game_s_tuning() {
        let p = json!({ "installed": [{ "id": "game.engine", "config": { "tick": { "hz": 20, "maxCatchUp": 2,
            "systems": { "npc.core.decide": 5, "world.lighting.update": { "every": 4, "offset": 1 }, "quest.graph.check": { "hz": 5 } } } } }] });
        assert!((tick_dt_of(&p) - 0.05).abs() < 1e-12);
        assert_eq!(max_catch_up_of(&p), 2);
        assert_eq!(
            resolve(&p, "npc.core", "decide", Every::new(1), 0.05).every,
            5
        );
        assert_eq!(
            resolve(&p, "world.lighting", "update", Every::new(1), 0.05),
            Every {
                every: 4,
                offset: 1
            }
        );
        assert_eq!(
            resolve(&p, "quest.graph", "check", Every::new(1), 0.05).every,
            4
        );
        assert_eq!(
            resolve(&p, "combat.melee", "hits", Every::new(1), 0.05),
            Every::new(1)
        );
        assert!((tick_dt_of(&json!({})) - 1.0 / 30.0).abs() < 1e-12);
        let fast =
            json!({ "installed": [{ "id": "game.engine", "config": { "tick": { "hz": 1000 } } }] });
        assert!((tick_dt_of(&fast) - 1.0 / MAX_TICK_HZ).abs() < 1e-12);
        assert_eq!(tuned_systems(&p, 0.05).len(), 3);
        assert!((Every::new(3).seconds(30.0) - 0.1).abs() < 1e-12);
        assert_eq!(max_catch_up_of(&json!({})), u32::MAX);
        let l = loops_of(&p);
        assert_eq!(
            (l.source.as_str(), l.physics_hz, l.interpolate),
            ("independent", Some(20.0), true)
        );
        let l = loops_of(
            &json!({ "installed": [{ "id": "game.engine", "config": { "tick": { "hz": 20, "source": "independent", "syncPhysics": false, "interpolate": false } } }] }),
        );
        assert_eq!(
            (l.source.as_str(), l.physics_hz, l.interpolate),
            ("independent", None, false)
        );
        let l = loops_of(
            &json!({ "installed": [{ "id": "game.engine", "config": { "tick": { "hz": 20, "physicsHz": 120, "source": "engine-physics" } } }] }),
        );
        assert_eq!(
            (l.source.as_str(), l.physics_hz),
            ("engine-physics", Some(120.0))
        );
        assert_eq!(loops_of(&json!({})).physics_hz, None);
        assert_eq!(loops_of(&json!({})).thread, "main");
        let own = json!({ "installed": [{ "id": "game.engine", "config": { "action": { "thread": "own" } } }] });
        assert_eq!(loops_of(&own).thread, "own");
        let odd = json!({ "installed": [{ "id": "game.engine", "config": { "action": { "thread": "gpu" } } }] });
        assert_eq!(loops_of(&odd).thread, "main");
    }
}
