//! The kit side of the debugger exposure contract `debug.expose@1` (docs/exposer-au-debogueur.md, docs/exposer.md).
//!
//! A kit exposes its elements to the debugger (scenes, entities, states, counters, settings, recent events) only when
//! a kit holds the debugger's role (`role.debug`, today debug.inspector): without it, [`expose`] returns at once (the kit's `kit.api.json` is not even
//! parsed), no counter is kept ([`Counters::set`] does nothing) and nothing is built.
//!
//! Used by: every kit module that lists `expose` entries for `debug` in its `kit.api.json` (game.engine first), at
//! `ready()`.
//! What the debugger does with it lives in its kit (methods `expose` and `counters` of `role.debug`); this file only
//! knows those two calls.

use crate::debug_pipe::{Record, Writer, KIND_VALUE};
use crate::host::Kx;
use serde_json::{json, Value as Json};
use std::cell::Cell;

/// The role of the debugger: nothing is called when nobody holds it.
const DEBUGGER: &str = gripforge_abi::roles::DEBUG;
/// The layout of the counter blocks this side writes (the debugger answers it with the block).
pub const LAYOUT: u64 = 4;

/// Hands the `expose` entries of `api_json` (the kit's own `kit.api.json`, usually
/// `include_str!("../kit.api.json")`) to the debugger, when it is loaded; it files those meant for it (`for` holds
/// `debug`) and skips the others. Returns whether it was loaded.
///
/// Call it once at `ready()` (every kit is mounted then). The JSON is parsed only when the debugger is there.
pub fn expose(kx: &mut Kx, kit: &str, api_json: &str) -> bool {
    crate::expose::hand(kx, kit, api_json, "debug", false)
}

/// The counters of a kit, in a block of `f64` the debugger allocates and reads in place (`debug.expose@1` §3).
///
/// Word 0 says whether someone looks (1.0: the panel is open or a snapshot is being made); words `1 ..= n` are the
/// last values of the counters, in the order of `values` of the `counters` entry; word `n + 1` is the window the
/// debugger reads next; words `n + 2 + 5 i ..` are counter `i`'s running window `[window, count, sum, min, max]`
/// (the debugger writes the closed windows after them, words `6 n + 2 ..`). Layout 4 ends with two words: `10 n + 2`
/// is the kit's number in the pipes, `10 n + 3` the address of the pipe of the thread that writes this block (0 while
/// no panel was ever opened: no pipe). `len = 4 + 10 n`.
///
/// Every value written while someone looks also goes, as it is, at the end of that pipe (`debug_pipe`, one per
/// thread, one writer): the panel empties the pipes at its rhythm and draws every value, so a one-tick peak a mean
/// hides stays visible, with what every other kit wrote at that tick. A write is a few additions and one record of 32
/// bytes; nothing waits, nothing is allocated.
#[derive(Debug, Default)]
pub struct Counters {
    ptr: usize,
    len: usize,
    n: usize,
    /// The tick the next values belong to (`watch`).
    tick: Cell<u64>,
    /// The pipe last seen at word `10 n + 3`, and its writer.
    pipe: Cell<(usize, Writer)>,
}

impl Counters {
    /// Asks the debugger for the block of `kit`'s counters named `names`, written from the tick's thread; an empty
    /// handle (every write ignored) when the debugger is not loaded or the kit modules do not share one memory (an
    /// isolated build).
    pub fn ask(kx: &mut Kx, kit: &str, names: &[&str]) -> Self {
        Self::ask_on(kx, kit, names, "tick")
    }

    /// As [`Counters::ask`], for a block written from another thread (`physics`, `render`): its values go to that
    /// thread's pipe. One block per kit and thread.
    pub fn ask_on(kx: &mut Kx, kit: &str, names: &[&str], thread: &str) -> Self {
        if !cfg!(any(feature = "common-memory", not(target_arch = "wasm32")))
            || !kx.host.has_service(DEBUGGER)
        {
            return Self::default();
        }
        let args = [json!(kit), json!(names), json!(thread)];
        let Ok(answer) = kx.call(DEBUGGER, "counters", &args) else {
            return Self::default();
        };
        let ptr = answer.get("ptr").and_then(Json::as_u64).unwrap_or(0) as usize;
        let len = answer.get("len").and_then(Json::as_u64).unwrap_or(0) as usize;
        let layout = answer.get("layout").and_then(Json::as_u64).unwrap_or(1);
        if ptr == 0 || !ptr.is_multiple_of(std::mem::align_of::<f64>()) || layout != LAYOUT {
            return Self::default();
        }
        // SAFETY: the block the debugger just handed out, `len` aligned f64 alive while it is loaded.
        unsafe { Self::at(ptr, len, names.len()) }
    }

    /// The handle of a block of layout 4 at `ptr` (`len` words, `n` counters); empty when the sizes do not match.
    ///
    /// # Safety
    /// `ptr` must be `len` aligned `f64` that stay alive while the handle is used (the debugger's block), and the
    /// pipe whose address it holds must be written from the handle's thread only.
    pub unsafe fn at(ptr: usize, len: usize, n: usize) -> Self {
        if ptr == 0 || n == 0 || len != 4 + 10 * n {
            return Self::default();
        }
        Self {
            ptr,
            len,
            n,
            ..Self::default()
        }
    }

