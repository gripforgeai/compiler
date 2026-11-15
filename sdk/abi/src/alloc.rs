//! The allocator of the kit logic modules — every kit module and the core (docs/kit-modules.md §7): size classes with free lists, in front of the system
//! allocator. A kit module allocates and frees small blocks all the time (values, strings, the arguments and the
//! answers of the calls): a quarter of the time of the kit modules was the general allocator. Here an allocation
//! of at most 4 KiB is a pop of the free list of its class (powers of two from 16 bytes), a free is a push; blocks
//! are carved from 64 KiB chunks asked of the system allocator and never given back (the memory of a module stays
//! at its peak, as it does with any allocator in wasm). Larger blocks go to the system allocator.
//!
//! A wasm module of a plain memory runs on one thread: the lists are plain statics. Built for a memory shared
//! between threads (`target_feature = "atomics"`: the Bevy player's shared memory, a Worker that joins it, docs/kit-modules.md
//! §7.19) each thread keeps lists and a chunk of its own (thread-locals): no lock, no atomic on the path of an
//! allocation, the same pop and push as on one thread. A block freed on another thread than the one that carved it
//! goes to the lists of the thread that frees it (a block is only bytes of its class: any thread may give it out
//! again). What a thread still holds in its lists when it ends stays carved (as the memory of a module stays at
//! its peak); the large blocks go to the system allocator, which locks.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;

const MIN_SHIFT: u32 = 4;
const MAX_SHIFT: u32 = 12;
const CLASSES: usize = (MAX_SHIFT - MIN_SHIFT + 1) as usize;
const CHUNK: usize = 64 * 1024;

struct State {
    free: [*mut u8; CLASSES],
    bump: usize,
    end: usize,
}

pub struct KitAlloc {
    #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
    state: UnsafeCell<State>,
}

// SAFETY: a wasm module without threads (the one state), or each thread its own state (thread-locals, built with
// atomics); natively the allocator is not installed.
unsafe impl Sync for KitAlloc {}

#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
thread_local! {
    /// The lists and the chunk of this thread (a memory shared between threads).
    static STATE: UnsafeCell<State> = const { UnsafeCell::new(State { free: [std::ptr::null_mut(); CLASSES], bump: 0, end: 0 }) };
}

impl KitAlloc {
    pub const fn new() -> KitAlloc {
        KitAlloc {
            #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
            state: UnsafeCell::new(State {
                free: [std::ptr::null_mut(); CLASSES],
                bump: 0,
                end: 0,
            }),
        }
    }

    /// The state of the calling thread (the one state without threads).
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    unsafe fn state(&self) -> &mut State {
        #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
        {
            &mut *STATE.with(|s| s.get())
        }
        #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
        {
            &mut *self.state.get()
        }
    }
}

impl Default for KitAlloc {
    fn default() -> KitAlloc {
        KitAlloc::new()
    }
}

/// The class of a layout: the power of two that holds its size and its alignment; `None` past the classes.
#[inline]
fn class_of(layout: Layout) -> Option<usize> {
    let size = layout.size().max(layout.align());
    if size > 1 << MAX_SHIFT {
        return None;
    }
    let shift = (usize::BITS - (size.max(1 << MIN_SHIFT) - 1).leading_zeros()).max(MIN_SHIFT);
    Some((shift - MIN_SHIFT) as usize)
}

unsafe impl GlobalAlloc for KitAlloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some(class) = class_of(layout) else {
            return System.alloc(layout);
        };
        let s = self.state();
        let head = s.free[class];
        if !head.is_null() {
            s.free[class] = *(head as *mut *mut u8);
            return head;
        }
        let size = 1usize << (class as u32 + MIN_SHIFT);
        // A block of 2^k bytes at a multiple of 2^k: aligned for every layout of its class.
        let mut at = (s.bump + size - 1) & !(size - 1);
        if at + size > s.end {
            let chunk = System.alloc(Layout::from_size_align_unchecked(CHUNK, 1 << MAX_SHIFT));
            if chunk.is_null() {
                return chunk;
            }
            at = chunk as usize;
            s.end = at + CHUNK;
        }
        s.bump = at + size;
        at as *mut u8
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let Some(class) = class_of(layout) else {
            return System.dealloc(ptr, layout);
        };
        let s = self.state();
        *(ptr as *mut *mut u8) = s.free[class];
        s.free[class] = ptr;
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = Layout::from_size_align_unchecked(new_size, layout.align());
        match (class_of(layout), class_of(new)) {
            // The block already holds the new size.
            (Some(a), Some(b)) if a == b => ptr,
            (None, None) => System.realloc(ptr, layout, new_size),
            _ => {
                let to = self.alloc(new);
                if !to.is_null() {
                    std::ptr::copy_nonoverlapping(ptr, to, layout.size().min(new_size));
                    self.dealloc(ptr, layout);
                }
                to
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_hold_their_layouts() {
        let l = |s, a| Layout::from_size_align(s, a).unwrap();
        assert_eq!(class_of(l(1, 1)), Some(0));
        assert_eq!(class_of(l(16, 8)), Some(0));
        assert_eq!(class_of(l(17, 1)), Some(1));
        assert_eq!(class_of(l(4096, 8)), Some(8));
        assert_eq!(class_of(l(4097, 8)), None);
        assert_eq!(class_of(l(8, 64)), Some(2));
        assert_eq!(class_of(l(8, 8192)), None);
    }

    #[test]
    fn blocks_are_reused_aligned_and_grow_in_place_within_a_class() {
        let a = KitAlloc::new();
        unsafe {
            let l = Layout::from_size_align(40, 8).unwrap();
            let p = a.alloc(l);
            assert_eq!(p as usize % 64, 0);
            p.write_bytes(7, 40);
            assert_eq!(a.realloc(p, l, 64), p);
            let q = a.realloc(p, l, 65);
            assert_ne!(q, p);
            assert_eq!(*q.add(39), 7);
            // The block of 64 was freed by the realloc: the next of its class takes it.
            assert_eq!(a.alloc(Layout::from_size_align(50, 1).unwrap()), p);
            let big = Layout::from_size_align(100_000, 8).unwrap();
            let b = a.alloc(big);
            assert!(!b.is_null());
            a.dealloc(b, big);
            // Many blocks: across chunks, all distinct.
            let mut seen = std::collections::HashSet::new();
            for _ in 0..200 {
                assert!(seen.insert(a.alloc(Layout::from_size_align(4096, 16).unwrap()) as usize));
            }
        }
    }
}
