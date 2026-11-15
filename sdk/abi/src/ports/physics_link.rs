//! The physics over a link (link.rs, docs/kit-modules.md §7.17): what the user (the core and the kits, on their
//! thread) and the physics (on its own loop — a Worker, a thread — or taken at once on the user's thread) share.
//!
//! - **Orders** ([`order`]): every change (a body added or removed, a force, an impulse, a teleport, a kinematic
//!   target, the gravity…) is a record of the ring, fire and forget; every question (a character's move, a cast, a
//!   raycast, an overlap) is a record with a ticket, answered in the slab of answers.
//! - **The published state** ([`PubSet`]): for every moving body and character, its pose, velocities and flags,
//!   written by the physics after each step and after the orders it took, in a triple buffer the user reads in place
//!   at any time; once per body ([`SlotInfo`]), what it is (kind, mass, first pose).
//! - **Contact events**: a second ring, from the physics to the user.
//!
//! Bodies are named by **slot** (a number the user gives when it adds the body); the names stay with the user,
//! except that the physics keeps them to speak to its port (`PhysicsPort` is by name).
//!
//! [`serve`] is the physics' side, generic over any `PhysicsPort`: the core runs it on its own thread (the orders
//! taken as they come, the state written in place — [`LIVE`]), the physics module runs it on its Worker in a loop
//! (`gfp_serve`), natively a thread can run it too. One code, the same records.

use crate::ports::link::{AnswerRec, Answers, Head, Ring, Triple, PAUSE, QUIT, RUN};

/// The questions (a ticket each): what the physics answers, and how the user reads the hits.
mod questions;
use crate::ports::{PhysicsPort, V3};
pub use questions::{hits_of, pack_hits, HIT_WORDS};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};

pub const MAGIC: u32 = 0x4846_5047; // "GPFH"

/// No slot (a ray that hit no body of a name, no exclusion).
pub const NO_SLOT: u32 = u32::MAX;

/// The kinds of the orders (the ring's record kind).
pub mod order {
    pub const ADD_BODY: u32 = 1;
    pub const ADD_CHARACTER: u32 = 2;
    pub const ADD_STATIC_BOX: u32 = 3;
    pub const ADD_DYNAMIC_BOX: u32 = 4;
    pub const REMOVE: u32 = 5;
    pub const SET_BODY: u32 = 6;
    pub const SET_POSITION: u32 = 7;
    pub const IMPULSE: u32 = 8;
    pub const FORCE: u32 = 9;
    pub const TORQUE_IMPULSE: u32 = 10;
    pub const MOVE_KINEMATIC: u32 = 11;
    pub const SET_CHARACTER_POSITION: u32 = 12;
    pub const SET_GRAVITY: u32 = 13;
    pub const SET_MANUAL: u32 = 14;
    /// A step asked by the user (manual stepping).
    pub const STEP: u32 = 15;
    /// Publish whether this body is grounded (a probe the user reads).
    pub const WATCH_GROUNDED: u32 = 16;
    /// The published state moves to a bigger set (its address).
    pub const GROW: u32 = 17;
    pub const SET_COLLISION_SPACE: u32 = 18;
    /// Field-level writes: one field of a body, applied in order on the physics' freshest state.
    pub const SET_LINVEL: u32 = 19;
    pub const SET_ANGVEL: u32 = 20;
    pub const SET_ROTATION: u32 = 21;
    /// Set the simulation-owned fixed step, seconds (no cross-thread field mutation).
    pub const SET_FIXED_DT: u32 = 22;
    /// A joint (1.9): the first body's slot, the joint's name, then the second body's slot and the joint's words.
    pub const ADD_JOINT: u32 = 23;
    /// Let go of a joint: its name.
    pub const REMOVE_JOINT: u32 = 24;
    // Questions: a ticket.
    pub const MOVE_CHARACTER: u32 = 32;
    pub const CAST_CHARACTER: u32 = 33;
    pub const RAYCAST: u32 = 34;
    pub const OVERLAP_SPHERE: u32 = 35;
    pub const OVERLAP_BOX: u32 = 36;
    /// 2.4: every hit of a ray through layers (`physics_link/questions.rs`).
    pub const RAYCAST_ALL: u32 = 37;
    pub const SHAPE_CAST: u32 = 38;
    pub const OVERLAP_SHAPE: u32 = 39;
    /// The last question.
    pub const QUESTION_LAST: u32 = 39;
}

/// What the published record of a body says (`BodyRec::flags`).
pub mod flag {
    /// The record holds the body's state.
    pub const BODY: u32 = 1;
    pub const CHARACTER: u32 = 2;
    pub const SLEEPING: u32 = 4;
    /// `GROUNDED` is known (the body is watched).
    pub const GROUND_KNOWN: u32 = 8;
    pub const GROUNDED: u32 = 16;
    pub const CHARACTER_GROUNDED: u32 = 32;
    /// `cp` holds the character's position.
    pub const CHARACTER_AT: u32 = 64;
    /// `pose` holds the pose of the transforms (the body is listed).
    pub const POSE: u32 = 128;
    /// The pose is a character's (its yaw is not the body's to give).
    pub const POSE_CHARACTER: u32 = 256;
}

/// What the port has under a slot's name (`SlotInfo::flags`).
pub mod info {
    pub const HAS_BODY: u32 = 1;
    pub const HAS_CHARACTER: u32 = 2;
}

/// The kind of a slot (`SlotInfo::kind`).
pub mod kind {
    pub const NONE: u32 = 0;
    pub const DYNAMIC: u32 = 1;
    pub const KINEMATIC: u32 = 2;
    pub const STATIC: u32 = 3;
    pub const CHARACTER: u32 = 4;
}

