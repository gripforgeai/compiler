//! The pipes of the debugger (`debug.expose@1` §4, docs/exposer-au-debogueur.md): one pipe per writing thread (tick,
//! physics, render), a circular queue in shared memory with one writer and one reader.
//!
//! The writer (a kit's `Counters::set`, the debugger's events) appends a record at the end and never waits: no lock,
//! no allocation, no system call, O(1), one record and one atomic store with release. When the pipe is full the record
//! is dropped and counted (`lost`), the reader's head being read again only then. The reader (the panel, at its
//! rhythm) empties the pipe at once: one atomic load of the write head, the whole range `[read, written)` handled in
//! place (two spans at most, the queue wraps), one atomic store of the read head. No call per record.
//!
//! Layout: the table contract `abi.debug.pipe` (core/abi/contracts/abi.debug.pipe.json, reference docs/contrats-abi.md;
//! docs/decisions/2026-11-20-abi-par-contrat.md, lot 5b). A `TableHead` (the slots, the stride of a record, read once
//! at bind), then the head: the constant line (`thread`), the writer's line (`written`: low 32 bits records written,
//! high 32 bits records lost, both wrapping; `seen`: the read head as the writer last read it), the reader's line
//! (`read`: records released), each on its own cache line (no false sharing); then `slots` records, record `k` in slot
//! `k % slots`, at the stride the writer announced. Every position is a generated constant; none is written here.
//!
//! The writer writes only slots the reader released and the reader reads only slots the writer published (acquire /
//! release on the two heads): a record is never read while written, so nothing is torn.
//!
//! Used by: `debug_expose.rs` (`Counters::set`), kits/debug_inspector/logic (`pipes.rs`: the pipes, the events, the
//! fallback reader), kits/debug_inspector/present/web/pipes.mjs (the same contract read and written from JavaScript).

use gripforge_contracts::abi_debug_pipe as pipe;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// One fact of a pipe: a row of `abi.debug.pipe` (`kit`, `item`, `kind`, `tick`, `value`, `extra`).
pub use pipe::Row as Record;

/// The contract of a pipe (`pipes()` answers it).
pub const CONTRACT: &str = pipe::NAME;

/// A counter's value (`item`: the counter's index in the kit's block).
pub const KIND_VALUE: u32 = 0;
/// An event of an exposed `events` entry (`item`: its ring, `extra`: its number in that ring).
pub const KIND_EVENT: u32 = 1;
/// A mark a writer adds (`item` and `extra` its own: a begin or end of a span, a frame).
pub const KIND_MARK: u32 = 2;

/// A cache line: the alignment of a pipe, and what keeps the writer's head and the reader's apart.
const LINE: usize = 64;
const _: () = {
    let (written, seen, read) = (
        pipe::at::head::WRITTEN,
        pipe::at::head::SEEN,
        pipe::at::head::READ,
    );
    assert!(
        written / LINE == seen / LINE,
        "the writer's two words share its line"
    );
    assert!(
        written / LINE != read / LINE,
        "the writer's head and the reader's on lines of their own"
    );
    assert!(
        written / LINE != 0 && read / LINE != 0,
        "the heads off the constant line"
    );
};

/// The bytes of a pipe of `slots` records (`slots` rounded up to a power of two, at least 2), whole cache lines.
pub fn bytes_for(slots: usize) -> usize {
    pipe::bytes(slots.max(2).next_power_of_two() as u32).div_ceil(LINE) * LINE
}

/// A cache line, for the alignment of the block.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Line([u8; LINE]);

/// A pipe's memory, owned by who allocated it (the debugger: "who allocates frees"); its address goes to the writers
/// and the reader.
pub struct PipeMem {
    lines: Box<[Line]>,
}

