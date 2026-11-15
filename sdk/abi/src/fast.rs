//! The short entries between modules of the common memory (docs/kit-modules.md §9.11): what is called every tick
//! crosses as scalars and addresses — nothing is encoded, nothing allocated, and the function is a pointer resolved
//! when the kit module is registered (its `INFO` names it), not a lookup per call.
//!
//! - [`Tick`]: the per-tick hooks of `kithost.kit@1` (`UPDATE`, `LATE_UPDATE`, `UPDATE_IDLE`): the context by
//!   address ([`HookCtx`]) and the step.
//! - [`Service`]: a kit → kit service call whose arguments are scalars ([`ServiceCall`], [`Scalar`]): numbers and
//!   booleans by value, strings by address (read in place by the provider for the time of the call). The answer is
//!   a scalar by value, a string given away, or — a free value — its bytes of the codec given away.
//!
//! Every type here is `repr(C)` with fixed-size fields: it does not depend on how the compiler lays out a Rust
//! type, only on the common memory and its one allocator.

use crate::{Ctx, Reader, Writer};
use core::mem::ManuallyDrop;
use serde_json::{Number, Value as Json};

/// [`Ctx`] as it is passed by address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HookCtx {
    pub inst: u32,
    pub kit: u32,
    pub services_gen: u32,
    pub config_gen: u32,
    pub tick: u64,
    pub frame: u64,
    pub elapsed: f64,
    pub fixed_dt: f64,
}

impl From<Ctx> for HookCtx {
    fn from(c: Ctx) -> HookCtx {
        HookCtx {
            inst: c.inst,
            kit: c.kit,
            services_gen: c.services_gen,
            config_gen: c.config_gen,
            tick: c.tick,
            frame: c.frame,
            elapsed: c.elapsed,
            fixed_dt: c.fixed_dt,
        }
    }
}

impl From<HookCtx> for Ctx {
    fn from(c: HookCtx) -> Ctx {
        Ctx {
            inst: c.inst,
            kit: c.kit,
            tick: c.tick,
            frame: c.frame,
            elapsed: c.elapsed,
            fixed_dt: c.fixed_dt,
            services_gen: c.services_gen,
            config_gen: c.config_gen,
        }
    }
}

/// A per-tick hook of a kit module of the common memory: `func` is `kit::UPDATE`, `LATE_UPDATE` or `UPDATE_IDLE`.
/// Answers 0 when the hook ran; anything else: nothing ran (no such instance, another function) and the caller
/// takes the general entry, which says why.
///
/// With [`RESTS`] set, the hook ran and the kit **rests** (`Kx::rest`, docs/kit-modules.md §9.14): the low bits are
/// what wakes it besides being entered ([`WAKE_INPUT`], [`WAKE_STATE`], [`WAKE_WORLD`]) — the core does not call
/// its per-tick hooks again until then.
pub type Tick = unsafe extern "C" fn(func: u32, ctx: *const HookCtx, dt: f64) -> u32;

/// The status of a [`Tick`] that ran and left its kit at rest; `status & WAKE_MASK` is what wakes it.
pub const RESTS: u32 = 1 << 31;
pub const WAKE_INPUT: u32 = 1;
pub const WAKE_STATE: u32 = 2;
pub const WAKE_WORLD: u32 = 4;
/// `WAKE_WORLD` narrowed to the structure of the world (an entity added or removed, a kind, a tag): the bit 8 is
/// only read beside `WAKE_WORLD`, so a core built before it wakes the kit at every write of the world.
pub const WAKE_STRUCTURE: u32 = WAKE_WORLD | 8;
pub const WAKE_MASK: u32 = 15;

/// Measures only (feature `entry-probe` of the core and of the façade, never in a shipped build; docs/kit-modules.md
/// §9.14): bits of `func` that make [`Tick`] run a part of an entry and no hook. [`PROBE_ENTER`]: the façade's own
/// prologue (its instance found, `Host::enter`). [`PROBE_LINK`]: `ctx.frame` calls of the cheapest direct link
/// (`st_phase`). [`PROBE_SVC`]: `ctx.frame` scalar service calls to a provider this kit is linked to, opened and
/// ended by the core, the provider entered, no method run.
pub const PROBE_ENTER: u32 = 1 << 30;
pub const PROBE_LINK: u32 = 1 << 29;
pub const PROBE_SVC: u32 = 1 << 28;
/// `ctx.frame` allocations of 48 bytes, each freed at once, by the kit module's allocator (in the common memory: the
/// core's, two calls to the core).
pub const PROBE_ALLOC: u32 = 1 << 27;
/// The `count` of a [`ServiceCall`] that enters the provider and runs no method (measures only, as above).
pub const PROBE_COUNT: usize = usize::MAX;

