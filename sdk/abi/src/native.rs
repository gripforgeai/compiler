//! `kithost.native@1`: a kit module loaded as a native library, in the memory of the process of its host
//! (docs/kit-modules-native.md). It is the common memory of the kit modules (docs/kit-modules.md §9) with addresses of
//! the machine: the same `kithost.*@1` contract, the same function numbers, the same fixed-layout blocks, its own
//! allocator per library (D3), the shared zone read and written where it is. What wasm gets from its linker — imports
//! wired by name, a shared function table — comes here from two `repr(C)` tables exchanged once at load
//! ([`HostTable`] → [`KitTable`], `gfkit_native_load`).
//!
//! Stability: this is a C interface, not a Rust one. Fields are only added at the end (a new minor, `struct_bytes`
//! says what the other side has); a meaning changed is a new major, refused at load by its name.
//!
//! Nothing here is copied per tick: a per-tick hook crosses as [`crate::fast::Tick`] (the context by address), a
//! scalar read as a link of [`native_links!`], a list or a dictionary as a block by address, a row of the zone is a
//! load.

use crate::fast::{HookCtx, Service, Tick};
use std::sync::atomic::{AtomicU32, AtomicUsize};

/// The contract of this file (`major << 16 | minor`). Major 2 (decision D3, docs/plan-refonte.md): no allocator is
/// shared between native libraries; a block given away carries the function that frees it, so whoever allocated frees.
///
/// History since 2.0, the value unchanged:
/// - 20/11 (feat/abi-dev, docs/decisions/2026-11-20-abi-dev.md): the value is `ABI_DEV` in the repository, the
///   number of core/abi/abi-versions.json is written at the tag only. Rebuild every binary once (player, modules,
///   gateways): a binary of before answers its old number and is refused.
/// - 20/11 (feat/abi-contrat-chargement, docs/decisions/2026-11-20-abi-par-contrat.md §8): `struct_bytes` is read
///   at load on both sides ([`HOST_TABLE_NEEDS`], [`KIT_TABLE_NEEDS`], [`covers`]): a table shorter than the fields
///   the reader uses is refused by name, a longer one (a later minor) is read up to what the reader knows. A kit
///   library may export `gf_contracts` (the contracts it reads or writes, [`crate::contract`]), checked by the host.
///   No field added, nothing to rebuild.
/// - lot 5 (feat/abi-contrat-lot5, docs/decisions/2026-11-20-abi-par-contrat.md §9): the tables are the record
///   contracts `abi.native.host`, `abi.native.kit`, `abi.native.links`, `abi.native.bytes`, `abi.native.zone_place`,
///   `abi.native.svc_provider`, declared by the host and every kit library (`gf_contracts`) and compared at load;
///   the word `layout` (the PROBE of `gripforge.structs`) becomes `reserved`, 0, no longer compared. Rebuild every
///   native kit library and the native player once.
pub const ABI: u32 = crate::ABI_DEV;
/// The contract as the manifest of the libraries names it.
pub const CONTRACT: &str = "kithost.native@2";
/// The manifest next to the libraries.
pub const MANIFEST: &str = "kit-modules-native.json";
/// The symbol a kit module exports.
pub const LOAD: &[u8] = b"gfkit_native_load\0";

/// `fn gfkit_native_load(host) -> kit`: answers null when the host's major is not this module's. Called once per
/// thread that plays the module (its state lives in thread-locals, as a host's own).
pub type Load = unsafe extern "C" fn(*const HostTable) -> *const KitTable;

/// How a block given away is freed: by the library that allocated it (`ptr`, `len`, `cap`).
pub type Release = unsafe extern "C" fn(ptr: *mut u8, len: usize, cap: usize);

/// A `Vec<u8>` given away between the host and a kit module. Natively each library keeps its own allocator (D3: the
/// heaps of two DLLs are not one under Windows), so the block travels with [`Release`], the function of the library
/// that allocated it: the receiver reads it in place and hands it back, it never frees it with its own allocator.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NativeBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
    /// Frees the block in the library that allocated it; `None` for an empty block.
    pub release: Option<Release>,
}

