//! Kit → kit service calls by direct link (docs/kit-modules.md §7; the core side: gripforge-kithost
//! src/kitcall/svc.rs): `kx.call(capability, method, args)` to a capability another kit module serves is a call of
//! that module's export `gfx_svc_call`, wasm to wasm — no hook entry by the bridge, no JavaScript, and the core
//! decodes nothing. The kit source does not change: `Host::call_service` tries [`call`] first.
//!
//! **Caller.** The kit crate lists the capabilities its source names (`link`, written by
//! tools/kit-modules/build.mjs; a kit written by hand: [`service_links!`]). Each is an import
//! `gf:svc.<capability>@^1` / `svc_call`, `svc_id` the host wires to the kit module that provides the capability in
//! this game, or to a stub when none does. A capability that is not listed, a stub, or a provider the core says is
//! not the one serving the capability now (it was replaced): the ordinary call, by the core.
//!
//! **Provider.** Every kit module exports `gfx_svc_call(capability, method, words)` and `gfx_svc_id()`
//! (`export_kit!`): it runs `KitModule::call` on its instance, as the hook `call` of `kithost.kit` does.
//!
//! The arguments and the answer are a message of the calls (the same bytes as by the bridge), carried 8 bytes at
//! a time through the core's words (`svc_push` / `svc_get`); names are atoms. When the core says the caller and
//! the provider both live in the common memory (`svc_begin` answers 2; the provider reads word 6 of `svc_ctx`),
//! lists and dictionaries go as fixed-layout blocks both ways (docs/kit-modules.md §9.7): the message then only
//! carries their addresses.
//!
//! **Between two kit modules of the common memory** (docs/kit-modules.md §9.11), a call whose arguments are scalars
//! (null, booleans, numbers, strings; at most `fast::MAX_ARGS`) does not go through the words: the core opens the
//! call (`mem_svc_open`: the same registry, the same care around the provider) and answers the provider's scalar
//! entry, a function of the table both kits share; the caller calls it with the names and the arguments by address
//! ([`scalar_call`], [`serve_scalar`]). A free value among the arguments: the words, as above.

use crate::direct::{atom, FastMap};
use crate::host::Kx;
use crate::val::Answer;
use gripforge_abi::{Ctx, Reader, Writer};
use serde_json::Value as Json;
use std::cell::{Cell, RefCell};

/// The import `svc_call(capability, method, words) -> status << 32 | bytes` of a provider.
pub type CallFn = unsafe extern "C" fn(u32, u32, u32) -> u64;
/// The import `svc_id() -> the atom of the provider's kit id + 1` (0: no provider linked).
pub type IdFn = unsafe extern "C" fn() -> u32;

struct Link {
    capability: &'static str,
    call: CallFn,
    id: IdFn,
    /// What `id` answered (asked once: 0 not asked, 1 no provider, else the atom + 2).
    provider: Cell<u32>,
    /// The atom of `capability` + 1 (0: not asked yet): a name is interned once, not hashed at every call.
    atom: Cell<u32>,
}

impl Link {
    fn atom(&self) -> u32 {
        if self.atom.get() == 0 {
            self.atom.set(atom(self.capability) + 1);
        }
        self.atom.get() - 1
    }
}

thread_local! {
    static LINKS: RefCell<Vec<&'static Link>> = const { RefCell::new(Vec::new()) };
    /// The names of the atoms a provider was called with (capability, method).
    static NAMES: RefCell<FastMap<u32, &'static str>> = RefCell::new(FastMap::default());
}

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
mod core {
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.services@^1.1")]
    extern "C" {
        pub fn svc_begin(capability: u32, provider: u32) -> u32;
        pub fn svc_end();
        pub fn svc_ctx(i: u32) -> u64;
        pub fn svc_clear();
        pub fn svc_push(word: u64);
        pub fn svc_get(i: u32) -> u64;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    pub use crate::native::link::{svc_begin, svc_clear, svc_ctx, svc_end, svc_get, svc_push};
}

#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
#[allow(clippy::missing_safety_doc)]
mod core {
    pub unsafe fn svc_begin(_: u32, _: u32) -> u32 {
        0
    }
    pub unsafe fn svc_end() {}
    pub unsafe fn svc_ctx(_: u32) -> u64 {
        0
    }
    pub unsafe fn svc_clear() {}
    pub unsafe fn svc_push(_: u64) {}
    pub unsafe fn svc_get(_: u32) -> u64 {
        0
    }
}

/// Declares a direct link to the provider of a capability (the generated kit crate calls it from `gf_calls_init`).
pub fn link(capability: &'static str, call: CallFn, id: IdFn) {
    let l: &'static Link = Box::leak(Box::new(Link {
        capability,
        call,
        id,
        provider: Cell::new(0),
        atom: Cell::new(0),
    }));
    LINKS.with(|v| v.borrow_mut().push(l));
}

