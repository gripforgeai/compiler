//! The ring of raw input samples of the zone: a neutral primitive of the hosts (no notion of what the input is for).
//! Generic over every raw continuous axis (the pointer's motion, the sticks, the wheel, a touch pad, a gyroscope
//! later): each sample names its device and its axis; core.input decides by its bindings which axis drives which
//! action (the look, a zoom on the wheel, an analog trigger…).
//!
//! The host's devices write every raw sample there AT EACH EVENT (the web page on `pointerrawupdate` when it exists,
//! natively the gateway at each motion it gets), with the time of the event on the host's clock. Each reader keeps
//! its own cursor (the last `seq` it read) and reads everything newer in order, in place: nobody takes anything out,
//! nobody copies the ring, the oldest samples are overwritten at the next lap. A reader more than one lap late
//! detects it (`seq - cursor > slots`), counts it ([`Cursor::overruns`], for the debugger) and goes on from the
//! oldest sample still there.
//!
//! Why (#655, the camera jerked when turning while walking, broken twice: before 03/11 and on 17/11): the mouse
//! reaches only the page's main thread, and the browser lines `mousemove` up with the page's frames, while the render
//! runs in a Worker on its own clock. A value read « the latest » at each render frame then moves by 0 or 2 steps per
//! frame (two clocks folding onto each other). Timestamped samples let the render resample the motion at the exact
//! instant it draws, whatever the clock of the writer. Guarded by `tests::two_readers_at_their_own_pace_see_every_sample_once`
//! and `tests::a_reader_late_by_less_than_the_ring_loses_nothing`.
//!
//! The size is declared in DATA, as a duration (`input.ringSeconds` of the game schema, 0.5 s by default: a garbage
//! collection or a tab switch) times the highest rate of the device (`input.pointerHz`, 1 000 Hz for a mouse),
//! rounded up: [`slots_for`]. The ring lives in a block of `REGIONS`; its offset in the zone is the `u32` at
//! `INPUT + input::RING` (0: no ring).
//!
//! The block is of the contract `abi.input.ring` (core/abi/contracts/abi.input.ring.json, reference
//! docs/contrats-abi.md): `TableHead` (its `slots` the samples the ring holds, its `stride` the bytes of one), the head
//! field `written` (samples written since the start: the write counter, advanced atomically AFTER the sample), then
//! sample `n` at row `n mod slots`: `t` (seconds, the host's clock), `x`, `y`, the device's `kind` (0 keyboard, 1 mouse,
//! 2 pad, 3 touch, 4 gyro, 5 virtual: the raw records' kinds) and `index`, the `axis` ([`axis`]), `flags`
//! ([`ABSOLUTE`]). Every reader and writer reads the stride the block stores (lot 3 of the contracts: the Rust reader
//! read the constant before), each field at the position of the generated contract.
//!
//! The host's clock: seconds since the UNIX epoch as a float (web: `(performance.timeOrigin + performance.now()) /
//! 1000`, the same on the page and in its Workers; natively the system clock). Written new.

use gripforge_contracts::abi_input_ring as c;
use std::sync::atomic::{AtomicU32, Ordering};

/// The contract of the ring's block.
pub use c::SPEC;
/// `flags`: the sample is the axis' value now (a stick), not a motion since the previous one (the pointer).
pub const ABSOLUTE: u32 = 1;

/// The axes a host writes (`Sample::axis`), named as the raw records name them (`StickLeft`, `StickRight`…).
pub mod axis {
    /// The pointer's relative motion (x right, y down), a motion.
    pub const POINTER: u16 = 0;
    /// The left stick of a pad (x right, y down), a value.
    pub const STICK_LEFT: u16 = 1;
    /// The right stick of a pad, a value.
    pub const STICK_RIGHT: u16 = 2;
    /// The wheel (x, y in notches, down positive), a motion.
    pub const WHEEL: u16 = 3;
    /// The state of an ACTION as the host wrote it at an event (`zone::action_state`, decision of 20/11): `index` the
    /// action's number in the bindings table's order of blocks, `x`, `y` its cumulated value, a value (`ABSOLUTE`).
    /// The dated journal of the states: each reader takes the last one before its instant.
    pub const ACTION: u16 = 4;
}
/// The duration the ring covers when the game schema says nothing (seconds).
pub const SECONDS: f64 = 0.5;
/// The highest rate of the pointer when the game schema says nothing (events a second).
pub const POINTER_HZ: f64 = 1000.0;

/// The samples a ring of `seconds` holds at `hz` events a second, rounded up (at least 2).
pub fn slots_for(seconds: f64, hz: f64) -> u32 {
    let n = (seconds.max(0.0) * hz.max(0.0)).ceil();
    if n.is_finite() {
        (n as u32).max(2)
    } else {
        2
    }
}

