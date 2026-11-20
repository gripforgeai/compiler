//! `gripforge:calls@1` — calls between GripForge modules (the reference: `docs/module-calls.md`).
//!
//! Every module (presentation or logic, one `.wasm` per kit) has its own linear memory and never sees another
//! module's. Modules talk through a **zone**: a byte region the host owns, distinct from every module memory.
//! A "pointer" of this contract is an offset in the zone (`0` is never a valid block: it means "none").
//!
//! A call is one message block in the zone: a 32-byte [`CallHeader`] ("I am <from>, call <call_id>, target
//! <name@version>, function <id or name>, args at <ptr, len>") followed by the target name, the function name and,
//! usually, the arguments. The caller hands the block to the host with the single calling import `gf_call(ptr)`.
//! The host stamps `from`, resolves the target and delivers the block to the target's export `gf_on_call(ptr)`;
//! the target answers a [`ReplyHeader`] block with the **same call id** ("I am <from>, reply to <call_id>,
//! status, result at <ptr, len>"), and the host hands it to the caller's export `gf_on_reply(ptr)`.
//!
//! **Asynchronous by default**: `gf_call` answers [`Status::Pending`] and the reply arrives later (when the
//! target is free, loaded, or in another thread / Web Worker, through a lock-free ring). This one model covers
//! loading on demand, workers and re-entrance (A→B→A just queues a call to A).
//!
//! **Synchronous calls are the exception** (a target already loaded, on the same thread, not on the call stack),
//! with three paths, fastest first:
//! 1. *direct link*: an import `gf:<name>@<req>` / `<fn>` the host wires at instantiation to the export `gfx_<fn>`
//!    of that module — a wasm→wasm call, scalars only (i32, i64, f32, f64, zone offsets);
//! 2. *bridge* (the base path, portable everywhere): `gf_bridge` — the host copies the arguments from the
//!    caller's memory into the target's (its `gf_alloc`), runs `gf_on_bridge`, copies the answer back;
//! 3. *registers and stack* ([`regs`]): `gf_invoke` — the arguments stay in the zone (large volumes, readable in
//!    place by a module that maps the zone as a second memory).
//!
//! The payloads are binary — postcard or gripforge-wire, never JSON.
//!
//! Layout: every integer little-endian; header blocks are 8-byte aligned by the host allocator.
// no_std in the source crate (gripforge-modcall): here the crate has std, so its `alloc::` paths are `std::`.

use std::string::String;
use std::vec::Vec;

#[cfg(feature = "guest")]
pub mod guest;
// The macros of `guest` name themselves `$crate::calls::…` (a #[macro_export] lands at the crate root).
#[cfg(feature = "guest")]
pub use crate::{export_calls, export_calls_native};
pub mod zone;