fn push_bytes(bytes: &[u8]) {
    // SAFETY: imports wired by the calls host to the core's exports of the same names.
    unsafe {
        core::svc_clear();
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            core::svc_push(u64::from_le_bytes(word));
        }
    }
}

fn pull_bytes(len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 8);
    // SAFETY: as `push_bytes`.
    for i in 0..len.div_ceil(8) {
        out.extend_from_slice(&unsafe { core::svc_get(i as u32) }.to_le_bytes());
    }
    out.truncate(len);
    out
}

/// `kx.call(capability, method, args)` by the direct link to the capability's provider. `None`: no link for this
/// call (take the ordinary path); nothing was run.
pub fn call(capability: &str, method: &str, args: &[Json]) -> Option<Result<Json, String>> {
    Some(linked(capability, method, args, false)?.map(|a| match a {
        Answer::Value(v) => v,
        // Not asked for: `linked` copies the answer when the caller wants a value.
        Answer::Block(b) => b.to_json().unwrap_or(Json::Null),
    }))
}

/// [`call`] for a caller that reads the answer where it is (`Kx::call_with`): a list or a dictionary answered by a
/// kit module of the common memory to a scalar call comes as its block, not rebuilt.
pub(crate) fn call_seen(
    capability: &str,
    method: &str,
    args: &[Json],
) -> Option<Result<Answer, String>> {
    linked(capability, method, args, true)
}

/// [`call_seen`] with typed arguments (`Kx::call_args_with`, docs/kit-modules.md §7.13): when the provider is a
/// kit module of the common memory and every argument is a scalar, they are lent as they are — a text by its
/// address, a number as JavaScript has it — and no value is built. `None`: not this path (the caller passes their
/// JSON values); nothing was run.
pub(crate) fn call_seen_args(
    capability: &str,
    method: &str,
    args: &[crate::typed::Arg],
) -> Option<Result<Answer, String>> {
    let l = LINKS.with(|v| {
        v.borrow()
            .iter()
            .find(|l| l.capability == capability)
            .copied()
    })?;
    if l.provider.get() == 0 {
        // SAFETY: as in `linked`.
        l.provider.set(unsafe { (l.id)() } + 1);
    }
    let provider = l.provider.get().checked_sub(2)?;
    let (status, out) = scalar_call_args(l.atom(), provider, capability, method, args)?;
    // SAFETY: as in `linked`.
    Some(unsafe {
        match status {
            0 => out.take_seen().map(|a| match a {
                Ok(block) => Answer::Block(block),
                Err(v) => Answer::Value(v),
            }),
            _ => Err(out.take_text()),
        }
    })
}

/// A typed argument as a scalar of the call (`None`: a list, a dictionary, a vector or a JSON value that is not a
/// scalar — lent as a block).
#[cfg(feature = "common-memory")]
fn scalar_of(a: &crate::typed::Arg) -> Option<gripforge_abi::fast::Scalar> {
    use crate::typed::Arg;
    use gripforge_abi::fast::{Scalar, S_FALSE, S_FLOAT, S_INT, S_STR, S_TRUE};
    let int = |i: i64| Scalar {
        kind: S_INT,
        len: 0,
        bits: i as u64,
        cap: 0,
    };
    let float = |f: f64| Scalar {
        kind: S_FLOAT,
        len: 0,
        bits: f.to_bits(),
        cap: 0,
    };
    Some(match *a {
        Arg::Null => Scalar::NULL,
        Arg::Bool(b) => Scalar {
            kind: if b { S_TRUE } else { S_FALSE },
            len: 0,
            bits: 0,
            cap: 0,
        },
        // `num()`: an integral value is an integer, a value that is not finite is null.
        Arg::Num(n) if n.is_finite() && n == n.trunc() && n.abs() < 9.007_199_254_740_992e15 => {
            int(if n == 0.0 { 0 } else { n as i64 })
        }
        Arg::Num(n) | Arg::F64(n) if n.is_finite() => float(n),
        Arg::Num(_) | Arg::F64(_) => Scalar::NULL,
        Arg::Int(i) => int(i),
        Arg::Str(s) if s.len() <= u32::MAX as usize => Scalar {
            kind: S_STR,
            len: s.len() as u32,
            bits: s.as_ptr() as usize as u64,
            cap: 0,
        },
        Arg::Value(v) => Scalar::lend(v)?,
        _ => return None,
    })
}

