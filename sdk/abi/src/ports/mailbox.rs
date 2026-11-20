//! The mailboxes between modules (docs/decisions/2026-11-19-canaux-directs.md): a module opens the boxes it needs, of
//! the size it needs, in the Worker it lives in; anyone writes into any box; the player only hosts the directory at
//! start and never sees a message. Neutral: no code, no capability is known here — the module and the kit share them
//! (`gripforge_kit_common::bus`).
//!
//! A box is one block of shared memory: a [`MailboxHead`] then a ring of 8-byte words. Several writers reserve their
//! record by one atomic add on [`MailboxHead::reserve`] and publish it by writing its header last; the one reader (the
//! module that opened the box) reads the records **in place** and frees them after taking them. A record is the head of
//! a message (`words`, `kind`, `from`, `tick`, `seq`, `code`), then the payload words.
//!
//! Three record contracts describe the bytes (docs/decisions/2026-11-20-abi-par-contrat.md, lot 5b; reference
//! docs/contrats-abi.md): `abi.mail.head` ([`MailboxHead`], held to it field by field in `layout.rs`),
//! `abi.mail.message` (the head of a message) and `abi.mail.table` / `abi.mail.entry` (the directory [`Table`]). Every
//! position below is a constant generated from them; none is written here.
//!
//! Nobody ever waits: a writer whose record does not fit keeps it in its own [`Outbox`] (an order is never dropped:
//! `full` counts it, said loudly once; a state value keeps only its newest per code: `late` counts the skipped one),
//! and a message sent before its box is in the [`Directory`] waits there too, then goes in order.

use gripforge_contracts::{
    abi_mail_entry as entry, abi_mail_message as msg, abi_mail_table as table,
};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};

/// What a box carries.
pub mod kind {
    /// Orders: never overwritten, never dropped.
    pub const ORDERS: u32 = 1;
    /// A state stream: only the newest value of a code counts.
    pub const STATE: u32 = 2;
}

/// The words before a message's payload: its head (`abi.mail.message`).
pub const MESSAGE_HEAD_WORDS: u32 = (msg::SIZE / 8) as u32;
/// The word of a message that publishes it: `words` and `kind` share it, stored last, atomically.
const HEADER_WORD: usize = msg::at::WORDS / 8;
const _: () = {
    assert!(msg::SIZE.is_multiple_of(8), "a message head is whole words");
    assert!(
        msg::at::KIND / 8 == HEADER_WORD,
        "words and kind share the word that publishes a message"
    );
};
const PAD: u32 = u32::MAX;
/// The first word of every box (`"MBOX"`): what a writer handed a bare address checks first.
pub const MAGIC: u32 = 0x4d42_4f58;

/// The head of a box, at the start of its block (the ring follows it). Held to its contract `abi.mail.head`
/// (`layout.rs`).
#[repr(C)]
pub struct MailboxHead {
    pub magic: u32,
    /// [`kind::ORDERS`] or [`kind::STATE`].
    pub kind: u32,
    /// Words of the ring; a power of two.
    pub cap: u32,
    /// 0.
    pub reserved: u32,
    /// Words reserved by the writers (positions wrap at `u32`).
    pub reserve: AtomicU32,
    /// Words freed by the reader.
    pub head: AtomicU32,
    /// Messages applied after their tick (a state value skipped, an order of a tick gone).
    pub late: AtomicU32,
    /// Orders that found the box full (kept by their writer, sent again in order).
    pub full: AtomicU32,
}

/// The words of a box block for a ring of `cap` words (`cap` a power of two).
pub const fn block_words(cap: u32) -> usize {
    gripforge_contracts::abi_mail_head::SIZE / 8 + cap as usize
}

/// One message read in place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Message<'a> {
    pub from: u32,
    pub tick: u32,
    pub seq: u32,
    pub code: u32,
    pub words: &'a [u64],
}

