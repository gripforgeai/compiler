//! The Rust types that cross between modules, held to their record contracts (docs/decisions/2026-11-20-abi-par-contrat.md,
//! lot 5; reference docs/contrats-abi.md).
//!
//! Each structure that crosses — a value of the fixed-layout blocks, the context of a call, the headers of the calls
//! and of the zone, the short entries, the native tables — is described once, as a record contract
//! (`core/abi/contracts/<name>.json`, `kind: record`); the tool computes its positions and generates
//! `gripforge_contracts::<name>` (`Row`, `at`, `SIZE`, `DECL`). The hand-written types below are held to it field by
//! field at compile time (`offset_of!` against the generated `Row`, the size, the alignment, the number of fields): a
//! type that drifts from its description does not build. No position is written here.
//!
//! What used to be here — the table `gripforge.structs` written by hand (`STRUCTS`, `field!`), its bytes in a custom
//! section of every module (`structs_section!`) and the number folded from it (`PROBE`, exported as `gf_layout()`),
//! which refused **every** module as soon as **any** structure changed — is retired: each module now declares the
//! contracts it reads or writes (section `gripforge.contracts`, [`crate::contract`]), and the host compares only
//! those, field by field, at load.

use crate::fast::{HookCtx, Scalar, ServiceCall};
use crate::fixed::{FixedBlock, FixedPair, FixedValue};
use crate::mem::Bytes;
use crate::{events, zone, Ctx};
use gripforge_contracts as c;

/// Holds a `repr(C)` type to its record contract: the same size and alignment, the same number of fields, each named
/// field at the position the tool computed (`offset_of!` on the generated `Row`, never a number).
#[macro_export]
#[doc(hidden)]
macro_rules! held_to_contract {
    ($t:ty, $m:ident, [$($f:ident),* $(,)?]) => {
        const _: () = {
            use core::mem::{align_of, offset_of, size_of};
            use $crate::layout::contracts::$m as k;
            assert!(size_of::<$t>() == k::SIZE, concat!(stringify!($t), ": not the size of its contract"));
            assert!(align_of::<$t>() == k::ALIGN, concat!(stringify!($t), ": not the alignment of its contract"));
            assert!([$(stringify!($f)),*].len() == k::FIELDS as usize, concat!(stringify!($t), ": a field of its contract is not held"));
            $(assert!(offset_of!($t, $f) == offset_of!(k::Row, $f), concat!(stringify!($t), ".", stringify!($f), ": not where its contract puts it"));)*
        };
    };
}

/// The generated crate, for [`held_to_contract!`] in the modules of this crate.
#[doc(hidden)]
pub use gripforge_contracts as contracts;

held_to_contract!(FixedValue, abi_mem_fixed_value, [tag, reserved, len, data]);
held_to_contract!(
    FixedPair,
    abi_mem_fixed_pair,
    [key, key_len, reserved, value]
);
held_to_contract!(
    FixedBlock,
    abi_mem_fixed_block,
    [magic, version, size, reserved, root]
);
held_to_contract!(Bytes, abi_mem_bytes, [ptr, len, cap]);
held_to_contract!(
    Ctx,
    abi_kit_ctx,
    [
        inst,
        kit,
        tick,
        frame,
        elapsed,
        fixed_dt,
        services_gen,
        config_gen
    ]
);
held_to_contract!(
    HookCtx,
    abi_fast_hook,
    [
        inst,
        kit,
        services_gen,
        config_gen,
        tick,
        frame,
        elapsed,
        fixed_dt
    ]
);
held_to_contract!(Scalar, abi_fast_scalar, [kind, len, bits, cap]);
held_to_contract!(
    ServiceCall,
    abi_fast_service,
    [
        ctx,
        capability,
        capability_len,
        method_len,
        method,
        args,
        count,
        reserved,
        out
    ]
);

// The head of a mailbox (src/ports/mailbox.rs): its ring starts at the size of its contract.
held_to_contract!(
    crate::ports::mailbox::MailboxHead,
    abi_mail_head,
    [magic, kind, cap, reserved, reserve, head, late, full]
);