/// The kinds of a [`Scalar`].
pub const S_NULL: u32 = 0;
pub const S_FALSE: u32 = 1;
pub const S_TRUE: u32 = 2;
/// `bits` is an `i64`.
pub const S_INT: u32 = 3;
/// `bits` is an `f64`.
pub const S_FLOAT: u32 = 4;
/// `bits` is the address of `len` bytes of UTF-8; `cap` ≠ 0 only when the string is given away.
pub const S_STR: u32 = 5;
/// An answer only: `bits`, `len`, `cap` are a `Vec<u8>` given away, the value in the codec.
pub const S_BYTES: u32 = 6;
/// 1.2: `bits` is the address of a fixed-layout block (`fixed::Owned`) that holds a list or a dictionary. As an
/// answer it is given away: the caller reads it where it is ([`Scalar::take_seen`]) or copies it
/// ([`Scalar::take`]). As an argument it is lent ([`Scalar::lend_block`]): the provider reads it
/// ([`Scalar::copied`]) and the caller frees it after the call.
pub const S_FIXED: u32 = 7;

/// The most arguments a scalar service call carries (more: the ordinary call).
pub const MAX_ARGS: usize = 6;

/// One argument or the answer of a scalar service call.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scalar {
    pub kind: u32,
    pub len: u32,
    pub bits: u64,
    pub cap: u64,
}

impl Default for Scalar {
    fn default() -> Scalar {
        Scalar::NULL
    }
}

/// A number as the codec carries it (`put_value`): an integer that fits an `i64` stays one, anything else is an
/// `f64` — so that a value means the same whether it was written out or passed here.
fn number(n: &Number) -> Scalar {
    match n.as_i64() {
        Some(i) => Scalar {
            kind: S_INT,
            len: 0,
            bits: i as u64,
            cap: 0,
        },
        None => {
            let f = if n.is_u64() {
                n.as_u64().unwrap_or(0) as f64
            } else {
                n.as_f64().unwrap_or(0.0)
            };
            Scalar {
                kind: S_FLOAT,
                len: 0,
                bits: f.to_bits(),
                cap: 0,
            }
        }
    }
}

impl Scalar {
    pub const NULL: Scalar = Scalar {
        kind: S_NULL,
        len: 0,
        bits: 0,
        cap: 0,
    };

    /// `v` as an argument: by value, a string by its address (lent: `v` outlives the call). `None`: not a scalar.
    pub fn lend(v: &Json) -> Option<Scalar> {
        Some(match v {
            Json::Null => Scalar::NULL,
            Json::Bool(b) => Scalar {
                kind: if *b { S_TRUE } else { S_FALSE },
                len: 0,
                bits: 0,
                cap: 0,
            },
            Json::Number(n) => number(n),
            Json::String(s) if s.len() <= u32::MAX as usize => Scalar {
                kind: S_STR,
                len: s.len() as u32,
                bits: s.as_ptr() as usize as u64,
                cap: 0,
            },
            _ => return None,
        })
    }

    /// The value of an argument, for the time of the call: a string is read where the caller holds it — the
    /// result is never dropped (it does not own the bytes) and only ever read.
    ///
    /// # Safety
    /// `self` was made by [`Scalar::lend`] in the same memory, from a value that is still alive.
    pub unsafe fn borrowed(&self) -> ManuallyDrop<Json> {
        ManuallyDrop::new(match self.kind {
            S_FALSE => Json::Bool(false),
            S_TRUE => Json::Bool(true),
            S_INT => Json::Number(Number::from(self.bits as i64)),
            S_FLOAT => Number::from_f64(f64::from_bits(self.bits))
                .map(Json::Number)
                .unwrap_or(Json::Null),
            S_STR if self.len != 0 => {
                let len = self.len as usize;
                Json::String(String::from_raw_parts(
                    self.bits as usize as *mut u8,
                    len,
                    len,
                ))
            }
            S_STR => Json::String(String::new()),
            _ => Json::Null,
        })
    }

