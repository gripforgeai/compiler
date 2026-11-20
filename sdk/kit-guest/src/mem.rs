//! The kit side of the common memory (feature `common-memory`, docs/kit-modules.md « Common memory »): this kit
//! module is built relocatable and imports the memory the core lives in; its allocator is the core's
//! (`gf:kithost.mem@^1`), so bytes go from one module to the other by their address:
//!
//! - a call to a core interface ([`call`]) passes the address of the arguments and receives a `Vec` it owns
//!   (the bridge copies the arguments into the core, the answer back, then once more into a `Vec`);
//! - a hook ([`enter`], exported as `gfx_mem_enter` and called by the core through its function table) reads its
//!   arguments where the core wrote them and gives its answer away;
//! - the shared zone is in the same memory: its two copies are a `memcpy` here ([`zone_copy_out`]).
//!
//! The kit's source does not change: [`common_memory!`] in the generated crate sets the allocator and the exports.

#[cfg(target_arch = "wasm32")]
use gripforge_abi::mem::Bytes;
use std::alloc::{GlobalAlloc, Layout};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.mem@^1")]
extern "C" {
    fn mem_alloc(size: usize, align: usize) -> *mut u8;
    fn mem_alloc_zeroed(size: usize, align: usize) -> *mut u8;
    fn mem_free(ptr: *mut u8, size: usize, align: usize);
    fn mem_realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8;
    fn mem_call(func: u32, args: *const u8, len: u32, out: *mut Bytes) -> u32;
    fn mem_zone_place() -> u32;
}
#[cfg(not(target_arch = "wasm32"))]
use crate::native::{mem_alloc, mem_alloc_zeroed, mem_free, mem_realloc};

/// Where the core keeps the zone's address and its bytes current (two words, `kithost.mem@1.3`): asked once, the
/// same for the life of the memory.
#[cfg(target_arch = "wasm32")]
#[inline(always)]
fn zone_place() -> *const u32 {
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
    static PLACE: AtomicU32 = AtomicU32::new(0);
    let p = PLACE.load(Relaxed);
    if p != 0 {
        return p as usize as *const u32;
    }
    // SAFETY: an import wired by the calls host to the core's export; it answers the address of a static.
    let p = unsafe { mem_zone_place() };
    PLACE.store(p, Relaxed);
    p as usize as *const u32
}