#[cfg(feature = "common-memory")]
fn scalar_call_args(
    cap: u32,
    provider: u32,
    capability: &str,
    method: &str,
    args: &[crate::typed::Arg],
) -> Option<(u32, gripforge_abi::fast::Scalar)> {
    use gripforge_abi::fast::{Scalar, MAX_ARGS};
    if args.len() > MAX_ARGS {
        return None;
    }
    let mut lent = [Scalar::NULL; MAX_ARGS];
    // A list, a dictionary or a vector is lent as the fixed-layout block made from it (no JSON value), kept here
    // until the call is over.
    let mut blocks: Vec<gripforge_abi::fixed::Owned> = Vec::new();
    for (slot, a) in lent.iter_mut().zip(args) {
        *slot = match scalar_of(a) {
            Some(s) => s,
            None => {
                let block = gripforge_abi::fixed::Owned::from_src(a)?;
                let s = Scalar::lend_block(&block);
                blocks.push(block);
                s
            }
        };
    }
    let out = invoke_scalar(cap, provider, capability, method, &lent[..args.len()]);
    drop(blocks);
    out
}

#[cfg(not(feature = "common-memory"))]
fn scalar_call_args(
    _: u32,
    _: u32,
    _: &str,
    _: &str,
    _: &[crate::typed::Arg],
) -> Option<(u32, gripforge_abi::fast::Scalar)> {
    None
}

fn linked(
    capability: &str,
    method: &str,
    args: &[Json],
    seen: bool,
) -> Option<Result<Answer, String>> {
    let l = LINKS.with(|v| {
        v.borrow()
            .iter()
            .find(|l| l.capability == capability)
            .copied()
    })?;
    if l.provider.get() == 0 {
        // SAFETY: an import wired by the calls host to the provider's export, or to a stub that answers 0.
        l.provider.set(unsafe { (l.id)() } + 1);
    }
    let provider = l.provider.get().checked_sub(2)?;
    let cap = l.atom();
    // Scalar arguments, both kits in the common memory: by address, nothing written out (`scalar_call`).
    if let Some((status, out)) = scalar_call(cap, provider, capability, method, args) {
        // SAFETY: given by the provider with the allocator both use, taken once.
        return Some(unsafe {
            match status {
                0 if seen => out.take_seen().map(|a| match a {
                    Ok(block) => Answer::Block(block),
                    Err(v) => Answer::Value(v),
                }),
                0 => out.take().map(Answer::Value),
                _ => Err(out.take_text()),
            }
        });
    }
    // SAFETY: as `push_bytes`.
    let begun = unsafe { core::svc_begin(cap, provider) };
    if begun == 0 {
        return None;
    }
    // 2: the core says the provider shares this module's memory.
    let common = begun == 2;
    let mut w = Writer::passing(common);
    w.values(args);
    push_bytes(w.bytes());
    // SAFETY: the provider's export (`serve` below, in the other module).
    let r = unsafe { (l.call)(cap, atom(method), w.bytes().len() as u32) };
    let answer = pull_bytes(r as u32 as usize);
    // SAFETY: as `push_bytes`.
    unsafe { core::svc_end() };
    Some(match r >> 32 {
        0 => Reader::from(&answer, common).value().map(Answer::Value),
        _ => Err(String::from_utf8_lossy(&answer).into_owned()),
    })
}

