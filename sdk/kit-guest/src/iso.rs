//! The kit side of an **isolated** kit module (feature `isolated`, docs/kit-third-party.md « Isolated »): a kit of
//! a third party the game does not trust with its memory. The module owns its memory and its allocator; it never
//! holds an address of the core's memory, and the core never holds one of its own. What it exchanges with the core
//! are the structures of the contract (docs/kit-structures.md), nothing else:
//!
//! - a hook ([`enter`], exported as `gfx_iso_enter`) and a call to a core interface ([`call`], the import
//!   `gf:kithost.iso@^1` / `iso_call`) have the signature of the common memory (`gripforge_abi::mem::Enter`):
//!   a function number, the message by address, the answer as a [`Bytes`]. The **host** stands between the two
//!   memories: it copies the message into the other one — once — and the answer back;
//! - in a message, a list or a dictionary is a fixed-layout block that **crosses** (`gripforge_abi::T_CROSS`:
//!   the layout of `FixedBlock` / `FixedValue` / `FixedPair` with offsets where a block of one memory holds
//!   addresses), read where the copy landed. The written-out encoding of containers is not used;
//! - the shared zone is read through the accessors of `kithost.zone@1` (the host copies; it refuses a write outside
//!   the event queue and the regions the kit asked);
//! - events go through the queue of the zone, as for every kit.
//!
//! Not used: the bridge of the calls contract (`gf_on_bridge`, the registers), the kit → kit links by words
//! (`gfx_svc_call`): an isolated kit module does not export them.
//!
//! The kit's source does not change: [`isolated_kit!`] in the generated crate (the build tool writes it) makes the
//! exports.

use gripforge_abi::mem::Bytes;

#[cfg(all(
    target_arch = "wasm32",
    feature = "isolated",
    not(feature = "common-memory")
))]
#[link(wasm_import_module = "gf:kithost.iso@^1")]
extern "C" {
    /// A core interface function, called across the two memories: the host copies `args` into the core, runs the
    /// core's `gfx_iso_call`, and gives the answer in `out` as a buffer of [`alloc`] this module owns.
    fn iso_call(func: u32, args: *const u8, len: u32, out: *mut Bytes) -> u32;
}

/// A call to a core interface: `Err((status, message))` when it did not answer `ok`. Only an isolated module calls
/// this way (`call_core`); a module of the common memory calls by pointer.
#[cfg(all(
    target_arch = "wasm32",
    feature = "isolated",
    not(feature = "common-memory")
))]
pub(crate) fn call(func: u32, args: &[u8]) -> Result<Vec<u8>, (u16, String)> {
    let mut out = Bytes::default();
    // SAFETY: the host reads `args` in this module's memory and writes `out` with a buffer it asked of `alloc`
    // (`len` bytes, alignment 1, capacity `len`): a `Vec<u8>` of this module's allocator.
    unsafe {
        let status = iso_call(func, args.as_ptr(), args.len() as u32, &mut out);
        let bytes = out.take();
        if status != 0 {
            return Err((status as u16, String::from_utf8_lossy(&bytes).into_owned()));
        }
        Ok(bytes)
    }
}

/// The entry of this kit module (`gfx_iso_enter`): one function of `kithost.kit@1`. The arguments were copied
/// into this module's memory by the host (a buffer of [`alloc`], which the host frees after the call); the answer
/// is given in `out`, a `Vec<u8>` of this module the host copies out and frees ([`free`]).
///
/// # Safety
/// `args..args+len` and `out` are in this module's memory, valid for the call.
pub unsafe fn enter(func: u32, args: *const u8, len: u32, out: *mut Bytes) -> u32 {
    let input = if len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(args, len as usize)
    };
    let (status, bytes) = match crate::host::entry_peer(func, input, false, true) {
        Ok(b) => (0, b),
        Err(msg) => (
            gripforge_abi::calls::Status::Failed as u32,
            msg.into_bytes(),
        ),
    };
    out.write(Bytes::give(bytes));
    status
}

/// `gfx_iso_alloc(size, align)`: a block of this module's allocator the host writes a message into (0: no room).
///
/// # Safety
/// `align` is a power of two.
pub unsafe fn alloc(size: usize, align: usize) -> *mut u8 {
    match std::alloc::Layout::from_size_align(size.max(1), align.max(1)) {
        Ok(layout) => std::alloc::alloc(layout),
        Err(_) => std::ptr::null_mut(),
    }
}