impl PipeMem {
    /// A pipe of `slots` records (rounded up to a power of two) for the thread `thread`, empty.
    pub fn new(slots: usize, thread: u32) -> Self {
        let slots = slots.max(2).next_power_of_two();
        let mut lines = vec![Line([0; LINE]); bytes_for(slots) / LINE].into_boxed_slice();
        let block = lines.as_mut_ptr() as *mut u8;
        // SAFETY: a zeroed block of `bytes_for(slots)` bytes, 64-aligned, nobody reads it yet.
        unsafe {
            pipe::init(block, slots as u32);
            pipe::head_mut(block).thread = thread;
        }
        Self { lines }
    }
    pub fn ptr(&self) -> usize {
        self.lines.as_ptr() as usize
    }
    pub fn len(&self) -> usize {
        self.lines.len() * LINE
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    /// The writer of this pipe (one thread at a time).
    pub fn writer(&self) -> Writer {
        // SAFETY: a block of this contract, alive while `self` is.
        unsafe { Writer::at(self.ptr()) }
    }
    /// The reader of this pipe (one only).
    pub fn reader(&self) -> Reader {
        // SAFETY: as in `writer`.
        unsafe { Reader::at(self.ptr()) }
    }
}

/// The parts of a pipe at `ptr`, its TableHead read once.
#[derive(Clone, Copy, Debug)]
struct Place {
    ptr: usize,
    rows: usize,
    stride: usize,
    mask: u32,
}

impl Place {
    /// # Safety
    /// `ptr` is 0 or a block of `abi.debug.pipe` (`PipeMem`), alive while the place is used.
    unsafe fn at(ptr: usize) -> Option<Self> {
        if ptr == 0 || !ptr.is_multiple_of(LINE) {
            return None;
        }
        // Refused (another contract, a writer that does not know a field read here): not a pipe, every push ignored.
        let t = pipe::Table::bind(ptr as *const u8).ok()?;
        let slots = t.slots();
        if !slots.is_power_of_two() {
            return None;
        }
        // In place as this build's record: the writer knows every field of it (else not a pipe for this build).
        let first = t.rows()?.get(0) as *const Record as usize;
        Some(Self {
            ptr,
            rows: first,
            stride: t.stride(),
            mask: slots - 1,
        })
    }
    fn atomic64(&self, o: usize) -> &AtomicU64 {
        // SAFETY: `o` is a 8-aligned word of the head of a live pipe (`at`).
        unsafe { &*((self.ptr + o) as *const AtomicU64) }
    }
    fn atomic32(&self, o: usize) -> &AtomicU32 {
        // SAFETY: as in `atomic64`, 4-aligned.
        unsafe { &*((self.ptr + o) as *const AtomicU32) }
    }
    fn written(&self) -> &AtomicU64 {
        self.atomic64(pipe::at::head::WRITTEN)
    }
    fn seen(&self) -> &AtomicU32 {
        self.atomic32(pipe::at::head::SEEN)
    }
    fn read(&self) -> &AtomicU32 {
        self.atomic32(pipe::at::head::READ)
    }
    fn slot(&self, k: u32) -> *mut Record {
        (self.rows + (k & self.mask) as usize * self.stride) as *mut Record
    }
    fn slots(&self) -> u32 {
        self.mask + 1
    }
}

/// The writing end of a pipe. Every writer of one pipe runs on the same thread (the pipe's): the writes follow each
/// other, the head and `seen` are that thread's alone.
#[derive(Clone, Copy, Debug, Default)]
pub struct Writer {
    place: Option<Place>,
}

impl Writer {
    /// The writer of the pipe at `ptr` (none when it is not a pipe this build can write: every push ignored).
    ///
    /// # Safety
    /// `ptr` is 0 or a pipe (`PipeMem`) alive while the writer is used, written from one thread at a time.
    pub unsafe fn at(ptr: usize) -> Self {
        Self {
            place: Place::at(ptr),
        }
    }

    /// Appends `r`, never waiting: dropped and counted when the reader has not released room. Returns whether it was
    /// kept.
    #[inline]
    pub fn push(&self, r: Record) -> bool {
        let Some(p) = self.place else {
            return false;
        };
        let head = p.written();
        let h = head.load(Ordering::Relaxed);
        let w = h as u32;
        let seen = p.seen();
        if w.wrapping_sub(seen.load(Ordering::Relaxed)) >= p.slots() {
            // Full as far as the writer knows: the reader's head read again, only now.
            let read = p.read().load(Ordering::Acquire);
            seen.store(read, Ordering::Relaxed);
            if w.wrapping_sub(read) >= p.slots() {
                head.store(h.wrapping_add(1 << 32), Ordering::Release);
                return false;
            }
        }
        // SAFETY: slot `w` is free: the reader released it (`w - read < slots`) and reads it only after the release
        // below; a slot holds a whole record (the stride, checked at bind).
        unsafe { std::ptr::write(p.slot(w), r) };
        head.store(
            (h & 0xffff_ffff_0000_0000) | u64::from(w.wrapping_add(1)),
            Ordering::Release,
        );
        true
    }
}

/// What one drain found: the records written up to then, and the records lost since the pipe was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drained {
    pub records: u32,
    pub lost: u32,
}

