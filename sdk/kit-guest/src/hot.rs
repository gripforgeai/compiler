//! The copies of entities a kit module keeps (docs/kit-modules.md §7 step 4; the core side: src/kitcall/hot.rs of
//! the kit host, the contract: gripforge-kitcall src/fns_hot.rs).
//!
//! Every entity of the core has a revision and the world an epoch (the clock value of its last change), read by a
//! direct link (`hot_epoch`: a wasm-to-wasm call). A copy checked at the epoch the world still has is the entity:
//! no call. When the epoch moved, the module tells the core which revision it holds and gets "unchanged", the
//! components changed since, or the whole entity; nothing it already has is encoded, copied or decoded again.

use crate::entities::Entity;
use gripforge_abi::{hot as f, Reader};
use serde_json::{Map, Value as Json};

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.world@^1.1")]
extern "C" {
    fn hot_epoch() -> f64;
}
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
use crate::native::link::hot_epoch;

/// The epoch of the world now.
#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
pub(crate) fn epoch() -> u64 {
    // In the common memory the core writes it in the header of the entity rows at every change: a load.
    if let Some(e) = crate::columns::table()
        .map(|t| t.epoch())
        .filter(|e| *e != 0)
    {
        return e;
    }
    // SAFETY: an import wired by the calls host to the export `gfx_hot_epoch` of the core.
    unsafe { hot_epoch() as u64 }
}

#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
pub(crate) fn epoch() -> u64 {
    let bytes = crate::host::call_core_ok(crate::func!(f::EPOCH), &gripforge_abi::Writer::new());
    Reader::new(&bytes).u64().unwrap_or(0)
}

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.world@^1.1")]
extern "C" {
    fn hot_structure() -> f64;
}
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
use crate::native::link::hot_structure;

/// The clock value of the last change of what a query reads (the list, a kind, tags).
#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
pub(crate) fn structure() -> u64 {
    // SAFETY: an import wired by the calls host to the export `gfx_hot_structure` of the core.
    unsafe { hot_structure() as u64 }
}

#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
pub(crate) fn structure() -> u64 {
    let bytes =
        crate::host::call_core_ok(crate::func!(f::STRUCTURE), &gripforge_abi::Writer::new());
    Reader::new(&bytes).u64().unwrap_or(0)
}

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.world@^1.1")]
extern "C" {
    fn hot_set_pose(lo: u64, hi: u64, mask: u32, x: f64, y: f64, z: f64, yaw: f64);
}
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
use crate::native::link::hot_set_pose;

/// The typed write of a transform by a direct link; false when the id does not pack in two words (≤ 16 bytes, no
/// zero byte): the caller takes the bridge.
#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
pub(crate) fn set_pose(id: &str, mask: u32, v: [f64; 4]) -> bool {
    let b = id.as_bytes();
    if b.len() > 16 || b.contains(&0) {
        return false;
    }
    let mut buf = [0u8; 16];
    buf[..b.len()].copy_from_slice(b);
    let lo = u64::from_le_bytes(buf[..8].try_into().unwrap());
    let hi = u64::from_le_bytes(buf[8..].try_into().unwrap());
    // SAFETY: an import wired by the calls host to the export `gfx_hot_set_pose` of the core.
    unsafe { hot_set_pose(lo, hi, mask, v[0], v[1], v[2], v[3]) };
    true
}

#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
pub(crate) fn set_pose(_: &str, _: u32, _: [f64; 4]) -> bool {
    false
}