/// A moving body's published state.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BodyRec {
    pub flags: u32,
    /// The publication that wrote it (the record is the body's only when it equals the buffer's).
    pub stamp: u32,
    pub p: V3,
    pub q: [f64; 4],
    pub v: V3,
    pub w: V3,
    /// A character's position (its controller's).
    pub cp: V3,
    /// The pose the port gives for the transforms (`for_each_pose`): x, y, z, yaw — when [`flag::POSE`].
    pub pose: [f64; 4],
}

/// What a slot is, written once when the physics took its body (`added` is then the order's number).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SlotInfo {
    pub kind: u32,
    /// The number of the order that added it (0: not yet).
    pub added: u32,
    /// [`info::HAS_BODY`], [`info::HAS_CHARACTER`]: what the port says it has under that name.
    pub flags: u32,
    pub _pad: u32,
    pub mass: f64,
    pub p: V3,
    pub q: [f64; 4],
}

/// The head of a published buffer, then `cap` slot numbers (the moving bodies listed: `count`), then `cap`
/// [`BodyRec`] by slot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PubHead {
    pub stamp: u32,
    pub count: u32,
    /// The number of the last order the physics took before writing it.
    pub applied: u32,
    pub _pad: u32,
    /// The physics' own time (its steps × the fixed step), and the clock of the user's thread when it was written
    /// (ms; `now_ms` of the loop): for an interpolation by time.
    pub time: f64,
    pub wall_ms: f64,
    pub steps: f64,
}

/// A set of published buffers for `cap` slots, and the table of what each slot is.
#[repr(C)]
pub struct PubSet {
    pub cap: u32,
    pub _pad: u32,
    pub tri: Triple,
    /// `cap` [`SlotInfo`].
    pub info: usize,
    /// The three buffers' bytes, and the info's (one block each, the user's to free).
    pub bytes: usize,
}

impl PubSet {
    /// Bytes of one buffer for `cap` slots.
    pub fn buffer_bytes(cap: u32) -> usize {
        let list = (cap as usize * 4).next_multiple_of(8);
        std::mem::size_of::<PubHead>() + list + cap as usize * std::mem::size_of::<BodyRec>()
    }
    /// Makes a set (three buffers and the info, zeroed) — the user's thread allocates it, and frees it with
    /// [`PubSet::free`].
    pub fn make(cap: u32) -> *mut PubSet {
        let one = PubSet::buffer_bytes(cap);
        let info = cap as usize * std::mem::size_of::<SlotInfo>();
        let total = 3 * one + info;
        let block: Box<[u64]> = vec![0u64; total.div_ceil(8)].into_boxed_slice();
        let base = Box::into_raw(block) as *mut u64 as usize;
        let bufs = [base, base + one, base + 2 * one];
        Box::into_raw(Box::new(PubSet {
            cap,
            _pad: 0,
            tri: Triple::new(bufs),
            info: base + 3 * one,
            bytes: total.div_ceil(8),
        }))
    }
    /// # Safety
    /// A set made by [`PubSet::make`], used by nobody now.
    pub unsafe fn free(set: *mut PubSet) {
        let s = Box::from_raw(set);
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            s.tri.bufs[0] as *mut u64,
            s.bytes,
        )));
    }
    /// The head of a buffer.
    ///
    /// # Safety
    /// `buf` one of the set's.
    pub unsafe fn head(buf: usize) -> &'static mut PubHead {
        &mut *(buf as *mut PubHead)
    }
    /// # Safety
    /// As [`PubSet::head`].
    pub unsafe fn list(&self, buf: usize) -> &'static mut [u32] {
        std::slice::from_raw_parts_mut(
            (buf + std::mem::size_of::<PubHead>()) as *mut u32,
            self.cap as usize,
        )
    }
    /// # Safety
    /// As [`PubSet::head`]; `slot` < `cap`.
    pub unsafe fn rec(&self, buf: usize, slot: u32) -> &'static mut BodyRec {
        let at = buf + std::mem::size_of::<PubHead>() + (self.cap as usize * 4).next_multiple_of(8);
        &mut *(at as *mut BodyRec).add(slot as usize)
    }
    /// # Safety
    /// `slot` < `cap`.
    pub unsafe fn info(&self, slot: u32) -> &'static mut SlotInfo {
        &mut *(self.info as *mut SlotInfo).add(slot as usize)
    }
}

/// The physics written in place on the user's thread (no copy of a buffer: the user's buffer IS the one written).
pub const LIVE: u32 = 0;
/// The physics on a loop of its own (the triple buffer).
pub const LOOP: u32 = 1;

/// The link of a physics: in the memory both share, made by the user ([`PhysicsLink::make`]).
#[repr(C)]
pub struct PhysicsLink {
    pub head: Head,
    pub orders: Ring,
    /// Contact events, from the physics to the user: `[a slot u32 | b slot u32][begin u32 | 0][impulse f64]`.
    pub events: Ring,
    pub answers: Answers,
    /// The set the physics writes (it changes it on [`order::GROW`]).
    pub set: AtomicUsize,
    /// The number of the last order the physics took.
    pub applied: AtomicU32,
    /// [`LIVE`] or [`LOOP`].
    pub mode: u32,
    pub fixed_dt: f64,
    pub max_steps: u32,
    /// Events dropped because the user did not take them in time.
    pub events_dropped: AtomicU32,
    /// The physics' own state ([`Sim`]), made by the side that serves (0: not yet).
    pub sim: AtomicUsize,
    /// Counters of the loop: steps, and the ms it spent stepping and serving (f64 bits).
    pub steps: AtomicU32,
    pub _pad2: u32,
    pub busy_ms: std::sync::atomic::AtomicU64,
    /// What the port says of itself once: `ground_clearance` (f64 bits) and whether it steps by hand.
    pub clearance_bits: std::sync::atomic::AtomicU64,
    pub manual: AtomicU32,
    pub _pad: u32,
    /// The address of the served port's shape table (ABI physique 2.6), told once by the side that serves (0: none
    /// yet, or a port that publishes none): what a user on another thread reads in place.
    pub shapes: AtomicUsize,
}

