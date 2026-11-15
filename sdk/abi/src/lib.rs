//! gripforge-abi — the contract alone between the kit host and everything it loads, with no logic of a host or of a kit
//! (docs/architecture.md). It gathers, unchanged, what was four crates in the sources (docs/sources.md):
//!
//! - this file and its siblings: the calls between the core and the kit modules (once `gripforge-kitcall`);
//! - [`calls`]: the calls between modules, the shared zone and the call headers (once `gripforge-modcall`);
//! - [`ports`]: the pure types of the ports and the physics link between threads (once `gripforge-ports`);
//! - [`wire`]: the binary frame format shared with the player and the network (once `gripforge-wire`).
//!
//! What follows is the text of `gripforge-kitcall`: the contract between the kit host core and the kit logic modules (one wasm per kit), carried
//! by the calls between modules (`gripforge:calls@1`, docs/module-calls.md). Reference: docs/kit-modules.md.
//!
//! - The core (`kithost.core`) owns the world and provides versioned interfaces: `kithost.world@1`,
//!   `kithost.state@1`, `kithost.services@1`, `kithost.physics@1`,
//!   `kithost.input@2`, `kithost.assets@1`, `kithost.platform@1`, `kithost.scene@1` (the engine's tree).
//! - Every kit module provides `kithost.kit@1` (its lifecycle: setup, update, on_event, call…), which the core calls.
//!
//! Function numbers are stable and unique across the interfaces of one provider (the bridge tells the callee the
//! function number, not the interface): interface `n` owns the block `n << 8 ..= n << 8 | 0xff`. A minor version only
//! adds numbers; nothing is renumbered, retyped or removed within a major. The `.gfi` files of `interfaces/` are the
//! schema (the tracer decodes with them); `tests/schema.rs` checks they match the constants here.
//!
//! Encoding (the `.gfi` rules): parameters one after the other, scalars little-endian at their size, `bool` one byte,
//! `string` a u32 byte count then UTF-8, `value` a tagged value (gripforge-wire v1 tags, below). No JSON text.

use serde_json::{Map, Number, Value as Json};
/// The contract version of this crate (`major << 16 | minor`).
pub const ABI: u32 = 1 << 16;

/// The name the core module is declared under.
pub const CORE_MODULE: &str = "kithost.core";

/// One interface: name, version, the block of its function numbers.
#[derive(Clone, Copy, Debug)]
pub struct Iface {
    pub name: &'static str,
    pub version: &'static str,
    pub base: u32,
}

macro_rules! ifaces {
    ($($id:ident = $name:literal @ $ver:literal, $n:literal;)*) => {
        $(pub const $id: Iface = Iface { name: $name, version: $ver, base: $n << 8 };)*
        /// Every interface of the core, in number order.
        pub const CORE_IFACES: &[Iface] = &[$($id),*];
    };
}

/// Provided by every kit module (the core calls it).
pub const KIT: Iface = Iface {
    name: "kithost.kit",
    version: "1.3.0",
    base: 0,
};

ifaces! {
    WORLD = "kithost.world" @ "1.8.0", 1;
    STATE = "kithost.state" @ "1.1.0", 2;
    // Retired, never reused: 3 was kithost.zones@1 (the kit world.zones, issue #1206), 4 was kithost.objectives@1
    // (the missions: the story.quest kit's).
    PHYSICS = "kithost.physics" @ "1.10.0", 5;
    SERVICES = "kithost.services" @ "1.7.0", 6;
    INPUT = "kithost.input" @ "2.0.0", 7;
    ASSETS = "kithost.assets" @ "1.0.0", 8;
    PLATFORM = "kithost.platform" @ "1.2.0", 9;
    ZONE = "kithost.zone" @ "1.0.0", 10;
    MEM = "kithost.mem" @ "1.3.0", 11;
    ISO = "kithost.iso" @ "1.0.0", 12;
    SCENE = "kithost.scene" @ "1.4.0", 13;
}

pub mod alloc;
/// The calls between modules (`gripforge:calls@1`): zone, headers, statuses, the module side (feature `guest`).
pub mod calls;
/// The pure types of the kit ports (physics, the shape table, assets) and the physics link between threads.
pub mod ports;
/// The binary format of the frames (values and delta frames), shared by the host, the player and the network.
pub mod wire;
/// The structures of the contract and of the engine table as `repr(C)` types, generated (tools/kit-modules/structs.mjs).
// Written by tools/kit-modules/structs.mjs and compared byte for byte (`--check`): kept as generated.
#[rustfmt::skip]
pub mod engine_structs;
pub mod events;
pub mod fast;
pub mod fixed;
mod fns_world;
#[path = "fns_hot.rs"]
pub mod hot;
pub mod layout;
pub mod mem;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod near;
pub mod rows;
pub mod zone;
pub use fns_world::{services, state, world};