#[cfg(all(feature = "common-memory", target_arch = "wasm32"))]
#[link(wasm_import_module = "gf:kithost.mem@^1")]
extern "C" {
    /// The core's `svc::open`: `begin` for a scalar call. 0, or the provider's scalar entry (its index in the
    /// function table both kits share) in the low half and the address of its context in the high half.
    fn mem_svc_open(capability: u32, provider: u32) -> u64;
}

/// The core's `svc::open`, with the provider's scalar entry as a function: 0 in wasm's words is "not this way".
#[cfg(feature = "common-memory")]
fn svc_open(
    cap: u32,
    provider: u32,
) -> Option<(
    gripforge_abi::fast::Service,
    *const gripforge_abi::fast::HookCtx,
)> {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: an export of the core; on wasm a function pointer is its index in the table, and the core answers
        // the index the provider gave for a function of this exact type (`host::fast_entries`).
        let opened = unsafe { mem_svc_open(cap, provider) };
        if opened == 0 {
            return None;
        }
        let f = unsafe {
            std::mem::transmute::<usize, gripforge_abi::fast::Service>(opened as u32 as usize)
        };
        Some((f, (opened >> 32) as usize as *const _))
    }
    #[cfg(not(target_arch = "wasm32"))]
    crate::native::svc_open(cap, provider)
}

/// The service call with scalar arguments, between two kit modules of the common memory (docs/kit-modules.md
/// §9.11): the provider's scalar entry is called through the function table with the arguments by address — no
/// codec, no word pushed through the core. `None`: not this path (an argument is a free value, more than
/// [`MAX_ARGS`] of them, this kit or the provider outside the common memory, the capability served by someone
/// else now) — nothing was run, the caller goes on with the words.
#[cfg(feature = "common-memory")]
fn scalar_call(
    cap: u32,
    provider: u32,
    capability: &str,
    method: &str,
    args: &[Json],
) -> Option<(u32, gripforge_abi::fast::Scalar)> {
    use gripforge_abi::fast::{Scalar, MAX_ARGS};
    if args.len() > MAX_ARGS {
        return None;
    }
    let mut lent = [std::mem::MaybeUninit::<Scalar>::uninit(); MAX_ARGS];
    // An argument that is a list or a dictionary is lent as a fixed-layout block, kept here until the call is over
    // (nothing is made for the calls that carry scalars only: most of them).
    let mut blocks: Vec<gripforge_abi::fixed::Owned> = Vec::new();
    for (slot, a) in lent.iter_mut().zip(args) {
        slot.write(match Scalar::lend(a) {
            Some(scalar) => scalar,
            None => {
                let block = gripforge_abi::fixed::Owned::from_json(a)?;
                // The block is one allocation that does not move with the list that keeps it.
                let scalar = Scalar::lend_block(&block);
                blocks.push(block);
                scalar
            }
        });
    }
    // SAFETY: the first `args.len()` are written above.
    let lent = unsafe { std::slice::from_raw_parts(lent.as_ptr() as *const Scalar, args.len()) };
    let out = invoke_scalar(cap, provider, capability, method, lent);
    drop(blocks);
    out
}

/// Opens the call by the core and enters the provider's scalar entry with `lent` (alive for the call).
#[cfg(feature = "common-memory")]
fn invoke_scalar(
    cap: u32,
    provider: u32,
    capability: &str,
    method: &str,
    lent: &[gripforge_abi::fast::Scalar],
) -> Option<(u32, gripforge_abi::fast::Scalar)> {
    use gripforge_abi::fast::ServiceCall;
    // From an open call to `svc_end` the provider's `Kx` runs.
    let (entry, ctx) = svc_open(cap, provider)?;
    // The provider reads no more than `count` arguments; addresses are u64 on every target (abi.fast.service).
    let mut call = ServiceCall::new(capability.as_bytes(), method.as_bytes(), lent);
    call.ctx = ctx as usize as u64;
    // SAFETY: the provider's entry of this exact type; it reads `call` and what it lends in place, and gives its
    // answer in `call.out`.
    let status = unsafe {
        let status = entry(&mut call);
        core::svc_end();
        status
    };
    Some((status, call.out))
}