/// What a link owns besides itself (freed with it).
#[allow(dead_code)]
struct Blocks {
    orders: Box<[u64]>,
    events: Box<[u64]>,
    answers: Box<[AnswerRec]>,
}

impl PhysicsLink {
    /// A link for a physics on `mode`, with rings of `order_words` and `event_words` (powers of two), `answers`
    /// records, and a first set of `cap` slots. The user's thread makes it and keeps it until the physics quit.
    pub fn make(
        mode: u32,
        fixed_dt: f64,
        order_words: u32,
        event_words: u32,
        answers: u32,
        cap: u32,
    ) -> *mut PhysicsLink {
        let mut orders = vec![0u64; order_words as usize].into_boxed_slice();
        let mut events = vec![0u64; event_words as usize].into_boxed_slice();
        // SAFETY: an answer record is plain numbers and atomics: zero is FREE.
        let mut recs: Box<[AnswerRec]> = (0..answers.min(crate::ports::link::ANSWERS_MAX))
            .map(|_| unsafe { std::mem::zeroed() })
            .collect();
        let link = PhysicsLink {
            head: Head {
                magic: MAGIC,
                control: AtomicU32::new(if mode == LIVE { RUN } else { PAUSE }),
                wake: AtomicU32::new(0),
                served: AtomicU32::new(0),
            },
            orders: Ring::new(orders.as_mut_ptr(), order_words),
            events: Ring::new(events.as_mut_ptr(), event_words),
            answers: Answers {
                recs: recs.as_mut_ptr() as usize,
                cap: recs.len() as u32,
                cursor: AtomicU32::new(0),
            },
            set: AtomicUsize::new(PubSet::make(cap) as usize),
            applied: AtomicU32::new(0),
            mode,
            fixed_dt,
            max_steps: 4,
            events_dropped: AtomicU32::new(0),
            sim: AtomicUsize::new(0),
            steps: AtomicU32::new(0),
            _pad2: 0,
            busy_ms: Default::default(),
            clearance_bits: Default::default(),
            manual: AtomicU32::new(0),
            _pad: 0,
            shapes: AtomicUsize::new(0),
        };
        // The blocks live as long as the link (kept beside it, freed by `free`).
        let blocks = Box::new(Blocks {
            orders,
            events,
            answers: recs,
        });
        let both = Box::new((link, blocks));
        Box::into_raw(both) as *mut PhysicsLink
    }

    /// Frees a link and its current set (the physics no longer serves it: quit, or this thread's).
    ///
    /// # Safety
    /// A link of [`PhysicsLink::make`], used by nobody now.
    pub unsafe fn free(link: *mut PhysicsLink) {
        let both = Box::from_raw(link as *mut (PhysicsLink, Box<Blocks>));
        let set = both.0.set.load(Acquire);
        if set != 0 {
            PubSet::free(set as *mut PubSet);
        }
        // Answers still holding names past their record.
        for r in both.1.answers.iter() {
            if r.more != 0 {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    r.more as *mut u32,
                    r.more_len as usize,
                )));
            }
        }
        drop(both);
    }
}

// ─── the records of the orders ────────────────────────────────────────────────

/// An order as the ring holds it: `[seq u32 | slot u32][ticket u32 | n_nums u16 | name_len u16]`, `n_nums` f64, then
/// the name's bytes (packed in words), then `extra` words (a mesh: its addresses and counts).
pub struct Order<'a> {
    pub seq: u32,
    pub slot: u32,
    pub ticket: u32,
    pub nums: &'a [f64],
    pub name: &'a str,
    pub extra: &'a [u64],
}

impl Order<'_> {
    /// The words of the record (header of the ring included).
    pub fn words(&self) -> u32 {
        1 + 2
            + self.nums.len() as u32
            + self.name.len().div_ceil(8) as u32
            + self.extra.len() as u32
    }
    /// Writes its body (after the ring's header).
    pub fn write(&self, body: &mut [u64]) {
        body[0] = self.seq as u64 | (self.slot as u64) << 32;
        body[1] =
            self.ticket as u64 | (self.nums.len() as u64) << 32 | (self.name.len() as u64) << 48;
        let mut at = 2;
        for n in self.nums {
            body[at] = n.to_bits();
            at += 1;
        }
        for chunk in self.name.as_bytes().chunks(8) {
            let mut w = [0u8; 8];
            w[..chunk.len()].copy_from_slice(chunk);
            body[at] = u64::from_le_bytes(w);
            at += 1;
        }
        for e in self.extra {
            body[at] = *e;
            at += 1;
        }
    }
}

/// An order read where it is.
pub struct Read<'a> {
    pub seq: u32,
    pub slot: u32,
    pub ticket: u32,
    pub nums: &'a [u64],
    pub name: &'a [u8],
    pub extra: &'a [u64],
}

impl<'a> Read<'a> {
    pub fn of(body: &'a [u64]) -> Read<'a> {
        let (n, len) = (
            ((body[1] >> 32) & 0xffff) as usize,
            (body[1] >> 48) as usize,
        );
        let nums = &body[2..2 + n];
        let words = len.div_ceil(8);
        // SAFETY: the words are bytes in memory order (little-endian, as written).
        let name = unsafe { std::slice::from_raw_parts(body[2 + n..].as_ptr() as *const u8, len) };
        Read {
            seq: body[0] as u32,
            slot: (body[0] >> 32) as u32,
            ticket: body[1] as u32,
            nums,
            name,
            extra: &body[2 + n + words..],
        }
    }
    pub fn num(&self, i: usize) -> f64 {
        self.nums.get(i).map_or(0.0, |w| f64::from_bits(*w))
    }
    pub fn v3(&self, i: usize) -> V3 {
        [self.num(i), self.num(i + 1), self.num(i + 2)]
    }
    pub fn name(&self) -> &'a str {
        std::str::from_utf8(self.name).unwrap_or("")
    }
}