/// The interface a function number belongs to.
pub fn iface_of(func: u32) -> Option<Iface> {
    if func >> 8 == 0 {
        return Some(KIT);
    }
    CORE_IFACES.iter().copied().find(|i| i.base == func & !0xff)
}

/// `kithost.kit@1`: what the core calls on a kit module. Every call starts with a [`Ctx`] unless its line says not.
pub mod kit {
    /// () → value `{ id, version, services: [..], kitcall }` — what the module is (read once at load).
    pub const INFO: u32 = 1;
    /// (inst u32, config value) → value `[cap, …]` (`services_for`).
    pub const SERVICES_FOR: u32 = 2;
    /// (project value, config value) → value `[{path, message}, …]` (`validate`).
    pub const VALIDATE: u32 = 3;
    /// (ctx, project value, config value, rng seed u32, rng a u32): a new instance (before `setup`).
    pub const MOUNT: u32 = 4;
    /// (inst u32, kit u32): the instance is gone (after `teardown`, or when the core drops its host).
    pub const DROP: u32 = 5;
    /// (ctx) — the `KitModule` hooks without arguments.
    pub const SETUP: u32 = 10;
    pub const READY: u32 = 11;
    pub const TEARDOWN: u32 = 12;
    /// (ctx, dt f64)
    pub const UPDATE: u32 = 13;
    pub const LATE_UPDATE: u32 = 14;
    pub const UPDATE_IDLE: u32 = 15;
    /// (ctx, tag string, event value)
    pub const ON_EVENT: u32 = 16;
    /// (ctx, tag string, data value)
    pub const ON_TIMER: u32 = 17;
    /// (ctx, verb string, args value, event value|null)
    pub const ACTION: u32 = 18;
    /// (ctx, verb string, args value, event value|null) → bool
    pub const CONDITION: u32 = 19;
    /// (ctx, spec value) → value (string id | null)
    pub const SPAWN: u32 = 20;
    /// (ctx, capability string, method string, args value[]) → value; failed + message on Err
    pub const CALL: u32 = 21;
    /// (ctx) → value | absent (status ok, empty result = None)
    pub const SAVE: u32 = 22;
    /// (ctx, data value)
    pub const LOAD: u32 = 23;
    /// (ctx, scene value)
    pub const SCENE_ENTER: u32 = 24;
    pub const SCENE_EXIT: u32 = 25;
    /// (ctx, name string, entity string, value value, instance value)
    pub const COMPONENT: u32 = 26;
    /// (ctx, tag string, path string, value value|absent, prev value|absent) — absent = tag 0xFF
    pub const ON_VAR: u32 = 27;
    /// (ctx, name string, args value[]) → value; failed + message on Err
    pub const CALLBACK: u32 = 28;
    // 29, 30, 31 were the mission steps of the core (step_start, step_update, step_event), retired with
    // kithost.objectives@1: the core no longer calls them (never reused).
    /// (ctx) → (seed u32, a u32): the kit's rng now (the core keeps its slot in step for `save`).
    pub const RNG: u32 = 32;
    include!("engine_contract.rs");
}

/// What the core tells a kit on every entry: which host instance and kit slot, the clock, and the generations of
/// what the kit may cache (the served capabilities, its config).
///
/// In a message it is 48 bytes, field after field — the same bytes as this `repr(C)` structure (`Ctx` of the
/// structures table, [`layout`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ctx {
    pub inst: u32,
    pub kit: u32,
    pub tick: u64,
    pub frame: u64,
    pub elapsed: f64,
    pub fixed_dt: f64,
    /// Bumped whenever the table of served capabilities changes (mount, replace, unmount).
    pub services_gen: u32,
    /// Bumped whenever the core changes a kit's config after mount (game.engine reactions).
    pub config_gen: u32,
}

impl Ctx {
    pub const LEN: usize = 48;
    pub fn write(&self, w: &mut Writer) {
        w.u32(self.inst)
            .u32(self.kit)
            .u64(self.tick)
            .u64(self.frame)
            .f64(self.elapsed)
            .f64(self.fixed_dt);
        w.u32(self.services_gen).u32(self.config_gen);
    }
    pub fn read(r: &mut Reader) -> Result<Ctx, String> {
        Ok(Ctx {
            inst: r.u32()?,
            kit: r.u32()?,
            tick: r.u64()?,
            frame: r.u64()?,
            elapsed: r.f64()?,
            fixed_dt: r.f64()?,
            services_gen: r.u32()?,
            config_gen: r.u32()?,
        })
    }
}