/// The address of the shared zone now (it moves when the zone grows: never kept) — a load, no call.
#[cfg(target_arch = "wasm32")]
#[inline(always)]
pub(crate) fn zone_base() -> usize {
    // SAFETY: two words of the core (atomics there), in the memory this module shares with it.
    unsafe {
        (*(zone_place() as *const std::sync::atomic::AtomicU32))
            .load(std::sync::atomic::Ordering::Relaxed) as usize
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use crate::native::{zone_base, zone_size};

/// Bytes of the shared zone now.
#[cfg(target_arch = "wasm32")]
#[inline(always)]
pub(crate) fn zone_size() -> u32 {
    // SAFETY: as `zone_base`.
    unsafe {
        (*(zone_place().add(1) as *const std::sync::atomic::AtomicU32))
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Reads at an offset of the zone, where it is (0 out of bounds; any alignment): the zone is in this module's memory.
#[inline(always)]
pub(crate) fn zone_load<T: Copy + Default>(off: u32) -> T {
    if off as u64 + std::mem::size_of::<T>() as u64 > zone_size() as u64 {
        return T::default();
    }
    // SAFETY: in bounds of the zone as it is now.
    unsafe { ((zone_base() + off as usize) as *const T).read_unaligned() }
}

/// Writes at an offset of the zone, where it is (nothing out of bounds).
#[inline(always)]
pub(crate) fn zone_store<T: Copy>(off: u32, v: T) {
    if off as u64 + std::mem::size_of::<T>() as u64 > zone_size() as u64 {
        return;
    }
    // SAFETY: as `zone_load`.
    unsafe { ((zone_base() + off as usize) as *mut T).write_unaligned(v) }
}

/// The core's allocator as this module's `#[global_allocator]` (set by [`common_memory!`]).
pub struct CoreAlloc;

// SAFETY: the four functions are the core's `std::alloc` ones, called with the same layouts.
unsafe impl GlobalAlloc for CoreAlloc {
    #[inline]
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        mem_alloc(l.size(), l.align())
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        mem_alloc_zeroed(l.size(), l.align())
    }
    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        mem_free(p, l.size(), l.align())
    }
    #[inline]
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new_size: usize) -> *mut u8 {
        mem_realloc(p, l.size(), l.align(), new_size)
    }
}

/// A call to a core interface by pointer: `Err((status, message))` when it did not answer `ok`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use crate::native::call;
#[cfg(target_arch = "wasm32")]
pub(crate) fn call(func: u32, args: &[u8]) -> Result<Vec<u8>, (u16, String)> {
    let mut out = Bytes::default();
    // SAFETY: the core reads `args` in the memory both share and gives its answer in `out`, allocated with the
    // allocator both use.
    let bytes = unsafe {
        let status = mem_call(func, args.as_ptr(), args.len() as u32, &mut out);
        let bytes = out.take();
        if status != 0 {
            return Err((status as u16, String::from_utf8_lossy(&bytes).into_owned()));
        }
        bytes
    };
    Ok(bytes)
}

/// The entry of this kit module (`gfx_mem_enter`): one function of `kithost.kit@1`, arguments by address.
///
/// # Safety
/// `args..args+len` and `out` are in the common memory, valid for the call.
#[cfg(target_arch = "wasm32")]
pub unsafe fn enter(func: u32, args: *const u8, len: u32, out: *mut Bytes) -> u32 {
    let input = if len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(args, len as usize)
    };
    // The caller is the core of the common memory (only it holds this entry): blocks pass both ways.
    let (status, bytes) = match crate::host::entry_from(func, input, true) {
        Ok(b) => (0, b),
        Err(msg) => (
            gripforge_abi::calls::Status::Failed as u32,
            msg.into_bytes(),
        ),
    };
    out.write(Bytes::give(bytes));
    status
}

/// zone → this module, in place (the zone is in the memory this module imports).
///
/// # Safety
/// `ptr..ptr+len` is writable.
pub unsafe fn zone_copy_out(off: u32, ptr: *mut u8, len: u32) -> u32 {
    if off as u64 + len as u64 > zone_size() as u64 {
        return 0;
    }
    std::ptr::copy_nonoverlapping((zone_base() + off as usize) as *const u8, ptr, len as usize);
    1
}

/// This module → zone, in place.
///
/// # Safety
/// `ptr..ptr+len` is readable.
pub unsafe fn zone_copy_in(off: u32, ptr: *const u8, len: u32) -> u32 {
    if off as u64 + len as u64 > zone_size() as u64 {
        return 0;
    }
    std::ptr::copy_nonoverlapping(ptr, (zone_base() + off as usize) as *mut u8, len as usize);
    1
}

/// `mem.zone_words(offset: u32, count: u32, add: u32) -> u64`, exposed by every kit module of the common memory
/// (by name, through the calls between modules): reads `count` 32-bit words of the shared zone from `offset`, adds
/// `add` to each in place, answers the sum of what it read. It needs nothing of the game, so it is what a kit
/// module placed on a Worker can be asked from the main thread — the proof that its code runs there on the same
/// memory (docs/kit-modules.md §9.4) — and a cheap probe of the zone anywhere.
pub fn zone_words(args: &[u8]) -> Result<Vec<u8>, String> {
    if args.len() != 12 {
        return Err(format!(
            "mem.zone_words takes (offset u32, count u32, add u32): 12 bytes, got {}",
            args.len()
        ));
    }
    let word = |i: usize| u32::from_le_bytes(args[i * 4..i * 4 + 4].try_into().unwrap());
    let (offset, count, add) = (word(0), word(1), word(2));
    let mut sum = 0u64;
    for i in 0..count {
        let at = offset + i * 4;
        let v = crate::zone::zone_read_u32(at);
        sum += v as u64;
        if add != 0 {
            crate::zone::zone_write_u32(at, v.wrapping_add(add));
        }
    }
    Ok(sum.to_le_bytes().to_vec())
}

