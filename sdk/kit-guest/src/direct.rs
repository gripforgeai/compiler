//! The kit side of the direct links of `kithost.world@1.1`, `kithost.state@1.1` and `kithost.services@1.1`
//! (docs/kit-modules.md §7; the core side: gripforge-kithost src/kitcall/direct.rs): the simple accesses as
//! wasm-to-wasm calls of scalars — no string crosses, nothing is copied nor allocated.
//!
//! - a name of the game's vocabulary (a component, a var path, a capability) is an **atom**, a number asked of the
//!   core once per name (`services::ATOM`) and kept here;
//! - an entity id travels **packed** in two `u64` (≤ 16 bytes; a longer one answers `None`: take the bridge);
//! - a scalar comes back as a kind and the registers of the core.
//!
//! Every function answers `None` when the direct link cannot carry the call (not a wasm build, a long id, a value
//! that is not a scalar): the caller then makes the same call by the bridge, which answers the same.

use serde_json::Value as Json;

/// The kinds of a scalar read (as the core's).
const ABSENT: u32 = 0;
const NULL: u32 = 1;
const FALSE: u32 = 2;
const TRUE: u32 = 3;
const INT: u32 = 4;
const FLOAT: u32 = 5;
const SHORT_STR: u32 = 7;

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
mod link {
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.world@^1.1")]
    extern "C" {
        pub fn w_has(lo: u64, hi: u64) -> u32;
        pub fn w_transform(lo: u64, hi: u64) -> u32;
        pub fn w_component(lo: u64, hi: u64, component: u32) -> u32;
        pub fn w_acting_player() -> u32;
        pub fn w_scope_push(lo: u64, hi: u64);
        pub fn w_scope_pop() -> u32;
    }
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.world@^1.8")]
    extern "C" {
        pub fn w_has_tag(lo: u64, hi: u64, tag: u32) -> u32;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    pub use crate::native::link::{
        w_acting_player, w_component, w_has, w_has_tag, w_scope_pop, w_scope_push, w_transform,
    };
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.state@^2.0")]
    extern "C" {
        pub fn st_get(path: u32) -> u32;
        pub fn st_set_f64(path: u32, value: f64);
        pub fn st_set_i64(path: u32, value: i64);
        pub fn st_set_flag(path: u32, what: u32);
        pub fn st_set_str(path: u32, lo: u64, hi: u64);
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    pub use crate::native::link::{st_get, st_set_f64, st_set_flag, st_set_i64, st_set_str};
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.services@^1.1")]
    extern "C" {
        pub fn sv_has(capability: u32) -> u32;
        pub fn reg_f64(i: u32) -> f64;
        pub fn reg_u64(i: u32) -> u64;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    pub use crate::native::link::{reg_f64, reg_u64, sv_has};
}

/// Without the links (a native build of the façade: `cargo check`, tests): every call takes the bridge.
#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
#[allow(clippy::missing_safety_doc)]
mod link {
    pub unsafe fn w_has(_: u64, _: u64) -> u32 {
        0
    }
    pub unsafe fn w_transform(_: u64, _: u64) -> u32 {
        0
    }
    pub unsafe fn w_component(_: u64, _: u64, _: u32) -> u32 {
        6
    }
    pub unsafe fn w_acting_player() -> u32 {
        6
    }
    pub unsafe fn w_scope_push(_: u64, _: u64) {}
    pub unsafe fn w_scope_pop() -> u32 {
        0
    }
    pub unsafe fn w_has_tag(_: u64, _: u64, _: u32) -> u32 {
        0
    }
    pub unsafe fn st_set_str(_: u32, _: u64, _: u64) {}
    pub unsafe fn st_get(_: u32) -> u32 {
        6
    }
    pub unsafe fn st_set_f64(_: u32, _: f64) {}
    pub unsafe fn st_set_i64(_: u32, _: i64) {}
    pub unsafe fn st_set_flag(_: u32, _: u32) {}
    pub unsafe fn sv_has(_: u32) -> u32 {
        0
    }
    pub unsafe fn reg_f64(_: u32) -> f64 {
        0.0
    }
    pub unsafe fn reg_u64(_: u32) -> u64 {
        0
    }
}

/// Whether the direct links exist in this build.
pub const LINKED: bool = cfg!(any(target_arch = "wasm32", feature = "common-memory"));

/// A short-key hasher (FNV-1a) for the tables of this façade: the keys are names of a few bytes looked up many
/// times per tick, where the default SipHash costs more than the lookup itself. Not for keys an attacker chooses.
#[derive(Clone, Copy)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// A `HashMap` on [`Fnv`].
pub type FastMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<Fnv>>;

/// The atom of a name: asked of the core at its first use (one bridge call), then kept for the life of the module.
pub fn atom(name: &str) -> u32 {
    use std::cell::{Cell, RefCell};
    thread_local! {
        static ATOMS: RefCell<FastMap<String, u32>> = RefCell::new(FastMap::default());
        /// The last short names asked, by their bytes (no hash: the same few names are asked many times a tick —
        /// the components of a neighbour search, an animation).
        static LAST: RefCell<[(u8, [u8; 23], u32); 16]> = const { RefCell::new([(0, [0; 23], 0); 16]) };
        static NEXT: Cell<usize> = const { Cell::new(0) };
    }
    let b = name.as_bytes();
    if !b.is_empty() && b.len() <= 23 {
        let hit = LAST.with(|l| {
            l.borrow()
                .iter()
                .find(|(n, bytes, _)| *n as usize == b.len() && &bytes[..b.len()] == b)
                .map(|e| e.2)
        });
        if let Some(id) = hit {
            return id;
        }
    }
    let remember = |id: u32| {
        if !b.is_empty() && b.len() <= 23 {
            let k = NEXT.with(|n| {
                let k = n.get();
                n.set((k + 1) % 16);
                k
            });
            let mut bytes = [0u8; 23];
            bytes[..b.len()].copy_from_slice(b);
            LAST.with(|l| l.borrow_mut()[k] = (b.len() as u8, bytes, id));
        }
        id
    };
    if let Some(id) = ATOMS.with(|a| a.borrow().get(name).copied()) {
        return remember(id);
    }
    let mut w = crate::host::writer();
    w.str(name);
    let bytes = crate::host::call_core_ok(crate::func!(gripforge_abi::services::ATOM), &w);
    let id = gripforge_abi::Reader::new(&bytes).u32().unwrap_or(u32::MAX);
    ATOMS.with(|a| a.borrow_mut().insert(name.to_string(), id));
    remember(id)
}

/// A short string packed in two `u64` (≤ 16 bytes, no zero byte).
/// An entity id packed for a direct link of another file of the façade (`None`: too long, or no links here).
#[allow(
    dead_code,
    reason = "kept for the other files of the façade that pack ids; unused in a build without them"
)]
pub(crate) fn pack_id(s: &str) -> Option<(u64, u64)> {
    pack(s)
}

fn pack(s: &str) -> Option<(u64, u64)> {
    let b = s.as_bytes();
    if !LINKED || b.len() > 16 || b.contains(&0) {
        return None;
    }
    let mut buf = [0u8; 16];
    buf[..b.len()].copy_from_slice(b);
    Some((
        u64::from_le_bytes(buf[..8].try_into().unwrap()),
        u64::from_le_bytes(buf[8..].try_into().unwrap()),
    ))
}

/// The short string of the registers 0 and 1.
fn reg_string() -> String {
    // SAFETY: imports wired by the calls host to the core's exports; they read two words.
    let (lo, hi) = unsafe { (link::reg_u64(0), link::reg_u64(1)) };
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&lo.to_le_bytes());
    buf[8..].copy_from_slice(&hi.to_le_bytes());
    let len = buf.iter().position(|b| *b == 0).unwrap_or(16);
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

/// The scalar of a kind: `Some(None)` absent, `Some(Some(value))`, or `None` (not a scalar: take the bridge).
fn scalar(kind: u32) -> Option<Option<Json>> {
    // SAFETY: as `reg_string`.
    Some(match kind {
        ABSENT => None,
        NULL => Some(Json::Null),
        FALSE => Some(Json::Bool(false)),
        TRUE => Some(Json::Bool(true)),
        INT => Some(Json::from(unsafe { link::reg_u64(0) } as i64)),
        FLOAT => Some(
            serde_json::Number::from_f64(unsafe { link::reg_f64(0) })
                .map(Json::Number)
                .unwrap_or(Json::Null),
        ),
        SHORT_STR => Some(Json::String(reg_string())),
        _ => return None,
    })
}

/// `entities.has(id)`.
pub fn has(id: &str) -> Option<bool> {
    let (lo, hi) = pack(id)?;
    // SAFETY: an import wired by the calls host to the core's export of the same name.
    Some(unsafe { link::w_has(lo, hi) } != 0)
}

/// `entities.transform(id)`: `Some(None)` when there is none.
pub fn transform(id: &str) -> Option<Option<[f64; 4]>> {
    let (lo, hi) = pack(id)?;
    // SAFETY: as `has`.
    unsafe {
        if link::w_transform(lo, hi) == 0 {
            return Some(None);
        }
        Some(Some([
            link::reg_f64(0),
            link::reg_f64(1),
            link::reg_f64(2),
            link::reg_f64(3),
        ]))
    }
}

/// [`transform`] with the revision the physics published for the entity's body when it owns the pose (`None`: the
/// pose is the entity's own): what a kept copy compares to know its transform still is the body's.
pub fn transform_body(id: &str) -> Option<Option<([f64; 4], Option<u32>)>> {
    let pose = transform(id)?;
    // SAFETY: the register the call above just wrote.
    let body = unsafe { link::reg_u64(4) };
    Some(pose.map(|p| (p, (body != u64::MAX).then_some(body as u32))))
}

/// `entities.component(id, name)` when it is absent or a scalar.
pub fn component(id: &str, name: &str) -> Option<Option<Json>> {
    let (lo, hi) = pack(id)?;
    // SAFETY: as `has`.
    scalar(unsafe { link::w_component(lo, hi, atom(name)) })
}

/// `host.acting_player()`.
pub fn acting_player() -> Option<Option<String>> {
    if !LINKED {
        return None;
    }
    // SAFETY: as `has`.
    match unsafe { link::w_acting_player() } {
        ABSENT => Some(None),
        SHORT_STR => Some(Some(reg_string())),
        _ => None,
    }
}

/// `kx.get_var(path)` when it is absent or a scalar.
pub fn var(path: &str) -> Option<Option<Json>> {
    if !LINKED {
        return None;
    }
    // SAFETY: as `has`.
    scalar(unsafe { link::st_get(atom(path)) })
}

/// `kx.set_var(path, value)` for a number, a boolean or null: false when the value is not one (take the bridge).
pub fn set_var(path: &str, value: &Json) -> bool {
    if !LINKED {
        return false;
    }
    // SAFETY: as `has`.
    unsafe {
        match value {
            Json::Null => link::st_set_flag(atom(path), 0),
            Json::Bool(b) => link::st_set_flag(atom(path), 1 + *b as u32),
            Json::Number(n) => match (n.as_i64(), n.is_f64()) {
                (Some(i), _) => link::st_set_i64(atom(path), i),
                (None, true) => link::st_set_f64(atom(path), n.as_f64().unwrap_or(0.0)),
                _ => return false,
            },
            Json::String(text) => match pack(text) {
                Some((lo, hi)) => link::st_set_str(atom(path), lo, hi),
                None => return false,
            },
            _ => return false,
        }
    }
    true
}

/// `host.player_scopes.push(id)`: false when the id does not pack (take the bridge).
pub fn scope_push(id: &str) -> bool {
    let Some((lo, hi)) = pack(id) else {
        return false;
    };
    // SAFETY: as `has`.
    unsafe { link::w_scope_push(lo, hi) };
    true
}

/// `host.player_scopes.pop()`: `None` when the links are missing (take the bridge).
pub fn scope_pop() -> Option<Option<String>> {
    if !LINKED {
        return None;
    }
    // SAFETY: as `has`.
    Some(match unsafe { link::w_scope_pop() } {
        ABSENT => None,
        SHORT_STR => Some(reg_string()),
        // Popped, but too long for the registers: the core kept it.
        _ => {
            let bytes = crate::host::call_core_ok(
                crate::func!(gripforge_abi::world::SCOPE_POPPED),
                &gripforge_abi::Writer::new(),
            );
            gripforge_abi::Reader::new(&bytes)
                .opt_str()
                .ok()
                .flatten()
                .map(str::to_string)
        }
    })
}

/// Whether an entity has a tag, without fetching a copy of it. `None`: off the links (a façade checked natively, an id
/// that does not pack): the caller reads its copy.
pub fn has_tag(entity: &str, tag: &str) -> Option<bool> {
    if !cfg!(any(target_arch = "wasm32", feature = "common-memory")) {
        return None;
    }
    let (lo, hi) = pack(entity)?;
    // SAFETY: as `has`.
    Some(unsafe { link::w_has_tag(lo, hi, atom(tag)) } != 0)
}

/// Whether an entity has a component right now (`None`: as [`has_tag`]): the read of `w_component`, nothing fetched.
pub fn has_component(entity: &str, component: &str) -> Option<bool> {
    if !cfg!(any(target_arch = "wasm32", feature = "common-memory")) {
        return None;
    }
    let (lo, hi) = pack(entity)?;
    // SAFETY: as `has`.
    Some(unsafe { link::w_component(lo, hi, atom(component)) } != ABSENT)
}

/// `host.has_service(capability)`.
pub fn has_service(capability: &str) -> Option<bool> {
    if !LINKED {
        return None;
    }
    // SAFETY: as `has`.
    Some(unsafe { link::sv_has(atom(capability)) } != 0)
}

/// Measures only (feature `entry-probe`): one call of the cheapest direct link (`w_acting_player`), its answer.
#[cfg(feature = "entry-probe")]
pub fn link_probe() -> u32 {
    // SAFETY: an import wired by the calls host to the core's export of the same name.
    unsafe { link::w_acting_player() }
}
