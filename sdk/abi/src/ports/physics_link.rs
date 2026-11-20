//! The physics over a link (link.rs, docs/kit-modules.md §7.17): what the user (the core and the kits, on their
//! thread) and the physics (on its own loop — a Worker, a thread — or taken at once on the user's thread) share.
//!
//! - **Orders** ([`order`]): every change (a body added or removed, a force, an impulse, a teleport, a kinematic
//!   target, the gravity…) is a record of the ring, fire and forget; every question (a character's move, a cast, a
//!   raycast, an overlap) is a record with a ticket, answered in the slab of answers.
//! - **The published state** ([`PubSet`]): for every moving body and character (and every static body an order
//!   moved), its pose, velocities, flags and revision (`rev` of `abi.physics.bodies`, raised when it moved; the head's
//!   `moves` counts the moves), written by the physics after each step and the orders it took, in four buffers
//!   (`link::Quad`) the user reads in place at any time; once per body (`abi.physics.slots`), what it is (kind, mass,
//!   first pose). The blocks follow the shared contracts (core/abi/contracts/abi.physics.*.json, [`PubSet`]).
//! - **Contact events**: a second ring, from the physics to the user.
//!
//! Bodies are named by **slot** (a number the user gives when it adds the body); the names stay with the user,
//! except that the physics keeps them to speak to its port (`PhysicsPort` is by name).
//!
//! [`serve`] is the physics' side, generic over any `PhysicsPort`: the core runs it on its own thread (the orders
//! taken as they come, the state written in place — [`LIVE`]), the physics module runs it on its Worker in a loop
//! (`gfp_serve`), natively a thread can run it too. One code, the same records.

use crate::ports::link::{Answers, Head, Ring, PAUSE, QUIT, RUN};

/// The body description an order carries, read back.
mod desc;
mod pair_events;
/// The questions (a ticket each): what the physics answers, and how the user reads the hits.
mod questions;
mod settings;
mod thrown;
mod unknown;
mod unlisted;
use crate::ports::{PhysicsPort, V3};
use desc::desc_of;
pub use desc::desc_words;
pub use questions::{hits_of, pack_hits, HIT_WORDS};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
pub use unknown::unknown_orders;
use unlisted::{listed_pose, pose_of};

pub const MAGIC: u32 = 0x4846_5047; // "GPFH"
/// No slot (a ray that hit no body of a name, no exclusion).
pub const NO_SLOT: u32 = u32::MAX;

/// The link's block of the contract `abi.physics.link`: its layout checked field by field, and its check.
mod block;
/// The codes of the link (orders, flags, slot info and kind).
mod codes;
pub use block::drop_sim;
use block::sim_of;
pub use codes::{flag, info, kind, order};

/// The published set (blocks of the contracts `abi.physics.bodies`, `moving`, `slots`) and the contracts a side that
/// serves declares.
mod published;
pub use published::*;

/// The physics written in place on the user's thread (no copy of a buffer: the user's buffer IS the one written).
pub const LIVE: u32 = 0;
/// The physics on a loop of its own (the triple buffer).
pub const LOOP: u32 = 1;

/// The link itself (contract `abi.physics.link`, core/abi/contracts/abi.physics.link.json): after the loop's head, a
/// `TableHead` and one row — [`PhysicsLink`] from `table` on, checked field by field below.
pub use gripforge_contracts::abi_physics_link as link_block;
use gripforge_contracts::table_head::{Refused, TableHead};