    /// An argument that is a list or a dictionary, lent as its block (which the caller keeps until the call is over).
    pub fn lend_block(block: &crate::fixed::Owned) -> Scalar {
        Scalar {
            kind: S_FIXED,
            len: 0,
            bits: block.address(),
            cap: 0,
        }
    }

    /// The value of an argument lent as a block ([`Scalar::lend_block`]), copied: the provider owns the copy and
    /// drops it. `None`: any other argument (read by [`Scalar::borrowed`]).
    ///
    /// # Safety
    /// `self` comes from [`Scalar::lend_block`] on a block that lives for the call, in a memory this module reads.
    pub unsafe fn copied(&self) -> Option<Json> {
        if self.kind != S_FIXED {
            return None;
        }
        // The block stays the caller's: read, never freed here.
        let block = ManuallyDrop::new(crate::fixed::Owned::take(self.bits).ok()?);
        block.to_json().ok()
    }

    /// `v` as an answer, given away: whoever calls [`Scalar::take`] owns what it holds.
    pub fn give(v: Json) -> Scalar {
        match v {
            Json::Null => Scalar::NULL,
            Json::Bool(b) => Scalar {
                kind: if b { S_TRUE } else { S_FALSE },
                len: 0,
                bits: 0,
                cap: 0,
            },
            Json::Number(n) => number(&n),
            Json::String(s) => Scalar::give_bytes(S_STR, s.into_bytes()),
            other => match crate::fixed::Owned::from_json(&other) {
                // A list, a dictionary: a block of the fixed layout, read in place by the caller.
                Some(block) => Scalar {
                    kind: S_FIXED,
                    len: 0,
                    bits: block.into_address(),
                    cap: 0,
                },
                None => {
                    let mut w = Writer::new();
                    w.value(&other);
                    Scalar::give_bytes(S_BYTES, w.buf)
                }
            },
        }
    }

    /// A text given away (the message of a failed call).
    pub fn give_text(s: String) -> Scalar {
        Scalar::give_bytes(S_STR, s.into_bytes())
    }

    fn give_bytes(kind: u32, v: Vec<u8>) -> Scalar {
        let mut v = ManuallyDrop::new(v);
        Scalar {
            kind,
            len: v.len() as u32,
            bits: v.as_mut_ptr() as usize as u64,
            cap: v.capacity() as u64,
        }
    }

    unsafe fn take_bytes(&self) -> Vec<u8> {
        if self.cap == 0 {
            return Vec::new();
        }
        Vec::from_raw_parts(
            self.bits as usize as *mut u8,
            self.len as usize,
            self.cap as usize,
        )
    }

    /// The answer, owned by the caller.
    ///
    /// # Safety
    /// `self` comes from one [`Scalar::give`] made in the same memory with the same allocator, taken once.
    pub unsafe fn take(self) -> Result<Json, String> {
        Ok(match self.kind {
            S_FALSE => Json::Bool(false),
            S_TRUE => Json::Bool(true),
            S_INT => Json::Number(Number::from(self.bits as i64)),
            S_FLOAT => Number::from_f64(f64::from_bits(self.bits))
                .map(Json::Number)
                .unwrap_or(Json::Null),
            S_STR => Json::String(self.take_text()),
            S_BYTES => return Reader::new(&self.take_bytes()).value(),
            S_FIXED => return crate::fixed::Owned::take(self.bits)?.to_json(),
            _ => Json::Null,
        })
    }

    /// The answer for a caller that reads it where it is: the block of a list or a dictionary (`Ok`), owned by
    /// the caller and freed when it drops it — nothing was rebuilt; any other answer as its value (`Err`).
    ///
    /// # Safety
    /// As [`Scalar::take`].
    pub unsafe fn take_seen(self) -> Result<Result<crate::fixed::Owned, Json>, String> {
        match self.kind {
            S_FIXED => crate::fixed::Owned::take(self.bits).map(Ok),
            _ => self.take().map(Err),
        }
    }

    /// The text of an answer made by [`Scalar::give_text`] (or a string answer).
    ///
    /// # Safety
    /// As [`Scalar::take`].
    pub unsafe fn take_text(self) -> String {
        String::from_utf8_unchecked(self.take_bytes())
    }
}

