//! The link of a module that runs **asynchronously** on a thread of its own (docs/kit-modules.md §7.17): the base
//! rule of the worker mode (owner, 2026-10-25). The module runs its own loop on the data of the shared memory and
//! writes its results there all the time; the thread that uses it reads them where they are, at any time, without
//! ever waiting for it or waking it; to act on it, it SENDS AN ORDER, and an order that asks something gets a
//! **ticket** whose answer is read as soon as the module took the order.
//!
//! The same link serves when the module runs on the user's thread (a page whose memory is not shared, a native
//! engine, the tests): the orders are then taken at once and the answers are there when the ticket is handed out.
//! What a user writes does not change between the two.
//!
//! Three generic pieces, all in one block of memory both threads share, none of them a lock:
//!
//! - [`Ring`]: the orders, one writer (the user) and one reader (the module), records of 8-byte words that never
//!   wrap. The writer never waits: when the ring is full the order waits in the writer's own backlog (in order).
//! - [`Answers`]: the answers of the tickets, a slab of fixed records. The user takes a free record (the ticket),
//!   the module fills it and marks it ready; the user reads it in place and frees it.
//! - [`Triple`]: the state the module publishes (three buffers: the module writes one, the user reads one, the
//!   third is the latest complete one). Neither side ever waits for the other; the user reads a consistent whole.
//!   [`Quad`] adds a fourth: the user also keeps its previous publication, stable while it reads it (the physics'
//!   published state, docs/decisions/2026-11-18-proprietaire-position.md).
//!
//! A module that wants its loop driven from JavaScript exposes `entry(link, now_ms) -> wait_ms` and the Worker calls
//! it in a loop, sleeping `wait_ms` on the word [`Head::WAKE`] between two calls (`web/module-calls-worker.mjs`
//! `runLoop`): a negative answer ends the loop.

use std::sync::atomic::{AtomicU32, Ordering::*};

/// The head every link starts with, at fixed offsets (JavaScript reads [`Head::WAKE`] and [`Head::CONTROL`]).
#[repr(C)]
pub struct Head {
    pub magic: u32,
    /// [`RUN`], [`PAUSE`] or [`QUIT`]: written by the user, read by the module's loop.
    pub control: AtomicU32,
    /// Moves on every order written (the ring's write position): the word the loop sleeps on.
    pub wake: AtomicU32,
    /// Loops of the module (each `entry` call), for the counters.
    pub served: AtomicU32,
}

impl Head {
    /// Byte offset of `control` in a link.
    pub const CONTROL: usize = 4;
    /// Byte offset of `wake` in a link.
    pub const WAKE: usize = 8;
}

pub const RUN: u32 = 0;
pub const PAUSE: u32 = 1;
pub const QUIT: u32 = 2;

// ─── the ring of orders ──────────────────────────────────────────────────────

/// A single-producer single-consumer ring of records in shared memory. A record is a whole number of 8-byte words:
/// `[u32 words][u32 kind]` then its payload; it never wraps (a record [`Ring::PAD`] fills the end when the next one
/// does not fit there). Positions count words since the start and wrap at `u32`.
///
/// Laid as the contract of the block that holds it says (the physics link: `orders_*`, `events_*` of
/// `abi.physics.link`, checked field by field in physics_link.rs): the address a u64 in wasm32 as natively.
#[repr(C)]
pub struct Ring {
    /// The address of its words.
    pub buf: u64,
    /// Words; a power of two.
    pub cap: u32,
    /// Read position (the consumer's).
    pub head: AtomicU32,
    /// Write position (the producer's).
    pub tail: AtomicU32,
}

impl Ring {
    pub const PAD: u32 = u32::MAX;

