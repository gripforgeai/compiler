//! The kit side of `kithost.native@1` (gripforge-kitcall `native`, docs/kit-modules-native.md): the common memory of
//! the kit modules when a module is a native library. The kit's source and its generated crate are the ones of the
//! wasm module; what wasm imports by name is here a table the host gave once ([`load`]).
//!
//! The answer of a hook and of a call is a `Vec` given away, both sides using the host's allocator; a panic of the
//! kit is the status `failed` with its message, never an unwinding across the C interface.

use gripforge_abi::fast::{HookCtx, Service, ServiceCall};
use gripforge_abi::native::{
    self as abi, HostTable, KitTable, NativeBytes, SvcProvider, ZonePlace,
};
use std::cell::Cell;
use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

static HOST: AtomicPtr<HostTable> = AtomicPtr::new(std::ptr::null_mut());

thread_local! {
    /// Where the zone of this thread is: the host's two words, asked once per thread. Per thread as the zone is: each
    /// thread that plays has its own game. No destructor and nothing allocated, so usable at any time.
    static ZONE: Cell<ZonePlace> = const {
        Cell::new(ZonePlace { base: std::ptr::null(), size: std::ptr::null() })
    };
}

#[inline(always)]
fn zone_place() -> ZonePlace {
    let place = ZONE.with(Cell::get);
    if !place.base.is_null() {
        return place;
    }
    ask_zone_place()
}

#[cold]
fn ask_zone_place() -> ZonePlace {
    // SAFETY: the table of the host, for the life of the process.
    let place = unsafe { (host().zone)() };
    ZONE.with(|z| z.set(place));
    place
}

#[inline(always)]
fn host() -> &'static HostTable {
    let p = HOST.load(Relaxed);
    assert!(!p.is_null(), "kit module: not loaded by a host");
    // SAFETY: stored once by `load`; the table of the host lives as long as the process.
    unsafe { &*p }
}

/// The host's allocator, once the module is loaded. Nothing allocates before `load` stores the table: it is the first
/// thing the host calls, and Rust has no life before it.
pub(crate) unsafe fn mem_alloc(size: usize, align: usize) -> *mut u8 {
    (host().alloc)(size, align)
}
pub(crate) unsafe fn mem_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    (host().alloc_zeroed)(size, align)
}
pub(crate) unsafe fn mem_free(ptr: *mut u8, size: usize, align: usize) {
    (host().free)(ptr, size, align)
}
pub(crate) unsafe fn mem_realloc(
    ptr: *mut u8,
    size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    (host().realloc)(ptr, size, align, new_size)
}

/// The address of the shared zone now (it moves when the zone grows: never kept): a load, no call.
#[inline(always)]
pub(crate) fn zone_base() -> usize {
    // SAFETY: a word of the host, alive as long as this thread (a zone that dies empties it).
    unsafe { (*zone_place().base).load(Relaxed) }
}

/// Bytes of the shared zone now: a load, no call.
#[inline(always)]
pub(crate) fn zone_size() -> u32 {
    // SAFETY: as `zone_base`.
    unsafe { (*zone_place().size).load(Relaxed) }
}

/// A call to a core interface by pointer: `Err((status, message))` when it did not answer `ok`.
pub(crate) fn call(func: u32, args: &[u8]) -> Result<Vec<u8>, (u16, String)> {
    let mut out = NativeBytes::default();
    // SAFETY: the host reads `args` for the call and gives its answer in `out`, with the allocator both use.
    let (status, bytes) = unsafe {
        let status = (host().call)(func, args.as_ptr(), args.len(), &mut out);
        (status, out.take())
    };
    if status != 0 {
        return Err((status as u16, String::from_utf8_lossy(&bytes).into_owned()));
    }
    Ok(bytes)
}

/// `svc::open` of the core: the provider's scalar entry and its context.
pub(crate) fn svc_open(capability: u32, provider: u32) -> Option<(Service, *const HookCtx)> {
    let (mut entry, mut ctx) = (None, std::ptr::null());
    // SAFETY: the host writes the two places for the call.
    if unsafe { (host().svc_open)(capability, provider, &mut entry, &mut ctx) } == 0 {
        return None;
    }
    entry.map(|f| (f, ctx))
}

fn said(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a kit module panicked".into())
}

