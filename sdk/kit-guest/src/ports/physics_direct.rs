//! The direct links of `kithost.physics@1.1` (the core: kitcall/physics.rs `direct`): scalars, the entity packed
//! in two `u64`, the state of a body in 14 `f64` this module names by address — so only in the common memory
//! (feature `common-memory`); elsewhere, and for a long id, `None`: the same call by the bridge.

use super::{BodyState, V3};
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.physics@^1.1")]
extern "C" {
    fn ph_has_body(lo: u64, hi: u64) -> u32;
    fn ph_body(lo: u64, hi: u64, out: *mut f64) -> u32;
    fn ph_set_body(lo: u64, hi: u64, state: *const f64);
    fn ph_vector(lo: u64, hi: u64, what: u32, x: f64, y: f64, z: f64);
    fn ph_scalar(lo: u64, hi: u64, what: u32) -> f64;
}
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
use crate::native::link::{
    ph_body, ph_character_position, ph_has_body, ph_scalar, ph_set_body, ph_vector,
};
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.physics@^1.7")]
extern "C" {
    fn ph_character_position(lo: u64, hi: u64, out: *mut f64) -> u32;
}
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.physics@^1.2")]
extern "C" {
    fn ph_contacts(what: u32, out: *mut u64, cap: u32) -> u32;
}
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
use crate::native::link::ph_contacts;
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.physics@^1.5")]
extern "C" {
    fn ph_ask(what: u32, args: *const f64, flags: u32, lo: u64, hi: u64) -> u32;
    fn ph_poll(ticket: u32, out: *mut u64, cap: u32) -> u32;
}
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
use crate::native::link::{ph_ask, ph_poll};
use gripforge_abi::rows::{self, contact};
/// `ask_*` by direct link: the ticket; `None` when a name does not pack (the call asks).
pub fn ask(what: u32, args: &[f64], flags: u32, name: Option<&str>) -> Option<u32> {
    let (lo, hi) = match name {
        Some(n) => rows::pack16(n)?,
        None => (0, 0),
    };
    // SAFETY: `args` holds the numbers `what` names, on this module's stack in the common memory.
    Some(unsafe {
        ph_ask(
            what,
            args.as_ptr(),
            flags | (name.is_some() as u32 * (what >= 2) as u32),
            lo,
            hi,
        )
    })
}
/// `poll` by direct link: `None` when the answer must be taken by the call (a name that does not pack).
pub fn poll(ticket: u32) -> Option<super::Answer> {
    use super::Answer;
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        // SAFETY: the result buffer of this module, `len` words.
        let n = unsafe { ph_poll(ticket, o.as_mut_ptr(), o.len() as u32) };
        let f = |i: usize| f64::from_bits(o[i]);
        Some(match n {
            0 => Answer::Pending,
            1 => Answer::Unknown,
            u32::MAX => return None,
            _ => match o[0] {
                1 => Answer::Moved((o[1] != 0).then(|| ([f(2), f(3), f(4)], o[5] != 0))),
                2 => Answer::Hit(if o[1] == 0 {
                    None
                } else {
                    let entity = if o[2] != 0 {
                        Some(rows::unpack16(o[3], o[4])?)
                    } else {
                        None
                    };
                    Some(super::RayHit {
                        entity,
                        toi: f(5),
                        point: [f(6), f(7), f(8)],
                        normal: [f(9), f(10), f(11)],
                    })
                }),
                _ => Answer::Found(
                    (0..o[1] as usize)
                        .map(|i| rows::unpack16(o[2 + 2 * i], o[3 + 2 * i]))
                        .collect::<Option<Vec<_>>>()?,
                ),
            },
        })
    })
}
thread_local! {
    /// The result buffer of the lists (contacts, overlaps): one place, kept from one call to the next.
    static OUT: std::cell::RefCell<Vec<u64>> = std::cell::RefCell::new(vec![0; 64 * contact::WORDS]);
}
/// Runs `ask(out, cap in words)` on the result buffer, grown once when the answer needs more: the count it
/// answered and the buffer; `None` when the answer cannot be read here (`u32::MAX`: a name does not pack).
fn listed<R>(
    words: usize,
    ask: impl Fn(*mut u64, u32) -> u32,
    read: impl FnOnce(&[u64], usize) -> Option<R>,
) -> Option<R> {
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        let mut n = ask(o.as_mut_ptr(), (o.len() / words) as u32);
        if n == u32::MAX {
            return None;
        }
        if n as usize * words > o.len() {
            o.resize(n as usize * words, 0);
            n = ask(o.as_mut_ptr(), n);
            if n == u32::MAX || n as usize * words > o.len() {
                return None;
            }
        }
        read(&o, n as usize)
    })
}
/// `contact_events()` (`what` 0) or `contacts()` (1): `(a, b, value, began)` in the order of the port.
pub fn contacts(what: u32) -> Option<Vec<(String, String, f64, bool)>> {
    // SAFETY: imports wired by the calls host to the core's exports; the buffer is this module's, in the
    // memory the core shares, `cap` records long.
    listed(
        contact::WORDS,
        |out, cap| unsafe { ph_contacts(what, out, cap) },
        |o, n| {
            (0..n)
                .map(|i| contact::read(&o[i * contact::WORDS..]))
                .collect()
        },
    )
}
use crate::direct::pack_id as pack;
pub fn has_body(entity: &str) -> Option<bool> {
    let (lo, hi) = pack(entity)?;
    // SAFETY: imports wired by the calls host to the core's exports of the same names.
    Some(unsafe { ph_has_body(lo, hi) } != 0)
}
pub fn body(entity: &str) -> Option<Option<BodyState>> {
    use gripforge_abi::ports::abi::{body_state as bs, BODY_WORDS};
    let (lo, hi) = pack(entity)?;
    let mut w = [0f64; BODY_WORDS];
    // SAFETY: as above; `w` is the words of `abi.physics.body_state` on this module's stack, in the memory the core
    // shares.
    if unsafe { ph_body(lo, hi, w.as_mut_ptr()) } == 0 {
        return Some(None);
    }
    Some(Some(BodyState {
        entity: entity.to_string(),
        p: bs::position(&w),
        q: bs::rotation(&w),
        v: bs::linvel(&w),
        w: bs::angvel(&w),
        sleeping: bs::sleeping(&w) != 0.0,
    }))
}
pub fn set_body(s: &BodyState) -> Option<()> {
    let (lo, hi) = pack(&s.entity)?;
    let w = gripforge_abi::ports::abi::state_words(s.p, s.q, s.v, s.w, s.sleeping);
    // SAFETY: as `body`.
    unsafe { ph_set_body(lo, hi, w.as_ptr()) };
    Some(())
}
pub fn vector(entity: &str, what: u32, v: V3) -> Option<()> {
    let (lo, hi) = pack(entity)?;
    // SAFETY: as `has_body`.
    unsafe { ph_vector(lo, hi, what, v[0], v[1], v[2]) };
    Some(())
}
pub fn scalar(entity: &str, what: u32) -> Option<f64> {
    let (lo, hi) = pack(entity)?;
    // SAFETY: as `has_body`.
    Some(unsafe { ph_scalar(lo, hi, what) })
}
pub fn character_position(entity: &str) -> Option<Option<V3>> {
    let (lo, hi) = pack(entity)?;
    let mut at = [0.0f64; 3];
    // SAFETY: three numbers of this stack, for the call.
    Some((unsafe { ph_character_position(lo, hi, at.as_mut_ptr()) } != 0).then_some(at))
}