// ---- values ------------------------------------------------------------------------------------------------------

pub const T_NULL: u8 = 0;
pub const T_FALSE: u8 = 1;
pub const T_TRUE: u8 = 2;
pub const T_INT: u8 = 3;
pub const T_FLOAT: u8 = 4;
pub const T_STRING: u8 = 5;
pub const T_ARRAY: u8 = 6;
pub const T_OBJECT: u8 = 7;
/// Not a value: "absent" (`None` of an `Option<&Json>`, `undefined`). Only where a signature says `value?`.
pub const T_ABSENT: u8 = 0xff;
/// A list or a dictionary passed as a **fixed-layout block** (docs/kit-modules.md §9.7): the address (u64) of a
/// [`fixed::FixedBlock`] — no byte of the value is written in the message. The reader takes the block, reads it in
/// place, copies what it keeps and frees it with the one allocator. Only between modules of the common memory: a
/// reader that does not know its writer is in its memory refuses it.
pub const T_FIXED: u8 = 0xfd;
/// A list or a dictionary that **crosses to another memory** (an isolated kit module, docs/kit-third-party.md): one
/// byte of padding count, that many zero bytes (the block then starts on a multiple of 8 of the message), then a
/// [`fixed::FixedBlock`] whose addresses are offsets from its start ([`fixed::CROSSING`]). The same structures as
/// [`T_FIXED`], in the message instead of beside it: whoever sees both memories copies the message once, the reader
/// reads the block where it landed. Any reader accepts it — every offset is checked against the block.
pub const T_CROSS: u8 = 0xfc;