/// The contract's name and version: `gripforge:calls@1`.
pub const CONTRACT: &str = "gripforge:calls@1";
/// The version of the base host ABI a module is built for: `major << 16 | minor` (export `gf_calls_abi`). It only
/// grows by additions at the end (new imports, new statuses, new register words past `regs::LEN` in a next minor);
/// a host refuses a module of another major, and tells what it offers with `gf_host_caps` ([`caps`]).
///
/// History since 1.0, the value unchanged:
/// - 20/11 (feat/abi-dev, docs/decisions/2026-11-20-abi-dev.md): the value is `ABI_DEV` in the repository, the
///   number of core/abi/abi-versions.json is written at the tag only. Rebuild every binary once (player, modules,
///   gateways): a binary of before answers its old number and is refused.
/// - 20/11 (feat/abi-dev, #1798): the web host (core/gateway-web/calls.mjs `ABI`) is ABI_DEV too (table line
///   `calls_web`); in development it compares the build print of the ABI sources (`gripforge.abi_print`) and warns.
/// - 20/11 (feat/abi-contrat-chargement): the build print `gripforge.abi_print` is gone; each module declares the
///   contracts it reads or writes (`gripforge.contracts`, [`crate::contract`]) and the host checks only those, by
///   their fields (works with ABI_DEV). Nothing to rebuild: a module of before carries no declaration and keeps the
///   older checks.
pub const ABI: u32 = crate::ABI_DEV;
/// What a host offers beyond the base (bits of `gf_host_caps()`); a module asks before using one.
pub mod caps {
    /// `gf_calls.zone` can be imported as a second memory (multi-memory): the zone read and written in place.
    pub const ZONE_MEMORY: u32 = 1;
    /// The zone is shared memory (Workers see it; the rings are lock-free).
    pub const SHARED_ZONE: u32 = 2;
    /// Modules may be placed in Workers (`thread` of the manifest).
    pub const WORKERS: u32 = 4;
    /// The tracer is on.
    pub const TRACE: u32 = 8;
}
/// The `version` byte of every header of this contract.
pub const VERSION: u8 = 1;
/// The wasm import module of the host functions (`gf_call`, `gf_zone_*`).
pub const IMPORT_MODULE: &str = "gf_calls";
/// Size of a call or reply header.
pub const HEADER_LEN: usize = 32;
/// `kind` byte of a call.
pub const KIND_CALL: u8 = 1;
/// `kind` byte of a reply.
pub const KIND_REPLY: u8 = 2;
/// Call flag reserved: a synchronous call never travels as a block (it takes the registers and the stack,
/// [`regs`]); a block with this flag is refused ([`Status::BadMessage`]).
pub const FLAG_SYNC: u16 = 1;
/// `func_id` of a call by name (`func_len > 0`).
pub const FUNC_BY_NAME: u32 = u32::MAX;
/// `from` of a reply the host wrote itself (the target never answered: not declared, trapped, load failed…).
pub const HOST_ID: u32 = 0;

/// The status of a reply, or what `gf_call` answers at once (low 32 bits of its `u64`).
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// The function answered; the result is its bytes.
    Ok = 0,
    /// `gf_call` only: posted, the reply will reach `gf_on_reply`.
    Pending = 1,
    /// Sync `gf_call` only: the target is not loaded; its load started, try again later (or call async).
    Loading = 2,
    /// The game (or a module manifest it declared) never declared this target: it is not loaded.
    NotDeclared = 3,
    /// The target has no such function.
    NoFunction = 4,
    /// The function ran and failed; the result is a UTF-8 message.
    Failed = 5,
    /// The target trapped, or has no `gf_on_call`, or answered something that is not a reply to this call.
    Trapped = 6,
    /// Sync `gf_call` only: the target is on the call stack (A→B→A); call it async instead.
    Reentrant = 7,
    /// The call block is not a call of this contract (kind, version, bounds, UTF-8).
    BadMessage = 8,
    /// A module of that name is declared but its interface version does not match the requirement.
    Version = 9,
    /// The zone has no room for the block.
    ZoneFull = 10,
    /// The target was declared but could not be loaded.
    LoadFailed = 11,
    /// The host's queue of pending messages is full.
    QueueFull = 12,
    /// A sync call would pass the stack's size or its maximum depth (`regs::MAX_DEPTH`).
    StackOverflow = 13,
    /// A sync call to a module placed on another thread (a Worker): call it async (the default) instead.
    OtherThread = 14,
}