/// The general entry: one function of `kithost.kit@1`, its arguments read where they are.
///
/// # Safety
/// `args..args+len` and `out` are valid for the call.
pub unsafe extern "C" fn enter(
    func: u32,
    args: *const u8,
    len: usize,
    out: *mut NativeBytes,
) -> u32 {
    let input = if len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(args, len)
    };
    // The caller is the core of the common memory: blocks pass both ways.
    let done = std::panic::catch_unwind(|| crate::host::entry_from(func, input, true))
        .unwrap_or_else(|p| Err(said(p)));
    let (status, bytes) = match done {
        Ok(b) => (0, b),
        Err(msg) => (
            gripforge_abi::calls::Status::Failed as u32,
            msg.into_bytes(),
        ),
    };
    out.write(NativeBytes::give(bytes));
    status
}

/// The short entry of the per-tick hooks. A panic: nothing is said to have run, the general entry then reports it.
///
/// # Safety
/// As `host::tick`.
unsafe extern "C" fn tick(func: u32, ctx: *const HookCtx, dt: f64) -> u32 {
    std::panic::catch_unwind(|| crate::host::tick(func, ctx, dt)).unwrap_or(1)
}

/// The scalar service entry. A panic: the call is answered `failed`.
///
/// # Safety
/// As `svc::serve_scalar`.
unsafe extern "C" fn svc(call: *mut ServiceCall) -> u32 {
    let at = call as usize;
    std::panic::catch_unwind(|| crate::svc::serve_scalar(at as *mut ServiceCall))
        .unwrap_or(gripforge_abi::calls::Status::Failed as u32)
}

/// A service of this kit called by another kit module: `svc::serve` behind the C interface. A panic: the status of an
/// error, with its message in the words.
unsafe extern "C" fn svc_call(capability: u32, method: u32, words: u32) -> u64 {
    std::panic::catch_unwind(|| crate::svc::serve(capability, method, words)).unwrap_or(1 << 32)
}

unsafe extern "C" fn svc_id() -> u32 {
    crate::svc::id()
}

/// The provider of a capability in this thread's kits, as a link of `service_links!` asks it at its first call.
pub fn svc_provider(capability: &str) -> SvcProvider {
    // SAFETY: the name is read for the call.
    unsafe { (host().svc_provider)(capability.as_ptr(), capability.len()) }
}

/// What the module sees and did, for a test or a measure ([`abi::view`]): read where it is, no effect on the game.
unsafe extern "C" fn view(which: u32) -> u64 {
    match which {
        abi::view::ZONE_BASE => zone_base() as u64,
        abi::view::ZONE_SIZE => zone_size() as u64,
        n if n >= abi::view::FIXED => gripforge_abi::fixed::stats::get()
            .get((n - abi::view::FIXED) as usize)
            .copied()
            .unwrap_or(0),
        _ => 0,
    }
}

/// `gfkit_native_load`, what `common_memory!` exports: keeps the host's table, runs the module's start on this thread,
/// answers the entries. Null: the host speaks another major, or lays out the contract otherwise.
///
/// # Safety
/// `table` is the host's table, alive for the life of the process.
pub unsafe fn load(table: *const HostTable, start: extern "C" fn()) -> *const KitTable {
    if table.is_null()
        || (*table).abi >> 16 != abi::ABI >> 16
        || (*table).layout != gripforge_abi::layout::probe()
    {
        return std::ptr::null();
    }
    HOST.store(table.cast_mut(), Relaxed);
    // Asked again at each load: a thread attached since names the zone of its owner.
    ask_zone_place();
    start();
    static KIT: KitTable = KitTable {
        struct_bytes: std::mem::size_of::<KitTable>() as u32,
        abi: abi::ABI,
        layout: gripforge_abi::layout::PROBE,
        enter,
        tick,
        svc,
        svc_call,
        svc_id,
        view,
    };
    &KIT
}

macro_rules! link_wrappers {
    ($($name:ident($($a:ident: $t:ty),*) -> $r:ty;)*) => {
        /// What a kit module of wasm imports from the core, by the same names.
        #[allow(clippy::missing_safety_doc, clippy::too_many_arguments)]
        pub mod link {
            use super::host;
            $(#[inline(always)]
            pub unsafe fn $name($($a: $t),*) -> $r {
                (host().links.$name)($($a),*)
            })*
        }
    };
}
gripforge_abi::native_links!(link_wrappers);
