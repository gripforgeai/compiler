//! The module side of `gripforge:calls@1`: call other modules, expose functions to them, share regions of the
//! zone, publish a state read by many. The reference: `docs/module-calls.md`.
//!
//! ```ignore
//! use gripforge_abi::calls::guest::{self, Target};
//! // Exposed once (in `init`), by name and number (a call by number skips the name lookup).
//! guest::expose_id(1, "sum", |args| Ok(guest::pack(&[guest::unpack::<f32>(args).iter().sum::<f32>()])));
//! // Resolved once (the only string): a handle the hot path uses.
//! let physics = Target::resolve("physics.query@^1").unwrap();
//! // Asynchronous, the default: posted now, answered later (take_reply, or a callback run by gf_on_reply).
//! let id = guest::call(physics, 1u32, &guest::pack(&rays))?;
//! guest::call_then(physics, 1u32, &args, |result| { /* … */ })?;
//! // The exceptions, synchronous: by the bridge (copies, portable) or by the zone stack (large volumes).
//! let bytes = guest::call_sync(physics, 1u32, &args)?;
//! let bytes = guest::call_stack(physics, 1u32, &args)?;
//! gripforge_abi::calls::export_calls!();   // gf_alloc, gf_on_call, gf_on_reply, gf_on_bridge, gf_on_invoke
//! ```
//!
//! One call per frame with arrays (`pack` / `unpack`) rather than one call per entity. A state many modules read
//! every frame (inputs, settings) is a [`Publication`]: written once per frame into the zone, read without a call.
//! A direct link (the fastest sync path, scalars only) needs no SDK: see `docs/module-calls.md` §Direct links.
//!
//! Re-entrance: a handler may post calls (async) freely, including to its own caller; a sync call to a module on
//! the call stack answers [`Status::Reentrant`]. The SDK never keeps a borrow across a host call.
use crate::calls::{
    encode_call, encode_reply, regs, CallHeader, Func, ReplyHeader, Status, TargetRef, HEADER_LEN,
};
use core::cell::RefCell;
use core::fmt;
use std::boxed::Box;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

/// Why a call gave no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    pub status: Status,
    /// The target's message for [`Status::Failed`] (and the host's for a reply it wrote), else empty.
    pub message: String,
}

impl CallError {
    pub fn new(status: Status) -> Self {
        CallError {
            status,
            message: String::new(),
        }
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "call {}", self.status.name())
        } else {
            write!(f, "call {}: {}", self.status.name(), self.message)
        }
    }
}

/// What a call answers.
pub type CallResult = Result<Vec<u8>, CallError>;
/// An exposed function: the argument bytes in, the result bytes out, or a message (a [`Status::Failed`] reply).
pub type Handler = Rc<dyn Fn(&[u8]) -> Result<Vec<u8>, String>>;
type Callback = Box<dyn FnOnce(CallResult)>;

struct Exposed {
    id: Option<u32>,
    name: String,
    run: Handler,
}

#[derive(Default)]
struct State {
    exposed: Vec<Exposed>,
    next_id: u32,
    replies: BTreeMap<u32, CallResult>,
    callbacks: BTreeMap<u32, Callback>,
    in_flight: BTreeMap<u32, ()>,
    answer: Vec<u8>,
}

// One module instance is one thread: a `static` with interior mutability is enough (no `std`).
struct Global(RefCell<Option<State>>);
unsafe impl Sync for Global {}
static STATE: Global = Global(RefCell::new(None));

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut s = STATE.0.borrow_mut();
    f(s.get_or_insert_with(State::default))
}

/// The module is about to be unloaded (`gf_calls_exit`, called by the host): everything this side keeps — the
/// functions it exposed, the replies and callbacks still waiting, its answer buffer — is dropped, so a module that
/// lives in a memory it shares with others (docs/kit-modules.md §9) gives that heap back.
pub fn exit() {
    STATE.0.borrow_mut().take();
}