impl Status {
    pub fn from_u16(v: u16) -> Option<Status> {
        use Status::*;
        Some(match v {
            0 => Ok,
            1 => Pending,
            2 => Loading,
            3 => NotDeclared,
            4 => NoFunction,
            5 => Failed,
            6 => Trapped,
            7 => Reentrant,
            8 => BadMessage,
            9 => Version,
            10 => ZoneFull,
            11 => LoadFailed,
            12 => QueueFull,
            13 => StackOverflow,
            14 => OtherThread,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        use Status::*;
        match self {
            Ok => "ok",
            Pending => "pending",
            Loading => "loading",
            NotDeclared => "not_declared",
            NoFunction => "no_function",
            Failed => "failed",
            Trapped => "trapped",
            Reentrant => "reentrant",
            BadMessage => "bad_message",
            Version => "version",
            ZoneFull => "zone_full",
            LoadFailed => "load_failed",
            QueueFull => "queue_full",
            StackOverflow => "stack_overflow",
            OtherThread => "other_thread",
        }
    }
}

/// What `gf_call` answers: `status | reply_ptr << 32` (the reply pointer only for a sync [`Status::Ok`]).
pub fn pack_call_result(status: Status, reply_ptr: u32) -> u64 {
    status as u64 | (reply_ptr as u64) << 32
}

/// The two halves of a `gf_call` answer (an unknown status reads as [`Status::BadMessage`]).
pub fn unpack_call_result(v: u64) -> (Status, u32) {
    (
        Status::from_u16(v as u16).unwrap_or(Status::BadMessage),
        (v >> 32) as u32,
    )
}

// The positions of the call header and of the reply header: their record contracts (core/abi/contracts/
// abi.calls.header.json, abi.calls.reply.json), never numbers written here.
use gripforge_contracts::abi_calls_header::at as H;
use gripforge_contracts::abi_calls_reply::at as R;
const _: () = {
    assert!(HEADER_LEN == gripforge_contracts::abi_calls_header::SIZE);
    assert!(HEADER_LEN == gripforge_contracts::abi_calls_reply::SIZE);
};

/// The contracts a module of `gripforge:calls@1` reads or writes by itself, with their role: the headers of its calls
/// and replies, and natively the table of the host functions (declared on every target: one section). A module that
/// speaks only this contract declares them once (`contracts_section!(gripforge_abi::calls::CONTRACTS)`, the test
/// modules of core/abi/test-modules); a kit module declares them in the façade's list (`gripforge_kit_guest::contracts`).
pub const CONTRACTS: &[(&[u8], u8)] = &[
    (
        gripforge_contracts::abi_calls_header::DECL,
        crate::contract::BOTH,
    ),
    (
        gripforge_contracts::abi_calls_reply::DECL,
        crate::contract::BOTH,
    ),
    (
        gripforge_contracts::abi_host_api::DECL,
        crate::contract::READ,
    ),
];

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn get16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn get32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The 32 bytes that open a call block.
///
/// | at | type | field |
/// |---|---|---|
/// | 0 | u8 | kind = 1 |
/// | 1 | u8 | version = 1 |
/// | 2 | u16 | flags ([`FLAG_SYNC`]) |
/// | 4 | u32 | from: the caller's module id (stamped by the host: a module cannot speak for another) |
/// | 8 | u32 | call_id: chosen by the caller, unique among its calls in flight |
/// | 12 | u16 | target_len: bytes of the target (`name` or `name@requirement`) right after the header |
/// | 14 | u16 | func_len: bytes of the function name after the target (0: by `func_id`) |
/// | 16 | u32 | func_id: the function number, or [`FUNC_BY_NAME`] |
/// | 20 | u32 | args_ptr: zone offset of the arguments (0 when `args_len` is 0) |
/// | 24 | u32 | args_len |
/// | 28 | u32 | target handle (of `gf_resolve`) when `target_len` is 0, else 0 |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CallHeader {
    pub flags: u16,
    pub from: u32,
    pub call_id: u32,
    pub target_len: u16,
    pub func_len: u16,
    pub func_id: u32,
    pub args_ptr: u32,
    pub args_len: u32,
    pub target_handle: u32,
}

impl CallHeader {
    pub fn write(&self, b: &mut [u8]) {
        b[H::KIND] = KIND_CALL;
        b[H::VERSION] = VERSION;
        put16(b, H::FLAGS, self.flags);
        put32(b, H::FROM, self.from);
        put32(b, H::CALL_ID, self.call_id);
        put16(b, H::TARGET_LEN, self.target_len);
        put16(b, H::FUNC_LEN, self.func_len);
        put32(b, H::FUNC_ID, self.func_id);
        put32(b, H::ARGS_PTR, self.args_ptr);
        put32(b, H::ARGS_LEN, self.args_len);
        put32(b, H::TARGET_HANDLE, self.target_handle);
    }
    pub fn read(b: &[u8]) -> Option<CallHeader> {
        // The version byte is no longer compared: the fields of the header are, at load (contract abi.calls.header).
        if b.len() < HEADER_LEN || b[H::KIND] != KIND_CALL {
            return None;
        }
        Some(CallHeader {
            flags: get16(b, H::FLAGS),
            from: get32(b, H::FROM),
            call_id: get32(b, H::CALL_ID),
            target_len: get16(b, H::TARGET_LEN),
            func_len: get16(b, H::FUNC_LEN),
            func_id: get32(b, H::FUNC_ID),
            args_ptr: get32(b, H::ARGS_PTR),
            args_len: get32(b, H::ARGS_LEN),
            target_handle: get32(b, H::TARGET_HANDLE),
        })
    }
    /// Bytes of the header and the names.
    pub fn names_end(&self) -> usize {
        HEADER_LEN + self.target_len as usize + self.func_len as usize
    }
}

/// The 32 bytes that open a reply block.
///
/// | at | type | field |
/// |---|---|---|
/// | 0 | u8 | kind = 2 |
/// | 1 | u8 | version = 1 |
/// | 2 | u16 | status ([`Status`]) |
/// | 4 | u32 | from: the module that answered (stamped by the host; [`HOST_ID`] for a reply the host wrote) |
/// | 8 | u32 | call_id: the id of the call it answers |
/// | 12 | u32 | to: the caller's module id (stamped by the host) |
/// | 16 | u32 | result_ptr: zone offset of the result (0 when `result_len` is 0) |
/// | 20 | u32 | result_len |
/// | 24 | u32 | reserved, 0 |
/// | 28 | u32 | reserved, 0 |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplyHeader {
    pub status: u16,
    pub from: u32,
    pub call_id: u32,
    pub to: u32,
    pub result_ptr: u32,
    pub result_len: u32,
}