/// Bytes of a ring of `slots` samples.
pub const fn bytes(slots: u32) -> u32 {
    c::HEAD_BYTES + c::STRIDE * slots
}

/// One raw sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// The host's clock at the event, seconds.
    pub t: f64,
    pub x: f64,
    pub y: f64,
    /// The device: its kind (as the raw records) and its index.
    pub kind: u8,
    pub index: u8,
    /// Which axis of the device ([`axis`]).
    pub axis: u16,
    /// [`ABSOLUTE`] for a value, 0 for a motion.
    pub flags: u32,
}

impl Sample {
    /// A motion of the pointer of mouse 0.
    pub fn pointer(t: f64, dx: f64, dy: f64) -> Sample {
        Sample {
            t,
            x: dx,
            y: dy,
            kind: 1,
            index: 0,
            axis: axis::POINTER,
            flags: 0,
        }
    }

    /// The state of an action (`axis::ACTION`) as the host wrote it at `t`: `index` its number, `(x, y)` its value.
    pub fn action(t: f64, index: u8, x: f64, y: f64) -> Sample {
        Sample {
            t,
            x,
            y,
            kind: 5,
            index,
            axis: axis::ACTION,
            flags: ABSOLUTE,
        }
    }

    /// A stick of a pad as it is now (`axis::STICK_LEFT` / `STICK_RIGHT`), at `t`.
    pub fn stick(t: f64, pad: u8, axis: u16, x: f64, y: f64) -> Sample {
        Sample {
            t,
            x,
            y,
            kind: 2,
            index: pad,
            axis,
            flags: ABSOLUTE,
        }
    }

    fn write(&self, slot: *mut u8) {
        // SAFETY (callers): `slot` is a whole sample of a ring, 8-aligned, of a writer that knows these fields.
        unsafe {
            (slot.add(c::at::T) as *mut f64).write_volatile(self.t);
            (slot.add(c::at::X) as *mut f64).write_volatile(self.x);
            (slot.add(c::at::Y) as *mut f64).write_volatile(self.y);
            slot.add(c::at::KIND).write_volatile(self.kind);
            slot.add(c::at::INDEX).write_volatile(self.index);
            (slot.add(c::at::AXIS) as *mut u16).write_volatile(self.axis);
            (slot.add(c::at::FLAGS) as *mut u32).write_volatile(self.flags);
        }
    }

    fn read(slot: *const u8) -> Sample {
        // SAFETY (callers): as `write`.
        unsafe {
            Sample {
                t: (slot.add(c::at::T) as *const f64).read_volatile(),
                x: (slot.add(c::at::X) as *const f64).read_volatile(),
                y: (slot.add(c::at::Y) as *const f64).read_volatile(),
                kind: slot.add(c::at::KIND).read_volatile(),
                index: slot.add(c::at::INDEX).read_volatile(),
                axis: (slot.add(c::at::AXIS) as *const u16).read_volatile(),
                flags: (slot.add(c::at::FLAGS) as *const u32).read_volatile(),
            }
        }
    }
}

/// Bytes of one sample handed in a message (`ExtCmd::WatchRing`, the player to a presentation module): a row of
/// `abi.input.ring` as this build lays it.
pub const SAMPLE_BYTES: usize = c::STRIDE as usize;

impl Sample {
    /// The sample as a row of `abi.input.ring` of this build (a message's sample).
    pub fn to_bytes(&self) -> [u8; SAMPLE_BYTES] {
        let mut row = [0u64; SAMPLE_BYTES / 8];
        self.write(row.as_mut_ptr() as *mut u8);
        let mut out = [0u8; SAMPLE_BYTES];
        for (o, w) in out.as_chunks_mut::<8>().0.iter_mut().zip(row) {
            *o = w.to_ne_bytes();
        }
        out
    }
    /// A sample from a row of this build ([`Sample::to_bytes`]); `None` when `b` is shorter.
    pub fn from_bytes(b: &[u8]) -> Option<Sample> {
        let b = b.get(..SAMPLE_BYTES)?;
        let mut row = [0u64; SAMPLE_BYTES / 8];
        for (w, o) in row.iter_mut().zip(b.as_chunks::<8>().0) {
            *w = u64::from_ne_bytes(*o);
        }
        Some(Sample::read(row.as_ptr() as *const u8))
    }
}

/// Lays an empty ring of `slots` samples at `at` (8-aligned, `bytes(slots)` long): its `TableHead` written before
/// anyone is handed its place.
///
/// # Safety
/// `at` points to `bytes(slots)` writable bytes, 8-aligned, that nobody reads yet.
pub unsafe fn lay(at: *mut u8, slots: u32) {
    std::ptr::write_bytes(at, 0, bytes(slots) as usize);
    c::init(at, slots);
    c::publish(at, slots);
}