/// Records of a pipe in place, one after the other at the stride its writer announced (no copy).
#[derive(Clone, Copy, Debug)]
pub struct Span<'a> {
    first: *const Record,
    stride: usize,
    n: usize,
    _m: PhantomData<&'a Record>,
}

impl<'a> Span<'a> {
    /// The records of the span.
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    /// Record `i`, in place.
    pub fn get(&self, i: usize) -> &'a Record {
        assert!(i < self.n, "record {i} of {}", self.n);
        // SAFETY: i < n, the records of the span lie at the stride in a pipe alive during the drain.
        unsafe { &*((self.first as usize + i * self.stride) as *const Record) }
    }
    /// The records, oldest first.
    pub fn iter(&self) -> Iter<'a> {
        Iter { span: *self, i: 0 }
    }
}

/// The records of a [`Span`], oldest first, in place.
#[derive(Clone, Copy, Debug)]
pub struct Iter<'a> {
    span: Span<'a>,
    i: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Record;
    fn next(&mut self) -> Option<&'a Record> {
        (self.i < self.span.n).then(|| {
            self.i += 1;
            self.span.get(self.i - 1)
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.span.n - self.i;
        (left, Some(left))
    }
}

impl<'a> IntoIterator for Span<'a> {
    type Item = &'a Record;
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

/// The reading end of a pipe: one reader only, which keeps its own position.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reader {
    place: Option<Place>,
    read: u32,
}

impl Reader {
    /// # Safety
    /// `ptr` is 0 or a pipe (`PipeMem`) alive while the reader is used; no other reader reads it.
    pub unsafe fn at(ptr: usize) -> Self {
        let place = Place::at(ptr);
        let read = place.map_or(0, |p| p.read().load(Ordering::Relaxed));
        Self { place, read }
    }