    /// A ring over `cap` words at `buf` (`cap` a power of two).
    pub fn new(buf: *mut u64, cap: u32) -> Ring {
        debug_assert!(cap.is_power_of_two());
        Ring {
            buf: buf as usize as u64,
            cap,
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    fn word(&self, pos: u32) -> *mut u64 {
        (self.buf as usize as *mut u64).wrapping_add((pos & (self.cap - 1)) as usize)
    }

    /// Writes one record of `words` words (header included) filled by `fill` (its slice starts after the header);
    /// `false` when it does not fit now (the producer keeps it and tries again later).
    ///
    /// # Safety
    /// One producer; `buf` valid for `cap` words.
    pub unsafe fn push(&self, kind: u32, words: u32, fill: impl FnOnce(&mut [u64])) -> bool {
        let words = words.max(1);
        if words > self.cap / 2 {
            return false;
        }
        let (head, tail) = (self.head.load(Acquire), self.tail.load(Relaxed));
        let at = tail & (self.cap - 1);
        let room_end = self.cap - at;
        let pad = if room_end < words { room_end } else { 0 };
        if self.cap - tail.wrapping_sub(head) < pad + words {
            return false;
        }
        let mut tail = tail;
        if pad != 0 {
            self.word(tail).write(header(pad, Ring::PAD));
            tail = tail.wrapping_add(pad);
        }
        let rec = self.word(tail);
        rec.write(header(words, kind));
        let body = std::slice::from_raw_parts_mut(rec.add(1), words as usize - 1);
        fill(body);
        self.tail.store(tail.wrapping_add(words), Release);
        true
    }

    /// Takes every record written so far, in order: `take(kind, body)`.
    ///
    /// # Safety
    /// One consumer; `buf` valid for `cap` words.
    pub unsafe fn drain(&self, mut take: impl FnMut(u32, &[u64])) -> u32 {
        let tail = self.tail.load(Acquire);
        let mut head = self.head.load(Relaxed);
        let mut n = 0;
        while head != tail {
            let h = self.word(head).read();
            let (words, kind) = ((h & 0xffff_ffff) as u32, (h >> 32) as u32);
            if words == 0 {
                break;
            }
            if kind != Ring::PAD {
                take(
                    kind,
                    std::slice::from_raw_parts(self.word(head).add(1), words as usize - 1),
                );
                n += 1;
            }
            head = head.wrapping_add(words);
            // Freed record by record: the producer may write behind.
            self.head.store(head, Release);
        }
        n
    }

    /// Whether something waits to be taken.
    pub fn pending(&self) -> bool {
        self.head.load(Acquire) != self.tail.load(Acquire)
    }
}

fn header(words: u32, kind: u32) -> u64 {
    words as u64 | (kind as u64) << 32
}

// ─── the answers of the tickets ─────────────────────────────────────────────

/// The rows of [`Answers`] (contract `abi.physics.answers`, core/abi/contracts/abi.physics.answers.json): one block,
/// a `TableHead` then a row per ticket record, read and written at the stride the block announces.
pub use gripforge_contracts::abi_physics_answers as answers;
use gripforge_contracts::table_head::{Refused, TableHead, Why};

/// A row of [`Answers`]: an answer of the module, read in place. Its layout is the contract's row (`answers::Row`,
/// checked below field by field); it is its own type for the atomic state and the methods.
#[repr(C)]
pub struct AnswerRec {
    /// [`FREE`], [`ASKED`] or [`READY`].
    pub state: AtomicU32,
    /// Moves each time the record is taken again (the ticket's check).
    pub generation: u32,
    /// The kind of the question (the module's numbering).
    pub kind: u32,
    /// What the answer counts (found or not, the number of names).
    pub found: u32,
    pub nums: [f64; 10],
    /// Names found, as the module's slot numbers; past [`AnswerRec::SLOTS`], the rest at `more` (`more_len`
    /// numbers the module allocated; the user frees them when it frees the record).
    pub first_slots: [u32; AnswerRec::SLOTS],
    pub more: u64,
    pub more_len: u32,
    pub _pad: u32,
}
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<AnswerRec>() == answers::STRIDE as usize);
    assert!(offset_of!(AnswerRec, state) == answers::at::STATE);
    assert!(offset_of!(AnswerRec, generation) == answers::at::GENERATION);
    assert!(offset_of!(AnswerRec, kind) == answers::at::KIND);
    assert!(offset_of!(AnswerRec, found) == answers::at::FOUND);
    assert!(offset_of!(AnswerRec, nums) == answers::at::NUMS);
    assert!(offset_of!(AnswerRec, first_slots) == answers::at::FIRST_SLOTS);
    assert!(offset_of!(AnswerRec, more) == answers::at::MORE);
    assert!(offset_of!(AnswerRec, more_len) == answers::at::MORE_LEN);
};

