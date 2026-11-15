//! The shared zone as a kit module sees it (docs/kit-modules.md §7; layout in bytes: gripforge-kitcall
//! src/zone.rs): fixed buffers reached by **offsets**, never an address kept — every access is one of the accessors
//! below.
//!
//! - A kit module of the common memory: the zone is in the memory it shares with the core, each accessor is a load
//!   or a store where the zone is (crate::mem), the two copies a `memcpy`.
//! - An isolated kit module: each accessor is a direct link (`gf:kithost.zone@^1`), a wasm-to-wasm call to the
//!   core's `gfx_zone_*`; the two copies (`zone_copy_out`, `zone_copy_in`) are served by the calls host, which
//!   sees both memories and refuses a write outside what the kit may write.
//!
//! Out of bounds: a read answers 0, a write does nothing, a copy answers false.
//!
//! ```ignore
//! use crate::zone::{self, buffers};
//! let world = buffers().world;                       // where the directory says the buffer is (asked once)
//! let x = zone::zone_read_f64(world.offset + 16);
//! zone::zone_write_u32(world.offset, 7);
//! let mut block = [0u8; 64];
//! zone::zone_copy(world.offset + 64, &mut block);    // zone → this module
//! zone::zone_copy_in(world.offset + 64, &block);     // this module → zone
//! ```

pub use gripforge_abi::zone::{self as layout, Buffer, Buffers};

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
// (In the common memory the typed accessors load and store in place: these links are then not used.)
#[cfg_attr(feature = "common-memory", allow(dead_code))]
mod raw {
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.zone@^1")]
    extern "C" {
        pub fn zone_read_u8(off: u32) -> u32;
        pub fn zone_read_u16(off: u32) -> u32;
        pub fn zone_read_u32(off: u32) -> u32;
        pub fn zone_read_u64(off: u32) -> u64;
        pub fn zone_read_f32(off: u32) -> f32;
        pub fn zone_read_f64(off: u32) -> f64;
        pub fn zone_write_u8(off: u32, v: u32);
        pub fn zone_write_u16(off: u32, v: u32);
        pub fn zone_write_u32(off: u32, v: u32);
        pub fn zone_write_u64(off: u32, v: u64);
        pub fn zone_write_f32(off: u32, v: f32);
        pub fn zone_write_f64(off: u32, v: f64);
        /// zone → the caller's memory; 1 when copied.
        #[cfg(not(feature = "common-memory"))]
        pub fn zone_copy_out(off: u32, ptr: *mut u8, len: u32) -> u32;
        /// The caller's memory → zone; 1 when copied.
        #[cfg(not(feature = "common-memory"))]
        pub fn zone_copy_in(off: u32, ptr: *const u8, len: u32) -> u32;
        pub fn zone_region(len: u32) -> u32;
        pub fn zone_region_free(off: u32) -> u32;
        pub fn zone_tick_region(len: u32) -> u32;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    pub use crate::native::link::{zone_region, zone_region_free, zone_tick_region};
    // In the common memory the zone is in this module's memory: the two copies are made here (crate::mem).
    #[cfg(feature = "common-memory")]
    pub use crate::mem::{zone_copy_in, zone_copy_out};
}

/// Off wasm (the façade checked or tested natively): a zone of this process with the same directory.
#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
mod raw {
    use gripforge_abi::zone as layout;
    use std::cell::RefCell;
    thread_local! {
        static ZONE: RefCell<Vec<u8>> = RefCell::new({
            let mut z = vec![0u8; layout::SIZE as usize];
            layout::write_directory(&mut z);
            z
        });
    }
    fn get<const N: usize>(off: u32) -> [u8; N] {
        ZONE.with(|z| {
            z.borrow()
                .get(off as usize..off as usize + N)
                .map_or([0; N], |b| b.try_into().unwrap())
        })
    }
    fn put(off: u32, b: &[u8]) -> u32 {
        ZONE.with(|z| {
            z.borrow_mut()
                .get_mut(off as usize..off as usize + b.len())
                .map_or(0, |d| {
                    d.copy_from_slice(b);
                    1
                })
        })
    }
    pub unsafe fn zone_read_u8(off: u32) -> u32 {
        get::<1>(off)[0] as u32
    }
    pub unsafe fn zone_read_u16(off: u32) -> u32 {
        u16::from_le_bytes(get(off)) as u32
    }
    pub unsafe fn zone_read_u32(off: u32) -> u32 {
        u32::from_le_bytes(get(off))
    }
    pub unsafe fn zone_read_u64(off: u32) -> u64 {
        u64::from_le_bytes(get(off))
    }
    pub unsafe fn zone_read_f32(off: u32) -> f32 {
        f32::from_le_bytes(get(off))
    }
    pub unsafe fn zone_read_f64(off: u32) -> f64 {
        f64::from_le_bytes(get(off))
    }
    pub unsafe fn zone_write_u8(off: u32, v: u32) {
        put(off, &[v as u8]);
    }
    pub unsafe fn zone_write_u16(off: u32, v: u32) {
        put(off, &(v as u16).to_le_bytes());
    }
    pub unsafe fn zone_write_u32(off: u32, v: u32) {
        put(off, &v.to_le_bytes());
    }
    pub unsafe fn zone_write_u64(off: u32, v: u64) {
        put(off, &v.to_le_bytes());
    }
    pub unsafe fn zone_write_f32(off: u32, v: f32) {
        put(off, &v.to_le_bytes());
    }
    pub unsafe fn zone_write_f64(off: u32, v: f64) {
        put(off, &v.to_le_bytes());
    }
    pub unsafe fn zone_copy_out(off: u32, ptr: *mut u8, len: u32) -> u32 {
        ZONE.with(|z| {
            z.borrow()
                .get(off as usize..off as usize + len as usize)
                .map_or(0, |b| {
                    std::ptr::copy_nonoverlapping(b.as_ptr(), ptr, b.len());
                    1
                })
        })
    }
    pub unsafe fn zone_copy_in(off: u32, ptr: *const u8, len: u32) -> u32 {
        put(off, std::slice::from_raw_parts(ptr, len as usize))
    }
    thread_local! {
        static NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(layout::REGIONS.offset + layout::regions::FIRST) };
    }
    // Off wasm the regions are a bump (nothing is given back): enough for the façade's native checks.
    pub unsafe fn zone_region(len: u32) -> u32 {
        NEXT.with(|n| {
            let next = n.get();
            let end = next.saturating_add(len.saturating_add(7) & !7);
            if end > layout::SIZE {
                return 0;
            }
            n.set(end);
            next
        })
    }
    pub unsafe fn zone_region_free(_off: u32) -> u32 {
        1
    }
    pub unsafe fn zone_tick_region(len: u32) -> u32 {
        zone_region(len)
    }
}