impl ReplyHeader {
    pub fn write(&self, b: &mut [u8]) {
        b[R::KIND] = KIND_REPLY;
        b[R::VERSION] = VERSION;
        put16(b, R::STATUS, self.status);
        put32(b, R::FROM, self.from);
        put32(b, R::CALL_ID, self.call_id);
        put32(b, R::TO, self.to);
        put32(b, R::RESULT_PTR, self.result_ptr);
        put32(b, R::RESULT_LEN, self.result_len);
        put32(b, R::RESERVED, 0);
        put32(b, R::RESERVED + 4, 0);
    }
    pub fn read(b: &[u8]) -> Option<ReplyHeader> {
        // The version byte is no longer compared: the fields are, at load (contract abi.calls.reply).
        if b.len() < HEADER_LEN || b[R::KIND] != KIND_REPLY {
            return None;
        }
        Some(ReplyHeader {
            status: get16(b, R::STATUS),
            from: get32(b, R::FROM),
            call_id: get32(b, R::CALL_ID),
            to: get32(b, R::TO),
            result_ptr: get32(b, R::RESULT_PTR),
            result_len: get32(b, R::RESULT_LEN),
        })
    }
}

/// Byte offsets of the fields a host stamps in place.
pub mod field {
    /// `from` of a call or a reply (the same place in both contracts, checked below).
    pub const FROM: usize = super::H::FROM;
    /// `to` of a reply.
    pub const REPLY_TO: usize = super::R::TO;
    const _: () = assert!(super::H::FROM == super::R::FROM);
}