// The records of the shared zone are bytes at fixed places, not Rust types: the constants every accessor of
// src/zone.rs and src/events.rs uses are those of their contracts.
const _: () = {
    assert!(zone::DIR_ENTRY as usize == c::abi_zone_buffer::SIZE);
    assert!(zone::DIR_SIZE as usize == c::abi_zone_directory::SIZE);
    assert!(zone::DIR_ENTRIES as usize == c::abi_zone_directory::at::ENTRIES);
    assert!(zone::state::REV as usize == c::abi_zone_state::at::REV);
    assert!(zone::state::MAILBOXES as usize == c::abi_zone_state::at::MAILBOXES);
    assert!(events::HEADER as usize == c::abi_zone_events::SIZE);
    assert!(events::HEAD as usize == c::abi_zone_events::at::HEAD);
    assert!(events::RECORDS as usize == c::abi_zone_events::at::RECORDS);
    assert!(events::WRAPS as usize == c::abi_zone_events::at::WRAPS);
    assert!(events::ANSWER as usize == c::abi_zone_events::at::ANSWER);
    assert!(events::ANSWER_LEN as usize == c::abi_zone_events::at::ANSWER_LEN);
    assert!(events::REC_HEADER as usize == c::abi_zone_event::SIZE);
    assert!(zone::regions::NEXT as usize == c::abi_zone_regions::at::USED);
    assert!(zone::regions::FIRST as usize == c::abi_zone_regions::SIZE);
};

// The engine table (core/abi/structs.json → engine_structs.rs) is generated from the same record contracts.
held_to_contract!(
    crate::engine_structs::ZoneEntityRow,
    abi_engine_entity,
    [id, flags, x, y, z, yaw]
);
held_to_contract!(
    crate::engine_structs::PresentPose,
    abi_engine_pose,
    [id, flags, x, y, z, qx, qy, qz, qw]
);
held_to_contract!(
    crate::engine_structs::ZoneWorldHeader,
    abi_engine_world,
    [
        magic,
        capacity,
        row_bytes,
        header_bytes,
        owner,
        lent,
        rows,
        graves,
        pending,
        inbox,
        inbox_capacity,
        inbox_len,
        epoch,
        phase,
        mirrors
    ]
);

// The native tables (kithost.native@2): addresses of the machine, 64 bits on every native target we build.
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod native {
    use crate::native::{HostTable, KitTable, Links, NativeBytes, SvcProvider, ZonePlace};

    held_to_contract!(
        HostTable,
        abi_native_host,
        [
            struct_bytes,
            abi,
            reserved,
            alloc,
            alloc_zeroed,
            free,
            realloc,
            call,
            zone,
            svc_open,
            svc_provider,
            links
        ]
    );
    held_to_contract!(
        KitTable,
        abi_native_kit,
        [
            struct_bytes,
            abi,
            reserved,
            enter,
            tick,
            svc,
            svc_call,
            svc_id,
            view
        ]
    );
    held_to_contract!(NativeBytes, abi_native_bytes, [ptr, len, cap, release]);
    held_to_contract!(ZonePlace, abi_native_zone_place, [base, size]);
    held_to_contract!(SvcProvider, abi_native_svc_provider, [call, id]);
    macro_rules! links_held {
        ($($name:ident($($a:ident: $t:ty),*) -> $r:ty;)*) => {
            held_to_contract!(Links, abi_native_links, [$($name),*]);
        };
    }
    crate::native_links!(links_held);
}

// The host functions of gripforge:calls@1 for a module built natively: the record contract `abi.host.api`.
#[cfg(all(
    feature = "guest",
    not(target_arch = "wasm32"),
    target_pointer_width = "64"
))]
held_to_contract!(
    crate::calls::guest::native::HostApi,
    abi_host_api,
    [
        abi,
        size,
        ctx,
        module_id,
        reserved,
        gf_call,
        gf_resolve,
        gf_zone_alloc,
        gf_zone_free,
        gf_zone_write,
        gf_zone_read,
        gf_zone_size,
        gf_regs,
        gf_sp,
        gf_push,
        gf_pop,
        gf_invoke,
        gf_ret,
        gf_bridge,
        gf_publish,
        gf_lookup,
        gf_host_caps,
        gf_zone_base,
        gf_link
    ]
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_of_a_call_is_its_structure() {
        // `Ctx` is written field by field in a message; its bytes there are the `repr(C)` structure.
        let c = Ctx {
            inst: 1,
            kit: 2,
            tick: 3,
            frame: 4,
            elapsed: 5.5,
            fixed_dt: 6.5,
            services_gen: 7,
            config_gen: 8,
        };
        let mut w = crate::Writer::new();
        c.write(&mut w);
        let raw: [u8; c::abi_kit_ctx::SIZE] = unsafe { std::mem::transmute(c) };
        assert_eq!(w.bytes(), &raw[..]);
    }
}