macro_rules! accessors {
    ($($(#[$doc:meta])* $read:ident $write:ident $t:ty, $wire:ty;)*) => {$(
        $(#[$doc])*
        #[inline(always)]
        pub fn $read(off: u32) -> $t {
            // In the common memory the zone is in this module's memory: a load where it is, no call.
            #[cfg(feature = "common-memory")]
            return crate::mem::zone_load::<$t>(off);
            // SAFETY: an import wired by the calls host to the zone; it checks the bounds.
            #[cfg(not(feature = "common-memory"))]
            unsafe { raw::$read(off) as $t }
        }
        #[inline(always)]
        pub fn $write(off: u32, v: $t) {
            #[cfg(feature = "common-memory")]
            return crate::mem::zone_store::<$t>(off, v);
            // SAFETY: as the read.
            #[cfg(not(feature = "common-memory"))]
            unsafe { raw::$write(off, v as $wire) }
        }
    )*};
}

accessors! {
    /// One byte of the zone.
    zone_read_u8 zone_write_u8 u8, u32;
    zone_read_u16 zone_write_u16 u16, u32;
    zone_read_u32 zone_write_u32 u32, u32;
    zone_read_u64 zone_write_u64 u64, u64;
    zone_read_f32 zone_write_f32 f32, f32;
    zone_read_f64 zone_write_f64 f64, f64;
}

/// Copies a block of the zone into `out` (zone → this module). False: out of the zone, nothing copied.
#[inline]
pub fn zone_copy(off: u32, out: &mut [u8]) -> bool {
    // SAFETY: `out` is this module's memory, `len` its length.
    unsafe { raw::zone_copy_out(off, out.as_mut_ptr(), out.len() as u32) != 0 }
}

/// Copies `src` into the zone at `off` (this module → zone). False: out of the zone, nothing copied.
#[inline]
pub fn zone_copy_in(off: u32, src: &[u8]) -> bool {
    // SAFETY: as `zone_copy`.
    unsafe { raw::zone_copy_in(off, src.as_ptr(), src.len() as u32) != 0 }
}

/// A block of the `REGIONS` buffer (8-byte aligned, zeroed): its offset, or 0 when the zone is at its ceiling (the
/// zone grows when the buffer is full; offsets never move). Given back with [`zone_region_free`].
pub fn zone_region(len: u32) -> u32 {
    // SAFETY: as the accessors.
    unsafe { raw::zone_region(len) }
}

/// Gives a block of [`zone_region`] back (false: not a block in use).
pub fn zone_region_free(off: u32) -> bool {
    // SAFETY: as the accessors.
    unsafe { raw::zone_region_free(off) != 0 }
}

/// A block that lives until the next tick (the tick's arena of the core): nothing to give back.
pub fn zone_tick_region(len: u32) -> u32 {
    // SAFETY: as the accessors.
    unsafe { raw::zone_tick_region(len) }
}

thread_local! {
    static GENERATION: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

/// The layout of the zone may have changed (a new game schema was given to the core): read the directory again.
/// Called when a kit is mounted; what a module derived from [`buffers`] is tagged with [`generation`].
pub fn refresh() {
    GENERATION.with(|g| g.set(g.get().wrapping_add(1).max(1)));
}

/// Changes at every [`refresh`]: the tag of what was derived from the directory.
pub fn generation() -> u32 {
    GENERATION.with(std::cell::Cell::get)
}

/// Where the buffers are in the zone of this game, read from the zone's directory (once per [`refresh`]: the core
/// lays the zone out from the game schema before the game starts). A zone of another major
/// layout gives buffers of size 0: the readers then keep to the calls.
pub fn buffers() -> Buffers {
    thread_local! {
        static BUFFERS: std::cell::Cell<Option<(u32, Buffers)>> = const { std::cell::Cell::new(None) };
    }
    let generation = generation();
    BUFFERS.with(|c| {
        if let Some((g, b)) = c.get() {
            if g == generation {
                return b;
            }
        }
        let none = |id| Buffer {
            id,
            offset: 0,
            size: 0,
        };
        let b = Buffers::read(zone_read_u32).unwrap_or(Buffers {
            input: none(1),
            state: none(2),
            events: none(3),
            world: none(4),
            regions: none(5),
        });
        c.set(Some((generation, b)));
        b
    })
}

/// One buffer of [`buffers`], where and how long: what the readers of the hot paths ask at every read (an entity
/// row, a var of the state, the event queue) — its own small cell, where [`buffers`] copies the five.
macro_rules! one_buffer {
    ($($(#[$doc:meta])* $name:ident),*) => {$(
        $(#[$doc])*
        #[inline]
        pub fn $name() -> Buffer {
            thread_local! {
                static AT: std::cell::Cell<(u32, Buffer)> = const { std::cell::Cell::new((0, Buffer { id: 0, offset: 0, size: 0 })) };
            }
            let generation = generation();
            let (known, b) = AT.with(std::cell::Cell::get);
            if known == generation {
                return b;
            }
            let b = buffers().$name;
            AT.with(|c| c.set((generation, b)));
            b
        }
    )*};
}
one_buffer!(
    /// The `INPUT` buffer (core.input' records, the pointer total, the look).
    input,
    /// The `WORLD` buffer (the entity rows).
    world,
    /// The `STATE` buffer.
    state,
    /// The `EVENTS` buffer.
    events
);

/// The look of player 0 now, read in place (the core's `kitcall::zone::look_now`): `look + (pointer - taken) × gain`,
/// as core.input wrote it at its last tick and the pointer moved since. `None` while no look is published.
pub fn look_now() -> Option<(f64, f64)> {
    use layout::input as l;
    let at = input().offset;
    if at == 0 || zone_read_u32(at + l::LOOK_ACTION) == l::NO_ACTION {
        return None;
    }
    let f = |off: u32| zone_read_f64(at + off);
    Some((
        f(l::LOOK) + (f(l::POINTER) - f(l::TAKEN)) * f(l::GAIN),
        f(l::LOOK + 8) + (f(l::POINTER + 8) - f(l::TAKEN + 8)) * f(l::GAIN + 8),
    ))
}

#[cfg(all(test, not(feature = "common-memory")))]
mod tests {
    use super::*;

    // With `common-memory` (switched on for the whole workspace by feature unification, as soon as game.engine is
    // built with it) the zone is the host's: it exists only once a host has loaded the module.
    #[test]
    fn local_zone() {
        let b = buffers();
        assert_eq!(b.input, layout::INPUT);
        let at = zone_region(24);
        assert!(at != 0);
        zone_write_f64(at, -2.5);
        zone_write_u8(at + 8, 0x1ff_u32 as u8);
        assert_eq!(zone_read_f64(at), -2.5);
        assert_eq!(zone_read_u8(at + 8), 0xff);
        let mut out = [0u8; 9];
        assert!(zone_copy(at, &mut out));
        assert_eq!(out[8], 0xff);
        assert!(zone_copy_in(at + 16, &out[..8]));
        assert_eq!(zone_read_f64(at + 16), -2.5);
        assert!(!zone_copy(layout::SIZE - 4, &mut out));
        assert_eq!(zone_read_u32(layout::SIZE), 0);
    }
}