/// The link of a physics: in the memory both share, made by the user ([`PhysicsLink::make`]). The generic head of a
/// loop first ([`Head`]: the word `wake` the loops sleep on, at the same place in every loop's link), then a block of
/// the contract `abi.physics.link` (its `TableHead`, one row): the side that serves checks it once
/// ([`PhysicsLink::check`]) before it reads the link in place. The addresses are u64 in wasm32 as natively.
///
/// History: until feat/abi-contrat-lot2c (20/11) no `TableHead`, `set`, `sim`, `shapes` and the addresses of the rings
/// and the answers in `usize` (4 bytes in wasm32), `skipped` and `dropped_ms` two former padding words; rebuild
/// kithost.core, the native player and the physics modules (rapier, null).
#[repr(C)]
pub struct PhysicsLink {
    pub head: Head,
    /// The `TableHead` of the `abi.physics.link` block (one row), written by [`PhysicsLink::make`].
    pub table: TableHead,
    pub orders: Ring,
    /// Contact events, from the physics to the user: `[a slot u32 | b slot u32][begin u32 | 0][impulse f64]`.
    pub events: Ring,
    pub answers: Answers,
    /// The address of the set the physics writes (it changes it on [`order::GROW`]).
    pub set: AtomicU64,
    /// The number of the last order the physics took.
    pub applied: AtomicU32,
    /// [`LIVE`] or [`LOOP`].
    pub mode: u32,
    pub fixed_dt: f64,
    pub max_steps: u32,
    /// Events dropped because the user did not take them in time.
    pub events_dropped: AtomicU32,
    /// The address of the physics' own state ([`Sim`]), made by the side that serves (0: not yet).
    pub sim: AtomicU64,
    /// Counters of the loop: steps, and the ms it spent stepping and serving (f64 bits).
    pub steps: AtomicU32,
    /// Fixed steps due and never played under saturation (a turn already played `max_steps`), counted; and below, the
    /// physics time thrown away, ms (those steps, a gap over 0.25 s beyond it). Two former padding words.
    pub skipped: AtomicU32,
    pub busy_ms: std::sync::atomic::AtomicU64,
    /// What the port says of itself once: `ground_clearance` (f64 bits) and whether it steps by hand.
    pub clearance_bits: std::sync::atomic::AtomicU64,
    pub manual: AtomicU32,
    pub dropped_ms: AtomicU32,
    /// The address of the served port's shape table (ABI physique 2.6), told once by the side that serves (0: none
    /// yet, or a port that publishes none): what a user on another thread reads in place.
    pub shapes: AtomicU64,
}