impl AnswerRec {
    pub const SLOTS: usize = 22;
}

pub const FREE: u32 = 0;
pub const ASKED: u32 = 1;
pub const READY: u32 = 2;

/// A slab of answers: the `abi.physics.answers` block at `recs` (its `TableHead`, then `cap` rows). Laid as the
/// `answers*` fields of `abi.physics.link` (checked in physics_link.rs).
#[repr(C)]
pub struct Answers {
    /// The address of the block (u64 in wasm32 as natively).
    pub recs: u64,
    pub cap: u32,
    /// The user's search position (only the user touches it).
    pub cursor: AtomicU32,
}

/// A ticket: the record (low 12 bits) and its generation (high 20 bits). 0 is no ticket.
pub const TICKET_RECORD_BITS: u32 = 12;
pub const ANSWERS_MAX: u32 = 1 << TICKET_RECORD_BITS;

impl Answers {
    /// # Safety
    /// `recs` valid for `cap` records, `cap` ≤ [`ANSWERS_MAX`].
    #[allow(
        clippy::mut_from_ref,
        reason = "the records live in memory shared between threads, reached through a raw pointer: the caller owns the record by its ticket (see # Safety)"
    )]
    pub unsafe fn rec(&self, i: u32) -> &mut AnswerRec {
        let recs = self.recs as usize;
        let h = &*(recs as *const TableHead);
        &mut *((recs + h.head_bytes as usize + i as usize * h.stride as usize) as *mut AnswerRec)
    }

    /// A block of `cap` rows, zeroed (every row [`FREE`]), its `TableHead` written: the user's, freed with the link.
    pub fn block(cap: u32) -> Box<[u64]> {
        let mut b = vec![0u64; answers::bytes(cap).div_ceil(8)].into_boxed_slice();
        // SAFETY: the block just allocated, aligned on 8, of the bytes of `cap` rows; nobody reads it yet.
        unsafe {
            answers::init(b.as_mut_ptr() as *mut u8, cap);
            answers::publish(b.as_mut_ptr() as *mut u8, cap);
        }
        b
    }

    /// Checks once, when a side takes a link it did not make, that the block is of its contract and holds this
    /// build's rows: they are then read and written in place. A refusal names the contract and what does not fit.
    ///
    /// # Safety
    /// `recs` a block made by [`Answers::block`] of any build.
    pub unsafe fn check(&self) -> Result<(), Refused> {
        let h = TableHead::read(self.recs as usize as *const u8);
        h.check(&answers::SPEC)?;
        if (h.stride as usize) < std::mem::size_of::<AnswerRec>() {
            return Err(Refused {
                contract: answers::SPEC.name,
                why: Why::Stride(h.stride),
            });
        }
        Ok(())
    }

    /// Takes a free record for a question of `kind`: its ticket, or 0 when none is free.
    ///
    /// # Safety
    /// The user's thread only.
    pub unsafe fn take(&self, kind: u32) -> u32 {
        let cursor = self.cursor.load(Relaxed);
        for k in 0..self.cap {
            let i = (cursor + k) % self.cap;
            let r = self.rec(i);
            if r.state.load(Acquire) == FREE {
                r.generation = (r.generation + 1) & ((1 << (32 - TICKET_RECORD_BITS)) - 1);
                if r.generation == 0 {
                    r.generation = 1;
                }
                r.kind = kind;
                r.found = 0;
                r.state.store(ASKED, Relaxed);
                self.cursor.store((i + 1) % self.cap, Relaxed);
                return r.generation << TICKET_RECORD_BITS | i;
            }
        }
        0
    }

    /// The record of a ticket while it is the ticket's (`None`: no ticket, or already freed).
    ///
    /// # Safety
    /// As [`Answers::rec`].
    #[allow(
        clippy::mut_from_ref,
        reason = "the records live in memory shared between threads, reached through a raw pointer: the caller owns the record by its ticket (see # Safety)"
    )]
    pub unsafe fn of(&self, ticket: u32) -> Option<&mut AnswerRec> {
        let i = ticket & (ANSWERS_MAX - 1);
        if ticket == 0 || i >= self.cap {
            return None;
        }
        let r = self.rec(i);
        (r.generation == ticket >> TICKET_RECORD_BITS && r.state.load(Acquire) != FREE).then_some(r)
    }

    /// The module's side: the record of a ticket it was given, to fill (then [`Answers::ready`]).
    ///
    /// # Safety
    /// As [`Answers::rec`]; the module's thread.
    #[allow(
        clippy::mut_from_ref,
        reason = "the records live in memory shared between threads, reached through a raw pointer: the caller owns the record by its ticket (see # Safety)"
    )]
    pub unsafe fn to_fill(&self, ticket: u32) -> Option<&mut AnswerRec> {
        let i = ticket & (ANSWERS_MAX - 1);
        (ticket != 0 && i < self.cap).then(|| self.rec(i))
    }

    /// The module's side: the answer is written.
    pub fn ready(r: &AnswerRec) {
        r.state.store(READY, Release);
    }

    /// The user's side: frees a record it read (the names past the record go back to the allocator).
    pub fn free(r: &mut AnswerRec) {
        if r.more != 0 {
            // SAFETY: allocated by the module as a boxed slice of `more_len` numbers (`AnswerRec::set_slots`), by the
            // allocator every module of the memory shares.
            unsafe {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    r.more as usize as *mut u32,
                    r.more_len as usize,
                )))
            };
            r.more = 0;
            r.more_len = 0;
        }
        r.state.store(FREE, Release);
    }
}