impl Default for NativeBytes {
    fn default() -> NativeBytes {
        NativeBytes {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
            release: None,
        }
    }
}

/// The [`Release`] of the library this copy of the crate is compiled into: rebuilds the `Vec` with this library's own
/// allocator and drops it.
unsafe extern "C" fn release_here(ptr: *mut u8, len: usize, cap: usize) {
    if cap != 0 {
        drop(Vec::from_raw_parts(ptr, len, cap));
    }
}

impl NativeBytes {
    /// Gives `v` away, nothing copied; the block keeps the way back to this library's allocator.
    pub fn give(v: Vec<u8>) -> NativeBytes {
        let mut v = core::mem::ManuallyDrop::new(v);
        NativeBytes {
            ptr: v.as_mut_ptr(),
            len: v.len(),
            cap: v.capacity(),
            release: Some(release_here),
        }
    }
    /// The bytes, read in place.
    ///
    /// # Safety
    /// `self` comes from one [`NativeBytes::give`] not yet released.
    pub unsafe fn as_slice(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        std::slice::from_raw_parts(self.ptr, self.len)
    }
    /// Takes the bytes as a `Vec`. A block of this library comes back without a copy; a block of another library is
    /// copied once into this library's allocator and handed back to its owner (a general call, never per tick).
    ///
    /// # Safety
    /// `self` comes from one [`NativeBytes::give`], taken once.
    pub unsafe fn take(self) -> Vec<u8> {
        let Some(release) = self.release else {
            return Vec::new();
        };
        if std::ptr::fn_addr_eq(release, release_here as Release) {
            if self.cap == 0 {
                return Vec::new();
            }
            return Vec::from_raw_parts(self.ptr, self.len, self.cap);
        }
        let out = self.as_slice().to_vec();
        release(self.ptr, self.len, self.cap);
        out
    }
}

/// `enter` of a kit module and `call` of the host: a function number, its arguments read where they are, the answer
/// given in `out`. The status is that of the calls (0 `ok`, else `out` holds the message).
pub type Enter =
    unsafe extern "C" fn(func: u32, args: *const u8, len: usize, out: *mut NativeBytes) -> u32;

/// Where the shared zone is now: two words of the host the module reads at each access (the zone moves when it
/// grows, the words never do).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZonePlace {
    pub base: *const AtomicUsize,
    pub size: *const AtomicU32,
}

/// The direct links of the core (what a kit module of wasm imports from `gf:kithost.*`): scalars and addresses. The
/// list is given to `$with`, which makes what it needs of it: the fields of [`HostTable`], the wrappers of the kit
/// side. Added at the end only.
#[macro_export]
macro_rules! native_links {
    ($with:ident) => {
        $with! {
            w_has(lo: u64, hi: u64) -> u32;
            w_transform(lo: u64, hi: u64) -> u32;
            w_component(lo: u64, hi: u64, component: u32) -> u32;
            w_acting_player() -> u32;
            w_scope_push(lo: u64, hi: u64) -> ();
            w_scope_pop() -> u32;
            st_get(path: u32) -> u32;
            st_set_f64(path: u32, value: f64) -> ();
            st_set_i64(path: u32, value: i64) -> ();
            st_set_flag(path: u32, what: u32) -> ();
            st_set_str(path: u32, lo: u64, hi: u64) -> ();
            sv_has(capability: u32) -> u32;
            reg_f64(i: u32) -> f64;
            reg_u64(i: u32) -> u64;
            hot_epoch() -> f64;
            hot_structure() -> f64;
            hot_set_pose(lo: u64, hi: u64, mask: u32, x: f64, y: f64, z: f64, yaw: f64) -> ();
            hot_pose(lo: u64, hi: u64, mask: u32, fields: *const f64, epoch: u64, out: *mut u64) -> u32;
            col_info(name: u32) -> u64;
            w_near(spec: *const u64, out: *mut u64, cap: u32) -> u32;
            w_fields(n: u32, count: u32, recs: *const u64, epoch: u64, out: *mut u64) -> u32;
            w_fields_many(entities: u32, block: *const u64, epoch: u64, out: *mut u64) -> u32;
            ev_next() -> u64;
            svc_begin(capability: u32, provider: u32) -> u32;
            svc_end() -> ();
            svc_ctx(i: u32) -> u64;
            svc_clear() -> ();
            svc_push(word: u64) -> ();
            svc_get(i: u32) -> u64;
            ph_has_body(lo: u64, hi: u64) -> u32;
            ph_body(lo: u64, hi: u64, out: *mut f64) -> u32;
            ph_set_body(lo: u64, hi: u64, state: *const f64) -> ();
            ph_vector(lo: u64, hi: u64, what: u32, x: f64, y: f64, z: f64) -> ();
            ph_scalar(lo: u64, hi: u64, what: u32) -> f64;
            ph_contacts(what: u32, out: *mut u64, cap: u32) -> u32;
            ph_ask(what: u32, args: *const f64, flags: u32, lo: u64, hi: u64) -> u32;
            ph_poll(ticket: u32, out: *mut u64, cap: u32) -> u32;
            pf_now() -> f64;
            pf_offset() -> f64;
            zone_region(len: u32) -> u32;
            zone_region_free(off: u32) -> u32;
            zone_tick_region(len: u32) -> u32;
            w_has_tag(lo: u64, hi: u64, tag: u32) -> u32;
            ph_character_position(lo: u64, hi: u64, out: *mut f64) -> u32;
        }
    };
}