/// A ring bound: its `TableHead` read once (the stride and the slots its layer stored).
#[derive(Clone, Copy, Debug)]
struct Bound {
    at: *const u8,
    rows: *const u8,
    stride: usize,
    slots: u32,
}

impl Bound {
    /// `None`: not a ring of a writer this build reads (another contract, a sample without its fields).
    unsafe fn of(at: *const u8) -> Option<Bound> {
        let h = gripforge_contracts::table_head::TableHead::read(at);
        h.check(&SPEC).ok()?;
        Some(Bound {
            at,
            rows: at.add(h.head_bytes as usize),
            stride: h.stride as usize,
            slots: h.slots,
        })
    }
    unsafe fn slot(&self, n: u32) -> *const u8 {
        self.rows.add(self.stride * (n % self.slots) as usize)
    }
    unsafe fn written(&self) -> &'static AtomicU32 {
        // SAFETY (callers): the head field of the block, 4-aligned, for the life of the ring.
        &*(self.at.add(c::at::head::WRITTEN) as *const AtomicU32)
    }
}

/// The samples the ring at `at` holds (0: not a ring).
///
/// # Safety
/// `at` points to a ring laid by [`lay`].
pub unsafe fn slots(at: *const u8) -> u32 {
    Bound::of(at).map_or(0, |b| b.slots)
}

/// The bytes the ring at `at` spans as its head says (0: not a ring): where a writer checks the ring is whole.
///
/// # Safety
/// `at` points to a ring laid by [`lay`].
pub unsafe fn span(at: *const u8) -> u64 {
    Bound::of(at).map_or(0, |b| {
        (b.rows as u64 - at as u64) + b.stride as u64 * u64::from(b.slots)
    })
}

/// The host writes one sample (ONE writer per ring): the sample in its slot, THEN the counter (release).
///
/// # Safety
/// `at` points to a ring laid by [`lay`], written by this thread only.
pub unsafe fn push(at: *mut u8, s: Sample) {
    let Some(b) = Bound::of(at) else { return };
    if b.slots == 0 {
        return;
    }
    let c = b.written().load(Ordering::Relaxed);
    s.write(b.slot(c) as *mut u8);
    b.written().store(c.wrapping_add(1), Ordering::Release);
}

/// A reader's own place in a ring: the next `seq` it reads, and the laps it missed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// The next sample to read (`None`: never read: starts at what is written now, nothing from before).
    pub next: Option<u32>,
    /// Times this reader was more than one lap late (samples lost), for the debugger.
    pub overruns: u32,
    /// Samples lost in those overruns.
    pub lost: u32,
}