impl AnswerRec {
    /// The module's side: the names found (slot numbers), in the record and past it.
    pub fn set_slots(&mut self, found: &[u32]) {
        self.found = found.len() as u32;
        let here = found.len().min(AnswerRec::SLOTS);
        self.first_slots[..here].copy_from_slice(&found[..here]);
        if found.len() > here {
            let rest: Box<[u32]> = found[here..].into();
            self.more_len = rest.len() as u32;
            self.more = Box::into_raw(rest) as *mut u32 as usize as u64;
        }
    }
    /// The user's side: the names found.
    pub fn slots_found(&self) -> impl Iterator<Item = u32> + '_ {
        let here = (self.found as usize).min(AnswerRec::SLOTS);
        // SAFETY: `more_len` numbers at `more` when it is set (`set_slots`).
        let rest: &[u32] = if self.more != 0 {
            unsafe {
                std::slice::from_raw_parts(self.more as usize as *const u32, self.more_len as usize)
            }
        } else {
            &[]
        };
        self.first_slots[..here]
            .iter()
            .copied()
            .chain(rest.iter().copied())
    }
}

// ─── the triple buffer of the published state ─────────────────────────────────

/// Three buffers and the index of the latest complete one. The module (the writer) fills its own, then swaps it
/// with the latest ([`Triple::publish`]); the user (the reader) swaps its own for the latest when there is a newer
/// one ([`Triple::latch`]). Neither waits; the reader's buffer never changes under it.
#[repr(C)]
pub struct Triple {
    pub bufs: [usize; 3],
    /// The latest index, and [`Triple::FRESH`] when the reader has not taken it.
    pub latest: AtomicU32,
    /// The writer's index (only the writer touches it).
    pub back: AtomicU32,
    /// The reader's index (only the reader touches it).
    pub front: AtomicU32,
    pub _pad: u32,
}

impl Triple {
    pub const FRESH: u32 = 4;

    pub fn new(bufs: [usize; 3]) -> Triple {
        Triple {
            bufs,
            latest: AtomicU32::new(1),
            back: AtomicU32::new(0),
            front: AtomicU32::new(2),
            _pad: 0,
        }
    }

    /// The writer's buffer.
    pub fn back(&self) -> usize {
        self.bufs[self.back.load(Relaxed) as usize]
    }