/// `gfx_iso_free(ptr, size, align)`: gives a block of [`alloc`] (or the buffer of an answer) back.
///
/// # Safety
/// `ptr` is a block of this allocator asked with `size` and `align`, not given back yet.
pub unsafe fn free(ptr: *mut u8, size: usize, align: usize) {
    if !ptr.is_null() {
        if let Ok(layout) = std::alloc::Layout::from_size_align(size.max(1), align.max(1)) {
            std::alloc::dealloc(ptr, layout)
        }
    }
}

/// The contracts of an isolated kit module (its custom section `gripforge.calls`): what every kit module consumes,
/// and `kithost.iso` — the calls across two memories on the structures of the contract. The widest list, as
/// `crate::host::CONTRACTS` (tools/kit-modules/contracts.mjs `sealKit` writes the real one).
pub const CONTRACTS: &str = "provides kithost.kit@1.1.0\nconsumes kithost.world@^1.3\nconsumes kithost.state@^2.0\nconsumes kithost.physics@^1.3\nconsumes kithost.services@^1.1\nconsumes kithost.input@^2.0\nconsumes kithost.assets@^1.0\nconsumes kithost.platform@^1.0\nconsumes kithost.zone@^1.0\nconsumes kithost.iso@^1.0\n";

/// Makes a crate an **isolated** kit module: `isolated_kit!(kit::module, "1.0.0", 0);` — its own memory, the entry
/// across memories, the structures table, the contracts section. In place of `export_kit!` (never beside it); the
/// crate turns the feature `isolated` of this crate on. Written by the build tool (`gripforge kit build`).
#[macro_export]
macro_rules! isolated_kit {
    ($module:path, $version:expr, $hooks:expr) => {
        // The frame of the calls contract (`gf_calls_abi`, `gf_calls_init`, `gf_calls_exit`, `gf_alloc`). Its bridge
        // is exported with it and never called: an isolated kit module registers no function there.
        $crate::gripforge_abi::calls::export_calls!();
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn gf_calls_init() {
            $crate::host::init_entries($module(), $version, $hooks);
        }
        // The contracts this module reads or writes, checked by the host at load (gripforge_abi::contract).
        $crate::kitcall::contracts_section!($crate::contracts::USES);
        /// 1: this module speaks to the core across two memories on the structures of the contract.
        #[no_mangle]
        pub extern "C" fn gf_iso_abi() -> u32 {
            1
        }
        /// Fixed-layout blocks this module wrote for the core (0) and received from it: 1 copied whole into a
        /// value it keeps, 2 written over a copy it had (the QA reads it).
        #[no_mangle]
        pub extern "C" fn gf_iso_stats(which: u32) -> u64 {
            match which {
                0 => $crate::kitcall::fixed::crossed_out(),
                n => $crate::kitcall::fixed::stats::get()
                    .get(n as usize - 1)
                    .copied()
                    .unwrap_or(0),
            }
        }
        #[no_mangle]
        pub unsafe extern "C" fn gfx_iso_enter(
            func: u32,
            args: *const u8,
            len: u32,
            out: *mut $crate::kitcall::mem::Bytes,
        ) -> u32 {
            $crate::iso::enter(func, args, len, out)
        }
        #[no_mangle]
        pub unsafe extern "C" fn gfx_iso_alloc(size: usize, align: usize) -> *mut u8 {
            $crate::iso::alloc(size, align)
        }
        #[no_mangle]
        pub unsafe extern "C" fn gfx_iso_free(ptr: *mut u8, size: usize, align: usize) {
            $crate::iso::free(ptr, size, align)
        }
        // No `gfx_kit_hook`: a function of this module is never put in the core's table (a trap in it would unwind
        // through the core). Every hook comes through `gfx_iso_enter`, where the host catches a trap.
        #[cfg(target_arch = "wasm32")]
        #[link_section = "gripforge.calls"]
        #[used]
        static GF_CONTRACTS: [u8; $crate::iso::CONTRACTS.len()] = {
            let s = $crate::iso::CONTRACTS.as_bytes();
            let mut out = [0u8; $crate::iso::CONTRACTS.len()];
            let mut i = 0;
            while i < s.len() {
                out[i] = s[i];
                i += 1;
            }
            out
        };
    };
}