// ─── the physics' side ──────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct LiveForce {
    current: V3,
    impulse: V3,
    active: bool,
}

/// The physics' own state, kept by the side that serves the link.
pub struct Sim {
    names: Vec<Option<String>>,
    kinds: Vec<u32>,
    watched: Vec<bool>,
    by_name: HashMap<String, u32>,
    moving: BTreeSet<u32>,
    posed: Vec<(u32, [f64; 4], bool)>,
    set: usize,
    stamp: u32,
    accumulator: f64,
    fixed_dt: Option<f64>,
    live_forces: Vec<LiveForce>,
    force_slots: Vec<u32>,
    last_ms: f64,
    time: f64,
    steps: f64,
    manual: bool,
}

impl Sim {
    fn new(set: usize) -> Sim {
        Sim {
            names: Vec::new(),
            kinds: Vec::new(),
            watched: Vec::new(),
            by_name: HashMap::new(),
            moving: BTreeSet::new(),
            posed: Vec::new(),
            set,
            stamp: 1,
            accumulator: 0.0,
            fixed_dt: None,
            live_forces: Vec::new(),
            force_slots: Vec::new(),
            last_ms: f64::NAN,
            time: 0.0,
            steps: 0.0,
            manual: false,
        }
    }

    fn name(&self, slot: u32) -> Option<&str> {
        self.names.get(slot as usize).and_then(|n| n.as_deref())
    }

    fn slot_of(&self, name: &str) -> u32 {
        self.by_name.get(name).copied().unwrap_or(NO_SLOT)
    }

    fn grow_to(&mut self, slot: u32) {
        let n = slot as usize + 1;
        if self.names.len() < n {
            self.names.resize(n, None);
            self.kinds.resize(n, kind::NONE);
            self.watched.resize(n, false);
            self.live_forces.resize(n, LiveForce::default());
        }
    }