/// Measures only (feature `entry-probe`): `n` scalar service calls to the first provider this kit is linked to and
/// has called — opened and ended by the core as any (`mem_svc_open`, `svc_end`), the provider entered by its scalar
/// entry, no argument lent, no method run, no answer.
#[cfg(all(feature = "entry-probe", feature = "common-memory"))]
pub fn probe(n: u64) {
    use gripforge_abi::fast::{ServiceCall, PROBE_COUNT};
    let Some(l) = LINKS.with(|v| v.borrow().iter().find(|l| l.provider.get() >= 2).copied()) else {
        return;
    };
    let (cap, provider) = (l.atom(), l.provider.get() - 2);
    for _ in 0..n {
        // SAFETY: as `scalar_call`.
        unsafe {
            let Some((entry, ctx)) = svc_open(cap, provider) else {
                return;
            };
            let mut call = ServiceCall::new(l.capability.as_bytes(), &[], &[]);
            call.ctx = ctx as usize as u64;
            call.count = PROBE_COUNT;
            std::hint::black_box(entry(&mut call));
            core::svc_end();
        }
    }
}

#[cfg(all(feature = "entry-probe", not(feature = "common-memory")))]
pub fn probe(_: u64) {}

#[cfg(not(feature = "common-memory"))]
fn scalar_call(
    _: u32,
    _: u32,
    _: &str,
    _: &str,
    _: &[Json],
) -> Option<(u32, gripforge_abi::fast::Scalar)> {
    None
}

/// The scalar service entry of this kit module (`gripforge_abi::fast::Service`; `INFO.svc`): what [`serve`]
/// does, with the context, the names and the arguments read where the caller holds them. The arguments are the
/// caller's for the time of the call: `KitModule::call` reads them, as it reads a slice it is given.
///
/// # Safety
/// `call` and everything it points to are valid for the call, in a memory this module reads (the common memory;
/// this process in a test); `call.out` is taken once by the caller.
pub unsafe extern "C" fn serve_scalar(call: *mut gripforge_abi::fast::ServiceCall) -> u32 {
    use gripforge_abi::fast::{Scalar, MAX_ARGS};
    use std::mem::{ManuallyDrop, MaybeUninit};
    let c = &mut *call;
    let ctx: Ctx = (*c.ctx()).into();
    #[cfg(feature = "entry-probe")]
    if c.count == gripforge_abi::fast::PROBE_COUNT {
        // Measures only: the provider entered (its instance, `Host::enter`), no method run.
        if let Some(ptr) = crate::host::instance(ctx.inst, ctx.kit) {
            (*ptr).enter(ctx);
        }
        return 2;
    }
    let text = |p: *const u8, len: usize| {
        if len == 0 {
            ""
        } else {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(p, len))
        }
    };
    let (cap, method) = (
        text(
            c.capability as usize as *const u8,
            c.capability_len as usize,
        ),
        text(c.method as usize as *const u8, c.method_len as usize),
    );
    let count = (c.count as usize).min(MAX_ARGS);
    let lent = c.args as usize as *const Scalar;
    // The module by address (kept alive by `MODULE`): no count touched; the instance: as `serve`.
    let (Some(m), Some(ptr)) = (
        crate::host::module_ptr(),
        crate::host::instance(ctx.inst, ctx.kit),
    ) else {
        let id = crate::host::module_ptr().map_or("?", |m| (*m).id());
        c.out = Scalar::give_text(format!(
            "kitcall: {id} has no instance {}/{} (MOUNT first)",
            ctx.inst, ctx.kit
        ));
        return 1;
    };
    let host: &mut crate::host::Host = &mut *ptr;
    host.enter(ctx);
    let mut kx = Kx {
        host,
        kit: ctx.kit as usize,
    };
    // A method the provider types (`KitModule::call_typed`, docs/kit-modules.md §7.13): the scalars as they were
    // lent — a text where the caller holds it —, the answer a scalar. Any other: the values below, as before.
    {
        use crate::typed::Arg;
        use gripforge_abi::fast::{S_FALSE, S_FLOAT, S_INT, S_NULL, S_STR, S_TRUE};
        let mut typed = [Arg::Null; MAX_ARGS];
        let all = (0..count).all(|i| {
            let a = &*lent.add(i);
            typed[i] = match a.kind {
                S_NULL => Arg::Null,
                S_FALSE => Arg::Bool(false),
                S_TRUE => Arg::Bool(true),
                S_INT => Arg::Int(a.bits as i64),
                S_FLOAT => Arg::F64(f64::from_bits(a.bits)),
                S_STR => Arg::Str(text(a.bits as usize as *const u8, a.len as usize)),
                _ => return false,
            };
            true
        });
        if all {
            if let Some(out) = (*m).call_typed(&mut kx, cap, method, &typed[..count]) {
                return match out {
                    Ok(r) => {
                        c.out = Scalar::give(r.to_json());
                        0
                    }
                    Err(msg) => {
                        c.out = Scalar::give_text(msg);
                        1
                    }
                };
            }
        }
    }
    // Only the arguments given are made. Never dropped unless copied here: the strings are the caller's.
    let mut args: [MaybeUninit<Json>; MAX_ARGS] = [const { MaybeUninit::uninit() }; MAX_ARGS];
    // Bit `i`: argument `i` was lent as a block and copied here — this module's value, dropped after the call.
    let mut copied = 0u32;
    for (i, slot) in args.iter_mut().enumerate().take(count) {
        let a = &*lent.add(i);
        slot.write(match a.copied() {
            Some(v) => {
                copied |= 1 << i;
                v
            }
            None => ManuallyDrop::into_inner(a.borrowed()),
        });
    }
    // SAFETY: the first `count` are written; `MaybeUninit<T>` is `T` in memory.
    let out = (*m).call(
        &mut kx,
        cap,
        method,
        std::slice::from_raw_parts(args.as_ptr() as *const Json, count),
    );
    if copied != 0 {
        for (i, slot) in args.iter_mut().enumerate().take(count) {
            if copied & 1 << i != 0 {
                // SAFETY: built here by `copied`, read by the call that is over, dropped once.
                slot.assume_init_drop();
            }
        }
    }
    match out {
        Ok(v) => {
            c.out = Scalar::give(v);
            0
        }
        Err(msg) => {
            c.out = Scalar::give_text(msg);
            1
        }
    }
}