/// A whole call block: header, names, then the arguments inline (`args_ptr` = `block + names_end`).
pub fn encode_call(
    block_ptr: u32,
    call_id: u32,
    flags: u16,
    target: TargetRef<'_>,
    func: Func<'_>,
    args: &[u8],
) -> Vec<u8> {
    let (target, handle) = match target {
        TargetRef::Name(n) => (n, 0),
        TargetRef::Handle(h) => ("", h),
    };
    let (func_id, name) = match func {
        Func::Id(id) => (id, ""),
        Func::Name(n) => (FUNC_BY_NAME, n),
    };
    let head = HEADER_LEN + target.len() + name.len();
    let mut b = std::vec![0u8; head + args.len()];
    CallHeader {
        flags,
        from: 0,
        call_id,
        target_len: target.len() as u16,
        func_len: name.len() as u16,
        func_id,
        args_ptr: if args.is_empty() {
            0
        } else {
            block_ptr + head as u32
        },
        args_len: args.len() as u32,
        target_handle: handle,
    }
    .write(&mut b);
    b[HEADER_LEN..HEADER_LEN + target.len()].copy_from_slice(target.as_bytes());
    b[HEADER_LEN + target.len()..head].copy_from_slice(name.as_bytes());
    b[head..].copy_from_slice(args);
    b
}

/// A whole reply block: header then the result inline.
pub fn encode_reply(block_ptr: u32, call_id: u32, status: Status, result: &[u8]) -> Vec<u8> {
    let mut b = std::vec![0u8; HEADER_LEN + result.len()];
    ReplyHeader {
        status: status as u16,
        from: 0,
        call_id,
        to: 0,
        result_ptr: if result.is_empty() {
            0
        } else {
            block_ptr + HEADER_LEN as u32
        },
        result_len: result.len() as u32,
    }
    .write(&mut b);
    b[HEADER_LEN..].copy_from_slice(result);
    b
}

/// The module a call names: `name@requirement` (resolved by the host at each call) or a handle of
/// `gf_resolve` (resolved once: no string on the way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRef<'a> {
    Name(&'a str),
    Handle(u32),
}

impl<'a> From<&'a str> for TargetRef<'a> {
    fn from(n: &'a str) -> Self {
        TargetRef::Name(n)
    }
}

/// The registers of a thread: 64 bytes in the zone (`gf_regs()` gives their offset), u32 little-endian each —
/// the calling convention of synchronous calls (`gf_invoke`). The caller pushes its arguments on the thread's
/// stack (`gf_push`, 8-byte aligned bump), the host fills the registers and runs the target's `gf_on_invoke()`,
/// which reads them, pushes its result above and sets `RESULT`, `RESULT_LEN`, `STATUS` (`gf_ret`); back in the
/// caller, it reads them and pops to the mark it took (`gf_sp` / `gf_pop`). No allocation; A→B→C nest.
pub mod regs {
    pub const CALLER: usize = 0;
    pub const CALL_ID: usize = 4;
    pub const TARGET: usize = 8;
    pub const FUNC_ID: usize = 12;
    pub const FUNC_NAME: usize = 16;
    pub const FUNC_NAME_LEN: usize = 20;
    pub const ARGS: usize = 24;
    pub const ARGS_LEN: usize = 28;
    pub const RESULT: usize = 32;
    pub const RESULT_LEN: usize = 36;
    pub const STATUS: usize = 40;
    pub const DEPTH: usize = 44;
    pub const SP: usize = 48;
    pub const STACK_BASE: usize = 52;
    pub const STACK_LIMIT: usize = 56;
    pub const MAX_DEPTH: usize = 60;
    /// Size of the register block.
    pub const LEN: usize = 64;
    /// `STATUS` while the target has not answered.
    pub const STATUS_UNSET: u32 = 0xffff;
    /// `gf_push(RESERVE_ONLY, len)`: room on the stack, nothing copied (a module that writes the zone in place).
    pub const RESERVE_ONLY: u32 = u32::MAX;
    /// Default stack of a thread and its maximum depth.
    pub const DEFAULT_STACK: u32 = 256 << 10;
    pub const DEFAULT_MAX_DEPTH: u32 = 64;
}