    fn set(&self) -> &'static PubSet {
        // SAFETY: the set the physics writes, alive while it does (the user frees one only after the physics left it).
        unsafe { &*(self.set as *const PubSet) }
    }

    /// The buffer written now: the user's own when live, the back one on a loop.
    fn target(&self, mode: u32) -> usize {
        let s = self.set();
        if mode == LIVE {
            s.tri.front()
        } else {
            s.tri.back()
        }
    }

    /// Writes the record of one slot into `buf`.
    fn write_rec(&self, p: &dyn PhysicsPort, buf: usize, slot: u32) {
        let Some(name) = self.name(slot) else { return };
        let set = self.set();
        if slot >= set.cap {
            return;
        }
        // SAFETY: `slot` < cap, `buf` a buffer of the set.
        let r = unsafe { set.rec(buf, slot) };
        let mut flags = 0;
        if let Some(b) = p.body(name) {
            flags |= flag::BODY;
            r.p = b.p;
            r.q = b.q;
            r.v = b.v;
            r.w = b.w;
            if b.sleeping {
                flags |= flag::SLEEPING;
            }
        }
        if self.kinds[slot as usize] == kind::CHARACTER {
            flags |= flag::CHARACTER;
            if let Some(cp) = p.character_position(name) {
                flags |= flag::CHARACTER_AT;
                r.cp = cp;
            }
            if p.character_grounded(name) {
                flags |= flag::CHARACTER_GROUNDED;
            }
        }
        if self.watched[slot as usize] {
            flags |= flag::GROUND_KNOWN;
            if p.grounded(name) {
                flags |= flag::GROUNDED;
            }
        }
        // SAFETY: as above.
        let stamp = unsafe { PubSet::head(buf) }.stamp;
        // A record of this publication keeps its place in the list; its pose follows the body.
        if r.stamp == stamp && r.flags & flag::POSE != 0 {
            let character = r.flags & flag::POSE_CHARACTER;
            flags |= flag::POSE | character;
            if character != 0 && flags & flag::CHARACTER_AT != 0 {
                r.pose = [r.cp[0], r.cp[1], r.cp[2], r.pose[3]];
            } else if flags & flag::BODY != 0 {
                let q = r.q;
                let yaw = (2.0 * (q[3] * q[1] + q[0] * q[2]))
                    .atan2(1.0 - 2.0 * (q[1] * q[1] + q[2] * q[2]));
                r.pose = [r.p[0], r.p[1], r.p[2], yaw];
            }
        }
        r.flags = flags;
        r.stamp = stamp;
    }

    /// Writes the whole buffer: every moving body, the list, the head.
    fn write_all(&mut self, p: &dyn PhysicsPort, buf: usize, applied: u32, wall_ms: f64) {
        // SAFETY: a buffer of the set.
        let head = unsafe { PubSet::head(buf) };
        self.stamp = self.stamp.wrapping_add(1).max(1);
        head.stamp = self.stamp;
        for &slot in &self.moving {
            self.write_rec(p, buf, slot);
        }
        self.list_poses(p, buf);
        head.applied = applied;
        head.time = self.time;
        head.steps = self.steps;
        head.wall_ms = wall_ms;
    }

    /// The poses of the transforms, in the port's order: the list of the buffer.
    fn list_poses(&mut self, p: &dyn PhysicsPort, buf: usize) {
        let set = self.set();
        // SAFETY: a buffer of the set.
        let head = unsafe { PubSet::head(buf) };
        let list = unsafe { set.list(buf) };
        let mut n = 0;
        let mut posed = std::mem::take(&mut self.posed);
        posed.clear();
        p.for_each_pose(&mut |name, at, yaw, character| {
            let slot = self.by_name.get(name).copied().unwrap_or(NO_SLOT);
            if slot < set.cap {
                posed.push((slot, [at[0], at[1], at[2], yaw], character));
            }
        });
        for &(slot, pose, character) in &posed {
            if !self.moving.contains(&slot) {
                self.write_rec(p, buf, slot);
            }
            // SAFETY: `slot` < cap.
            let r = unsafe { set.rec(buf, slot) };
            r.pose = pose;
            r.flags |= flag::POSE | if character { flag::POSE_CHARACTER } else { 0 };
            list[n] = slot;
            n += 1;
        }
        self.posed = posed;
        head.count = n as u32;
    }

    /// The info of a slot, after its body was added.
    fn write_info(&self, p: &dyn PhysicsPort, slot: u32, seq: u32) {
        let set = self.set();
        if slot >= set.cap {
            return;
        }
        let Some(name) = self.name(slot) else { return };
        // SAFETY: `slot` < cap.
        let info = unsafe { set.info(slot) };
        info.kind = self.kinds[slot as usize];
        info.mass = p.mass(name);
        info.flags = if p.has_body(name) { info::HAS_BODY } else { 0 }
            | if p.has_character(name) {
                info::HAS_CHARACTER
            } else {
                0
            };
        if let Some(b) = p.body(name) {
            info.p = b.p;
            info.q = b.q;
        } else if let Some(cp) = p.character_position(name) {
            info.p = cp;
            info.q = [0.0, 0.0, 0.0, 1.0];
        }
        info.added = seq;
    }

    fn named(&mut self, slot: u32, name: &str, k: u32) {
        self.grow_to(slot);
        if let Some(old) = self.names[slot as usize].take() {
            self.by_name.remove(&old);
        }
        // A name given to another slot before (a body added again under the same name): that slot is gone.
        if let Some(other) = self.by_name.insert(name.to_string(), slot) {
            if other != slot {
                self.names[other as usize] = None;
                self.kinds[other as usize] = kind::NONE;
                self.moving.remove(&other);
            }
        }
        self.names[slot as usize] = Some(name.to_string());
        self.kinds[slot as usize] = k;
        if matches!(k, kind::DYNAMIC | kind::KINEMATIC | kind::CHARACTER) {
            self.moving.insert(slot);
        } else {
            self.moving.remove(&slot);
        }
    }

    /// Takes one order. Answers whether it changed what is published (and which slot: `NO_SLOT` for all).
    fn take(
        &mut self,
        p: &mut dyn PhysicsPort,
        link: &PhysicsLink,
        k: u32,
        o: &Read,
    ) -> Option<u32> {
        use order::*;
        let name = |s: &Sim| s.name(o.slot).map(str::to_string);
        match k {
            ADD_BODY => {
                let d = desc_of(o);
                let kd = match d.kind {
                    crate::ports::BodyKind::Dynamic => kind::DYNAMIC,
                    crate::ports::BodyKind::Kinematic => kind::KINEMATIC,
                    crate::ports::BodyKind::Static => kind::STATIC,
                };
                self.named(o.slot, o.name(), kd);
                p.add_body(o.name(), &d);
                self.write_info(p, o.slot, o.seq);
                Some(o.slot)
            }
            ADD_CHARACTER => {
                self.named(o.slot, o.name(), kind::CHARACTER);
                let opts =
                    crate::ports::CharacterOptions::of(o.num(5), o.num(6), o.num(7), o.num(8));
                p.add_character(o.name(), o.v3(0), o.num(3), o.num(4), opts);
                self.write_info(p, o.slot, o.seq);
                Some(o.slot)
            }
            ADD_STATIC_BOX => {
                self.named(o.slot, o.name(), kind::STATIC);
                p.add_static_box(o.v3(0), o.v3(3), o.name());
                self.write_info(p, o.slot, o.seq);
                Some(o.slot)
            }
            ADD_DYNAMIC_BOX => {
                self.named(o.slot, o.name(), kind::DYNAMIC);
                p.add_dynamic_box(o.name(), o.v3(0), o.v3(3), o.num(6));
                self.write_info(p, o.slot, o.seq);
                Some(o.slot)
            }
            SET_COLLISION_SPACE => {
                let n = name(self)?;
                p.set_collision_space(&n, o.name());
                None
            }
            REMOVE => {
                let n = name(self)?;
                p.remove(&n);
                self.by_name.remove(&n);
                self.names[o.slot as usize] = None;
                self.kinds[o.slot as usize] = kind::NONE;
                self.watched[o.slot as usize] = false;
                self.live_forces[o.slot as usize] = LiveForce::default();
                self.force_slots.retain(|slot| *slot != o.slot);
                self.moving.remove(&o.slot);
                let set = self.set();
                if o.slot < set.cap {
                    // SAFETY: `slot` < cap.
                    unsafe { *set.info(o.slot) = SlotInfo::default() };
                }
                Some(o.slot)
            }
            SET_BODY => {
                let n = name(self)?;
                let s = crate::ports::BodyState {
                    entity: n,
                    p: o.v3(0),
                    q: [o.num(3), o.num(4), o.num(5), o.num(6)],
                    v: o.v3(7),
                    w: o.v3(10),
                    sleeping: o.num(13) != 0.0,
                };
                p.set_body(&s);
                self.refresh_static(p, o.slot);
                Some(o.slot)
            }
            SET_POSITION => {
                p.set_position(self.name(o.slot)?, o.v3(0));
                self.refresh_static(p, o.slot);
                Some(o.slot)
            }
            SET_LINVEL => {
                p.set_linvel(self.name(o.slot)?, o.v3(0));
                self.refresh_static(p, o.slot);
                Some(o.slot)
            }
            SET_ANGVEL => {
                p.set_angvel(self.name(o.slot)?, o.v3(0));
                Some(o.slot)
            }
            SET_ROTATION => {
                p.set_rotation(self.name(o.slot)?, [o.num(0), o.num(1), o.num(2), o.num(3)]);
                self.refresh_static(p, o.slot);
                Some(o.slot)
            }
            IMPULSE => {
                p.apply_impulse(self.name(o.slot)?, o.v3(0));
                Some(o.slot)
            }
            FORCE => {
                if link.mode == LIVE
                    && self.fixed_dt.is_some()
                    && !self.manual
                    && !p.manual_stepping()
                {
                    let f = &mut self.live_forces[o.slot as usize];
                    for (sum, value) in f.current.iter_mut().zip(o.v3(0)) {
                        *sum += value;
                    }
                    if !f.active {
                        f.active = true;
                        self.force_slots.push(o.slot);
                    }
                } else {
                    p.apply_force(self.name(o.slot)?, o.v3(0));
                }
                None
            }
            TORQUE_IMPULSE => {
                p.apply_torque_impulse(self.name(o.slot)?, o.v3(0));
                Some(o.slot)
            }
            MOVE_KINEMATIC => {
                let q = (o.num(7) != 0.0).then(|| [o.num(3), o.num(4), o.num(5), o.num(6)]);
                p.move_kinematic(self.name(o.slot)?, o.v3(0), q);
                Some(o.slot)
            }
            SET_CHARACTER_POSITION => {
                p.set_character_position(self.name(o.slot)?, o.v3(0));
                Some(o.slot)
            }
            SET_GRAVITY => {
                p.set_gravity(o.v3(0));
                None
            }
            SET_MANUAL => {
                self.flush_live_forces(p);
                self.manual = o.num(0) != 0.0;
                p.set_manual_stepping(self.manual);
                link.manual.store(self.manual as u32, Release);
                None
            }
            ADD_JOINT => {
                let j: [f64; crate::ports::JOINT_WORDS] = std::array::from_fn(|i| o.num(i + 1));
                let b = self.name(o.num(0) as u32);
                if let (Some(a), Some(b), Some(j)) =
                    (self.name(o.slot), b, crate::ports::JointDesc::of_words(&j))
                {
                    p.add_joint(o.name(), a, b, &j);
                }
                None
            }
            REMOVE_JOINT => {
                p.remove_joint(o.name());
                None
            }
            SET_FIXED_DT => {
                let dt = o.num(0);
                if dt.is_finite() && (1.0 / 240.0..=1.0).contains(&dt) {
                    self.fixed_dt = Some(dt);
                }
                None
            }
            STEP => {
                self.flush_live_forces(p);
                self.step(p, link, o.num(0));
                Some(NO_SLOT)
            }
            WATCH_GROUNDED => {
                self.grow_to(o.slot);
                self.watched[o.slot as usize] = true;
                Some(o.slot)
            }
            GROW => {
                self.regrow(p, link, o.extra.first().copied().unwrap_or(0) as usize);
                Some(NO_SLOT)
            }
            MOVE_CHARACTER..=QUESTION_LAST => self.question(p, link, k, o),
            _ => None,
        }
    }

    // Explicit/manual steps keep the original one-step force contract.
    fn flush_live_forces(&mut self, p: &mut dyn PhysicsPort) {
        for &slot in &self.force_slots {
            if let Some(name) = self.name(slot) {
                p.apply_force(name, self.live_forces[slot as usize].current);
            }
            self.live_forces[slot as usize].current = [0.0; 3];
        }
    }

    fn step(&mut self, p: &mut dyn PhysicsPort, link: &PhysicsLink, dt: f64) {
        p.step(dt);
        self.time += dt;
        self.steps += 1.0;
        link.steps.fetch_add(1, Relaxed);
        // The contact events of this step, by slot, for the user.
        // Read where the port keeps them (no name copied, P39).
        p.for_each_contact_event(&mut |a, b, begin, impulse| {
            self.push_pair(link, 1, a, b, begin as u64, impulse);
        });
        // The collision pairs of this step with their speed (`contacts()`, the port's own definition).
        p.for_each_contact(&mut |a, b, speed| self.push_pair(link, 2, a, b, 0, speed));
        // The end of a step's records (the user keeps what came since the last step it saw).
        // SAFETY: as above.
        unsafe { link.events.push(3, 1, |_| {}) };
    }

    /// A record of the events ring: `[a slot | b slot][flag][value][a name bytes | b name bytes]` then the names of
    /// the slots this link does not know (a collider under a name nobody added through it).
    fn push_pair(&self, link: &PhysicsLink, k: u32, a: &str, b: &str, flag: u64, value: f64) {
        let (sa, sb) = (self.slot_of(a), self.slot_of(b));
        let na = if sa == NO_SLOT { a.as_bytes() } else { &[] };
        let nb = if sb == NO_SLOT { b.as_bytes() } else { &[] };
        let words = 5 + (na.len() + nb.len()).div_ceil(8) as u32;
        // SAFETY: the physics is the one producer of the events ring.
        let pushed = unsafe {
            link.events.push(k, words, |w| {
                w[0] = sa as u64 | (sb as u64) << 32;
                w[1] = flag;
                w[2] = value.to_bits();
                w[3] = na.len() as u64 | (nb.len() as u64) << 32;
                let bytes = std::slice::from_raw_parts_mut(
                    w[4..].as_mut_ptr() as *mut u8,
                    (w.len() - 4) * 8,
                );
                bytes[..na.len()].copy_from_slice(na);
                bytes[na.len()..na.len() + nb.len()].copy_from_slice(nb);
            })
        };
        if !pushed {
            link.events_dropped.fetch_add(1, Relaxed);
        }
    }

    /// A static body moved by an order: its info follows (a static body is not published every step).
    fn refresh_static(&self, p: &dyn PhysicsPort, slot: u32) {
        if self.kinds.get(slot as usize) == Some(&kind::STATIC) && slot < self.set().cap {
            // SAFETY: `slot` < cap.
            let seq = unsafe { self.set().info(slot) }.added;
            self.write_info(p, slot, seq);
        }
    }

    fn regrow(&mut self, p: &dyn PhysicsPort, link: &PhysicsLink, to: usize) {
        if to == 0 || to == self.set {
            return;
        }
        let (old, new) = (self.set(), unsafe { &*(to as *const PubSet) });
        for slot in 0..old.cap.min(new.cap) {
            // SAFETY: both below their cap.
            unsafe { *new.info(slot) = *old.info(slot) };
        }
        self.set = to;
        // The new set starts complete: the user reads it as soon as it sees it.
        let applied = link.applied.load(Relaxed);
        if link.mode == LIVE {
            let buf = new.tri.front();
            self.write_all(p, buf, applied, f64::NAN);
        } else {
            let buf = new.tri.back();
            self.write_all(p, buf, applied, f64::NAN);
            new.tri.publish();
        }
        link.set.store(to, Release);
    }
}