fn name_of(id: u32) -> &'static str {
    if let Some(n) = NAMES.with(|n| n.borrow().get(&id).copied()) {
        return n;
    }
    let mut w = Writer::new();
    w.u32(id);
    let bytes = crate::host::call_core_ok(crate::func!(gripforge_abi::services::ATOM_NAME), &w);
    let name = Reader::new(&bytes)
        .opt_str()
        .ok()
        .flatten()
        .unwrap_or("")
        .to_string();
    let name: &'static str = Box::leak(name.into_boxed_str());
    NAMES.with(|n| n.borrow_mut().insert(id, name));
    name
}

/// The export `gfx_svc_id` of a kit module: the atom of its kit id + 1.
pub fn id() -> u32 {
    thread_local! {
        static ID: Cell<u32> = const { Cell::new(0) };
    }
    if ID.with(Cell::get) == 0 {
        ID.with(|c| c.set(atom(crate::host::module().id()) + 1));
    }
    ID.with(Cell::get)
}

/// The export `gfx_svc_call` of a kit module: runs `KitModule::call(capability, method, args)` on the instance the
/// core says runs (`svc_ctx`), the arguments read from the core's words, the answer pushed there. Answers
/// `status << 32 | bytes` (status 0: the value; else the error's text).
pub fn serve(capability: u32, method: u32, len: u32) -> u64 {
    // Everything of the call is read before anything else is called (the words are shared with nested calls).
    let args = pull_bytes(len as usize);
    // SAFETY: as `push_bytes`.
    let c: [u64; 6] = std::array::from_fn(|i| unsafe { core::svc_ctx(i as u32) });
    // Word 6: 1 when the core says the caller shares this module's memory (blocks pass both ways).
    // SAFETY: as `push_bytes`.
    let common = unsafe { core::svc_ctx(6) } == 1;
    let ctx = Ctx {
        inst: c[0] as u32,
        kit: (c[0] >> 32) as u32,
        tick: c[1],
        frame: c[2],
        elapsed: f64::from_bits(c[3]),
        fixed_dt: f64::from_bits(c[4]),
        services_gen: c[5] as u32,
        config_gen: (c[5] >> 32) as u32,
    };
    let (cap, method) = (name_of(capability), name_of(method));
    let m = crate::host::module();
    let out = (|| {
        let args = Reader::from(&args, common).values()?;
        let ptr = crate::host::instance(ctx.inst, ctx.kit).ok_or_else(|| {
            format!(
                "kitcall: {} has no instance {}/{} (MOUNT first)",
                m.id(),
                ctx.inst,
                ctx.kit
            )
        })?;
        // SAFETY: as the hook entry (`host::entry`): the instance lives until DROP, and a nested entry of it only
        // happens while the outer one waits on a call that holds no borrow of it.
        let host: &mut crate::host::Host = unsafe { &mut *ptr };
        host.enter(ctx);
        let mut kx = Kx {
            host,
            kit: ctx.kit as usize,
        };
        m.call(&mut kx, cap, method, &args)
    })();
    match out {
        Ok(v) => {
            let mut w = Writer::passing(common);
            w.value(&v);
            push_bytes(w.bytes());
            w.bytes().len() as u64
        }
        Err(msg) => {
            push_bytes(msg.as_bytes());
            1 << 32 | msg.len() as u64
        }
    }
}