/// The host functions (`gf_calls` imports). Natively there is no host: every call answers `Trapped`.
#[allow(clippy::missing_safety_doc)]
pub mod sys {
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf_calls")]
    unsafe extern "C" {
        pub fn gf_call(ptr: u32) -> u64;
        pub fn gf_resolve(name: *const u8, len: u32) -> u32;
        pub fn gf_zone_alloc(len: u32) -> u32;
        pub fn gf_zone_free(ptr: u32);
        pub fn gf_zone_write(ptr: u32, src: *const u8, len: u32) -> u32;
        pub fn gf_zone_read(ptr: u32, dst: *mut u8, len: u32) -> u32;
        pub fn gf_regs() -> u32;
        pub fn gf_sp() -> u32;
        pub fn gf_push(src: *const u8, len: u32) -> u32;
        pub fn gf_pop(mark: u32);
        pub fn gf_invoke(
            handle: u32,
            func_id: u32,
            name: *const u8,
            name_len: u32,
            args: u32,
            args_len: u32,
        ) -> u32;
        pub fn gf_ret(status: u32, ptr: u32, len: u32);
        pub fn gf_bridge(
            handle: u32,
            func_id: u32,
            name: *const u8,
            name_len: u32,
            args: *const u8,
            args_len: u32,
        ) -> u64;
        pub fn gf_publish(name: *const u8, len: u32, ptr: u32, size: u32) -> u32;
        pub fn gf_lookup(name: *const u8, len: u32) -> u64;
        pub fn gf_host_caps() -> u32;
    }
    // Natively (a module built as a library: `.so` / `.dll` / `.dylib`) the host hands a table of the same
    // functions at load (`gf_calls_bind`, [`super::native`]). Without a host (a unit test), every call answers as
    // if nothing was declared.
    #[cfg(not(target_arch = "wasm32"))]
    mod native {
        use super::super::native::api;
        use crate::calls::{pack_call_result, Status};
        pub unsafe fn gf_call(ptr: u32) -> u64 {
            api().map_or(pack_call_result(Status::Trapped, 0), |a| unsafe {
                (a.gf_call)(a.ctx, ptr)
            })
        }
        pub unsafe fn gf_resolve(name: *const u8, len: u32) -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_resolve)(a.ctx, name, len) })
        }
        pub unsafe fn gf_zone_alloc(len: u32) -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_zone_alloc)(a.ctx, len) })
        }
        pub unsafe fn gf_zone_free(ptr: u32) {
            if let Some(a) = api() {
                unsafe { (a.gf_zone_free)(a.ctx, ptr) }
            }
        }
        pub unsafe fn gf_zone_write(ptr: u32, src: *const u8, len: u32) -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_zone_write)(a.ctx, ptr, src, len) })
        }
        pub unsafe fn gf_zone_read(ptr: u32, dst: *mut u8, len: u32) -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_zone_read)(a.ctx, ptr, dst, len) })
        }
        pub unsafe fn gf_regs() -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_regs)(a.ctx) })
        }
        pub unsafe fn gf_sp() -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_sp)(a.ctx) })
        }
        pub unsafe fn gf_push(src: *const u8, len: u32) -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_push)(a.ctx, src, len) })
        }
        pub unsafe fn gf_pop(mark: u32) {
            if let Some(a) = api() {
                unsafe { (a.gf_pop)(a.ctx, mark) }
            }
        }
        pub unsafe fn gf_invoke(
            handle: u32,
            func_id: u32,
            name: *const u8,
            name_len: u32,
            args: u32,
            args_len: u32,
        ) -> u32 {
            api().map_or(Status::Trapped as u32, |a| unsafe {
                (a.gf_invoke)(a.ctx, handle, func_id, name, name_len, args, args_len)
            })
        }
        pub unsafe fn gf_ret(status: u32, ptr: u32, len: u32) {
            if let Some(a) = api() {
                unsafe { (a.gf_ret)(a.ctx, status, ptr, len) }
            }
        }
        /// The bridge of a native module: the answer `[status u32][bytes]` is read where the host says (`null`:
        /// refused, the status in `out_len`), valid until this module calls the host again.
        pub unsafe fn gf_bridge_native(
            handle: u32,
            func_id: u32,
            name: *const u8,
            name_len: u32,
            args: *const u8,
            args_len: u32,
            out_len: *mut u32,
        ) -> *const u8 {
            match api() {
                Some(a) => unsafe {
                    (a.gf_bridge)(
                        a.ctx, handle, func_id, name, name_len, args, args_len, out_len,
                    )
                },
                None => {
                    unsafe { *out_len = Status::Trapped as u32 };
                    core::ptr::null()
                }
            }
        }
        pub unsafe fn gf_publish(name: *const u8, len: u32, ptr: u32, size: u32) -> u32 {
            api().map_or(0, |a| unsafe {
                (a.gf_publish)(a.ctx, name, len, ptr, size)
            })
        }
        pub unsafe fn gf_lookup(name: *const u8, len: u32) -> u64 {
            api().map_or(0, |a| unsafe { (a.gf_lookup)(a.ctx, name, len) })
        }
        pub unsafe fn gf_host_caps() -> u32 {
            api().map_or(0, |a| unsafe { (a.gf_host_caps)(a.ctx) })
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub use native::*;
}

/// What the host offers ([`crate::calls::caps`] bits).
pub fn host_caps() -> u32 {
    unsafe { sys::gf_host_caps() }
}

// ---- zone, regions ---------------------------------------------------------------------------------------------

/// `len` bytes of the zone at `ptr` (copied into this module's memory).
pub fn zone_read(ptr: u32, len: u32) -> Option<Vec<u8>> {
    let mut b = vec![0u8; len as usize];
    if len == 0 {
        return Some(b);
    }
    (unsafe { sys::gf_zone_read(ptr, b.as_mut_ptr(), len) } != 0).then_some(b)
}

fn zone_u32(ptr: u32) -> u32 {
    let mut b = [0u8; 4];
    unsafe { sys::gf_zone_read(ptr, b.as_mut_ptr(), 4) };
    u32::from_le_bytes(b)
}

/// A region of the zone this module owns (a channel, a large buffer shared over several calls: pass its offset in
/// the arguments). Its offset never changes, even when the zone grows. Given back when dropped.
#[derive(Debug)]
pub struct Region {
    ptr: u32,
    len: u32,
}