    /// Empties the pipe at once: one atomic load of the write head, `f` given the whole range in place (two spans at
    /// most, the oldest first), one atomic store of the read head. Nothing is copied.
    pub fn drain(&mut self, f: impl FnOnce(Span<'_>, Span<'_>)) -> Drained {
        let Some(p) = self.place else {
            return Drained::default();
        };
        let h = p.written().load(Ordering::Acquire);
        #[cfg(test)]
        tests::READER_ATOMICS.with(|c| c.set(c.get() + 1));
        let (w, lost) = (h as u32, (h >> 32) as u32);
        let n = w.wrapping_sub(self.read);
        let start = self.read & p.mask;
        let first = n.min(p.slots() - start);
        let span = |at: u32, n: u32| Span {
            first: p.slot(at),
            stride: p.stride,
            n: n as usize,
            _m: PhantomData,
        };
        // Records `read .. w` were published by the release of the head and are not written again before the store
        // of the read head below.
        f(span(start, first), span(0, n - first));
        self.read = w;
        p.read().store(w, Ordering::Release);
        #[cfg(test)]
        tests::READER_ATOMICS.with(|c| c.set(c.get() + 1));
        Drained { records: n, lost }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// The atomic operations of the reader on this thread (two per drain: the write head loaded, the read head
        /// stored).
        pub(super) static READER_ATOMICS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn rec(i: u64) -> Record {
        Record {
            kit: 1,
            item: (i % 3) as u16,
            kind: KIND_VALUE,
            tick: i,
            value: i as f64,
            extra: 0,
        }
    }

    #[test]
    fn one_drain_gives_every_record_in_order() {
        let pipe = PipeMem::new(64, 0);
        assert_eq!(pipe.ptr() % 64, 0, "aligned on a cache line");
        let w = pipe.writer();
        let mut r = pipe.reader();
        // Wrap once: 40 written and read, then 50 more cross the end of the 64 slots.
        for i in 0..40 {
            assert!(w.push(rec(i)));
        }
        r.drain(|_, _| {});
        for i in 40..90 {
            assert!(w.push(rec(i)));
        }
        let mut got = Vec::new();
        READER_ATOMICS.with(|c| c.set(0));
        let d = r.drain(|a, b| {
            assert!(!b.is_empty(), "the range wraps: two slices");
            got.extend(a.iter().copied());
            got.extend(b.iter().copied());
        });
        assert_eq!(
            d,
            Drained {
                records: 50,
                lost: 0
            }
        );
        assert_eq!(got, (40..90).map(rec).collect::<Vec<_>>());
        assert_eq!(
            READER_ATOMICS.with(|c| c.get()),
            2,
            "one load, one store for the whole range"
        );
        assert_eq!(
            r.drain(|a, b| assert!(a.is_empty() && b.is_empty()))
                .records,
            0
        );
    }

    #[test]
    fn a_full_pipe_drops_and_counts_never_waits() {
        let pipe = PipeMem::new(8, 0);
        let w = pipe.writer();
        let kept = (0..20).filter(|&i| w.push(rec(i))).count();
        assert_eq!(kept, 8);
        let mut r = pipe.reader();
        let mut got = Vec::new();
        let d = r.drain(|a, b| got.extend(a.iter().chain(b).map(|x| x.tick)));
        assert_eq!(
            d,
            Drained {
                records: 8,
                lost: 12
            }
        );
        assert_eq!(got, (0..8).collect::<Vec<_>>(), "the oldest kept, in order");
        assert!(w.push(rec(20)), "room again after the drain");
    }

    #[test]
    fn a_block_of_another_contract_is_not_a_pipe() {
        let pipe = PipeMem::new(8, 0);
        // SAFETY: the TableHead of a live pipe, nobody reads it now.
        let head = unsafe { &mut *(pipe.ptr() as *mut gripforge_contracts::table_head::TableHead) };
        head.contract ^= 1;
        assert!(
            !pipe.writer().push(rec(1)),
            "refused at bind: every push ignored"
        );
        assert_eq!(pipe.reader().drain(|_, _| {}), Drained::default());
    }

    #[test]
    fn a_wider_writer_is_read_at_its_stride() {
        // A newer writer whose records are 8 bytes longer: the same pipe read at the stride its TableHead announces.
        let pipe = PipeMem::new(16, 0);
        // SAFETY: the TableHead of a live pipe, nobody reads it now; 16 slots of the wider stride fit (32 of 32 bytes).
        let head = unsafe { &mut *(pipe.ptr() as *mut gripforge_contracts::table_head::TableHead) };
        head.stride += 8;
        head.slots = 8;
        let stride = head.stride as usize;
        let w = pipe.writer();
        for i in 0..5 {
            assert!(w.push(rec(i)));
        }
        let mut got = Vec::new();
        pipe.reader()
            .drain(|a, b| got.extend(a.iter().chain(b).copied()));
        assert_eq!(got, (0..5).map(rec).collect::<Vec<_>>());
        // SAFETY: inside the pipe: the second record at the wider stride.
        let second =
            unsafe { &*((pipe.ptr() + pipe::HEAD_BYTES as usize + stride) as *const Record) };
        assert_eq!(*second, rec(1));
    }

    #[test]
    fn not_a_pipe_is_ignored() {
        let w = unsafe { Writer::at(0) };
        assert!(!w.push(rec(1)));
        let mut r = unsafe { Reader::at(0) };
        assert_eq!(
            r.drain(|_, _| panic!("nothing to read")),
            Drained::default()
        );
    }

    /// The ticks of a writer thread: `ticks` ticks of `per_tick` records each, with a fixed work between (a tick's
    /// own logic). Returns the time of a tick in ns, the best of rounds of 500 ticks (the least disturbed by the
    /// machine: the other tests, the reader's own thread).
    fn writer_ticks(ptr: usize, ticks: u64, per_tick: u64) -> f64 {
        use std::time::Instant;
        // SAFETY: the pipe outlives the thread (joined by the caller); this thread is its only writer.
        let w = unsafe { Writer::at(ptr) };
        let mut x = 1u64;
        let mut best = f64::MAX;
        for round in 0..ticks / 500 {
            let t = Instant::now();
            for tick in round * 500..(round + 1) * 500 {
                for i in 0..per_tick {
                    w.push(rec(tick * per_tick + i));
                }
                for _ in 0..2000 {
                    x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
                }
            }
            best = best.min(t.elapsed().as_nanos() as f64 / 500.0);
        }
        best
    }

    /// A writer thread ticks while a slow reader empties its pipe now and then (every 5 ms, a pipe too small for
    /// that): the tick time stays the same as with nobody reading (the writer never waits), every record is either
    /// read, in order, or counted lost.
    #[test]
    fn a_slow_reader_never_holds_the_writer() {
        // A measure of time: a loaded machine (the other tests, the other jobs of a runner) can slow one run down. A
        // writer that waits for the reader is slow on every run; the noise rarely three times in a row.
        let runs: Vec<(f64, f64)> = (0..3)
            .map(|_| slow_reader_run())
            .take_while(|&(quiet, busy)| busy >= quiet * 1.5 + 200.0)
            .collect();
        assert!(
            runs.len() < 3,
            "the writer waited for the reader on 3 runs (busy ns against quiet ns): {runs:?}"
        );
    }

    /// One run of [`a_slow_reader_never_holds_the_writer`]: the order and the count checked; the tick time with nobody
    /// reading and with a slow reader, in ns.
    fn slow_reader_run() -> (f64, f64) {
        use std::time::Duration;
        const TICKS: u64 = 40_000;
        const PER: u64 = 16;
        // Alone: nobody reads, the pipe fills and the rest is counted lost.
        let alone = PipeMem::new(256, 0);
        let p = alone.ptr();
        let quiet = std::thread::spawn(move || writer_ticks(p, TICKS, PER))
            .join()
            .unwrap();
        let pipe = PipeMem::new(256, 0);
        let ptr = pipe.ptr();
        let writer = std::thread::spawn(move || writer_ticks(ptr, TICKS, PER));
        let mut r = pipe.reader();
        let (mut last, mut read, mut drains) = (None::<u64>, 0u64, 0);
        let mut lost: u32;
        loop {
            let done = writer.is_finished();
            std::thread::sleep(Duration::from_millis(5)); // the reader is slow
            let d = r.drain(|a, b| {
                for x in a.iter().chain(b) {
                    assert!(last.is_none_or(|l| x.tick > l), "in order");
                    last = Some(x.tick);
                }
            });
            read += u64::from(d.records);
            lost = d.lost;
            drains += 1;
            if done {
                break;
            }
        }
        let busy = writer.join().unwrap();
        assert_eq!(
            read + u64::from(lost),
            TICKS * PER,
            "read or counted lost, nothing else"
        );
        assert!(
            lost > 0,
            "the reader was too slow for this pipe: losses counted"
        );
        println!(
            "tick with {PER} pushes: {quiet:.0} ns alone, {busy:.0} ns with a slow reader; {read} read, {lost} lost, {drains} drains"
        );
        (quiet, busy)
    }

    /// The cost of one push, the pipe being read (never full): a few ns.
    #[test]
    fn a_push_costs_a_few_nanoseconds() {
        let pipe = PipeMem::new(1 << 16, 0);
        let (w, mut r) = (pipe.writer(), pipe.reader());
        let mut best = f64::MAX;
        for _ in 0..20 {
            r.drain(|_, _| {});
            let t = std::time::Instant::now();
            for i in 0..60_000 {
                w.push(rec(i));
            }
            best = best.min(t.elapsed().as_nanos() as f64 / 60_000.0);
        }
        println!("pipe push: {best:.2} ns");
        let max: f64 = std::env::var("GF_PIPE_PUSH_MAX_NS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if cfg!(debug_assertions) { 400.0 } else { 40.0 });
        assert!(best < max, "a push costs {best:.2} ns (more than {max} ns)");
    }

    /// One pipe per thread: three writers at once, each on its own pipe, one reader for all.
    #[test]
    fn one_pipe_per_thread_with_one_reader() {
        let pipes: Vec<PipeMem> = (0..3).map(|t| PipeMem::new(4096, t)).collect();
        let ptrs: Vec<usize> = pipes.iter().map(PipeMem::ptr).collect();
        let threads: Vec<_> = ptrs
            .iter()
            .enumerate()
            .map(|(t, &ptr)| {
                std::thread::spawn(move || {
                    // SAFETY: the pipes outlive the threads; one writer per pipe.
                    let w = unsafe { Writer::at(ptr) };
                    for i in 0..3000u64 {
                        assert!(w.push(Record {
                            kit: t as u16,
                            tick: i,
                            ..rec(i)
                        }));
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        for (t, p) in pipes.iter().enumerate() {
            let mut ticks = Vec::new();
            let d = p
                .reader()
                .drain(|a, b| ticks.extend(a.iter().chain(b).map(|x| (x.kit, x.tick))));
            assert_eq!(d.records, 3000);
            assert!(ticks
                .iter()
                .enumerate()
                .all(|(i, &(k, tick))| k as usize == t && tick == i as u64));
        }
    }
}