/// Blocks this module passed so far (the QA reads it: how many values went without being written out).
pub fn passed() -> u64 {
    PASSED.load(std::sync::atomic::Ordering::Relaxed)
}
static PASSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn put_uvar(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends a tagged value. Unlike the frame codec, a number keeps its kind: an integer of serde_json is tag 3, an
/// f64 is always tag 4 (a kit may tell `as_u64()` of `3` from `3.0`: both sides must see the same value).
pub fn put_value(out: &mut Vec<u8>, v: &Json) {
    match v {
        Json::Null => out.push(T_NULL),
        Json::Bool(false) => out.push(T_FALSE),
        Json::Bool(true) => out.push(T_TRUE),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push(T_INT);
                put_uvar(out, ((i << 1) ^ (i >> 63)) as u64);
            } else if let (Some(u), true) = (n.as_u64(), n.is_u64()) {
                // Past i64::MAX: tag 4 would lose bits; a u64 as its 8 bytes after a float tag would not be a
                // float. Rare enough: as f64 (what JavaScript would hold anyway).
                out.push(T_FLOAT);
                out.extend_from_slice(&(u as f64).to_le_bytes());
            } else {
                out.push(T_FLOAT);
                out.extend_from_slice(&n.as_f64().unwrap_or(0.0).to_le_bytes());
            }
        }
        Json::String(s) => {
            out.push(T_STRING);
            put_uvar(out, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
        Json::Array(a) => {
            out.push(T_ARRAY);
            put_uvar(out, a.len() as u64);
            for x in a {
                put_value(out, x);
            }
        }
        Json::Object(o) => {
            out.push(T_OBJECT);
            put_uvar(out, o.len() as u64);
            for (k, x) in o {
                put_uvar(out, k.len() as u64);
                out.extend_from_slice(k.as_bytes());
                put_value(out, x);
            }
        }
    }
}

/// Writes the arguments (or the result) of one call.
#[derive(Default, Debug)]
pub struct Writer {
    pub buf: Vec<u8>,
    /// The reader is a module of the same memory: non-empty lists and dictionaries are passed as fixed-layout
    /// blocks ([`T_FIXED`]) instead of being written out. The message must then be read exactly once (each block
    /// belongs to whoever reads it); one that is never read leaks them, nothing worse.
    pub direct: bool,
    /// The reader is a module of **another memory** that speaks the structures of the contract (an isolated kit
    /// module, or the core answering one): non-empty lists and dictionaries are written as fixed-layout blocks
    /// inside the message ([`T_CROSS`]) — nothing of the old written-out encoding of containers.
    pub cross: bool,
}

impl Writer {
    pub fn new() -> Writer {
        Writer {
            buf: Vec::with_capacity(64),
            direct: false,
            cross: false,
        }
    }
    /// A writer for a reader of another memory that reads the structures of the contract (see [`Writer::cross`]).
    pub fn crossing() -> Writer {
        Writer {
            buf: Vec::with_capacity(64),
            direct: false,
            cross: true,
        }
    }
    /// A writer for a peer that is known: of the same memory (`direct`), of another one on the structures of the
    /// contract (`cross`), or neither (everything written out).
    pub fn to_peer(direct: bool, cross: bool) -> Writer {
        Writer {
            buf: Vec::with_capacity(64),
            direct,
            cross: cross && !direct,
        }
    }
    /// A writer for a reader of the common memory (see [`Writer::direct`]); the plain encoding when `direct` is
    /// false.
    pub fn passing(direct: bool) -> Writer {
        Writer {
            buf: Vec::with_capacity(64),
            direct,
            cross: false,
        }
    }
    pub fn with_ctx(ctx: &Ctx) -> Writer {
        let mut w = Writer {
            buf: Vec::with_capacity(128),
            direct: false,
            cross: false,
        };
        ctx.write(&mut w);
        w
    }
    pub fn clear(&mut self) {
        self.buf.clear();
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.buf.push(v as u8);
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn str(&mut self, v: &str) -> &mut Self {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v.as_bytes());
        self
    }
    /// An optional string: a bool then the string when present.
    pub fn opt_str(&mut self, v: Option<&str>) -> &mut Self {
        match v {
            Some(s) => self.bool(true).str(s),
            None => self.bool(false),
        }
    }
    /// A value: written out, or — to a reader of the common memory, for a non-empty list or dictionary — put in a
    /// fixed-layout block whose address is written (the reader reads it in place and frees it).
    pub fn value(&mut self, v: &Json) -> &mut Self {
        if self.direct
            && (matches!(v, Json::Array(a) if !a.is_empty())
                | matches!(v, Json::Object(o) if !o.is_empty()))
        {
            if let Some(block) = fixed::Owned::from_json(v) {
                PASSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.buf.push(T_FIXED);
                return self.u64(block.into_address());
            }
        }
        if self.cross
            && (matches!(v, Json::Array(a) if !a.is_empty())
                | matches!(v, Json::Object(o) if !o.is_empty()))
        {
            let start = self.buf.len();
            // The tag, how many zero bytes follow, then the block on a multiple of 8 of the message.
            let pad = (8 - (start + 2) % 8) % 8;
            self.buf.push(T_CROSS);
            self.buf.push(pad as u8);
            self.buf.resize(start + 2 + pad, 0);
            if fixed::write_crossing(v, &mut self.buf) {
                return self;
            }
            self.buf.truncate(start);
        }
        put_value(&mut self.buf, v);
        self
    }
    /// A block kept by this module ([`fixed::Kept`]), as a block that crosses ([`T_CROSS`]): its bytes copied as
    /// they are — no value walked, whatever memory reads it.
    pub fn kept(&mut self, k: &fixed::Kept) -> &mut Self {
        let start = self.buf.len();
        let pad = (8 - (start + 2) % 8) % 8;
        self.buf.push(T_CROSS);
        self.buf.push(pad as u8);
        self.buf.resize(start + 2 + pad, 0);
        self.buf.extend_from_slice(k.bytes());
        self
    }
    /// A value as a block that crosses ([`T_CROSS`]) when it is a non-empty list or dictionary (a reader that keeps
    /// it typed takes the bytes: [`Reader::kept`]), else written out.
    pub fn value_crossing(&mut self, v: &Json) -> &mut Self {
        if matches!(v, Json::Array(a) if !a.is_empty())
            | matches!(v, Json::Object(o) if !o.is_empty())
        {
            let start = self.buf.len();
            let pad = (8 - (start + 2) % 8) % 8;
            self.buf.push(T_CROSS);
            self.buf.push(pad as u8);
            self.buf.resize(start + 2 + pad, 0);
            if fixed::write_crossing(v, &mut self.buf) {
                return self;
            }
            self.buf.truncate(start);
        }
        put_value(&mut self.buf, v);
        self
    }
    /// [`Writer::value_crossing`] of a typed value (`fixed::Source`, docs/kit-modules.md §7.13): a list or a
    /// dictionary as the same block, no JSON value made; anything else as `fallback()`'s value.
    pub fn value_crossing_src<T: fixed::Source>(
        &mut self,
        v: &T,
        fallback: impl FnOnce() -> Json,
    ) -> &mut Self {
        if matches!(v.node(), fixed::Node::List(a) if !a.is_empty())
            | matches!(v.node(), fixed::Node::Dict(d) if !d.is_empty())
            | matches!(v.node(), fixed::Node::Vec3(_))
        {
            let start = self.buf.len();
            let pad = (8 - (start + 2) % 8) % 8;
            self.buf.push(T_CROSS);
            self.buf.push(pad as u8);
            self.buf.resize(start + 2 + pad, 0);
            if fixed::write_crossing_src(v, &mut self.buf) {
                return self;
            }
            self.buf.truncate(start);
        }
        put_value(&mut self.buf, &fallback());
        self
    }
    /// `value?`: a value or "absent".
    pub fn opt_value(&mut self, v: Option<&Json>) -> &mut Self {
        match v {
            Some(v) => self.value(v),
            None => self.u8(T_ABSENT),
        }
    }
    /// A list of values: u32 count then each value.
    pub fn values(&mut self, v: &[Json]) -> &mut Self {
        self.u32(v.len() as u32);
        for x in v {
            self.value(x);
        }
        self
    }
    /// A list of strings: u32 count then each string.
    pub fn strs<S: AsRef<str>>(&mut self, v: &[S]) -> &mut Self {
        self.u32(v.len() as u32);
        for s in v {
            self.str(s.as_ref());
        }
        self
    }
    /// A list of f64: u32 count then each.
    pub fn f64s(&mut self, v: &[f64]) -> &mut Self {
        self.u32(v.len() as u32);
        for x in v {
            self.f64(*x);
        }
        self
    }
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }
}