/// Bytes of a zone at the start, its smallest ceiling, the most it grows at once.
pub const ZONE_INITIAL: u32 = 4 << 20;
pub const ZONE_MIN_MAX: u32 = 32 << 20;
pub const ZONE_GROW_STEP_MAX: u32 = 32 << 20;

/// The ceiling of the zone on a device (the same table as `deviceZoneMax` of the web host): memory under 2 GB →
/// 64 MiB, under 8 GB → 128 MiB, 8 GB and more (or unknown) → 256 MiB; a mobile → half.
pub fn zone_max_for_device(memory_bytes: Option<u64>, mobile: bool) -> u32 {
    const GB: u64 = 1 << 30;
    let mib: u32 = match memory_bytes {
        None => 256,
        Some(m) if m < 2 * GB => 64,
        Some(m) if m < 8 * GB => 128,
        Some(_) => 256,
    };
    (if mobile { mib / 2 } else { mib }) << 20
}

/// The size a zone grows to when `need` more bytes do not fit: doubles, +32 MiB at most at once (more only if the
/// request itself needs it), never past `max`; `None` when even that cannot hold the request.
pub fn zone_next_size(size: u32, need: u32, max: u32) -> Option<u32> {
    const PAGE: u64 = 65536;
    let step = (size.min(ZONE_GROW_STEP_MAX) as u64).max(need as u64);
    let grown = ((size as u64 + step).div_ceil(PAGE) * PAGE).min(max as u64 / PAGE * PAGE);
    (grown >= size as u64 + need as u64).then_some(grown as u32)
}

/// The function a call names: a number (fast, fixed by the module's interface) or a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func<'a> {
    Id(u32),
    Name(&'a str),
}

impl<'a> From<&'a str> for Func<'a> {
    fn from(n: &'a str) -> Self {
        Func::Name(n)
    }
}
impl From<u32> for Func<'_> {
    fn from(id: u32) -> Self {
        Func::Id(id)
    }
}

/// A semantic version `major.minor.patch` (pre-release and build tags are ignored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let core = s.split(['-', '+']).next()?;
        let mut it = core.split('.');
        let a = it.next()?.parse().ok()?;
        let b = it.next().map_or(Some(0), |x| x.parse().ok())?;
        let c = it.next().map_or(Some(0), |x| x.parse().ok())?;
        if it.next().is_some() {
            return None;
        }
        Some(Version(a, b, c))
    }
}

/// `name@requirement` split: `test.b@^1.2` → (`test.b`, `^1.2`); no `@` → any version.
pub fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('@') {
        Some((n, r)) => (n, r),
        None => (target, "*"),
    }
}

/// Whether `version` meets `req`: `*` or empty (any), `=1.2.3` (exactly), `~1.2` (same major.minor, at least),
/// `^1.2` or a bare `1.2` (same major — same minor under 1.0 — and at least), `>=1.2`.
pub fn version_matches(req: &str, version: &str) -> bool {
    let req = req.trim();
    let Some(v) = Version::parse(version) else {
        return false;
    };
    if req.is_empty() || req == "*" {
        return true;
    }
    let (op, rest) = if let Some(r) = req.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = req.strip_prefix('=') {
        ("=", r)
    } else if let Some(r) = req.strip_prefix('~') {
        ("~", r)
    } else if let Some(r) = req.strip_prefix('^') {
        ("^", r)
    } else {
        ("^", req)
    };
    let Some(want) = Version::parse(rest.trim()) else {
        return false;
    };
    match op {
        ">=" => v >= want,
        "=" => v == want,
        "~" => v.0 == want.0 && v.1 == want.1 && v >= want,
        _ => {
            if want.0 > 0 {
                v.0 == want.0 && v >= want
            } else {
                v.0 == 0 && v.1 == want.1 && v >= want
            }
        }
    }
}