    /// The writer: its buffer is complete.
    pub fn publish(&self) {
        let old = self
            .latest
            .swap(self.back.load(Relaxed) | Triple::FRESH, AcqRel);
        self.back.store(old & 3, Relaxed);
    }

    /// The reader: takes the latest buffer when there is a newer one; answers whether it did.
    pub fn latch(&self) -> bool {
        if self.latest.load(Acquire) & Triple::FRESH == 0 {
            return false;
        }
        let old = self.latest.swap(self.front.load(Relaxed), AcqRel);
        self.front.store(old & 3, Relaxed);
        true
    }

    /// The reader's buffer.
    pub fn front(&self) -> usize {
        self.bufs[self.front.load(Relaxed) as usize]
    }
}

/// Four buffers: the [`Triple`]'s three, and the reader's **previous** one, kept as it was while the reader holds the
/// current one — two dated states read in place, never copied (docs/decisions/2026-11-18-proprietaire-position.md).
/// The writer's side is the triple buffer's ([`Quad::back`], [`Quad::publish`]); at a latch, the reader's current
/// buffer becomes its previous one and the previous one goes back to the writer. Laid as the `bufs`, `latest`,
/// `back`, `front`, `prev` of `abi.physics.set` (checked in physics_link/published.rs): addresses as u64.
#[repr(C)]
pub struct Quad {
    pub bufs: [u64; 4],
    /// The latest index, and [`Triple::FRESH`] when the reader has not taken it.
    pub latest: AtomicU32,
    /// The writer's index (only the writer touches it).
    pub back: AtomicU32,
    /// The reader's current index, and its previous one (only the reader touches them).
    pub front: AtomicU32,
    pub prev: AtomicU32,
}

impl Quad {
    pub fn new(bufs: [usize; 4]) -> Quad {
        Quad {
            bufs: bufs.map(|b| b as u64),
            latest: AtomicU32::new(1),
            back: AtomicU32::new(0),
            front: AtomicU32::new(2),
            prev: AtomicU32::new(3),
        }
    }

    /// The writer's buffer.
    pub fn back(&self) -> usize {
        self.bufs[self.back.load(Relaxed) as usize] as usize
    }

    /// The writer: its buffer is complete.
    pub fn publish(&self) {
        let old = self
            .latest
            .swap(self.back.load(Relaxed) | Triple::FRESH, AcqRel);
        self.back.store(old & 3, Relaxed);
    }

    /// The reader: takes the latest buffer when there is a newer one (its current one becomes its previous one, the
    /// previous one goes to the writer); answers whether it did.
    pub fn latch(&self) -> bool {
        if self.latest.load(Acquire) & Triple::FRESH == 0 {
            return false;
        }
        let old = self.latest.swap(self.prev.load(Relaxed), AcqRel);
        self.prev.store(self.front.load(Relaxed), Relaxed);
        self.front.store(old & 3, Relaxed);
        true
    }

    /// The reader's current buffer.
    pub fn front(&self) -> usize {
        self.bufs[self.front.load(Relaxed) as usize] as usize
    }