/// A scalar service call, by address: the provider reads everything in place and writes its answer in `out`.
#[repr(C)]
pub struct ServiceCall {
    /// The context of the provider's instance, as a hook entry carries it (written by the core).
    pub ctx: *const HookCtx,
    pub capability: *const u8,
    pub capability_len: usize,
    pub method: *const u8,
    pub method_len: usize,
    /// `count` arguments (≤ [`MAX_ARGS`]), lent.
    pub args: *const Scalar,
    pub count: usize,
    /// The answer, given away: the value when the status is 0, else the text of the failure.
    pub out: Scalar,
}

/// The scalar service entry of a kit module of the common memory. Answers 0 (`out` is the value) or 1 (`out` is
/// the message of `Err`).
pub type Service = unsafe extern "C" fn(call: *mut ServiceCall) -> u32;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn by_codec(v: &Json) -> Json {
        let mut w = Writer::new();
        w.value(v);
        Reader::new(w.bytes()).value().unwrap()
    }

    #[test]
    fn scalars_mean_what_the_codec_means() {
        let values = [
            json!(null),
            json!(true),
            json!(false),
            json!(0),
            json!(-7),
            json!(3),
            json!(3.0),
            json!(-0.0),
            json!(1.5),
            json!(i64::MAX),
            json!(i64::MIN),
            json!(u64::MAX),
            json!(9_007_199_254_740_993u64),
            json!(""),
            json!("e12"),
            json!("héros — 勇者"),
        ];
        for v in &values {
            let lent = Scalar::lend(v).expect("a scalar");
            // SAFETY: lent from `v`, alive here.
            let seen = unsafe { lent.borrowed() };
            assert_eq!(*seen, by_codec(v), "lent {v}");
            assert_eq!(seen.is_u64(), by_codec(v).is_u64(), "kind of {v}");
            assert_eq!(seen.is_f64(), by_codec(v).is_f64(), "kind of {v}");
            // SAFETY: given once, taken once, one allocator.
            let back = unsafe { Scalar::give(v.clone()).take() }.unwrap();
            assert_eq!(back, by_codec(v), "given {v}");
            assert_eq!(back.is_f64(), by_codec(v).is_f64(), "kind of {v}");
        }
    }

    #[test]
    fn a_free_value_is_not_lent_and_is_given_as_a_block() {
        for v in [
            json!([1, "a"]),
            json!({ "hp": 3, "tags": ["x"], "at": { "x": 1.5, "name": "mid" } }),
            json!({}),
        ] {
            assert!(Scalar::lend(&v).is_none());
            let s = Scalar::give(v.clone());
            assert_eq!(s.kind, S_FIXED);
            // SAFETY: as above. Copied: the value the codec would carry.
            assert_eq!(unsafe { s.take() }.unwrap(), by_codec(&v));
            // Read where it is: the block, nothing rebuilt; it reads as the value.
            let block = unsafe { Scalar::give(v.clone()).take_seen() }
                .unwrap()
                .expect("a block");
            assert_eq!(block.to_json().unwrap(), v);
            assert_eq!(
                block.view().len(),
                if v.is_array() {
                    v.as_array().unwrap().len()
                } else {
                    v.as_object().unwrap().len()
                }
            );
        }
        // A scalar answer has no block: its value.
        assert_eq!(
            unsafe { Scalar::give(json!("mid")).take_seen() }
                .unwrap()
                .unwrap_err(),
            json!("mid")
        );
        // As an argument: lent as its block, copied by the provider, still the caller's afterwards.
        let v = json!([1.5, 0, -3.25]);
        let block = crate::fixed::Owned::from_json(&v).unwrap();
        let lent = Scalar::lend_block(&block);
        assert_eq!(unsafe { lent.copied() }, Some(v.clone()));
        assert_eq!(unsafe { Scalar::lend(&json!(3)).unwrap().copied() }, None);
        assert_eq!(block.to_json().unwrap(), v);
        // SAFETY: as above.
        assert_eq!(
            unsafe { Scalar::give_text("no such entity".into()).take_text() },
            "no such entity"
        );
    }

    #[test]
    fn the_context_is_the_same_by_address() {
        let c = Ctx {
            inst: 3,
            kit: 9,
            tick: 1 << 40,
            frame: 77,
            elapsed: 12.5,
            fixed_dt: 1.0 / 60.0,
            services_gen: 4,
            config_gen: 2,
        };
        assert_eq!(Ctx::from(HookCtx::from(c)), c);
        assert_eq!(core::mem::size_of::<HookCtx>(), 48);
        assert_eq!(core::mem::size_of::<Scalar>(), 24);
    }
}