/// An interface a module provides, as its manifest says: `name@version` (`physics.query@1.2.0`).
pub fn parse_provides(s: &str) -> Option<(String, Version)> {
    let (n, v) = s.split_once('@')?;
    Some((n.into(), Version::parse(v)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_round_trip() {
        let b = encode_call(
            64,
            7,
            FLAG_SYNC,
            TargetRef::Name("test.b@^1"),
            Func::Name("add"),
            &[1, 2, 3],
        );
        let h = CallHeader::read(&b).unwrap();
        assert_eq!(h.call_id, 7);
        assert_eq!(h.flags, FLAG_SYNC);
        assert_eq!(
            &b[HEADER_LEN..HEADER_LEN + h.target_len as usize],
            b"test.b@^1"
        );
        assert_eq!(h.func_id, FUNC_BY_NAME);
        assert_eq!(h.args_ptr, 64 + h.names_end() as u32);
        assert_eq!(&b[h.names_end()..], &[1, 2, 3]);
        assert!(ReplyHeader::read(&b).is_none());
        let b = encode_call(8, 1, 0, TargetRef::Handle(3), Func::Id(2), &[]);
        let h = CallHeader::read(&b).unwrap();
        assert_eq!(
            (h.target_len, h.target_handle, h.func_id, h.args_ptr),
            (0, 3, 2, 0)
        );
    }

    #[test]
    fn zone_sizes() {
        const M: u32 = 1 << 20;
        assert_eq!(zone_max_for_device(Some(1 << 30), false), 64 * M);
        assert_eq!(zone_max_for_device(Some(4 << 30), false), 128 * M);
        assert_eq!(zone_max_for_device(Some(16 << 30), true), 128 * M);
        assert_eq!(zone_max_for_device(None, false), 256 * M);
        assert_eq!(zone_next_size(4 * M, 100, 256 * M), Some(8 * M));
        assert_eq!(zone_next_size(64 * M, 100, 256 * M), Some(96 * M));
        assert_eq!(zone_next_size(64 * M, 40 * M, 256 * M), Some(104 * M));
        assert_eq!(zone_next_size(250 * M, 10 * M, 256 * M), None);
        assert_eq!(zone_next_size(250 * M, M, 256 * M), Some(256 * M));
    }

    #[test]
    fn reply_round_trip() {
        let b = encode_reply(8, 9, Status::Failed, b"no");
        let h = ReplyHeader::read(&b).unwrap();
        assert_eq!(
            (h.call_id, Status::from_u16(h.status)),
            (9, Some(Status::Failed))
        );
        assert_eq!(h.result_ptr, 8 + 32);
        let (s, p) = unpack_call_result(pack_call_result(Status::Ok, 4096));
        assert_eq!((s, p), (Status::Ok, 4096));
    }

    #[test]
    fn versions() {
        assert!(version_matches("^1.2", "1.4.0"));
        assert!(!version_matches("^1.2", "2.0.0"));
        assert!(!version_matches("^1.2", "1.1.9"));
        assert!(version_matches("1", "1.9.9"));
        assert!(version_matches("^0.2", "0.2.5"));
        assert!(!version_matches("^0.2", "0.3.0"));
        assert!(version_matches("~1.2", "1.2.7"));
        assert!(!version_matches("~1.2", "1.3.0"));
        assert!(version_matches("=1.0.0", "1.0.0"));
        assert!(version_matches(">=1.0", "3.0.0"));
        assert!(version_matches("*", "0.0.1"));
        assert_eq!(split_target("a.b@^1"), ("a.b", "^1"));
        assert_eq!(split_target("a.b"), ("a.b", "*"));
        assert_eq!(
            parse_provides("p.q@1.2.0"),
            Some(("p.q".into(), Version(1, 2, 0)))
        );
    }
}