impl Region {
    /// A region of `len` bytes; `None` when the zone is at its ceiling (see `docs/module-calls.md` §Zone size).
    pub fn new(len: u32) -> Option<Region> {
        let ptr = unsafe { sys::gf_zone_alloc(len) };
        (ptr != 0).then_some(Region { ptr, len })
    }
    pub fn ptr(&self) -> u32 {
        self.ptr
    }
    pub fn len(&self) -> u32 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Writes `bytes` at `offset`; false when it does not fit.
    pub fn write(&self, offset: u32, bytes: &[u8]) -> bool {
        offset as usize + bytes.len() <= self.len as usize
            && unsafe { sys::gf_zone_write(self.ptr + offset, bytes.as_ptr(), bytes.len() as u32) }
                != 0
    }
    /// `len` bytes at `offset`.
    pub fn read(&self, offset: u32, len: u32) -> Option<Vec<u8>> {
        (offset + len <= self.len)
            .then(|| zone_read(self.ptr + offset, len))
            .flatten()
    }
    /// Keeps the region past this value (a region handed to the host, or freed later with `free_region`).
    pub fn leak(self) -> u32 {
        let p = self.ptr;
        core::mem::forget(self);
        p
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        unsafe { sys::gf_zone_free(self.ptr) }
    }
}

/// Gives back a region kept with [`Region::leak`].
pub fn free_region(ptr: u32) {
    unsafe { sys::gf_zone_free(ptr) }
}

/// A block of the zone holding `bytes`, built by `build` for its own offset; `None` when the zone is full.
fn post_block(len: usize, build: impl FnOnce(u32) -> Vec<u8>) -> Option<u32> {
    let ptr = unsafe { sys::gf_zone_alloc(len as u32) };
    if ptr == 0 {
        return None;
    }
    let bytes = build(ptr);
    if unsafe { sys::gf_zone_write(ptr, bytes.as_ptr(), bytes.len() as u32) } == 0 {
        unsafe { sys::gf_zone_free(ptr) };
        return None;
    }
    Some(ptr)
}

// ---- publication -----------------------------------------------------------------------------------------------

/// A state written once per frame into the zone and read by many without a call: `[seq u32][len u32][bytes]`,
/// `seq` odd while it is written (a seqlock: a reader on another thread retries).
pub struct Publication {
    region: Region,
    seq: u32,
}

impl Publication {
    /// Publishes under `name` (`input.state`, `settings`) with room for `capacity` bytes.
    pub fn new(name: &str, capacity: u32) -> Option<Publication> {
        let region = Region::new(8 + capacity)?;
        region.write(0, &[0u8; 8]);
        let ok =
            unsafe { sys::gf_publish(name.as_ptr(), name.len() as u32, region.ptr, region.len) }
                != 0;
        ok.then_some(Publication { region, seq: 0 })
    }
    /// Writes this frame's state; false when it is larger than the capacity.
    pub fn write(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() as u32 > self.region.len - 8 {
            return false;
        }
        self.seq = self.seq.wrapping_add(1) | 1;
        self.region.write(0, &self.seq.to_le_bytes());
        self.region.write(4, &(bytes.len() as u32).to_le_bytes());
        self.region.write(8, bytes);
        self.seq = self.seq.wrapping_add(1);
        self.region.write(0, &self.seq.to_le_bytes())
    }
}

/// The reading side of a [`Publication`]: found once by name, then read without a call.
pub struct Subscription {
    ptr: u32,
    size: u32,
    seen: u32,
}

impl Subscription {
    pub fn open(name: &str) -> Option<Subscription> {
        let r = unsafe { sys::gf_lookup(name.as_ptr(), name.len() as u32) };
        (r != 0).then_some(Subscription {
            ptr: r as u32,
            size: (r >> 32) as u32,
            seen: 0,
        })
    }
    /// The state when it changed since the last read (`None`: unchanged, or being written on another thread).
    pub fn read(&mut self) -> Option<Vec<u8>> {
        let seq = zone_u32(self.ptr);
        if seq == self.seen || seq & 1 == 1 {
            return None;
        }
        let len = zone_u32(self.ptr + 4).min(self.size - 8);
        let bytes = zone_read(self.ptr + 8, len)?;
        (zone_u32(self.ptr) == seq).then(|| {
            self.seen = seq;
            bytes
        })
    }
}

// ---- arrays (one call per frame, not one per entity) -----------------------------------------------------------

/// A fixed-size little-endian scalar of [`pack`] / [`unpack`].
pub trait Scalar: Copy {
    const SIZE: usize;
    fn put(self, out: &mut Vec<u8>);
    fn get(b: &[u8]) -> Self;
}
macro_rules! scalar {
    ($($t:ty),*) => {$(
        impl Scalar for $t {
            const SIZE: usize = core::mem::size_of::<$t>();
            fn put(self, out: &mut Vec<u8>) { out.extend_from_slice(&self.to_le_bytes()); }
            fn get(b: &[u8]) -> Self { <$t>::from_le_bytes(b[..Self::SIZE].try_into().unwrap()) }
        }
    )*};
}
scalar!(u8, u16, u32, u64, i32, i64, f32, f64);

/// An array of scalars as bytes (the arguments of a batch call).
pub fn pack<T: Scalar>(values: &[T]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * T::SIZE);
    for v in values {
        v.put(&mut out);
    }
    out
}

/// Bytes as an array of scalars (a trailing partial value is ignored).
pub fn unpack<T: Scalar>(bytes: &[u8]) -> Vec<T> {
    bytes.chunks_exact(T::SIZE).map(T::get).collect()
}

// ---- exposing --------------------------------------------------------------------------------------------------

/// Exposes `name` to the other modules.
pub fn expose(name: &str, run: impl Fn(&[u8]) -> Result<Vec<u8>, String> + 'static) {
    expose_inner(None, name, Rc::new(run));
}