macro_rules! links_struct {
    ($($name:ident($($a:ident: $t:ty),*) -> $r:ty;)*) => {
        /// The direct links ([`native_links!`]) as a table of functions.
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct Links {
            $(pub $name: unsafe extern "C" fn($($t),*) -> $r,)*
        }
    };
}
native_links!(links_struct);

/// `mem_svc_open`: a scalar service call opened (`gripforge_kithost::kitcall::svc::open`). 1 and the provider's scalar
/// entry and context in `entry` and `ctx`; 0: not this way.
pub type SvcOpen = unsafe extern "C" fn(
    capability: u32,
    provider: u32,
    entry: *mut Option<Service>,
    ctx: *mut *const HookCtx,
) -> u32;

/// `svc_call` of a kit module: the `gfx_svc_call` of wasm, `status << 32 | bytes`.
pub type SvcCall = unsafe extern "C" fn(capability: u32, method: u32, words: u32) -> u64;
/// `svc_id` of a kit module: the atom of its kit id + 1.
pub type SvcId = unsafe extern "C" fn() -> u32;

/// The kit module of this thread that serves a capability: what wasm's linker wires to `gf:svc.<capability>@^1`. Both
/// null: none (a kit of the core, a game without it). The core still checks at each call that it serves it now.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SvcProvider {
    pub call: Option<SvcCall>,
    pub id: Option<SvcId>,
}

/// What the host gives a kit module.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostTable {
    /// Bytes of this structure as the host knows it, and its ABI. `reserved`: 0 (was `layout`, the number folded from
    /// the table `gripforge.structs`, retired at lot 5: the contracts are compared one by one at load). The layout
    /// of the table is the record contract `abi.native.host` (src/layout.rs).
    pub struct_bytes: u32,
    pub abi: u32,
    pub reserved: u64,
    /// The host's allocator, for the shared zones and the buffers the host owns (D3): a kit module never makes it its
    /// global allocator, and frees with it only what it allocated with it.
    pub alloc: unsafe extern "C" fn(usize, usize) -> *mut u8,
    pub alloc_zeroed: unsafe extern "C" fn(usize, usize) -> *mut u8,
    pub free: unsafe extern "C" fn(*mut u8, usize, usize),
    pub realloc: unsafe extern "C" fn(*mut u8, usize, usize, usize) -> *mut u8,
    /// A core interface function.
    pub call: Enter,
    pub zone: unsafe extern "C" fn() -> ZonePlace,
    pub svc_open: SvcOpen,
    pub svc_provider: unsafe extern "C" fn(capability: *const u8, len: usize) -> SvcProvider,
    pub links: Links,
}

/// The bytes of [`HostTable`] a kit module reads: up to the end of `links`, the last field of minor 0. Fixed: a field
/// added later is read only when the host's `struct_bytes` covers it ([`covers`]).
pub const HOST_TABLE_NEEDS: usize =
    core::mem::offset_of!(HostTable, links) + core::mem::size_of::<Links>();