/// The link of [`service_links!`] in a native library. Two definitions chosen HERE: a `cfg(feature)` inside an exported
/// macro would be read in the kit's crate, which has no such feature. The provider is asked when first called (it may
/// be loaded after this kit), per thread as the kits are, until there is one (a game on this thread can load more).
#[cfg(feature = "common-memory")]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_service_link {
    ($capability:literal) => {
        thread_local! {
            static PROVIDER: ::std::cell::Cell<Option<$crate::kitcall::native::SvcProvider>> =
                const { ::std::cell::Cell::new(None) };
        }
        fn provider() -> $crate::kitcall::native::SvcProvider {
            if let Some(p) = PROVIDER
                .with(::std::cell::Cell::get)
                .filter(|p| p.id.is_some())
            {
                return p;
            }
            let p = $crate::native::svc_provider($capability);
            PROVIDER.with(|c| c.set(Some(p)));
            p
        }
        unsafe extern "C" fn call(capability: u32, method: u32, words: u32) -> u64 {
            match provider().call {
                Some(f) => f(capability, method, words),
                None => 1 << 32,
            }
        }
        unsafe extern "C" fn id() -> u32 {
            match provider().id {
                Some(f) => f(),
                None => 0,
            }
        }
        $crate::svc::link($capability, call, id);
    };
}

/// Off the common memory (the façade checked or tested natively) there is no host to link through.
#[cfg(not(feature = "common-memory"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_service_link {
    ($capability:literal) => {};
}

/// Declares the direct links of a kit crate written by hand:
/// `service_links!(link_health = "combat.health" in "gf:svc.combat.health@^1");` declares the imports of that
/// capability (the import module is `gf:svc.<capability>@<requirement>`, written out: an attribute takes a literal)
/// and a function `link_health()` to call once at the module's start (the 4th argument of `export_kit!`).
/// tools/kit-modules/build.mjs writes the same for the capabilities a kit's source names.
#[macro_export]
macro_rules! service_links {
    ($($name:ident = $capability:literal in $module:literal),* $(,)?) => {
        $(
            #[cfg(target_arch = "wasm32")]
            pub fn $name() {
                #[link(wasm_import_module = $module)]
                extern "C" {
                    fn svc_call(capability: u32, method: u32, words: u32) -> u64;
                    fn svc_id() -> u32;
                }
                // Wrapped: the address of an IMPORTED function is a symbol left for a dynamic linker in a
                // relocatable module (the common memory); the address of a function of this module is not.
                unsafe extern "C" fn call(capability: u32, method: u32, words: u32) -> u64 {
                    svc_call(capability, method, words)
                }
                unsafe extern "C" fn id() -> u32 {
                    svc_id()
                }
                $crate::svc::link($capability, call, id);
            }
            // A native library of the kit: the same link, wired by the host (`__native_service_link!`).
            #[cfg(not(target_arch = "wasm32"))]
            pub fn $name() {
                $crate::__native_service_link!($capability);
            }
        )*
    };
}