/// `mem.column(op: u32, entity: u32, x: f64, y: f64, z: f64, module…, 0, column…) -> [status u32, pad u32, f64 × 3]`,
/// exposed by every kit module of the common memory: reads (`op` 0), writes (1) or adds to (2) the column of the
/// row of entity `e<entity>`. The rights and the write phases are those the game schema gives **the kit that
/// serves the call** (the module the host entered: nothing a caller writes here changes them — `module` only names
/// it in the refusals); the check is the one of `kx.column` / `kx.set_column`. Status 0 done (the three numbers are the column, after the
/// write), 1 the entity has no row; a refusal (no such column, not its right, not its phase) is the call's error,
/// in words. It needs the rows of the zone and nothing else of the game: what a kit module placed on a Worker is
/// asked from the main thread — the proof that its code reads and writes the world's columns there, in place,
/// under the schema (docs/kit-modules.md §9.4).
pub fn column(args: &[u8]) -> Result<Vec<u8>, String> {
    if args.len() < 32 {
        return Err(format!("mem.column takes (op u32, entity u32, 3 × f64, \"module\\0column\"): at least 32 bytes, got {}", args.len()));
    }
    let word = |i: usize| u32::from_le_bytes(args[i * 4..i * 4 + 4].try_into().unwrap());
    let num = |i: usize| f64::from_le_bytes(args[8 + i * 8..16 + i * 8].try_into().unwrap());
    let names = std::str::from_utf8(&args[32..])
        .map_err(|_| "mem.column: the names are not UTF-8".to_string())?;
    let (module, name) = names
        .split_once('\0')
        .ok_or("mem.column: \"module\\0column\" expected")?;
    let (op, id, v) = (word(0), format!("e{}", word(1)), [num(0), num(1), num(2)]);
    // The layout may be another game's since the last call (the owner took a new schema): nothing derived is kept.
    crate::zone::refresh();
    let done = match op {
        0 => true,
        1 => crate::columns::write(module, &id, name, v, false)?,
        2 => crate::columns::write(module, &id, name, v, true)?,
        _ => return Err(format!("mem.column: op {op} (0 read, 1 write, 2 add)")),
    };
    let got = crate::columns::read(module, &id, name)?;
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&u32::from(!(done && got.is_some())).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for x in got.unwrap_or([0.0; 3]) {
        out.extend_from_slice(&x.to_le_bytes());
    }
    Ok(out)
}

/// Makes the generated crate of a kit module one of the common memory: the core's allocator, the entry by
/// pointer, and the two heap symbols the allocator of `std` still names (a relocatable module has no heap of its
/// own). Invoked beside `export_kit!` by tools/kit-modules/build.mjs (`--common`).
#[macro_export]
macro_rules! common_memory {
    () => {
        // Only in wasm, where every module shares one memory (D3): natively each library keeps the system allocator
        // and frees only what it allocated (kithost.native@2 hands blocks back with their release).
        #[cfg(target_arch = "wasm32")]
        #[global_allocator]
        static GF_COMMON_ALLOC: $crate::mem::CoreAlloc = $crate::mem::CoreAlloc;
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub static __heap_base: u8 = 0;
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub static __heap_end: u8 = 0;
        /// What this module did with the fixed-layout blocks it received: 0 copied whole into a value it keeps,
        /// 1 written over a copy it had, 2 read in place (the QA reads it, per kit).
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn gf_fixed_stats(which: u32) -> u64 {
            $crate::kitcall::fixed::stats::get()
                .get(which as usize)
                .copied()
                .unwrap_or(0)
        }
        /// The entry of a native library of the kit (`kithost.native@1`).
        #[cfg(not(target_arch = "wasm32"))]
        #[no_mangle]
        pub unsafe extern "C" fn gfkit_native_load(
            host: *const $crate::kitcall::native::HostTable,
        ) -> *const $crate::kitcall::native::KitTable {
            $crate::native::load(host, gfx_native_start)
        }
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub unsafe extern "C" fn gfx_mem_enter(
            func: u32,
            args: *const u8,
            len: u32,
            out: *mut $crate::kitcall::mem::Bytes,
        ) -> u32 {
            $crate::mem::enter(func, args, len, out)
        }
    };
}