/// Reads the arguments (or the result) of one call; every read fails with a message past the end.
pub struct Reader<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
    /// Whether the writer is a module of the same memory: it may have passed fixed-layout blocks ([`T_FIXED`]).
    pub trusted: bool,
}

fn short(what: &str) -> String {
    format!("kitcall: message too short ({what})")
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader {
            bytes,
            at: 0,
            trusted: cfg!(feature = "trusted-peer"),
        }
    }
    /// A reader of bytes whose writer is known: `trusted` when it is a module of the same memory.
    pub fn from(bytes: &'a [u8], trusted: bool) -> Reader<'a> {
        Reader {
            bytes,
            at: 0,
            trusted,
        }
    }
    pub fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        let s = self
            .bytes
            .get(self.at..self.at + n)
            .ok_or_else(|| short(what))?;
        self.at += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1, "u8")?[0])
    }
    pub fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    pub fn bool(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4, "u32")?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4, "i32")?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8, "u64")?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4, "f32")?.try_into().unwrap()))
    }
    pub fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8, "f64")?.try_into().unwrap()))
    }
    pub fn str(&mut self) -> Result<&'a str, String> {
        let n = self.u32()? as usize;
        std::str::from_utf8(self.take(n, "string")?)
            .map_err(|_| "kitcall: string is not UTF-8".to_string())
    }
    pub fn string(&mut self) -> Result<String, String> {
        Ok(self.str()?.to_string())
    }
    pub fn opt_str(&mut self) -> Result<Option<&'a str>, String> {
        Ok(if self.bool()? {
            Some(self.str()?)
        } else {
            None
        })
    }
    fn uvar(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
            if shift > 63 {
                return Err("kitcall: varint too long".into());
            }
        }
    }
    fn key(&mut self) -> Result<String, String> {
        let n = self.uvar()? as usize;
        std::str::from_utf8(self.take(n, "string")?)
            .map(str::to_string)
            .map_err(|_| "kitcall: string is not UTF-8".to_string())
    }
    pub fn value(&mut self) -> Result<Json, String> {
        Ok(match self.u8()? {
            T_NULL => Json::Null,
            T_FALSE => Json::Bool(false),
            T_TRUE => Json::Bool(true),
            T_INT => {
                let z = self.uvar()?;
                Json::Number(Number::from(((z >> 1) as i64) ^ -((z & 1) as i64)))
            }
            T_FLOAT => Number::from_f64(self.f64()?)
                .map(Json::Number)
                .unwrap_or(Json::Null),
            T_STRING => Json::String(self.key()?),
            T_ARRAY => {
                let n = self.uvar()? as usize;
                let mut a = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    a.push(self.value()?);
                }
                Json::Array(a)
            }
            T_OBJECT => {
                let n = self.uvar()? as usize;
                let mut o = Map::new();
                for _ in 0..n {
                    let k = self.key()?;
                    o.insert(k, self.value()?);
                }
                Json::Object(o)
            }
            T_FIXED => {
                fixed::stats::copied();
                self.block()?.to_json()?
            }
            T_CROSS => {
                fixed::stats::copied();
                fixed::crossed(self.crossed()?, |view| view.to_json())?
            }
            t => return Err(format!("kitcall: unknown value tag {t}")),
        })
    }
    /// The next value **kept typed** when it is a block that crossed ([`T_CROSS`]): its bytes copied once onto 8
    /// ([`fixed::Kept`]), nothing turned into JSON. `None`: another kind of value, nothing read.
    pub fn kept(&mut self) -> Result<Option<fixed::Kept>, String> {
        if self.peek() != Some(T_CROSS) {
            return Ok(None);
        }
        self.at += 1;
        fixed::Kept::from_bytes(self.crossed()?).map(Some)
    }
    /// The block of a [`T_FIXED`] (the tag is next): taken from the message, freed when the caller drops it.
    fn block(&mut self) -> Result<fixed::Owned, String> {
        let at = self.u64()?;
        if !self.trusted {
            return Err("kitcall: a fixed-layout value in a message that may not carry one (the writer is not a module of this memory)".into());
        }
        // SAFETY: written by `Writer::value` (or by another writer of the contract) in a module of the same memory
        // and allocator — the reader was told so (`trusted`); a message is read once, so the block is taken once.
        unsafe { fixed::Owned::take(at) }
    }
    /// The bytes of a block that crossed from another memory ([`T_CROSS`], the tag is read): where they landed in
    /// the message.
    fn crossed(&mut self) -> Result<&'a [u8], String> {
        let pad = self.u8()? as usize;
        self.take(pad, "padding")?;
        let size = fixed::crossed_size(self.bytes.get(self.at..).unwrap_or(&[]))?;
        self.take(size, "block")
    }
    /// A value **read in place**: when the next value is a fixed-layout block, the block itself — the caller reads
    /// what it needs through [`fixed::Owned::view`] and frees it by dropping it, nothing is copied. `None`: the next
    /// value is written out in the message (a scalar, a text, an empty container, or a writer of another memory);
    /// nothing was read, [`Reader::value`] reads it.
    pub fn fixed(&mut self) -> Result<Option<fixed::Owned>, String> {
        if self.peek() != Some(T_FIXED) {
            return Ok(None);
        }
        self.at += 1;
        fixed::stats::viewed();
        self.block().map(Some)
    }
    /// Reads a value into `slot`, which becomes exactly what [`Reader::value`] would give — reusing what `slot`
    /// already holds when it has the same shape (an object with the same keys in the same order, an array of the
    /// same length, a string): no map, key or buffer is allocated again. For a copy brought up to date.
    pub fn value_into(&mut self, slot: &mut Json) -> Result<(), String> {
        let start = self.at;
        match (self.peek(), &mut *slot) {
            (Some(T_OBJECT), Json::Object(o)) => {
                self.at += 1;
                if self.uvar()? as usize == o.len() {
                    let mut same = true;
                    for (k, v) in o.iter_mut() {
                        let n = self.uvar()? as usize;
                        if self.take(n, "string")? != k.as_bytes() {
                            same = false;
                            break;
                        }
                        self.value_into(v)?;
                    }
                    if same {
                        return Ok(());
                    }
                }
            }
            (Some(T_ARRAY), Json::Array(a)) => {
                self.at += 1;
                if self.uvar()? as usize == a.len() {
                    for v in a.iter_mut() {
                        self.value_into(v)?;
                    }
                    return Ok(());
                }
            }
            (Some(T_STRING), Json::String(text)) => {
                self.at += 1;
                let n = self.uvar()? as usize;
                let read = std::str::from_utf8(self.take(n, "string")?)
                    .map_err(|_| "kitcall: string is not UTF-8".to_string())?;
                text.clear();
                text.push_str(read);
                return Ok(());
            }
            (Some(T_FIXED), _) => {
                self.at += 1;
                fixed::stats::updated();
                return self.block()?.view().write_into(slot);
            }
            (Some(T_CROSS), _) => {
                self.at += 1;
                fixed::stats::updated();
                return fixed::crossed(self.crossed()?, |view| view.write_into(slot));
            }
            _ => {}
        }
        // Another shape: read it whole (what was reused above is replaced).
        self.at = start;
        *slot = self.value()?;
        Ok(())
    }
    /// `value?`: `None` for "absent".
    pub fn opt_value(&mut self) -> Result<Option<Json>, String> {
        if self.peek() == Some(T_ABSENT) {
            self.at += 1;
            return Ok(None);
        }
        self.value().map(Some)
    }
    pub fn values(&mut self) -> Result<Vec<Json>, String> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            out.push(self.value()?);
        }
        Ok(out)
    }
    pub fn strs(&mut self) -> Result<Vec<String>, String> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            out.push(self.string()?);
        }
        Ok(out)
    }
    pub fn f64s(&mut self) -> Result<Vec<f64>, String> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            out.push(self.f64()?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lists_and_dictionaries_passed_as_fixed_blocks() {
        use serde_json::json;
        // An object and an array are passed; scalars, strings and empty containers are written out as always.
        let big =
            json!({ "name": "torch", "tags": ["a", "b", "c"], "n": 3, "f": 3.0, "u": u64::MAX });
        let list = json!([1, 2, { "deep": [true, null] }]);
        let before = passed();
        let mut w = Writer::passing(true);
        w.value(&big)
            .value(&list)
            .value(&json!("text"))
            .value(&json!(7))
            .value(&json!([]))
            .opt_value(None);
        assert!(
            passed() >= before + 2,
            "only the object and the non-empty array were passed"
        );
        assert_eq!(
            w.bytes().len(),
            9 + 9 + 6 + 2 + 2 + 1,
            "no byte of the two big values was written"
        );
        let bytes = w.buf; // the writer is gone: the blocks live on, owned by the message
                           // A reader that does not know its writer is in its memory refuses the message.
        assert!(Reader::from(&bytes, false)
            .value()
            .unwrap_err()
            .contains("fixed-layout value in a message that may not carry one"));
        let mut r = Reader::from(&bytes, true);
        // Read in place: nothing copied, the text borrowed from the block; the block is freed at the end of the scope.
        {
            let block = r.fixed().unwrap().expect("a block");
            let v = block.view();
            assert_eq!(v.get("name").unwrap().as_str(), Some("torch"));
            assert_eq!(v.get("tags").unwrap().index(2).unwrap().as_str(), Some("c"));
            assert_eq!(
                (
                    v.get("n").unwrap().as_u64(),
                    v.get("f").unwrap().as_u64(),
                    v.get("u").unwrap().as_u64()
                ),
                (Some(3), None, Some(u64::MAX))
            );
            assert_eq!(
                block.to_json().unwrap(),
                big,
                "the copy a reader keeps is the writer's value"
            );
        }
        let kept = r.value().unwrap();
        assert_eq!(kept, list);
        assert!(
            r.fixed().unwrap().is_none(),
            "a text is in the message, not in a block"
        );
        assert_eq!(r.value().unwrap(), json!("text"));
        assert_eq!(r.value().unwrap(), json!(7));
        assert_eq!(r.value().unwrap(), json!([]));
        assert_eq!(r.opt_value().unwrap(), None);
        assert!(r.done());
        // The reader owns its copy after the message and its blocks are gone.
        drop(bytes);
        assert_eq!(kept[2]["deep"][0], json!(true));
        // The plain encoding is unchanged, and read by both kinds of reader.
        let mut plain = Writer::new();
        plain.value(&big);
        assert!(plain.bytes().len() > 30);
        assert_eq!(
            Reader::from(plain.bytes(), true).value().unwrap()["tags"],
            big["tags"]
        );
        assert_eq!(
            Reader::from(plain.bytes(), false).value().unwrap()["tags"],
            big["tags"]
        );
        // A copy brought up to date from a block; lists of values; an optional value.
        let mut into = json!({ "name": "old", "tags": [], "n": 0, "f": 0, "u": 0 });
        let mut w = Writer::passing(true);
        w.value(&big)
            .values(&[big.clone(), json!(1)])
            .opt_value(Some(&big));
        let mut r = Reader::from(w.bytes(), true);
        r.value_into(&mut into).unwrap();
        assert_eq!(
            serde_json::to_string(&into).unwrap(),
            serde_json::to_string(&big).unwrap()
        );
        assert_eq!(r.values().unwrap(), vec![big.clone(), json!(1)]);
        assert_eq!(r.opt_value().unwrap(), Some(big));
        assert!(r.done());
    }
    #[test]
    fn lists_and_dictionaries_cross_to_another_memory_as_blocks_in_the_message() {
        use serde_json::json;
        let big =
            json!({ "name": "torch", "tags": ["a", "b", "c"], "n": 3, "f": 3.0, "u": u64::MAX });
        let list = json!([1, 2, { "deep": [true, null] }]);
        let mut w = Writer::crossing();
        w.u8(9)
            .value(&big)
            .str("between")
            .value(&list)
            .value(&json!("text"))
            .value(&json!(7))
            .value(&json!({}))
            .opt_value(None)
            .opt_value(Some(&list));
        // Nothing of the written-out encoding of a container: each one is a block, on a multiple of 8.
        let bytes = w.buf.clone();
        assert_eq!(bytes[1], T_CROSS);
        assert_eq!(
            (2 + bytes[2] as usize + 1) % 8,
            0,
            "the block starts on a multiple of 8 of the message"
        );
        assert_eq!(&bytes[3 + bytes[2] as usize..][..4], b"GFV1");
        // "The other memory": the same bytes at other addresses; any reader reads them (no trust needed).
        for trusted in [false, true] {
            let landed = bytes.clone();
            let mut r = Reader::from(&landed, trusted);
            assert_eq!(r.u8().unwrap(), 9);
            assert!(r.fixed().unwrap().is_none(), "not a block of this memory");
            assert_eq!(
                serde_json::to_string(&r.value().unwrap()).unwrap(),
                serde_json::to_string(&big).unwrap()
            );
            assert_eq!(r.str().unwrap(), "between");
            let mut slot = json!([0, 0, { "deep": [] }]);
            r.value_into(&mut slot).unwrap();
            assert_eq!(slot, list);
            assert_eq!(r.value().unwrap(), json!("text"));
            assert_eq!(r.value().unwrap(), json!(7));
            assert_eq!(r.value().unwrap(), json!({}));
            assert_eq!(r.opt_value().unwrap(), None);
            assert_eq!(r.opt_value().unwrap(), Some(list.clone()));
            assert!(r.done());
        }
        // A block cut short is refused with the reason, not read.
        assert!(Reader::from(&bytes[..40], false).values().is_err());
        let mut r = Reader::from(&bytes[..40], false);
        r.u8().unwrap();
        assert!(r.value().unwrap_err().contains("bytes of message"));
        // `to_peer`: the same memory wins over the crossing (a block by address), neither writes everything out.
        assert!(
            !Writer::to_peer(true, true).cross
                && Writer::to_peer(false, true).cross
                && !Writer::to_peer(false, false).cross
        );
    }
    use super::*;
    use serde_json::json;

    #[test]
    fn values_keep_their_number_kind_and_order() {
        let v = json!({"b": 3, "a": 3.0, "c": [-1, 1.5e300, null, true, "é"], "d": {"z": {}, "y": []}, "u": u64::MAX});
        let mut w = Writer::new();
        w.value(&v).opt_value(None).str("x").f64(2.5);
        let mut r = Reader::new(w.bytes());
        let back = r.value().unwrap();
        assert_eq!(back["b"].as_u64(), Some(3));
        assert_eq!(back["a"].as_u64(), None);
        assert_eq!(back["c"], v["c"]);
        assert_eq!(
            back.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["b", "a", "c", "d", "u"]
        );
        assert_eq!(r.opt_value().unwrap(), None);
        assert_eq!(r.str().unwrap(), "x");
        assert_eq!(r.f64().unwrap(), 2.5);
        assert!(r.done());
    }

    #[test]
    fn a_value_read_into_a_copy_is_the_value_read() {
        let cases = [
            (
                json!({"x": 1, "y": 2.5, "tags": ["a", "b"], "o": {"k": "v"}}),
                json!({"x": 1.0, "y": -0.0, "tags": ["c", "dd"], "o": {"k": "w"}}),
            ),
            (json!({"x": 1, "y": 2}), json!({"y": 2, "x": 1})),
            (json!({"x": 1, "y": 2}), json!({"x": 1})),
            (json!({"x": 1}), json!({"x": 1, "y": {"deep": [1, 2, 3]}})),
            (json!([1, 2, 3]), json!([1, 2])),
            (
                json!([{"a": 1}, "s"]),
                json!([{"a": null}, "longer string"]),
            ),
            (json!("text"), json!(3)),
            (json!(null), json!({"a": [true, false]})),
        ];
        for (was, now) in cases {
            let mut w = Writer::new();
            w.value(&now).u32(7);
            let mut slot = was.clone();
            let mut r = Reader::new(w.bytes());
            r.value_into(&mut slot).unwrap();
            assert_eq!(
                r.u32().unwrap(),
                7,
                "{was} -> {now}: the reader is after the value"
            );
            // Strictly the value: the text keeps the order of the keys and the kind of the numbers.
            assert_eq!(
                serde_json::to_string(&slot).unwrap(),
                serde_json::to_string(&Reader::new(w.bytes()).value().unwrap()).unwrap()
            );
        }
    }

    #[test]
    fn ctx_is_48_bytes_and_numbers_are_unique() {
        let mut w = Writer::new();
        Ctx::default().write(&mut w);
        assert_eq!(w.buf.len(), Ctx::LEN);
        let mut seen = std::collections::HashSet::new();
        for i in CORE_IFACES {
            assert!(seen.insert(i.base));
            assert_eq!(iface_of(i.base | 7).unwrap().name, i.name);
        }
    }
}