fn answer(link: &PhysicsLink, ticket: u32, fill: impl FnOnce(&mut AnswerRec)) {
    // SAFETY: the record of a ticket the user handed out with the order.
    if let Some(r) = unsafe { link.answers.to_fill(ticket) } {
        r.count = 0;
        fill(r);
        Answers::ready(r);
    }
}

/// A body description as the order carries it (`DESC_WORDS` numbers; a mesh's addresses in `extra`).
fn desc_of(o: &Read) -> crate::ports::BodyDesc {
    use crate::ports::{BodyDesc, BodyKind, Shape};
    let kind = match o.num(0) as u32 {
        1 => BodyKind::Kinematic,
        2 => BodyKind::Static,
        _ => BodyKind::Dynamic,
    };
    let s = o.v3(2);
    let code = o.num(1) as u32;
    let shape = if (3..=6).contains(&code) && o.extra.len() >= 4 {
        let (vp, nv, ip, ni) = (
            o.extra[0] as usize,
            o.extra[1] as usize,
            o.extra[2] as usize,
            o.extra[3] as usize,
        );
        // SAFETY: the user keeps the streams where these say until the physics took the order.
        unsafe {
            let f: &[f32] = if nv == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(vp as *const f32, nv)
            };
            let i: &[u32] = if ni == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(ip as *const u32, ni)
            };
            Shape::from_wire(code, s, f, i)
        }
    } else {
        Shape::from_wire(if code <= 2 || code == 7 { code } else { 0 }, s, &[], &[])
    };
    let mut d = BodyDesc::new(kind, shape, o.v3(5));
    d.rotation = [o.num(8), o.num(9), o.num(10), o.num(11)];
    d.mass = o.num(12);
    d.friction = o.num(13);
    d.restitution = o.num(14);
    d.linear_damping = o.num(15);
    d.angular_damping = o.num(16);
    d.ccd = o.num(17) != 0.0;
    d.can_sleep = o.num(18) != 0.0;
    d.lock_rotation = o.num(19) != 0.0;
    d.lock_tilt = o.num(20) != 0.0;
    d.gravity_scale = o.num(21);
    d.ground_probe = o.num(22);
    // The layers came with 2.4: an order of fewer words keeps the defaults (the first layer, colliding with all).
    if o.nums.len() >= crate::ports::abi::DESC_WORDS_4 {
        d.layers = o.num(23) as u32;
        d.collides_with = o.num(24) as u32;
    }
    if o.nums.len() >= crate::ports::abi::DESC_WORDS_5 {
        d.sensor = o.num(25) != 0.0;
    }
    d
}