impl MailboxHead {
    /// Makes a box of `kind` over `block` (at least [`block_words`]`(cap)` words, 8-aligned).
    ///
    /// # Safety
    /// `block` valid, zeroed and owned by this box for its whole life.
    pub unsafe fn init(block: *mut u64, kind: u32, cap: u32) -> &'static MailboxHead {
        assert!(cap.is_power_of_two() && cap >= 16);
        let h = block as *mut MailboxHead;
        h.write(MailboxHead {
            magic: MAGIC,
            kind,
            cap,
            reserved: 0,
            reserve: AtomicU32::new(0),
            head: AtomicU32::new(0),
            late: AtomicU32::new(0),
            full: AtomicU32::new(0),
        });
        &*h
    }

    /// The box at `at` (an address from the directory).
    ///
    /// # Safety
    /// `at` a block made by [`MailboxHead::init`], alive.
    pub unsafe fn at(at: usize) -> &'static MailboxHead {
        let h = &*(at as *const MailboxHead);
        debug_assert_eq!(h.magic, MAGIC);
        h
    }

    fn word(&self, pos: u32) -> *mut u64 {
        let ring = (self as *const MailboxHead as *mut u64)
            .wrapping_add(gripforge_contracts::abi_mail_head::SIZE / 8);
        ring.wrapping_add((pos & (self.cap - 1)) as usize)
    }

    /// Writes one message; false when it does not fit now (nothing written, nothing reserved).
    ///
    /// # Safety
    /// The box alive.
    pub unsafe fn write(&self, from: u32, tick: u32, seq: u32, code: u32, words: &[u64]) -> bool {
        let n = MESSAGE_HEAD_WORDS + words.len() as u32;
        if n > self.cap / 2 {
            return false;
        }
        // Reserve by compare-and-swap: a failed reservation leaves no hole.
        let mut tail = self.reserve.load(Relaxed);
        let (at, pad) = loop {
            let head = self.head.load(Acquire);
            let room_end = self.cap - (tail & (self.cap - 1));
            let pad = if room_end < n { room_end } else { 0 };
            if self.cap - tail.wrapping_sub(head) < pad + n {
                return false;
            }
            match self.reserve.compare_exchange_weak(
                tail,
                tail.wrapping_add(pad + n),
                AcqRel,
                Relaxed,
            ) {
                Ok(_) => break (tail, pad),
                Err(t) => tail = t,
            }
        };
        let rec = at.wrapping_add(pad);
        let w = self.word(rec);
        let b = w as *mut u8;
        b.add(msg::at::FROM).cast::<u32>().write(from);
        b.add(msg::at::TICK).cast::<u32>().write(tick);
        b.add(msg::at::SEQ).cast::<u32>().write(seq);
        b.add(msg::at::CODE).cast::<u32>().write(code);
        std::ptr::copy_nonoverlapping(words.as_ptr(), payload(w), words.len());
        // Published by its header, written last; the pad before it the same way.
        header_of(w).store(header(n, 0), Release);
        if pad != 0 {
            header_of(self.word(at)).store(header(pad, PAD), Release);
        }
        true
    }

    /// The messages published so far, from the reader's position, up to the first one not yet published:
    /// `(position, message)`, read in place. Nothing freed.
    ///
    /// # Safety
    /// The one reader of the box.
    pub unsafe fn peek(&self, mut each: impl FnMut(u32, Message<'_>)) {
        let end = self.reserve.load(Acquire);
        let mut pos = self.head.load(Relaxed);
        while pos != end {
            let w = self.word(pos);
            let (n, k) = unheader(header_of(w).load(Acquire));
            if n == 0 {
                break; // reserved, not yet written
            }
            if k != PAD {
                let b = w as *const u8;
                let field = |at: usize| b.add(at).cast::<u32>().read();
                each(
                    pos,
                    Message {
                        from: field(msg::at::FROM),
                        tick: field(msg::at::TICK),
                        seq: field(msg::at::SEQ),
                        code: field(msg::at::CODE),
                        words: std::slice::from_raw_parts(
                            payload(w),
                            (n - MESSAGE_HEAD_WORDS) as usize,
                        ),
                    },
                );
            }
            pos = pos.wrapping_add(n);
        }
    }

    /// [`MailboxHead::peek`] for a reader that is not the box's owner (a state stream): from the owner's position
    /// as it is now; a record may be freed under it (see [`newest`]).
    ///
    /// # Safety
    /// The box alive.
    pub unsafe fn peek_shared(&self, each: impl FnMut(u32, Message<'_>)) {
        self.peek(each)
    }

    /// The words written into this box since it was opened (its traffic: wraps at `u32`).
    pub fn traffic(&self) -> u32 {
        self.reserve.load(Relaxed)
    }

    /// The words waiting to be taken.
    pub fn waiting(&self) -> u32 {
        self.reserve
            .load(Relaxed)
            .wrapping_sub(self.head.load(Relaxed))
    }

    /// Frees every record before `upto` (a position from [`MailboxHead::peek`], or past the last one taken).
    ///
    /// # Safety
    /// The one reader; no message before `upto` read any more.
    pub unsafe fn free_to(&self, upto: u32) {
        let mut pos = self.head.load(Relaxed);
        while pos != upto {
            let w = self.word(pos);
            let n = unheader(header_of(w).load(Relaxed)).0;
            if n == 0 {
                break;
            }
            // Zeroed whole: a later record may start anywhere in it, and a header read before its writer wrote it
            // must read 0, never an old word.
            for i in 0..n {
                self.word(pos.wrapping_add(i)).write(0);
            }
            pos = pos.wrapping_add(n);
        }
        self.head.store(pos, Release);
    }
}

/// The word that publishes a message: its `words` and `kind` at their places in it (`abi.mail.message`).
fn header(words: u32, kind: u32) -> u64 {
    u64::from(words) << (8 * (msg::at::WORDS % 8)) | u64::from(kind) << (8 * (msg::at::KIND % 8))
}

/// `(words, kind)` of the word that publishes a message.
fn unheader(h: u64) -> (u32, u32) {
    (
        (h >> (8 * (msg::at::WORDS % 8))) as u32,
        (h >> (8 * (msg::at::KIND % 8))) as u32,
    )
}

/// The atomic word that publishes the message whose head is at `w`.
///
/// # Safety
/// `w` the first word of a record in a live ring.
unsafe fn header_of<'a>(w: *mut u64) -> &'a AtomicU64 {
    &*(w.add(HEADER_WORD) as *const AtomicU64)
}

/// The payload of the message whose head is at `w`: right after its head.
fn payload(w: *mut u64) -> *mut u64 {
    w.wrapping_add(MESSAGE_HEAD_WORDS as usize)
}

/// Takes, from `boxes` (one module's boxes), every message of a tick up to `tick`, in the one deterministic order:
/// tick, then sender, then the sender's number — whatever box they came by and whenever they arrived. A box stops at
/// its first message of a later tick (it waits for its step). Messages of a tick gone are taken and counted in `late`.
/// `scratch` is the caller's, kept from tick to tick (no allocation in regime). Returns the messages taken.
///
/// # Safety
/// The one reader of every box in `boxes`.
pub unsafe fn take_tick(
    boxes: &[&MailboxHead],
    tick: u32,
    scratch: &mut Vec<(u32, u32, u32, usize, u32)>,
    mut take: impl FnMut(Message<'_>),
) -> usize {
    scratch.clear();
    let mut ends = [0u32; 32];
    assert!(boxes.len() <= ends.len(), "a module has at most 32 boxes");
    for (i, b) in boxes.iter().enumerate() {
        let mut end = b.head.load(Relaxed);
        let mut stop = false;
        b.peek(|pos, m| {
            if stop || m.tick > tick {
                stop = true;
                return;
            }
            scratch.push((m.tick, m.from, m.seq, i, pos));
            end = pos.wrapping_add(MESSAGE_HEAD_WORDS + m.words.len() as u32);
        });
        ends[i] = end;
    }
    scratch.sort_unstable();
    for &(t, _, _, i, pos) in scratch.iter() {
        let b = boxes[i];
        if t < tick {
            b.late.fetch_add(1, Relaxed);
        }
        let mut once = Some(&mut take);
        b.peek(|p, m| {
            if p == pos {
                if let Some(f) = once.take() {
                    f(m);
                }
            }
        });
    }
    for (i, b) in boxes.iter().enumerate() {
        b.free_to(ends[i]);
    }
    scratch.len()
}

/// A box in the directory: the Worker it lives in, its name, its block.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub worker: u32,
    pub name: String,
    pub at: usize,
    pub kind: u32,
}

/// The directory of the boxes, hosted by the player at start: a box opened is written here once, each Worker is
/// handed it once, a sender resolves an address once. It never sees a message.
#[derive(Default)]
pub struct Directory {
    entries: Vec<Entry>,
    blocks: Vec<Box<[u64]>>,
}

impl Directory {
    /// Opens the box `worker/name` of `kind`, `cap` words (`gfp_mailbox_open`); the same box when already open.
    pub fn open(&mut self, worker: u32, name: &str, kind: u32, cap: u32) -> usize {
        if let Some(at) = self.resolve(worker, name) {
            return at;
        }
        let mut block = vec![0u64; block_words(cap)].into_boxed_slice();
        // SAFETY: a zeroed block kept by the directory as long as it lives.
        let at = unsafe { MailboxHead::init(block.as_mut_ptr(), kind, cap) } as *const _ as usize;
        self.blocks.push(block);
        self.entries.push(Entry {
            worker,
            name: name.to_string(),
            at,
            kind,
        });
        at
    }

    /// The block of `worker/name`, when open.
    pub fn resolve(&self, worker: u32, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .find(|e| e.worker == worker && e.name == name)
            .map(|e| e.at)
    }

    /// Every box (what an observer reads in place: F9, the save, the replay, the network).
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

/// Where a box is: its block and its kind (the [`Directory`] of a process, the [`Table`] of the shared zone).
pub trait Resolve {
    fn find(&self, worker: u32, name: &str) -> Option<(usize, u32)>;
}

impl Resolve for Directory {
    fn find(&self, worker: u32, name: &str) -> Option<(usize, u32)> {
        self.entries
            .iter()
            .find(|e| e.worker == worker && e.name == name)
            .map(|e| (e.at, e.kind))
    }
}

/// The directory as the player hosts it: a table of fixed layout in the shared zone (`zone::state::MAILBOXES` gives
/// its offset), laid by the core with the zone, so every Worker that has the zone has the directory, given once.
/// A module's boxes are written in it when its port is installed; a sender reads it in place, an observer (F9) too.
///
/// Layout: the record contract `abi.mail.table` (its entries `abi.mail.entry`), positions generated.
#[derive(Clone, Copy)]
pub struct Table(pub *mut u8);

/// A table as this build lays it out, for the sizes of its arrays (never read: the table is read in place).
const LAID: table::Row = unsafe { std::mem::zeroed() };
/// Entries of a [`Table`].
pub const TABLE_ENTRIES: u32 = LAID.boxes.len() as u32;
/// Bytes of one entry of a [`Table`].
pub const TABLE_ENTRY: usize = entry::SIZE;
/// Bytes of a name in a [`Table`].
pub const TABLE_NAME: usize = LAID.boxes[0].name.len();
/// Bytes of a [`Table`].
pub const TABLE_BYTES: u32 = table::SIZE as u32;

impl Table {
    fn word(&self, at: usize) -> &AtomicU32 {
        // SAFETY: inside the table (callers stay in its layout), 4-aligned.
        unsafe { &*(self.0.add(at) as *const AtomicU32) }
    }

    /// Lays an empty table over a zeroed block of [`TABLE_BYTES`] (8-aligned).
    ///
    /// # Safety
    /// `p` valid for [`TABLE_BYTES`], nobody reads it yet.
    pub unsafe fn lay(p: *mut u8) -> Table {
        let t = Table(p);
        t.word(table::at::CAPACITY).store(TABLE_ENTRIES, Release);
        t
    }

    /// Entries written.
    pub fn len(&self) -> usize {
        (self.word(table::at::ENTRIES).load(Acquire) as usize).min(TABLE_ENTRIES as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Entry `i`: `(worker, kind, at, name)`, read in place.
    pub fn entry(&self, i: usize) -> Option<(u32, u32, usize, &str)> {
        if i >= self.len() {
            return None;
        }
        let e = slot(i);
        let worker = self.word(e + entry::at::WORKER).load(Relaxed);
        let kind = self.word(e + entry::at::KIND).load(Relaxed);
        // SAFETY: an entry written before `len` moved past it (Release / Acquire).
        let at = unsafe { (self.0.add(e + entry::at::AT) as *const u64).read() } as usize;
        let len = (self.word(e + entry::at::LEN).load(Relaxed) as usize).min(TABLE_NAME);
        let name = unsafe { std::slice::from_raw_parts(self.0.add(e + entry::at::NAME), len) };
        Some((worker, kind, at, std::str::from_utf8(name).unwrap_or("")))
    }

    /// Writes the box `worker/name` (already there: its block and kind replaced). False: the table is full or the name
    /// too long.
    pub fn register(&self, worker: u32, name: &str, kind: u32, at: usize) -> bool {
        if name.len() > TABLE_NAME {
            return false;
        }
        while self
            .word(table::at::LOCK)
            .compare_exchange_weak(0, 1, Acquire, Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        let n = self.len();
        let at_now = (0..n).find(|&i| {
            self.entry(i)
                .is_some_and(|(w, _, _, e)| w == worker && e == name)
        });
        let ok = if let Some(i) = at_now {
            // A new world of the same module (a game loaded again): its blocks take the place of the old ones.
            let e = slot(i);
            self.word(e + entry::at::KIND).store(kind, Relaxed);
            // SAFETY: an entry of the table, rewritten under the lock.
            unsafe { (self.0.add(e + entry::at::AT) as *mut u64).write(at as u64) };
            std::sync::atomic::fence(Release);
            true
        } else if n < TABLE_ENTRIES as usize {
            let e = slot(n);
            self.word(e + entry::at::WORKER).store(worker, Relaxed);
            self.word(e + entry::at::KIND).store(kind, Relaxed);
            // SAFETY: an entry nobody reads before `len` covers it.
            unsafe {
                (self.0.add(e + entry::at::AT) as *mut u64).write(at as u64);
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    self.0.add(e + entry::at::NAME),
                    name.len(),
                );
            }
            self.word(e + entry::at::LEN)
                .store(name.len() as u32, Relaxed);
            self.word(table::at::ENTRIES).store(n as u32 + 1, Release);
            true
        } else {
            false
        };
        self.word(table::at::LOCK).store(0, Release);
        ok
    }
}

/// Where entry `i` of a [`Table`] starts.
const fn slot(i: usize) -> usize {
    table::at::BOXES + i * entry::SIZE
}

impl Resolve for Table {
    fn find(&self, worker: u32, name: &str) -> Option<(usize, u32)> {
        (0..self.len())
            .filter_map(|i| self.entry(i))
            .find(|&(w, _, _, n)| w == worker && n == name)
            .map(|(_, k, at, _)| (at, k))
    }
}

/// The newest message of `code` whose payload starts with `key`, copied into `out` (a state stream read by anyone,
/// in place, while its writer may free older records): `Some(words copied)`, `None` when there is none.
///
/// # Safety
/// `b` a box alive.
pub unsafe fn newest(b: &MailboxHead, code: u32, key: &[u64], out: &mut [u64]) -> Option<usize> {
    for _ in 0..4 {
        let mut found = None;
        b.peek_shared(|pos, m| {
            if m.code == code && m.words.len() >= key.len() && &m.words[..key.len()] == key {
                found = Some(pos);
            }
        });
        let pos = found?;
        let w = b.word(pos);
        let h = header_of(w).load(Acquire);
        let n = unheader(h).0 as usize;
        if n < MESSAGE_HEAD_WORDS as usize + key.len() {
            continue;
        }
        let len = (n - MESSAGE_HEAD_WORDS as usize - key.len()).min(out.len());
        std::ptr::copy_nonoverlapping(payload(w).add(key.len()), out.as_mut_ptr(), len);
        // Freed records are zeroed header first: a header still whole after the copy means a whole copy.
        std::sync::atomic::fence(Acquire);
        if header_of(w).load(Acquire) == h {
            return Some(len);
        }
    }
    None
}

/// A writer's side: its number, the next message number (one count over all its boxes, the order of dependent
/// orders), and what waits (a box not yet in the directory, a box full).
pub struct Outbox {
    pub from: u32,
    seq: u32,
    waiting: Vec<Waiting>,
    said_full: bool,
}

struct Waiting {
    worker: u32,
    name: String,
    tick: u32,
    seq: u32,
    code: u32,
    words: Vec<u64>,
}

impl Outbox {
    pub fn new(from: u32) -> Outbox {
        Outbox {
            from,
            seq: 0,
            waiting: Vec::new(),
            said_full: false,
        }
    }

    /// Sends a message to `worker/name` for `tick`. It goes at once when nothing of this sender waits and the box is
    /// known and has room; else it waits here, in order (a state value replaces the one of its code that waits).
    pub fn send(
        &mut self,
        dir: &impl Resolve,
        worker: u32,
        name: &str,
        tick: u32,
        code: u32,
        words: &[u64],
    ) {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        self.flush(dir);
        if self.waiting.is_empty() {
            if let Some((at, _)) = dir.find(worker, name) {
                // SAFETY: a block of the directory.
                let b = unsafe { MailboxHead::at(at) };
                if unsafe { b.write(self.from, tick, seq, code, words) } {
                    return;
                }
                self.on_full(b);
            }
        }
        let state = dir
            .find(worker, name)
            .is_some_and(|(_, k)| k == kind::STATE);
        if state {
            if let Some(w) = self
                .waiting
                .iter_mut()
                .find(|w| w.worker == worker && w.name == name && w.code == code)
            {
                if let Some((at, _)) = dir.find(worker, name) {
                    // SAFETY: a block of the directory.
                    unsafe { MailboxHead::at(at) }.late.fetch_add(1, Relaxed);
                }
                (w.tick, w.seq, w.words) = (tick, seq, words.to_vec());
                return;
            }
        }
        self.waiting.push(Waiting {
            worker,
            name: name.to_string(),
            tick,
            seq,
            code,
            words: words.to_vec(),
        });
    }

    /// Sends what waits, in order, as far as it goes (call it each tick).
    pub fn flush(&mut self, dir: &impl Resolve) {
        let mut sent = 0;
        for w in &self.waiting {
            let Some((at, _)) = dir.find(w.worker, &w.name) else {
                break;
            };
            // SAFETY: a block of the directory.
            let b = unsafe { MailboxHead::at(at) };
            if !unsafe { b.write(self.from, w.tick, w.seq, w.code, &w.words) } {
                break;
            }
            sent += 1;
        }
        self.waiting.drain(..sent);
    }

    /// Messages waiting here.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    fn on_full(&mut self, b: &MailboxHead) {
        if b.kind == kind::ORDERS {
            b.full.fetch_add(1, Relaxed);
            if !self.said_full {
                self.said_full = true;
                #[cfg(not(target_arch = "wasm32"))]
                eprintln!("mailbox: a box of orders is full: orders kept by their sender (size it larger)");
            }
        }
    }
}

#[cfg(test)]
#[path = "mailbox_tests.rs"]
mod tests;