/// The typed write of a transform by the direct link of a module that keeps its copies (common memory): the
/// fields of `mask`, the epoch this module holds. `None`: no such link here or the id does not pack — the caller
/// takes its other way. `Some(Err(()))`: the core refused the write (the call says why). `Some(Ok(None))`: no such
/// entity. `Some(Ok(Some([before, after, epoch, other])))`: as `SET_IF`, then the entity's `core.shape@1`.
#[cfg(feature = "common-memory")]
pub(crate) fn pose(
    id: &str,
    mask: u32,
    v: [f64; 4],
    epoch: u64,
) -> Option<Result<Option<[u64; 4]>, ()>> {
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.world@^1.2")]
    extern "C" {
        fn hot_pose(
            lo: u64,
            hi: u64,
            mask: u32,
            fields: *const f64,
            epoch: u64,
            out: *mut u64,
        ) -> u32;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    use crate::native::link::hot_pose;
    let (lo, hi) = crate::direct::pack_id(id)?;
    let mut out = [0u64; 4];
    // SAFETY: an import wired by the calls host to the export `gfx_hot_pose` of the core, which reads 4 `f64` and
    // writes 4 `u64` in this module's stack — in the memory both share.
    Some(
        match unsafe { hot_pose(lo, hi, mask, v.as_ptr(), epoch, out.as_mut_ptr()) } {
            0 => Ok(Some(out)),
            1 => Ok(None),
            _ => Err(()),
        },
    )
}

#[cfg(not(feature = "common-memory"))]
pub(crate) fn pose(_: &str, _: u32, _: [f64; 4], _: u64) -> Option<Result<Option<[u64; 4]>, ()>> {
    None
}

/// `*dst = src.clone()` that keeps what `dst` already holds of the same shape (an object with the same keys in the
/// same order, a string's buffer): nothing is allocated when a component is written again with new numbers.
pub(crate) fn assign(dst: &mut Json, src: &Json) {
    match (dst, src) {
        (Json::Object(d), Json::Object(s))
            if d.len() == s.len() && d.keys().zip(s.keys()).all(|(a, b)| a == b) =>
        {
            for (dv, sv) in d.values_mut().zip(s.values()) {
                assign(dv, sv);
            }
        }
        (Json::Array(d), Json::Array(s)) if d.len() == s.len() => {
            for (dv, sv) in d.iter_mut().zip(s) {
                assign(dv, sv);
            }
        }
        (Json::String(d), Json::String(s)) => {
            d.clear();
            d.push_str(s);
        }
        (d, s) => *d = s.clone(),
    }
}

/// A copy of an entity, kept from one hook to the next.
pub(crate) struct Kept {
    pub entity: Entity,
    /// The revision of the core this copy is.
    pub rev: u64,
    /// The check (`Reads::check`) it was last found current at.
    pub seen: u64,
    /// The era (`Reads::era`) a reference to it was last handed out in: it is not changed in place in that era.
    pub lent: u64,
    /// The entity's `core.shape@1` when this copy was the entity (its last change that is more than new x, y, z,
    /// yaw; 0: not known): while the entity's row still says that value, the copy differs by its base at most.
    pub other: u64,
}

/// Strictly the same value (what the core would store): `0.0` is not `-0.0`, an object's keys are in the same order.
pub(crate) fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Null, Json::Null) => true,
        (Json::Bool(x), Json::Bool(y)) => x == y,
        (Json::String(x), Json::String(y)) => x == y,
        (Json::Number(x), Json::Number(y)) => {
            if x.is_f64() || y.is_f64() {
                x.is_f64()
                    && y.is_f64()
                    && x.as_f64().map(f64::to_bits) == y.as_f64().map(f64::to_bits)
            } else {
                x == y
            }
        }
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Json::Object(x), Json::Object(y)) => same_map(x, y),
        _ => false,
    }
}

pub(crate) fn same_map(x: &Map<String, Json>, y: &Map<String, Json>) -> bool {
    x.len() == y.len() && x.iter().zip(y).all(|((k, p), (l, q))| k == l && same(p, q))
}

/// The rest of a `whole` delta (after its status byte).
pub(crate) fn read_whole(r: &mut Reader) -> Result<(u64, Entity), String> {
    let rev = r.u64()?;
    let (id, kind, owner) = (r.string()?, r.string()?, r.string()?);
    let tags = r.strs()?;
    let n = r.u32()? as usize;
    let mut components = Vec::with_capacity(n);
    for _ in 0..n {
        let k = r.string()?;
        components.push((k, r.value()?));
    }
    let n = r.u32()? as usize;
    let mut refs = Map::with_capacity(n);
    for _ in 0..n {
        let k = r.string()?;
        refs.insert(k, r.value()?);
    }
    Ok((
        rev,
        Entity {
            id,
            kind,
            tags,
            components,
            owner,
            refs,
        },
    ))
}

/// The rest of a `changes` delta (after its status byte, and its id in a list), applied to the copy.
pub(crate) fn read_changes(r: &mut Reader, e: &mut Entity) -> Result<u64, String> {
    let rev = r.u64()?;
    if r.bool()? {
        e.tags = r.strs()?;
    }
    for _ in 0..r.u32()? {
        let k = r.str()?;
        match e.components.iter_mut().find(|(n, _)| n == k) {
            // The copy's value is brought up to date in place (a transform keeps its map and its keys).
            Some(slot) => r.value_into(&mut slot.1)?,
            None => {
                let v = r.value()?;
                e.components.push((k.to_string(), v));
            }
        }
    }
    Ok(rev)
}

/// What `get_mut` changed in a copy, as the changes of `PATCH`: `None` when a list of changes cannot say it (a
/// component removed or moved, the kind, the owner or the references changed).
pub(crate) fn changes<'a>(
    was: &Entity,
    now: &'a Entity,
) -> Option<(bool, Vec<&'a (String, Json)>)> {
    if was.id != now.id
        || was.kind != now.kind
        || was.owner != now.owner
        || !same_map(&was.refs, &now.refs)
    {
        return None;
    }
    if now.components.len() < was.components.len() {
        return None;
    }
    let mut set = Vec::new();
    for (i, c) in now.components.iter().enumerate() {
        match was.components.get(i) {
            Some((k, v)) => {
                if *k != c.0 {
                    return None;
                }
                if !same(v, &c.1) {
                    set.push(c);
                }
            }
            None => set.push(c),
        }
    }
    Some((was.tags != now.tags, set))
}

pub(crate) use f::{D_ABSENT, D_CHANGES, D_SAME, D_WHOLE};