impl Cursor {
    /// Reads every sample newer than the cursor, in order, in place, and moves the cursor past them. A first read
    /// starts at the end (what was written before the reader came is not its motion).
    ///
    /// # Safety
    /// `at` points to a ring laid by [`lay`] (written by another thread or not).
    pub unsafe fn read(&mut self, at: *const u8, mut each: impl FnMut(u32, Sample)) -> u32 {
        let Some(b) = Bound::of(at) else { return 0 };
        let n = b.slots;
        let written = b.written().load(Ordering::Acquire);
        let Some(mut next) = self.next else {
            self.next = Some(written);
            return 0;
        };
        if n == 0 {
            return 0;
        }
        let mut read = 0;
        while next != written {
            let late = written.wrapping_sub(next);
            if late > n {
                // More than one lap late: what was not read is gone; from the oldest still there.
                self.overruns += 1;
                self.lost = self.lost.wrapping_add(late - n);
                next = written.wrapping_sub(n);
                continue;
            }
            let s = Sample::read(b.slot(next));
            // The writer may have lapped this slot while it was read: then it is lost, not wrong.
            let now = b.written().load(Ordering::Acquire);
            if now.wrapping_sub(next) > n {
                self.overruns += 1;
                self.lost = self.lost.wrapping_add(1);
                next = next.wrapping_add(1);
                continue;
            }
            each(next, s);
            read += 1;
            next = next.wrapping_add(1);
        }
        self.next = Some(next);
        read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(slots: u32) -> Vec<u64> {
        let mut v = vec![0u64; (bytes(slots) as usize).div_ceil(8)];
        unsafe { lay(v.as_mut_ptr() as *mut u8, slots) };
        v
    }

    fn s(i: u32) -> Sample {
        Sample {
            t: i as f64 * 0.001,
            x: i as f64,
            y: -(i as f64),
            kind: 2,
            index: (i % 3) as u8,
            axis: axis::STICK_RIGHT,
            flags: ABSOLUTE,
        }
    }

    /// Tomorrow's writer adds a field at the end of a sample (a wider stride, stored in the ring's head): today's
    /// reader reads every sample where that writer put it (lot 3: it read the constant before).
    #[test]
    fn a_ring_of_a_wider_stride_is_read_at_the_stride_it_stores() {
        let wide = c::STRIDE + 8;
        let mut v = vec![0u64; (c::HEAD_BYTES + wide * 4) as usize / 8];
        let at = v.as_mut_ptr() as *mut u8;
        unsafe {
            c::init(at, 4);
            c::publish(at, 4);
            let h = &mut *(at as *mut gripforge_contracts::table_head::TableHead);
            h.stride = wide;
            h.fields += 1;
            let mut cur = Cursor::default();
            cur.read(at, |_, _| {});
            for i in 0..3 {
                // Written by the wider writer: at ITS stride.
                s(i).write(at.add((c::HEAD_BYTES + wide * i) as usize));
            }
            (*(at.add(c::at::head::WRITTEN) as *const AtomicU32)).store(3, Ordering::Release);
            let mut got = Vec::new();
            cur.read(at, |n, x| got.push((n, x)));
            assert_eq!(got, (0..3).map(|i| (i, s(i))).collect::<Vec<_>>());
            assert_eq!(slots(at), 4);
        }
    }

    #[test]
    fn a_sample_goes_through_a_message_whole() {
        assert_eq!(Sample::from_bytes(&s(7).to_bytes()), Some(s(7)));
        assert_eq!(Sample::from_bytes(&[0u8; 8]), None);
    }

    #[test]
    fn the_size_is_a_duration_times_the_rate_rounded_up() {
        assert_eq!(slots_for(SECONDS, POINTER_HZ), 500);
        assert_eq!(slots_for(0.2, 1000.0), 200);
        assert_eq!(slots_for(0.0105, 1000.0), 11);
        assert_eq!(slots_for(0.0, 1000.0), 2);
        assert!(bytes(500) <= 16 * 1024, "about 16 KB for 0.5 s at 1 000 Hz");
    }

    /// Two readers at their own pace (one every 3 samples, one every 7) see every sample, once, in order.
    #[test]
    fn two_readers_at_their_own_pace_see_every_sample_once() {
        let mut v = ring(16);
        let at = v.as_mut_ptr() as *mut u8;
        let (mut a, mut b) = (Cursor::default(), Cursor::default());
        unsafe {
            a.read(at, |_, _| {});
            b.read(at, |_, _| {});
        }
        let (mut seen_a, mut seen_b) = (Vec::new(), Vec::new());
        for i in 0..200 {
            unsafe { push(at, s(i)) };
            if i % 3 == 0 {
                unsafe { a.read(at, |n, x| seen_a.push((n, x))) };
            }
            if i % 7 == 0 {
                unsafe { b.read(at, |n, x| seen_b.push((n, x))) };
            }
        }
        unsafe {
            a.read(at, |n, x| seen_a.push((n, x)));
            b.read(at, |n, x| seen_b.push((n, x)));
        }
        for seen in [&seen_a, &seen_b] {
            assert_eq!(seen.len(), 200);
            for (i, (n, x)) in seen.iter().enumerate() {
                assert_eq!((*n, *x), (i as u32, s(i as u32)));
            }
        }
        assert_eq!((a.overruns, b.overruns), (0, 0));
    }

    /// A ring of 0.5 s at 1 000 Hz: a reader late by 0.49 s at the highest rate loses nothing.
    #[test]
    fn a_reader_late_by_less_than_the_ring_loses_nothing() {
        let slots = slots_for(SECONDS, POINTER_HZ);
        let mut v = ring(slots);
        let at = v.as_mut_ptr() as *mut u8;
        let mut c = Cursor::default();
        unsafe { c.read(at, |_, _| {}) };
        for i in 0..490 {
            unsafe { push(at, s(i)) };
        }
        let mut got = 0;
        unsafe {
            c.read(at, |n, x| {
                assert_eq!(x, s(n));
                got += 1;
            })
        };
        assert_eq!((got, c.overruns, c.lost), (490, 0, 0));
    }

    /// More than one lap late: detected, counted, and the reader goes on from the oldest sample still there.
    #[test]
    fn a_reader_more_than_a_lap_late_counts_it_and_goes_on_from_the_oldest() {
        let mut v = ring(8);
        let at = v.as_mut_ptr() as *mut u8;
        let mut c = Cursor::default();
        unsafe { c.read(at, |_, _| {}) };
        for i in 0..20 {
            unsafe { push(at, s(i)) };
        }
        let mut got = Vec::new();
        unsafe { c.read(at, |n, _| got.push(n)) };
        assert_eq!(got, (12..20).collect::<Vec<_>>());
        assert_eq!((c.overruns, c.lost), (1, 12));
        unsafe { push(at, s(20)) };
        got.clear();
        unsafe { c.read(at, |n, _| got.push(n)) };
        assert_eq!(got, vec![20]);
    }
}