    /// The reader's previous buffer (the one it held before its last latch), unchanged while it holds it.
    pub fn prev(&self) -> usize {
        self.bufs[self.prev.load(Relaxed) as usize] as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_order_and_never_wraps_a_record() {
        let mut buf = vec![0u64; 16];
        let r = Ring::new(buf.as_mut_ptr(), 16);
        let mut seen = Vec::new();
        for round in 0..50u64 {
            for k in 0..3u64 {
                let n = 1 + ((round + k) % 4) as u32;
                // SAFETY: the buffer above.
                assert!(unsafe {
                    r.push(7, n + 1, |b| b.iter_mut().for_each(|w| *w = round * 10 + k))
                });
            }
            unsafe { r.drain(|kind, body| seen.push((kind, body.to_vec()))) };
        }
        assert_eq!(seen.len(), 150);
        assert!(seen.iter().enumerate().all(|(i, (kind, b))| *kind == 7
            && b.iter().all(|w| *w == (i as u64 / 3) * 10 + i as u64 % 3)));
        // Full: refused, nothing lost.
        let mut n = 0;
        while unsafe { r.push(1, 4, |_| {}) } {
            n += 1;
        }
        assert!(n >= 3, "the end of the ring padded at most once");
        assert_eq!(unsafe { r.drain(|_, _| {}) }, n);
    }

    #[test]
    fn the_answers_are_read_at_the_stride_of_their_block_and_a_narrower_one_is_refused() {
        // A writer with a field added at the end (stride + 8): accepted, every row read at its stride.
        let wide = answers::STRIDE + 8;
        let mut b = vec![0u64; (answers::HEAD_BYTES + 2 * wide).div_ceil(8) as usize];
        let at = b.as_mut_ptr() as usize;
        unsafe {
            answers::init(at as *mut u8, 2);
            (*(at as *mut TableHead)).stride = wide;
        }
        let a = Answers {
            recs: at as u64,
            cap: 2,
            cursor: AtomicU32::new(0),
        };
        unsafe {
            assert!(a.check().is_ok());
            let t = a.take(5);
            assert_eq!(t & (ANSWERS_MAX - 1), 0);
            let t = a.take(6);
            a.to_fill(t).unwrap().nums[0] = 7.0;
            let second = (at + answers::HEAD_BYTES as usize + wide as usize) as *const AnswerRec;
            assert_eq!(((*second).kind, (*second).nums[0]), (6, 7.0));
            // A row narrower than this build's (a field taken away): refused by name.
            (*(at as *mut TableHead)).stride = answers::STRIDE - 8;
            let e = a.check().unwrap_err();
            assert_eq!(e.contract, "abi.physics.answers");
        }
    }

    #[test]
    fn a_ticket_answers_once_and_its_record_is_taken_again() {
        let mut recs = Answers::block(2);
        let a = Answers {
            recs: recs.as_mut_ptr() as usize as u64,
            cap: 2,
            cursor: AtomicU32::new(0),
        };
        unsafe {
            let t = a.take(3);
            assert!(a.of(t).is_some() && a.of(t).unwrap().state.load(Relaxed) == ASKED);
            let r = a.to_fill(t).unwrap();
            r.set_slots(&(0..30).collect::<Vec<u32>>());
            Answers::ready(r);
            let r = a.of(t).unwrap();
            assert_eq!(
                r.slots_found().collect::<Vec<_>>(),
                (0..30).collect::<Vec<u32>>()
            );
            Answers::free(r);
            assert!(a.of(t).is_none());
            let (t2, t3) = (a.take(1), a.take(1));
            assert!(t2 != 0 && t3 != 0 && a.take(1) == 0, "two records");
            assert_ne!(t2, t, "a record taken again is another ticket");
        }
    }

    #[test]
    fn the_fourth_buffer_keeps_the_reader_s_previous_state_out_of_the_writer_s_reach() {
        let q = Quad::new([10, 11, 12, 13]);
        let first = q.back();
        q.publish();
        assert!(q.latch());
        assert_eq!(q.front(), first);
        for _ in 0..3 {
            let next = q.back();
            assert_ne!(next, q.front(), "never the reader's current buffer");
            q.publish();
            let (cur, before) = (q.front(), q.prev());
            assert!(q.latch());
            assert_eq!((q.front(), q.prev()), (next, cur));
            assert_ne!(q.prev(), before);
            // The writer goes on: neither of the reader's two buffers is ever handed to it.
            for _ in 0..4 {
                let w = q.back();
                assert!(w != q.front() && w != q.prev());
                q.publish();
            }
        }
    }

    #[test]
    fn the_triple_buffer_gives_the_reader_the_latest_complete_buffer() {
        let t = Triple::new([10, 11, 12]);
        assert!(!t.latch());
        let w = t.back();
        t.publish();
        assert!(t.latch());
        assert_eq!(t.front(), w);
        let (a, _) = (t.back(), t.publish());
        let (b, _) = (t.back(), t.publish());
        assert_ne!(a, b);
        assert!(t.latch());
        assert_eq!(t.front(), b, "the latest of two");
        assert!(!t.latch());
    }
}