/// The words of a body description for [`order::ADD_BODY`] (the inverse of what the physics reads).
pub fn desc_words(d: &crate::ports::BodyDesc) -> [f64; crate::ports::abi::DESC_WORDS_5] {
    use crate::ports::BodyKind;
    let (shape, s) = (d.shape.code() as f64, d.shape.numbers());
    let kind = match d.kind {
        BodyKind::Dynamic => 0.0,
        BodyKind::Kinematic => 1.0,
        BodyKind::Static => 2.0,
    };
    let b = |x: bool| x as u8 as f64;
    [
        kind,
        shape,
        s[0],
        s[1],
        s[2],
        d.position[0],
        d.position[1],
        d.position[2],
        d.rotation[0],
        d.rotation[1],
        d.rotation[2],
        d.rotation[3],
        d.mass,
        d.friction,
        d.restitution,
        d.linear_damping,
        d.angular_damping,
        b(d.ccd),
        b(d.can_sleep),
        b(d.lock_rotation),
        b(d.lock_tilt),
        d.gravity_scale,
        d.ground_probe,
        d.layers as f64,
        d.collides_with as f64,
        b(d.sensor),
    ]
}

fn sim_of(link: &PhysicsLink) -> &'static mut Sim {
    let mut at = link.sim.load(Acquire);
    if at == 0 {
        at = Box::into_raw(Box::new(Sim::new(link.set.load(Acquire)))) as usize;
        link.sim.store(at, Release);
    }
    // SAFETY: made above by this side, the only one that serves the link.
    unsafe { &mut *(at as *mut Sim) }
}

/// Drops the physics' own state (the side that served the link, when it stops).
///
/// # Safety
/// The side that served `link`, which serves it no more.
pub unsafe fn drop_sim(link: &PhysicsLink) {
    let at = link.sim.swap(0, AcqRel);
    if at != 0 {
        drop(Box::from_raw(at as *mut Sim));
    }
}

