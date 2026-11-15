//! Façade of src/state.rs: the variable specs are shared with the core (src/state/var_spec.rs, which the core compiles too); the
//! state lives in the core (`kithost.state@1`). A value read by reference is a copy kept until the next write or
//! entry of the core (as src/entities.rs of this crate).

mod var_spec;
pub use var_spec::{HudVarSpec, VarChange, PHASES, PHASE_KEY};

use crate::entities::Arena;
use crate::host::call_core_ok;
use gripforge_abi::{state as f, Reader, Writer};
use serde_json::Value as Json;
use std::cell::RefCell;

pub(crate) fn call(func: crate::uses::Func, args: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = crate::host::writer();
    args(&mut w);
    call_core_ok(func, &w)
}

/// The phase as read now (`host.state.phase`, a field of the core's `GameState`): compares with a `&str` and
/// clones to a `String`, asking the core each time.
#[derive(Default)]
pub struct Phase {}

impl Phase {
    /// The phase now.
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> String {
        if let Some(p) = crate::direct::phase() {
            return p;
        }
        let bytes = call(crate::func!(f::PHASE), |_| {});
        Reader::new(&bytes).string().unwrap_or_default()
    }
    pub fn as_str(&self) -> String {
        self.clone()
    }
}

impl PartialEq<&str> for Phase {
    fn eq(&self, other: &&str) -> bool {
        self.clone() == *other
    }
}
impl PartialEq<str> for Phase {
    fn eq(&self, other: &str) -> bool {
        self.clone() == other
    }
}
impl PartialEq<String> for Phase {
    fn eq(&self, other: &String) -> bool {
        self.clone() == *other
    }
}
impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.clone())
    }
}

impl Reads {
    /// Drops every copy and keeps the tables (most entries read the same few paths again).
    fn clear(&mut self, rev: u32) {
        if !self.vars.is_empty() || !self.values.is_empty() || self.phase.is_some() {
            self.vars.clear();
            self.values = Arena::default();
            self.strings = Arena::default();
            self.phase = None;
        }
        self.rev = rev;
    }
}

#[derive(Default)]
struct Reads {
    values: Arena<Json>,
    strings: Arena<str>,
    /// path → its copy (None: no value).
    vars: crate::direct::FastMap<String, Option<*const Json>>,
    phase: Option<*const str>,
    /// The revision of the state in the shared zone these copies were read under (0: unknown — dropped at the next
    /// entry).
    rev: u32,
}

/// The revision of the game state in the shared zone (the `STATE` buffer, docs/kit-modules.md §7): it changes at
/// every write of a var or of the phase, whoever makes it. 0: no zone (off wasm) or nothing published.
fn state_rev() -> u32 {
    #[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
    {
        let b = crate::zone::state();
        if b.size != 0 {
            return crate::zone::zone_read_u32(b.offset + crate::zone::layout::state::REV);
        }
    }
    0
}

/// `host.state`.
#[derive(Default)]
pub struct GameState {
    pub phase: Phase,
    reads: RefCell<Reads>,
    /// The moment of the module this façade last looked at (`host::moment`, as `Entities::settle`).
    seen: std::cell::Cell<u64>,
}

impl GameState {
    /// The revision of the state in the shared zone, as the core's `GameState::rev` (0: not known here — a reader
    /// that keys a cache on it compares the values instead).
    pub fn rev(&self) -> u32 {
        state_rev()
    }

    /// A new entry of the core (or a call that may have run other kits): drop what was read.
    ///
    /// What was read holds while the revision of the state in the shared zone is the one it was read under: the
    /// copies then live from one entry to the next (most entries change no var).
    pub fn enter(&mut self) {
        self.seen.set(crate::host::moment());
        let rev = state_rev();
        let reads = self.reads.get_mut();
        if rev == 0 || reads.rev != rev {
            reads.clear(rev);
        }
    }

    /// The first read since the moment moved (a new entry, a call that may have run other kits: no reference
    /// handed out before is alive): what [`GameState::enter`] does.
    #[inline]
    fn settle(&self) {
        let now = crate::host::moment();
        if self.seen.get() != now {
            self.seen.set(now);
            let rev = state_rev();
            let mut reads = self.reads.borrow_mut();
            if rev == 0 || reads.rev != rev {
                reads.clear(rev);
            }
        }
    }

    pub(crate) fn wrote(&mut self) {
        // Also the negative answers (a var cached as absent may now be set).
        self.reads.get_mut().clear(0);
    }

    pub fn phase(&self) -> &str {
        self.settle();
        if let Some(p) = self.reads.borrow().phase {
            // SAFETY: kept in the arena, which is only cleared through `&mut self`.
            return unsafe { &*p };
        }
        let s = self.phase.clone();
        let mut r = self.reads.borrow_mut();
        let p = r.strings.keep(s.into_boxed_str());
        r.phase = Some(p);
        // SAFETY: as above.
        unsafe { &*p }
    }

    /// The value of a path (a copy kept until the next write or entry).
    pub fn get(&self, path: &str) -> Option<&Json> {
        self.settle();
        if let Some(hit) = self.reads.borrow().vars.get(path).copied() {
            // SAFETY: as `phase`.
            return hit.map(|p| unsafe { &*p });
        }
        // Absent or a scalar: a direct link; else the bridge.
        let got = crate::direct::var(path).unwrap_or_else(|| {
            let bytes = call(crate::func!(f::GET), |w| {
                w.str(path);
            });
            Reader::new(&bytes).opt_value().ok().flatten()
        });
        let mut r = self.reads.borrow_mut();
        let p = got.map(|v| r.values.keep(Box::new(v)));
        r.vars.insert(path.to_string(), p);
        // SAFETY: as `phase`.
        p.map(|p| unsafe { &*p })
    }

    /// `GameState::set`: a direct write (no guard, no drain).
    pub fn set(&mut self, path: &str, value: Json) {
        self.wrote();
        call(crate::func!(f::SET), |w| {
            w.str(path).value(&value);
        });
    }

    pub fn inc(&mut self, path: &str, delta: f64) -> f64 {
        self.wrote();
        let bytes = call(crate::func!(f::INC), |w| {
            w.str(path).f64(delta);
        });
        Reader::new(&bytes).f64().unwrap_or(0.0)
    }

    /// Every value plus `$phase`.
    pub fn snapshot(&self) -> Json {
        let bytes = call(crate::func!(f::SNAPSHOT), |_| {});
        Reader::new(&bytes).value().unwrap_or(Json::Null)
    }

    /// The values in definition order (`None`: the path holds `undefined`), `$phase` apart.
    pub fn values(&self) -> Vec<(String, Option<Json>)> {
        let bytes = call(crate::func!(f::VALUES), |_| {});
        let mut r = Reader::new(&bytes);
        let n = r.u32().unwrap_or(0);
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let (Ok(k), Ok(v)) = (r.string(), r.opt_value()) else {
                break;
            };
            out.push((k, v));
        }
        out
    }

    pub fn vars(&self) -> Vec<HudVarSpec> {
        let bytes = call(crate::func!(f::VARS), |_| {});
        let mut r = Reader::new(&bytes);
        let n = r.u32().unwrap_or(0);
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let Ok(v) = r.value() else { break };
            let s = |k: &str| v.get(k).and_then(Json::as_str).unwrap_or("").to_string();
            out.push(HudVarSpec {
                path: s("path"),
                kind: s("type"),
                label: s("label"),
                format: v.get("format").and_then(Json::as_str).map(str::to_string),
                owner: s("owner"),
            });
        }
        out
    }
}