/// Exposes `name` under the number `id` as well (a call by number skips the name lookup).
pub fn expose_id(id: u32, name: &str, run: impl Fn(&[u8]) -> Result<Vec<u8>, String> + 'static) {
    expose_inner(Some(id), name, Rc::new(run));
}

fn expose_inner(id: Option<u32>, name: &str, run: Handler) {
    with(|s| {
        s.exposed
            .retain(|e| e.name != name && (id.is_none() || e.id != id));
        s.exposed.push(Exposed {
            id,
            name: name.to_string(),
            run,
        });
    });
}

fn find(func_id: u32, name: &[u8]) -> Result<Handler, (Status, String)> {
    with(|s| {
        s.exposed
            .iter()
            .find(|e| {
                if name.is_empty() {
                    e.id == Some(func_id)
                } else {
                    e.name.as_bytes() == name
                }
            })
            .map(|e| e.run.clone())
    })
    .ok_or_else(|| {
        let what = if name.is_empty() {
            std::format!("#{func_id}")
        } else {
            String::from_utf8_lossy(name).into_owned()
        };
        (Status::NoFunction, what)
    })
}

fn run(func_id: u32, name: &[u8], args: &[u8]) -> (Status, Vec<u8>) {
    match find(func_id, name).and_then(|h| h(args).map_err(|m| (Status::Failed, m))) {
        Ok(bytes) => (Status::Ok, bytes),
        Err((status, message)) => (status, message.into_bytes()),
    }
}

// ---- calling ---------------------------------------------------------------------------------------------------

/// A module resolved once by name (`gf_resolve`): calls by handle carry no string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target(pub u32);

impl Target {
    /// `name@requirement`; `None` when the game does not declare a matching module.
    pub fn resolve(name: &str) -> Option<Target> {
        let h = unsafe { sys::gf_resolve(name.as_ptr(), name.len() as u32) };
        (h != 0).then_some(Target(h))
    }
}

impl From<Target> for TargetRef<'_> {
    fn from(t: Target) -> Self {
        TargetRef::Handle(t.0)
    }
}

fn handle_of(target: TargetRef<'_>) -> Result<u32, CallError> {
    match target {
        TargetRef::Handle(h) => Ok(h),
        TargetRef::Name(n) => Target::resolve(n)
            .map(|t| t.0)
            .ok_or(CallError::new(Status::NotDeclared)),
    }
}

fn func_parts<'a>(func: Func<'a>) -> (u32, &'a [u8]) {
    match func {
        Func::Id(id) => (id, &[]),
        Func::Name(n) => (crate::calls::FUNC_BY_NAME, n.as_bytes()),
    }
}

fn next_id() -> u32 {
    with(|s| {
        s.next_id = s.next_id.wrapping_add(1).max(1);
        while s.in_flight.contains_key(&s.next_id) {
            s.next_id = s.next_id.wrapping_add(1).max(1);
        }
        s.next_id
    })
}

/// Posts a call (asynchronous, the default). The reply is kept for [`take_reply`].
pub fn call<'a, 'b>(
    target: impl Into<TargetRef<'a>>,
    func: impl Into<Func<'b>>,
    args: &[u8],
) -> Result<u32, CallError> {
    let (target, func) = (target.into(), func.into());
    let tlen = if let TargetRef::Name(n) = target {
        n.len()
    } else {
        0
    };
    let flen = if let Func::Name(n) = func { n.len() } else { 0 };
    if tlen > u16::MAX as usize || flen > u16::MAX as usize {
        return Err(CallError::new(Status::BadMessage));
    }
    let id = next_id();
    let ptr = post_block(HEADER_LEN + tlen + flen + args.len(), |at| {
        encode_call(at, id, 0, target, func, args)
    })
    .ok_or(CallError::new(Status::ZoneFull))?;
    // The block is the host's from here on (it frees it): never touched again.
    match crate::calls::unpack_call_result(unsafe { sys::gf_call(ptr) }).0 {
        Status::Pending => {
            with(|s| s.in_flight.insert(id, ()));
            Ok(id)
        }
        status => Err(CallError::new(status)),
    }
}

/// Posts a call whose reply runs `then` (from `gf_on_reply`, outside of this module's other exports).
pub fn call_then<'a, 'b>(
    target: impl Into<TargetRef<'a>>,
    func: impl Into<Func<'b>>,
    args: &[u8],
    then: impl FnOnce(CallResult) + 'static,
) -> Result<u32, CallError> {
    let id = call(target, func, args)?;
    with(|s| s.callbacks.insert(id, Box::new(then)));
    Ok(id)
}

/// The reply of the call `id` once it arrived (taken: asked again, `None`).
pub fn take_reply(id: u32) -> Option<CallResult> {
    with(|s| s.replies.remove(&id))
}

/// Calls still waiting for their reply.
pub fn in_flight() -> usize {
    with(|s| s.in_flight.len())
}