/// The physics' side of a [`LIVE`] link, on the user's thread: takes every order waiting, each written in place at
/// once (the user's buffer is the one written). Answers the orders taken.
///
/// # Safety
/// `link` a live link of [`PhysicsLink::make`]; this thread serves it alone.
pub unsafe fn serve_live(link: &PhysicsLink, p: &mut dyn PhysicsPort) -> u32 {
    let sim = sim_of(link);
    let mut relist = false;
    let n = link.orders.drain(|k, body| {
        let o = Read::of(body);
        relist |= matches!(
            k,
            order::ADD_BODY
                | order::ADD_CHARACTER
                | order::ADD_STATIC_BOX
                | order::ADD_DYNAMIC_BOX
                | order::REMOVE
        );
        let touched = sim.take(p, link, k, &o);
        link.applied.store(o.seq, Release);
        let buf = sim.target(LIVE);
        match touched {
            Some(NO_SLOT) => {
                let applied = o.seq;
                sim.write_all(p, buf, applied, f64::NAN);
            }
            Some(slot) => {
                if k == order::REMOVE {
                    if slot < sim.set().cap {
                        sim.set().rec(buf, slot).flags = 0;
                    }
                } else if sim.moving.contains(&slot)
                    || sim.watched.get(slot as usize).copied().unwrap_or(false)
                {
                    sim.write_rec(p, buf, slot);
                }
            }
            None => {}
        }
    });
    if relist {
        sim.list_poses(p, sim.target(LIVE));
    }
    if n > 0 {
        PubSet::head(sim.target(LIVE)).applied = link.applied.load(Relaxed);
    }
    n
}

/// A step of a [`LIVE`] link, on the user's thread (its fixed step): steps unless stepping is manual, then writes
/// every moving body in place. Answers whether it stepped.
///
/// # Safety
/// As [`serve_live`].
pub unsafe fn step_live(link: &PhysicsLink, p: &mut dyn PhysicsPort, dt: f64) -> bool {
    serve_live(link, p);
    let sim = sim_of(link);
    let mut stepped = false;
    if !sim.manual && !p.manual_stepping() {
        if let Some(fixed_dt) = sim.fixed_dt {
            // A force belongs to the elapsed action tick, not to the next entire physics step.
            // Integrate its impulse over each overlap; keep the remainder for a slower clock and
            // reuse the force across all substeps of a faster clock. Storage is retained by slot.
            let mut remaining = dt.clamp(0.0, 0.25);
            while remaining > 1e-12 || sim.accumulator >= fixed_dt - 1e-9 {
                let portion = remaining.min((fixed_dt - sim.accumulator).max(0.0));
                for &slot in &sim.force_slots {
                    let f = &mut sim.live_forces[slot as usize];
                    for (impulse, force) in f.impulse.iter_mut().zip(f.current) {
                        *impulse += force * portion;
                    }
                }
                sim.accumulator += portion;
                remaining -= portion;
                if sim.accumulator >= fixed_dt - 1e-9 {
                    let fraction = (fixed_dt / sim.accumulator).min(1.0);
                    for &slot in &sim.force_slots {
                        if let Some(name) = sim.name(slot) {
                            p.apply_force(
                                name,
                                sim.live_forces[slot as usize]
                                    .impulse
                                    .map(|v| v * fraction / fixed_dt),
                            );
                        }
                        for impulse in &mut sim.live_forces[slot as usize].impulse {
                            *impulse *= 1.0 - fraction;
                        }
                    }
                    sim.step(p, link, fixed_dt);
                    sim.accumulator -= fixed_dt;
                    stepped = true;
                }
            }
            sim.force_slots.retain(|slot| {
                let f = &mut sim.live_forces[*slot as usize];
                f.current = [0.0; 3];
                f.active = f.impulse != [0.0; 3];
                f.active
            });
        } else {
            sim.step(p, link, dt);
            stepped = true;
        }
    }
    let buf = sim.target(LIVE);
    sim.write_all(p, buf, link.applied.load(Relaxed), f64::NAN);
    stepped
}

/// The physics' side of a [`LOOP`] link: one turn of its loop at `now_ms` — the orders waiting taken, the fixed steps
/// due (while the user says [`RUN`]), then the state published when anything changed. Answers how long the loop
/// may sleep before the next turn (ms; the orders are looked at again by then), negative to end the loop
/// ([`QUIT`]).
///
/// # Safety
/// `link` a loop link of [`PhysicsLink::make`]; one thread serves it.
pub unsafe fn serve_loop(link: &PhysicsLink, p: &mut dyn PhysicsPort, now_ms: f64) -> f64 {
    let control = link.head.control.load(Acquire);
    if control == QUIT {
        drop_sim(link);
        return -1.0;
    }
    let sim = sim_of(link);
    link.head.served.fetch_add(1, Relaxed);
    if link.clearance_bits.load(Relaxed) == 0 {
        link.clearance_bits
            .store(p.ground_clearance().to_bits() | 1 << 63, Relaxed);
    }
    if link.shapes.load(Relaxed) == 0 {
        if let Some(at) = p.shapes_view() {
            link.shapes.store(at, Release);
        }
    }
    let mut changed = link.orders.drain(|k, body| {
        let o = Read::of(body);
        sim.take(p, link, k, &o);
        link.applied.store(o.seq, Release);
    }) > 0;
    let dt = sim.fixed_dt.unwrap_or(link.fixed_dt);
    if sim.last_ms.is_nan() || control != RUN {
        sim.last_ms = now_ms;
    }
    if control == RUN && !sim.manual {
        sim.accumulator += ((now_ms - sim.last_ms) / 1000.0).clamp(0.0, 0.25);
        sim.last_ms = now_ms;
        let mut n = 0;
        while sim.accumulator >= dt - 1e-9 {
            if n >= link.max_steps {
                sim.accumulator %= dt;
                break;
            }
            sim.step(p, link, dt);
            sim.accumulator -= dt;
            n += 1;
            changed = true;
        }
    }
    if changed {
        let buf = sim.target(LOOP);
        sim.write_all(p, buf, link.applied.load(Relaxed), now_ms);
        sim.set().tri.publish();
    }
    if control == RUN && !sim.manual {
        ((dt - sim.accumulator) * 1000.0).clamp(0.0, 2.0)
    } else {
        2.0
    }
}

/// Adds to the ms the loop spent working (its counters: `busy_ms`).
pub fn add_busy(link: &PhysicsLink, ms: f64) {
    let _ = link.busy_ms.try_update(Relaxed, Relaxed, |b| {
        Some((f64::from_bits(b) + ms).to_bits())
    });
}