    /// Whether the debugger gave a block and someone looks now (word 0): the kit counts only then.
    pub fn watched(&self) -> bool {
        // SAFETY: `ptr` is a block of `len` aligned f64 the debugger keeps alive while the game runs (it allocates
        // it once per kit and frees it at its unload, after the kits); word 0 exists since len >= 1.
        self.len > 0 && unsafe { std::ptr::read_volatile(self.ptr as *const f64) } != 0.0
    }

    /// [`Counters::watched`], and the tick the next values belong to (`kx.tick()`): call it before a round of `set`.
    pub fn watch(&self, tick: u64) -> bool {
        self.tick.set(tick);
        self.watched()
    }

    /// Writes counter `i` (0-based, the order of `values`) in place, only while someone looks: its last value, its
    /// running window (count, sum, min, max), started again when the debugger moved on to the next window, and the
    /// value at the end of its thread's pipe.
    pub fn set(&self, i: usize, value: f64) {
        let n = self.n;
        if i >= n || !self.watched() {
            return;
        }
        let p = self.ptr as *mut f64;
        // SAFETY: as in `watched`; `i < n` and `len = 4 + 10 n` keep every word below inside the block.
        let (kit, pipe) = unsafe {
            let w = |k: usize, v: f64| std::ptr::write_volatile(p.add(k), v);
            let r = |k: usize| std::ptr::read_volatile(p.add(k));
            w(1 + i, value);
            let window = r(n + 1);
            let live = n + 2 + 5 * i;
            if r(live) != window {
                w(live, window);
                w(live + 1, 1.0);
                w(live + 2, value);
                w(live + 3, value);
                w(live + 4, value);
            } else {
                w(live + 1, r(live + 1) + 1.0);
                w(live + 2, r(live + 2) + value);
                w(live + 3, r(live + 3).min(value));
                w(live + 4, r(live + 4).max(value));
            }
            (r(10 * n + 2) as u16, r(10 * n + 3) as usize)
        };
        let (seen, mut writer) = self.pipe.get();
        if seen != pipe {
            // SAFETY: the debugger's pipe for this block's thread, alive until it unloads (or 0: none).
            writer = unsafe { Writer::at(pipe) };
            self.pipe.set((pipe, writer));
        }
        writer.push(Record {
            kit,
            item: i as u16,
            kind: KIND_VALUE,
            tick: self.tick.get(),
            value,
            extra: 0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_handle_ignores_every_write() {
        let c = Counters::default();
        assert!(!c.watched());
        c.set(0, 1.0); // no block: nothing written, no panic
    }

    #[test]
    fn a_block_is_written_only_while_someone_looks() {
        use crate::debug_pipe::PipeMem;
        // Two counters: 4 + 10 * 2 words, window 1, kit number 3, no pipe yet.
        let mut block = vec![0.0f64; 24];
        block[3] = 1.0;
        block[22] = 3.0;
        // SAFETY: `block` outlives `c`.
        let c = unsafe { Counters::at(block.as_mut_ptr() as usize, 24, 2) };
        assert!(c.n == 2, "a block of layout 4");
        c.set(0, 5.0);
        assert_eq!(block[1], 0.0, "nobody looks: nothing kept");
        block[0] = 1.0;
        assert!(c.watch(7));
        c.set(0, 5.0);
        c.set(0, 3.0);
        c.set(1, 7.0);
        c.set(2, 9.0); // out of the block: ignored
        assert_eq!(&block[1..3], &[3.0, 7.0], "the last values");
        assert_eq!(
            &block[4..9],
            &[1.0, 2.0, 8.0, 3.0, 5.0],
            "window 1 of counter 0: count, sum, min, max"
        );
        block[3] = 2.0; // the debugger moved on
        c.set(0, 4.0);
        assert_eq!(
            &block[4..9],
            &[2.0, 1.0, 4.0, 4.0, 4.0],
            "a new window starts again"
        );
        // A panel opened: the pipe of the thread is there, every value goes at its end, a one-tick peak as it is.
        let pipe = PipeMem::new(16, 0);
        block[23] = pipe.ptr() as f64;
        c.watch(8);
        c.set(1, 99.0);
        c.set(0, 6.0);
        let mut got = Vec::new();
        pipe.reader()
            .drain(|a, b| got.extend(a.iter().chain(b).map(|r| (r.kit, r.item, r.tick, r.value))));
        assert_eq!(got, vec![(3, 1, 8, 99.0), (3, 0, 8, 6.0)]);
    }

    #[test]
    fn a_block_of_another_size_is_refused() {
        let mut block = vec![0.0f64; 33];
        // SAFETY: `block` outlives the handle.
        let c = unsafe { Counters::at(block.as_mut_ptr() as usize, 33, 2) };
        assert!(c.n == 0 && !c.watched(), "layout 3: refused");
    }
}