fn status_result(status: Status, bytes: Vec<u8>) -> CallResult {
    if status == Status::Ok {
        Ok(bytes)
    } else {
        Err(CallError {
            status,
            message: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }
}

/// Synchronous by the bridge (the base sync path): the host copies `args` into the target's memory, runs it and
/// copies the answer back. Only for a target loaded on this thread and not on the call stack — otherwise
/// [`Status::Loading`] (its load started: try again later, or call async) or [`Status::Reentrant`].
pub fn call_sync<'a, 'b>(
    target: impl Into<TargetRef<'a>>,
    func: impl Into<Func<'b>>,
    args: &[u8],
) -> CallResult {
    let handle = handle_of(target.into())?;
    let (fid, name) = func_parts(func.into());
    #[cfg(not(target_arch = "wasm32"))]
    let answer = {
        // A native module: `[status u32][bytes]` read where the host says, copied before anything else runs.
        let mut len = 0u32;
        let ptr = unsafe {
            sys::gf_bridge_native(
                handle,
                fid,
                name.as_ptr(),
                name.len() as u32,
                args.as_ptr(),
                args.len() as u32,
                &mut len,
            )
        };
        if ptr.is_null() {
            return Err(CallError::new(
                Status::from_u16(len as u16).unwrap_or(Status::Trapped),
            ));
        }
        unsafe { core::slice::from_raw_parts(ptr, len as usize) }.to_vec()
    };
    #[cfg(target_arch = "wasm32")]
    let answer = {
        let r = unsafe {
            sys::gf_bridge(
                handle,
                fid,
                name.as_ptr(),
                name.len() as u32,
                args.as_ptr(),
                args.len() as u32,
            )
        };
        let (ptr, len) = (r as u32, (r >> 32) as u32);
        if ptr == 0 {
            return Err(CallError::new(
                Status::from_u16(len as u16).unwrap_or(Status::Trapped),
            ));
        }
        // `[status u32][bytes]`, in a buffer of ours the host asked with gf_alloc.
        unsafe { take(ptr, len) }
    };
    if answer.len() < 4 {
        return Err(CallError::new(Status::Trapped));
    }
    let status = Status::from_u16(u32::from_le_bytes(answer[..4].try_into().unwrap()) as u16)
        .unwrap_or(Status::Trapped);
    status_result(status, answer[4..].to_vec())
}

/// Synchronous by the registers and the stack of the zone: the arguments are pushed once into the zone and the
/// target reads them there (in place when it maps the zone). For large volumes; same rules as [`call_sync`].
pub fn call_stack<'a, 'b>(
    target: impl Into<TargetRef<'a>>,
    func: impl Into<Func<'b>>,
    args: &[u8],
) -> CallResult {
    let handle = handle_of(target.into())?;
    let (fid, name) = func_parts(func.into());
    let mark = unsafe { sys::gf_sp() };
    let at = if args.is_empty() {
        0
    } else {
        unsafe { sys::gf_push(args.as_ptr(), args.len() as u32) }
    };
    if !args.is_empty() && at == 0 {
        return Err(CallError::new(Status::StackOverflow));
    }
    let status = unsafe {
        sys::gf_invoke(
            handle,
            fid,
            name.as_ptr(),
            name.len() as u32,
            at,
            args.len() as u32,
        )
    };
    let rg = unsafe { sys::gf_regs() };
    let result = zone_read(
        zone_u32(rg + regs::RESULT as u32),
        zone_u32(rg + regs::RESULT_LEN as u32),
    )
    .unwrap_or_default();
    unsafe { sys::gf_pop(mark) };
    status_result(
        Status::from_u16(status as u16).unwrap_or(Status::Trapped),
        result,
    )
}

// ---- the exports ---------------------------------------------------------------------------------------------

/// The memory the host writes into (`gf_alloc`): a buffer of `len` bytes, taken back with [`take`].
pub fn alloc_buffer(len: u32) -> u32 {
    let buffer = vec![0u8; len as usize].into_boxed_slice();
    Box::into_raw(buffer) as *mut u8 as usize as u32
}

/// Takes back a buffer of [`alloc_buffer`].
///
/// # Safety
/// `ptr`, `len` must be those of one `alloc_buffer` not taken yet.
pub unsafe fn take(ptr: u32, len: u32) -> Box<[u8]> {
    unsafe {
        Box::from_raw(core::ptr::slice_from_raw_parts_mut(
            ptr as usize as *mut u8,
            len as usize,
        ))
    }
}

fn answer(status: Status, bytes: &[u8]) -> u64 {
    with(|s| {
        s.answer.clear();
        s.answer.extend_from_slice(&(status as u32).to_le_bytes());
        s.answer.extend_from_slice(bytes);
        s.answer.as_ptr() as usize as u64 | (s.answer.len() as u64) << 32
    })
}

/// The export `gf_on_bridge(func_id, name_len, ptr, len)`: `[name][args]` in a buffer of `gf_alloc`; answers
/// `ptr | len << 32` of `[status u32][bytes]`, valid until the next bridge call of this module.
pub fn on_bridge(func_id: u32, name_len: u32, ptr: u32, len: u32) -> u64 {
    let input = unsafe { take(ptr, len) };
    let n = (name_len as usize).min(input.len());
    let (status, bytes) = run(func_id, &input[..n], &input[n..]);
    answer(status, &bytes)
}

/// The export `gf_on_invoke()`: reads the registers, runs the function, pushes the result and sets it (`gf_ret`).
pub fn on_invoke() {
    let rg = unsafe { sys::gf_regs() };
    let Some(r) = zone_read(rg, regs::LEN as u32) else {
        return;
    };
    let reg = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    let name = zone_read(reg(regs::FUNC_NAME), reg(regs::FUNC_NAME_LEN)).unwrap_or_default();
    let args = zone_read(reg(regs::ARGS), reg(regs::ARGS_LEN)).unwrap_or_default();
    let (status, bytes) = run(reg(regs::FUNC_ID), &name, &args);
    let at = if bytes.is_empty() {
        0
    } else {
        unsafe { sys::gf_push(bytes.as_ptr(), bytes.len() as u32) }
    };
    if !bytes.is_empty() && at == 0 {
        unsafe { sys::gf_ret(Status::StackOverflow as u32, 0, 0) };
    } else {
        unsafe { sys::gf_ret(status as u32, at, bytes.len() as u32) };
    }
}

/// `(call_id, result)` of the reply block at `ptr`.
fn read_reply(ptr: u32) -> Option<(u32, CallResult)> {
    let h = ReplyHeader::read(&zone_read(ptr, HEADER_LEN as u32)?)?;
    let bytes = zone_read(h.result_ptr, h.result_len)?;
    Some((
        h.call_id,
        status_result(
            Status::from_u16(h.status).unwrap_or(Status::BadMessage),
            bytes,
        ),
    ))
}

/// The export `gf_on_reply(ptr)`: a reply to one of this module's async calls (the host frees the block after).
pub fn on_reply(ptr: u32) {
    let Some((id, result)) = read_reply(ptr) else {
        return;
    };
    let then = with(|s| {
        s.in_flight.remove(&id);
        s.callbacks.remove(&id)
    });
    match then {
        Some(then) => then(result),
        None => {
            with(|s| s.replies.insert(id, result));
        }
    }
}

/// The export `gf_on_call(ptr) -> reply_ptr`: runs the function the call names and answers its reply block
/// (0: the zone had no room; the host answers [`Status::ZoneFull`]). The host frees the call block after.
pub fn on_call(ptr: u32) -> u32 {
    let Some(h) = zone_read(ptr, HEADER_LEN as u32).and_then(|b| CallHeader::read(&b)) else {
        return 0;
    };
    let names =
        zone_read(ptr + HEADER_LEN as u32, (h.names_end() - HEADER_LEN) as u32).unwrap_or_default();
    let fname = names.get(h.target_len as usize..).unwrap_or(&[]);
    let (status, result) = match zone_read(h.args_ptr, h.args_len) {
        Some(args) => run(h.func_id, fname, &args),
        None => (Status::BadMessage, Vec::new()),
    };
    post_block(HEADER_LEN + result.len(), |at| {
        encode_reply(at, h.call_id, status, &result)
    })
    .unwrap_or(0)
}

/// Exports the module side from a wasm module (natively it exports nothing). Invoke it once:
/// `export_calls!()` — with `gf_alloc`; `export_calls!(no_alloc)` in a presentation module whose
/// `export_module!` already exports the same `gf_alloc`.
#[macro_export]
macro_rules! export_calls {
    () => {
        $crate::calls::export_calls!(no_alloc);
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn gf_alloc(len: u32) -> u32 {
            $crate::calls::guest::alloc_buffer(len)
        }
    };
    (no_alloc) => {
        #[cfg(target_arch = "wasm32")]
        mod __gf_calls_exports {
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_calls_abi() -> u32 {
                $crate::calls::ABI
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_calls_exit() {
                $crate::calls::guest::exit()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_on_call(ptr: u32) -> u32 {
                $crate::calls::guest::on_call(ptr)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_on_reply(ptr: u32) {
                $crate::calls::guest::on_reply(ptr)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_on_bridge(func_id: u32, name_len: u32, ptr: u32, len: u32) -> u64 {
                $crate::calls::guest::on_bridge(func_id, name_len, ptr, len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_on_invoke() {
                $crate::calls::guest::on_invoke()
            }
        }
    };
}

// ---- a module built as a native library ---------------------------------------------------------------------

/// The module side natively: the same module source built as a library (`.so` / `.dll` / `.dylib`) for a native
/// calls host (`kits/modhost`, docs/module-calls-native.md). The host functions are not imports but a
/// table the host hands once at load ([`HostApi`], export `gf_calls_bind`); the exports keep their names, with
/// memory pointers at the width of the machine. Zone offsets, handles, function numbers, statuses and every byte
/// layout are those of the `.wasm` form.
#[cfg(not(target_arch = "wasm32"))]
pub mod native {
    extern crate std;
    use super::{run, with, Status};
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicPtr, Ordering};

    /// The host functions of `gripforge:calls@1` as a C table. `ctx` goes back as the first argument of each.
    /// It grows only at the end (`size` is its length in bytes for the host that filled it).
    #[repr(C)]
    pub struct HostApi {
        /// The host's ABI: `major << 16 | minor`.
        pub abi: u32,
        /// Bytes of this table.
        pub size: u32,
        pub ctx: *mut c_void,
        /// The id the host gave this module (what it stamps in `from`).
        pub module_id: u32,
        pub reserved: u32,
        pub gf_call: unsafe extern "C" fn(*mut c_void, u32) -> u64,
        pub gf_resolve: unsafe extern "C" fn(*mut c_void, *const u8, u32) -> u32,
        pub gf_zone_alloc: unsafe extern "C" fn(*mut c_void, u32) -> u32,
        pub gf_zone_free: unsafe extern "C" fn(*mut c_void, u32),
        pub gf_zone_write: unsafe extern "C" fn(*mut c_void, u32, *const u8, u32) -> u32,
        pub gf_zone_read: unsafe extern "C" fn(*mut c_void, u32, *mut u8, u32) -> u32,
        pub gf_zone_size: unsafe extern "C" fn(*mut c_void) -> u32,
        pub gf_regs: unsafe extern "C" fn(*mut c_void) -> u32,
        pub gf_sp: unsafe extern "C" fn(*mut c_void) -> u32,
        /// `src` null: room only, nothing copied.
        pub gf_push: unsafe extern "C" fn(*mut c_void, *const u8, u32) -> u32,
        pub gf_pop: unsafe extern "C" fn(*mut c_void, u32),
        pub gf_invoke: unsafe extern "C" fn(*mut c_void, u32, u32, *const u8, u32, u32, u32) -> u32,
        pub gf_ret: unsafe extern "C" fn(*mut c_void, u32, u32, u32),
        /// `(ctx, handle, func_id, name, name_len, args, args_len, out_len) -> answer`: `[status u32][bytes]`, or
        /// null with the refusal status in `out_len`.
        pub gf_bridge: unsafe extern "C" fn(
            *mut c_void,
            u32,
            u32,
            *const u8,
            u32,
            *const u8,
            u32,
            *mut u32,
        ) -> *const u8,
        pub gf_publish: unsafe extern "C" fn(*mut c_void, *const u8, u32, u32, u32) -> u32,
        pub gf_lookup: unsafe extern "C" fn(*mut c_void, *const u8, u32) -> u64,
        pub gf_host_caps: unsafe extern "C" fn(*mut c_void) -> u32,
        /// The zone in place: its first byte, valid until the next host call that may grow it (an allocation, a
        /// call). A native module reads and writes its own blocks there without a copy.
        pub gf_zone_base: unsafe extern "C" fn(*mut c_void) -> *mut u8,
        /// A direct link: `(ctx, "name@req", len, "fn", len)` → the provider's `gfx_<fn>` (null: not declared, not
        /// loaded, version, or a provider that is not a native library).
        pub gf_link:
            unsafe extern "C" fn(*mut c_void, *const u8, u32, *const u8, u32) -> *const c_void,
    }

    static API: AtomicPtr<HostApi> = AtomicPtr::new(core::ptr::null_mut());

    /// The export `gf_calls_bind(api)`: called once by the host before anything else; `api` lives as long as the
    /// module stays loaded.
    ///
    /// # Safety
    /// `api` is a table of the host that loaded this library.
    pub unsafe fn bind(api: *const HostApi) {
        API.store(api as *mut HostApi, Ordering::Release);
    }

    /// The host's table, once bound.
    pub fn api() -> Option<&'static HostApi> {
        unsafe { API.load(Ordering::Acquire).as_ref() }
    }

    /// What a native export answers when the module panicked (the host leaves it out: `trapped`).
    pub const TRAPPED: u32 = u32::MAX;

    fn guarded<R>(f: impl FnOnce() -> R) -> Option<R> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
    }

    /// The export `gf_alloc(len)`: a buffer the embedder writes into; given back with [`free`].
    pub fn alloc(len: u32) -> *mut u8 {
        std::boxed::Box::into_raw(std::vec![0u8; len as usize].into_boxed_slice()) as *mut u8
    }

    /// The export `gf_free(ptr, len)`.
    ///
    /// # Safety
    /// `ptr`, `len` are those of one [`alloc`] not freed yet.
    pub unsafe fn free(ptr: *mut u8, len: u32) {
        drop(unsafe {
            std::boxed::Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr, len as usize))
        });
    }

    /// The export `gf_on_bridge(func_id, name, name_len, args, args_len, out_len) -> answer`: the arguments are read
    /// where they are (no copy); the answer `[status u32][bytes]` stays valid until the next bridge call of this
    /// module. A panic answers null with [`TRAPPED`] in `out_len`.
    ///
    /// # Safety
    /// The pointers are readable for their lengths; `out_len` is writable.
    pub unsafe fn on_bridge(
        func_id: u32,
        name: *const u8,
        name_len: u32,
        args: *const u8,
        args_len: u32,
        out_len: *mut u32,
    ) -> *const u8 {
        let slice = |p: *const u8, l: u32| {
            if l == 0 {
                &[][..]
            } else {
                unsafe { core::slice::from_raw_parts(p, l as usize) }
            }
        };
        let (name, args) = (slice(name, name_len), slice(args, args_len));
        match guarded(|| run(func_id, name, args)) {
            Some((status, bytes)) => with(|s| {
                s.answer.clear();
                s.answer.extend_from_slice(&(status as u32).to_le_bytes());
                s.answer.extend_from_slice(&bytes);
                unsafe { *out_len = s.answer.len() as u32 };
                s.answer.as_ptr()
            }),
            None => {
                unsafe { *out_len = TRAPPED };
                core::ptr::null()
            }
        }
    }

    /// The export `gf_on_call(ptr) -> reply_ptr` ([`TRAPPED`] after a panic).
    pub fn on_call(ptr: u32) -> u32 {
        guarded(|| super::on_call(ptr)).unwrap_or(TRAPPED)
    }

    /// The export `gf_on_reply(ptr) -> 0` ([`TRAPPED`] after a panic).
    pub fn on_reply(ptr: u32) -> u32 {
        guarded(|| super::on_reply(ptr)).map_or(TRAPPED, |_| 0)
    }

    /// The export `gf_on_invoke() -> 0` ([`TRAPPED`] after a panic).
    pub fn on_invoke() -> u32 {
        guarded(super::on_invoke).map_or(TRAPPED, |_| 0)
    }

    /// Runs a module's own export body, a panic turned into `None` (the generated `gf_calls_init` wrapper).
    pub fn guard<R>(f: impl FnOnce() -> R) -> Option<R> {
        guarded(f)
    }

    /// A direct link of a native module: the provider's `gfx_<fn>`, asked to the host once then kept.
    pub struct DirectLink {
        target: &'static str,
        func: &'static str,
        ptr: AtomicPtr<c_void>,
    }

    impl DirectLink {
        pub const fn new(target: &'static str, func: &'static str) -> Self {
            DirectLink {
                target,
                func,
                ptr: AtomicPtr::new(core::ptr::null_mut()),
            }
        }
        /// The function's address. Panics (a trap for the host) when the host cannot link it.
        #[inline]
        pub fn get(&self) -> *const c_void {
            let p = self.ptr.load(Ordering::Relaxed);
            if !p.is_null() {
                return p;
            }
            let p = api().map_or(core::ptr::null(), |a| unsafe {
                (a.gf_link)(
                    a.ctx,
                    self.target.as_ptr(),
                    self.target.len() as u32,
                    self.func.as_ptr(),
                    self.func.len() as u32,
                )
            });
            if p.is_null() {
                panic!(
                    "direct link gf:{}/{} is not linked by the host",
                    self.target, self.func
                );
            }
            self.ptr.store(p as *mut c_void, Ordering::Relaxed);
            p
        }
    }

    /// `Status` of a trap, for hosts written against this crate.
    pub const STATUS_TRAPPED: Status = Status::Trapped;
}

/// Exports the module side from a native library (`crate-type = ["cdylib"]`); expands to nothing for wasm32, where
/// [`export_calls!`] does it. Invoke it once, beside `export_calls!()`. See docs/module-calls-native.md.
#[macro_export]
macro_rules! export_calls_native {
    () => {
        #[cfg(not(target_arch = "wasm32"))]
        mod __gf_calls_native_exports {
            use $crate::calls::guest::native;
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_calls_abi() -> u32 {
                $crate::calls::ABI
            }
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn gf_calls_bind(api: *const native::HostApi) {
                unsafe { native::bind(api) }
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_alloc(len: u32) -> *mut u8 {
                native::alloc(len)
            }
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn gf_free(ptr: *mut u8, len: u32) {
                unsafe { native::free(ptr, len) }
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_on_call(ptr: u32) -> u32 {
                native::on_call(ptr)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_on_reply(ptr: u32) -> u32 {
                native::on_reply(ptr)
            }
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn gf_on_bridge(
                func_id: u32,
                name: *const u8,
                name_len: u32,
                args: *const u8,
                args_len: u32,
                out_len: *mut u32,
            ) -> *const u8 {
                unsafe { native::on_bridge(func_id, name, name_len, args, args_len, out_len) }
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn gf_on_invoke() -> u32 {
                native::on_invoke()
            }
        }
    };
}

// ---- the fixed binary encoding of interface calls (the generated bindings use it) -------------------------------

/// Writes the arguments or the result of an interface function (`docs/module-calls.md` §Interfaces): scalars
/// little-endian at their size, `bool` as one byte, `string` and `list<scalar>` as a u32 count then the items.
#[derive(Debug, Default, Clone)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn new() -> Self {
        Writer(Vec::new())
    }
    pub fn scalar<T: Scalar>(&mut self, v: T) -> &mut Self {
        v.put(&mut self.0);
        self
    }
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.0.push(v as u8);
        self
    }
    pub fn string(&mut self, v: &str) -> &mut Self {
        self.0.extend_from_slice(&(v.len() as u32).to_le_bytes());
        self.0.extend_from_slice(v.as_bytes());
        self
    }
    pub fn list<T: Scalar>(&mut self, v: &[T]) -> &mut Self {
        self.0.extend_from_slice(&(v.len() as u32).to_le_bytes());
        for x in v {
            x.put(&mut self.0);
        }
        self
    }
    pub fn finish(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.0)
    }
}

/// Reads what a [`Writer`] wrote; a short or malformed input is an error (a `Failed` reply), never a panic.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| "arguments too short".to_string())?;
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }
    pub fn scalar<T: Scalar>(&mut self) -> Result<T, String> {
        Ok(T::get(self.take(T::SIZE)?))
    }
    pub fn bool(&mut self) -> Result<bool, String> {
        Ok(self.take(1)?[0] != 0)
    }
    pub fn string(&mut self) -> Result<String, String> {
        let n = self.scalar::<u32>()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "string not UTF-8".to_string())
    }
    pub fn list<T: Scalar>(&mut self) -> Result<Vec<T>, String> {
        let n = self.scalar::<u32>()? as usize;
        let b = self.take(
            n.checked_mul(T::SIZE)
                .ok_or_else(|| "list too long".to_string())?,
        )?;
        Ok(b.chunks_exact(T::SIZE).map(T::get).collect())
    }
    /// Bytes not read yet (a newer caller may send fields added at the end: they are ignored).
    pub fn rest(&self) -> usize {
        self.bytes.len() - self.at
    }
}

/// A status as a [`CallError`] when a reply decodes badly.
pub fn decode_error(message: String) -> CallError {
    CallError {
        status: Status::BadMessage,
        message,
    }
}