/// What a link owns besides itself (freed with it).
#[allow(dead_code)]
struct Blocks {
    orders: Box<[u64]>,
    events: Box<[u64]>,
    answers: Box<[u64]>,
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
        // An answer row is plain numbers and atomics: zero is FREE (the block's TableHead written).
        let records = answers.min(crate::ports::link::ANSWERS_MAX);
        let mut recs = Answers::block(records);
        let link = PhysicsLink {
            head: Head {
                magic: MAGIC,
                control: AtomicU32::new(if mode == LIVE { RUN } else { PAUSE }),
                wake: AtomicU32::new(0),
                served: AtomicU32::new(0),
            },
            table: TableHead::default(),
            orders: Ring::new(orders.as_mut_ptr(), order_words),
            events: Ring::new(events.as_mut_ptr(), event_words),
            answers: Answers {
                recs: recs.as_mut_ptr() as usize as u64,
                cap: records,
                cursor: AtomicU32::new(0),
            },
            set: AtomicU64::new(PubSet::make(cap) as usize as u64),
            applied: AtomicU32::new(0),
            mode,
            fixed_dt,
            max_steps: 4,
            events_dropped: AtomicU32::new(0),
            sim: AtomicU64::new(0),
            steps: AtomicU32::new(0),
            skipped: AtomicU32::new(0),
            busy_ms: Default::default(),
            clearance_bits: Default::default(),
            manual: AtomicU32::new(0),
            dropped_ms: AtomicU32::new(0),
            shapes: AtomicU64::new(0),
        };
        // The blocks live as long as the link (kept beside it, freed by `free`).
        let blocks = Box::new(Blocks {
            orders,
            events,
            answers: recs,
        });
        let both = Box::into_raw(Box::new((link, blocks))) as *mut PhysicsLink;
        // SAFETY: the link just made, read by nobody yet; its block (TableHead, one row) begins at `table`.
        unsafe {
            let block = std::ptr::addr_of_mut!((*both).table).cast::<u8>();
            link_block::init(block, 1);
            link_block::publish(block, 1);
        }
        both
    }

    /// Frees a link and its current set (the physics no longer serves it: quit, or this thread's).
    ///
    /// # Safety
    /// A link of [`PhysicsLink::make`], used by nobody now.
    pub unsafe fn free(link: *mut PhysicsLink) {
        let both = Box::from_raw(link as *mut (PhysicsLink, Box<Blocks>));
        let set = both.0.set.load(Acquire) as usize;
        if set != 0 {
            PubSet::free(set as *mut PubSet);
        }
        // Answers still holding names past their record.
        let a = &both.0.answers;
        for i in 0..a.cap {
            let r = a.rec(i);
            if r.more != 0 {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    r.more as usize as *mut u32,
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
    /// The numbers as `f64`, in place (the words of a contract of words, read by its generated accessors).
    pub fn f64s(&self) -> &'a [f64] {
        // SAFETY: a u64 and an f64 have the same size and alignment; the numbers were written as `f64::to_bits`.
        unsafe { std::slice::from_raw_parts(self.nums.as_ptr() as *const f64, self.nums.len()) }
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
    /// The static bodies an order moved (`SET_POSITION`, `SET_ROTATION`, `SET_BODY`): the writer publishes every body it
    /// changed, whatever its kind, so their pose is in each publication from then on (its revision raised once).
    placed: BTreeSet<u32>,
    posed: Vec<(u32, [f64; 7], bool)>,
    /// Per slot, the last pose published and its revision (`bodies::Row::rev`); the moves counted (`bodies::Head::moves`).
    last_pose: Vec<([f64; 7], u32)>,
    /// Per slot, the turn of [`Sim::list_poses`] that listed it (the moving bodies the port left out are published too).
    listed: Vec<u32>,
    turn: u32,
    moves: u32,
    set: usize,
    /// The set taken does not hold this build's rows ([`PubSet::check`]): nothing is written into it (the user reads
    /// no publication: rebuild the side that serves, or the user, as the refusal says).
    refused: Option<gripforge_contracts::table_head::Refused>,
    stamp: u32,
    accumulator: f64,
    fixed_dt: Option<f64>,
    /// The speed of the time played on a loop ([`order::SET_SPEED`]; 1: real time).
    speed: f64,
    /// Held while the page is hidden ([`order::SET_HELD`]).
    held: bool,
    live_forces: Vec<LiveForce>,
    force_slots: Vec<u32>,
    last_ms: f64,
    time: f64,
    steps: f64,
    manual: bool,
    unknown: u32,
}

impl Sim {
    /// The state of a side that serves a link whose block was checked (`refused`: why it may not be served).
    fn new(set: usize, refused: Option<Refused>) -> Sim {
        Sim {
            names: Vec::new(),
            kinds: Vec::new(),
            watched: Vec::new(),
            by_name: HashMap::new(),
            moving: BTreeSet::new(),
            placed: BTreeSet::new(),
            posed: Vec::new(),
            last_pose: Vec::new(),
            listed: Vec::new(),
            turn: 0,
            moves: 0,
            // SAFETY: the set of the link, made by its user (read once the link's block was checked).
            refused: refused.or_else(|| unsafe { (*(set as *const PubSet)).check() }.err()),
            set,
            stamp: 1,
            accumulator: 0.0,
            fixed_dt: None,
            speed: 1.0,
            held: false,
            live_forces: Vec::new(),
            force_slots: Vec::new(),
            last_ms: f64::NAN,
            time: 0.0,
            steps: 0.0,
            manual: false,
            unknown: 0,
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
            self.last_pose.resize(n, ([f64::NAN; 7], 0));
            self.listed.resize(n, 0);
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
        if slot >= set.cap || self.refused.is_some() {
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
        let stamp = unsafe { PubSet::head(buf) }.seq;
        // A record of this publication keeps its place in the list; its pose follows the body.
        if r.stamp == stamp && r.flags & flag::POSE != 0 {
            let character = r.flags & flag::POSE_CHARACTER;
            flags |= flag::POSE | character;
            if character != 0 && flags & flag::CHARACTER_AT != 0 {
                r.pose[..3].copy_from_slice(&r.cp);
            } else if flags & flag::BODY != 0 {
                r.pose = pose_of(r.p, r.q);
            }
        }
        r.flags = flags;
        r.stamp = stamp;
    }

    /// Writes the whole buffer: every moving body, the list, the head.
    fn write_all(&mut self, p: &dyn PhysicsPort, buf: usize, applied: u32, wall_ms: f64) {
        // SAFETY: a buffer of the set.
        if self.refused.is_some() {
            return;
        }
        let head = unsafe { PubSet::head(buf) };
        self.stamp = self.stamp.wrapping_add(1).max(1);
        head.seq = self.stamp;
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
        let mut n = 0u32;
        let mut posed = std::mem::take(&mut self.posed);
        posed.clear();
        self.turn = self.turn.wrapping_add(1).max(1);
        let (turn, listed) = (self.turn, &mut self.listed);
        p.for_each_pose(&mut |name, at, yaw, character| {
            let slot = self.by_name.get(name).copied().unwrap_or(NO_SLOT);
            if slot < set.cap {
                posed.push((slot, listed_pose(p, name, at, yaw, character), character));
                if let Some(l) = listed.get_mut(slot as usize) {
                    *l = turn;
                }
            }
        });
        self.post_unlisted(p, set.cap, turn, &mut posed);
        for &(slot, pose, character) in &posed {
            if !self.moving.contains(&slot) {
                self.write_rec(p, buf, slot);
            }
            // SAFETY: `slot` < cap.
            let r = unsafe { set.rec(buf, slot) };
            r.pose = pose;
            // The body's revision: raised when its pose changed since the last publication.
            if let Some(last) = self.last_pose.get_mut(slot as usize) {
                if last.0 != pose {
                    last.0 = pose;
                    last.1 = last.1.wrapping_add(1);
                    self.moves = self.moves.wrapping_add(1);
                }
                r.rev = last.1;
            }
            r.flags |= flag::POSE | if character { flag::POSE_CHARACTER } else { 0 };
            // SAFETY: `n` < cap (one row per slot listed, each below the cap); this side writes `buf`.
            unsafe { PubSet::list_put(buf, n, slot) };
            n += 1;
        }
        self.posed = posed;
        // SAFETY: as above; the count published after the rows.
        unsafe { PubSet::list_publish(buf, n) };
        head.moves = self.moves;
    }

    /// The info of a slot, after its body was added.
    fn write_info(&self, p: &dyn PhysicsPort, slot: u32, seq: u32) {
        let set = self.set();
        if slot >= set.cap || self.refused.is_some() {
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
                self.placed.remove(&other);
            }
        }
        self.placed.remove(&slot);
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
                use crate::ports::abi::character_desc as cd;
                let w = o.f64s();
                cd::check(w).ok()?;
                self.named(o.slot, o.name(), kind::CHARACTER);
                let opts = crate::ports::CharacterOptions::of(
                    cd::max_slope_deg(w),
                    cd::step_height(w),
                    cd::snap_distance(w),
                    cd::skin_ratio(w),
                );
                p.add_character(
                    o.name(),
                    cd::position(w),
                    cd::radius(w),
                    cd::height(w),
                    opts,
                );
                self.write_info(p, o.slot, o.seq);
                Some(o.slot)
            }
            ADD_STATIC_BOX => {
                use crate::ports::abi::box_desc as bx;
                let w = o.f64s();
                bx::check(w).ok()?;
                self.named(o.slot, o.name(), kind::STATIC);
                p.add_static_box(bx::center(w), bx::size(w), o.name());
                self.write_info(p, o.slot, o.seq);
                Some(o.slot)
            }
            ADD_DYNAMIC_BOX => {
                use crate::ports::abi::box_desc as bx;
                let w = o.f64s();
                bx::check(w).ok()?;
                self.named(o.slot, o.name(), kind::DYNAMIC);
                p.add_dynamic_box(o.name(), bx::center(w), bx::size(w), bx::mass(w));
                self.write_info(p, o.slot, o.seq);
                Some(o.slot)
            }
            HOLD_BODY => {
                let n = name(self)?;
                p.hold_body(&n, o.num(0) as u32);
                None
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
                self.placed.remove(&o.slot);
                let set = self.set();
                if o.slot < set.cap {
                    // SAFETY: `slot` < cap.
                    unsafe { *set.info(o.slot) = slots::Row::default() };
                }
                Some(o.slot)
            }
            SET_BODY => {
                let n = name(self)?;
                use crate::ports::abi::body_state as bs;
                let w = o.f64s();
                bs::check(w).ok()?;
                let s = crate::ports::BodyState {
                    entity: n,
                    p: bs::position(w),
                    q: bs::rotation(w),
                    v: bs::linvel(w),
                    w: bs::angvel(w),
                    sleeping: bs::sleeping(w) != 0.0,
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
            SET_HELD => self.set_held(o.num(0) != 0.0),
            SET_SPEED => self.set_speed(o.num(0)),
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
            _ => unknown::unknown(&mut self.unknown, k),
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

    /// A static body moved by an order: its info follows (a static body is not published every step).
    fn refresh_static(&mut self, p: &dyn PhysicsPort, slot: u32) {
        if self.kinds.get(slot as usize) == Some(&kind::STATIC) && slot < self.set().cap {
            // Published from now on: the writer publishes every body an order changed.
            self.placed.insert(slot);
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
        // SAFETY: a set made by the user; checked once, before anything is written into it.
        let was = std::mem::replace(&mut self.refused, unsafe { new.check() }.err());
        if was.is_some() || self.refused.is_some() {
            self.set = to;
            return;
        }
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
        link.set.store(to as u64, Release);
    }
}

/// The physics' side of a [`LIVE`] link, on the user's thread: takes every order waiting, each written in place at
/// once (the user's buffer is the one written). Answers the orders taken.
///
/// # Safety
/// `link` a live link of [`PhysicsLink::make`]; this thread serves it alone.
pub unsafe fn serve_live(link: &PhysicsLink, p: &mut dyn PhysicsPort) -> u32 {
    let sim = sim_of(link);
    if sim.set == 0 {
        // The link's own block was refused: nothing of it is read (its fields are not where this build reads them).
        return 0;
    }
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
                } else if sim.placed.contains(&slot) {
                    // A static body an order moved: listed with its pose from now on.
                    relist = true;
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
            link.throw(dt - 0.25, 0);
            let mut remaining = dt.clamp(0.0, 0.25); // never × sim.speed: dt is game time, already slowed
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
    if sim.set == 0 {
        // The link's own block was refused (`PhysicsLink::check`): nothing of it is read; the loop waits for its quit.
        return 250.0;
    }
    link.head.served.fetch_add(1, Relaxed);
    if link.clearance_bits.load(Relaxed) == 0 {
        link.clearance_bits
            .store(p.ground_clearance().to_bits() | 1 << 63, Relaxed);
    }
    if link.shapes.load(Relaxed) == 0 {
        if let Some(at) = p.shapes_view() {
            link.shapes.store(at as u64, Release);
        }
    }
    let mut changed = link.orders.drain(|k, body| {
        let o = Read::of(body);
        sim.take(p, link, k, &o);
        link.applied.store(o.seq, Release);
    }) > 0;
    let dt = sim.fixed_dt.unwrap_or(link.fixed_dt);
    let run = control == RUN && !sim.held;
    if sim.last_ms.is_nan() || !run {
        sim.last_ms = now_ms;
    }
    if run && !sim.manual {
        let gap = ((now_ms - sim.last_ms) / 1000.0).max(0.0);
        link.throw(gap - 0.25, 0);
        sim.accumulator += gap.min(0.25) * sim.speed;
        sim.last_ms = now_ms;
        let mut n = 0;
        while sim.accumulator >= dt - 1e-9 {
            if n >= link.max_steps {
                // Saturated: the steps still due are thrown away (the physics' budget), and counted.
                let skipped = ((sim.accumulator + 1e-9) / dt).floor();
                link.throw(skipped * dt, skipped as u32);
                sim.accumulator = (sim.accumulator - skipped * dt).max(0.0);
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