/// The bytes of [`KitTable`] the host reads: up to the end of `view`, the last field of minor 0.
pub const KIT_TABLE_NEEDS: usize = core::mem::offset_of!(KitTable, view)
    + core::mem::size_of::<unsafe extern "C" fn(u32) -> u64>();

/// Whether a table of `struct_bytes` (what its writer says it has) holds a field ending at `end`.
pub const fn covers(struct_bytes: u32, end: usize) -> bool {
    struct_bytes as usize >= end
}

// SAFETY: a table of functions and numbers, written once.
unsafe impl Send for HostTable {}
unsafe impl Sync for HostTable {}

/// What a kit module answers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KitTable {
    /// As [`HostTable`]: `reserved` 0 (was `layout`); the layout is the record contract `abi.native.kit`.
    pub struct_bytes: u32,
    pub abi: u32,
    pub reserved: u64,
    /// The general entry: one function of `kithost.kit@1`.
    pub enter: Enter,
    /// The short entries (`gripforge_abi::fast`).
    pub tick: Tick,
    pub svc: Service,
    /// A service of this kit called by another kit module, through the core's words ([`SvcProvider`]).
    pub svc_call: SvcCall,
    pub svc_id: SvcId,
    /// What the module sees and did ([`view`]): a test or a measure reads it, never a game.
    pub view: unsafe extern "C" fn(which: u32) -> u64,
}

/// The questions of [`KitTable::view`].
pub mod view {
    /// The address of the zone as the module reads it now: the host's own, or nothing is shared.
    pub const ZONE_BASE: u32 = 0;
    /// Its bytes as the module reads them now.
    pub const ZONE_SIZE: u32 = 1;
    /// From here, the counts of `fixed::stats`: blocks copied whole, written over a copy, read in place.
    pub const FIXED: u32 = 2;
}

// SAFETY: as `HostTable`.
unsafe impl Send for KitTable {}
unsafe impl Sync for KitTable {}

/// The name of the library of a kit on this system: `fx.hit_flash` → `libgfkit_fx_hit_flash.so`.
pub fn file_name(kit: &str) -> String {
    let stem = kit.replace(['.', '-'], "_");
    if cfg!(target_os = "windows") {
        format!("gfkit_{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("libgfkit_{stem}.dylib")
    } else {
        format!("libgfkit_{stem}.so")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_say_their_bytes_and_the_needs_are_those_of_minor_0() {
        assert_eq!(HOST_TABLE_NEEDS, std::mem::size_of::<HostTable>());
        assert_eq!(KIT_TABLE_NEEDS, std::mem::size_of::<KitTable>());
        assert!(covers(KIT_TABLE_NEEDS as u32, KIT_TABLE_NEEDS));
        assert!(!covers(KIT_TABLE_NEEDS as u32 - 8, KIT_TABLE_NEEDS));
    }

    #[test]
    fn bytes_cross_without_a_copy() {
        let v = vec![1u8, 2, 3];
        let at = v.as_ptr();
        let back = unsafe { NativeBytes::give(v).take() };
        assert_eq!(back.as_ptr(), at);
        assert_eq!(back, [1, 2, 3]);
        assert!(unsafe { NativeBytes::default().take() }.is_empty());
    }

    unsafe extern "C" fn foreign_release(ptr: *mut u8, len: usize, cap: usize) {
        FREED.with(|f| f.set(f.get() + 1));
        drop(Vec::from_raw_parts(ptr, len, cap));
    }
    thread_local! { static FREED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

    #[test]
    fn a_block_of_another_library_is_freed_by_its_owner() {
        let mut given = NativeBytes::give(vec![4u8, 5]);
        given.release = Some(foreign_release);
        let back = unsafe { given.take() };
        assert_eq!(back, [4, 5]);
        assert_eq!(FREED.with(|f| f.get()), 1);
    }

    #[test]
    fn a_file_is_named_after_its_kit() {
        assert!(file_name("fx.hit_flash").contains("gfkit_fx_hit_flash."));
    }
}
