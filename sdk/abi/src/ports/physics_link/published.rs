// The published state of the physics link: the set of four buffers the side that serves writes and the user reads in
// place, laid out by the shared contracts (core/abi/contracts/abi.physics.{bodies,moving,slots}.json), and the
// contracts a side that serves declares. Part of `physics_link.rs` (size ceiling of the file).

use crate::ports::link::Quad;

/// The published state of the bodies, one row per slot (contract `abi.physics.bodies`, core/abi/contracts/
/// abi.physics.bodies.json): `bodies::Row` (flags, stamp, p, q, v, w, cp, pose, rev) and `bodies::Head` (seq, applied,
/// moves, time, wall_ms, steps) are generated; the positions are computed by the tool, never written here.
pub use gripforge_contracts::abi_physics_bodies as bodies;
/// The slots a publication wrote, in the port's order (contract `abi.physics.moving`).
pub use gripforge_contracts::abi_physics_moving as moving;
/// The set itself (contract `abi.physics.set`, core/abi/contracts/abi.physics.set.json): its `TableHead`, then one row
/// ([`PubSet`] after its head, checked field by field below).
pub use gripforge_contracts::abi_physics_set as set;
/// What each slot is, written once when the physics took its body (contract `abi.physics.slots`).
pub use gripforge_contracts::abi_physics_slots as slots;
use gripforge_contracts::table_head::{Refused, TableHead, Why};

/// A set of published buffers for `cap` slots, and the table of what each slot is. Made by the user of the link
/// ([`PubSet::make`], the blocks laid out by its build: a `TableHead` each), written by the side that serves at the
/// stride each block announces ([`PubSet::check`] once when it takes the set), read in place.
///
/// A buffer: the `abi.physics.bodies` block (`TableHead`, its head, `cap` rows by slot), then the `abi.physics.moving`
/// block (`TableHead`, `cap` rows; its count: the slots listed). The info: one `abi.physics.slots` block of `cap` rows.
/// Every row of a block is reached at `block + head_bytes + slot × stride`, the two words read in its `TableHead`: a
/// writer or a reader of another build reads the layout of the build that made the set.
///
/// The set itself is a block of the contract `abi.physics.set` (one row after its `TableHead`): the side that serves
/// checks it once ([`PubSet::check`]) before reading its addresses, u64 in wasm32 as natively.
///
/// History: until feat/abi-contrat-lot2 (20/11) a `PubHead` (stamp, count, applied, moves, time, wall_ms, steps), `cap`
/// slot numbers and `cap` `BodyRec` at `size_of` (positions known only by the build); until feat/abi-contrat-lot2c
/// the set itself with `usize` addresses (4 bytes in wasm32) and no `TableHead`. Rebuild kithost.core, the native
/// player and the physics modules (rapier, null).
#[repr(C)]
pub struct PubSet {
    /// The set's `TableHead` (contract `abi.physics.set`, one row).
    pub table: TableHead,
    pub cap: u32,
    pub _pad: u32,
    /// Four buffers: the reader keeps its previous publication as it was (two dated states, read in place).
    pub tri: Quad,
    /// The `abi.physics.slots` block.
    pub info: u64,
    /// The four buffers' words and the info's (one allocation, the user's to free), in u64.
    pub words: u64,
}
const _: () = {
    use std::mem::{offset_of, size_of};
    let row = set::HEAD_BYTES as usize;
    assert!(offset_of!(PubSet, cap) == row + set::at::CAP);
    assert!(offset_of!(PubSet, tri) + offset_of!(Quad, bufs) == row + set::at::BUFS);
    assert!(offset_of!(PubSet, tri) + offset_of!(Quad, latest) == row + set::at::LATEST);
    assert!(offset_of!(PubSet, tri) + offset_of!(Quad, back) == row + set::at::BACK);
    assert!(offset_of!(PubSet, tri) + offset_of!(Quad, front) == row + set::at::FRONT);
    assert!(offset_of!(PubSet, tri) + offset_of!(Quad, prev) == row + set::at::PREV);
    assert!(offset_of!(PubSet, info) == row + set::at::INFO);
    assert!(offset_of!(PubSet, words) == row + set::at::WORDS);
    assert!(size_of::<PubSet>() == row + set::STRIDE as usize);
};

/// The offset of the rows and the stride of a block, read in its `TableHead`.
///
/// # Safety
/// `block` a block of a contract (aligned on 8, its `TableHead` written).
#[inline]
unsafe fn layout(block: usize) -> (usize, usize) {
    let h = &*(block as *const TableHead);
    (h.head_bytes as usize, h.stride as usize)
}

/// Checks the `TableHead` of `block` against `spec` and that a row of this build (`row` bytes) and its head (`head`
/// bytes) fit in it: this build's `Row` and `Head` are then read and written in place.
///
/// # Safety
/// `block` aligned on 8, its `TableHead` written.
unsafe fn fits(
    block: usize,
    spec: &gripforge_contracts::table_head::Spec,
    row: usize,
    head: usize,
) -> Result<(), Refused> {
    let h = TableHead::read(block as *const u8);
    h.check(spec)?;
    let why = if (h.stride as usize) < row {
        Some(Why::Stride(h.stride))
    } else if (h.head_bytes as usize) < head {
        Some(Why::HeadBytes(h.head_bytes))
    } else {
        None
    };
    why.map_or(Ok(()), |why| {
        Err(Refused {
            contract: spec.name,
            why,
        })
    })
}

/// Checks a block of one row read in place as a struct of this build (the link, the set): of its contract, its row
/// where this build puts it (`head_bytes` equal: a head grown by another build would move the row) and holding this
/// build's row (`stride`).
///
/// # Safety
/// `block` aligned on 8, its `TableHead` written.
pub(crate) unsafe fn one_row(
    block: usize,
    spec: &gripforge_contracts::table_head::Spec,
) -> Result<(), Refused> {
    let h = TableHead::read(block as *const u8);
    h.check(spec)?;
    let why = if h.head_bytes != spec.head_bytes {
        Some(Why::HeadBytes(h.head_bytes))
    } else if h.stride < spec.stride {
        Some(Why::Stride(h.stride))
    } else {
        None
    };
    why.map_or(Ok(()), |why| {
        Err(Refused {
            contract: spec.name,
            why,
        })
    })
}

impl PubSet {
    /// Bytes of the bodies block of one buffer for `cap` slots (a multiple of 8): the moving block begins there.
    fn bodies_bytes(cap: u32) -> usize {
        bodies::bytes(cap).next_multiple_of(8)
    }
    /// Bytes of one buffer for `cap` slots, as this build lays it out.
    pub fn buffer_bytes(cap: u32) -> usize {
        PubSet::bodies_bytes(cap) + moving::bytes(cap).next_multiple_of(8)
    }
    /// Makes a set (four buffers and the info, zeroed, every block's `TableHead` written) — the user's thread
    /// allocates it, and frees it with [`PubSet::free`].
    pub fn make(cap: u32) -> *mut PubSet {
        let one = PubSet::buffer_bytes(cap);
        let info = slots::bytes(cap).next_multiple_of(8);
        let total = 4 * one + info;
        let block: Box<[u64]> = vec![0u64; total.div_ceil(8)].into_boxed_slice();
        let base = Box::into_raw(block) as *mut u64 as usize;
        let bufs = [base, base + one, base + 2 * one, base + 3 * one];
        // SAFETY: the blocks just allocated (aligned on 8, zeroed, their bytes counted above), read by nobody yet.
        unsafe {
            for b in bufs {
                bodies::init(b as *mut u8, cap);
                bodies::publish(b as *mut u8, cap);
                moving::init((b + PubSet::bodies_bytes(cap)) as *mut u8, cap);
            }
            slots::init((base + 4 * one) as *mut u8, cap);
            slots::publish((base + 4 * one) as *mut u8, cap);
        }
        let made = Box::into_raw(Box::new(PubSet {
            table: TableHead::default(),
            cap,
            _pad: 0,
            tri: Quad::new(bufs),
            info: (base + 4 * one) as u64,
            words: total.div_ceil(8) as u64,
        }));
        // SAFETY: the set just made (aligned on 8, one row of this build after its head), read by nobody yet.
        unsafe {
            set::init(made.cast::<u8>(), 1);
            set::publish(made.cast::<u8>(), 1);
        }
        made
    }
    /// # Safety
    /// A set made by [`PubSet::make`], used by nobody now.
    pub unsafe fn free(set: *mut PubSet) {
        let s = Box::from_raw(set);
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            s.tri.bufs[0] as usize as *mut u64,
            s.words as usize,
        )));
    }
    /// Checks once, when a side takes a set it did not make (the side that serves, of another build), that every block
    /// is of its contract and holds this build's rows and heads: they are then read and written in place. A refusal
    /// names the contract and what does not fit (rebuild the side named).
    ///
    /// # Safety
    /// A set made by [`PubSet::make`] of any build.
    pub unsafe fn check(&self) -> Result<(), Refused> {
        use std::mem::size_of;
        // The set first: its addresses are read in place, at this build's positions.
        one_row((self as *const PubSet) as usize, &set::SPEC)?;
        for b in self.tri.bufs.map(|b| b as usize) {
            fits(
                b,
                &bodies::SPEC,
                size_of::<bodies::Row>(),
                size_of::<bodies::Head>(),
            )?;
            fits(
                PubSet::moving_block(b),
                &moving::SPEC,
                size_of::<moving::Row>(),
                size_of::<moving::Head>(),
            )?;
        }
        fits(
            self.info as usize,
            &slots::SPEC,
            size_of::<slots::Row>(),
            size_of::<slots::Head>(),
        )
    }
    /// The moving block of a buffer (after its bodies block, whose rows it counts).
    ///
    /// # Safety
    /// `buf` a buffer of a set.
    #[inline]
    unsafe fn moving_block(buf: usize) -> usize {
        let h = &*(buf as *const TableHead);
        buf + (h.head_bytes as usize + h.slots as usize * h.stride as usize).next_multiple_of(8)
    }
    /// The head of a buffer (its `TableHead` and the head of `abi.physics.bodies`).
    ///
    /// # Safety
    /// `buf` one of the set's; a set checked ([`PubSet::check`]) or made by this build.
    pub unsafe fn head(buf: usize) -> &'static mut bodies::Head {
        &mut *(buf as *mut bodies::Head)
    }
    /// The slots the publication in `buf` listed (its moving block's count, read with Acquire).
    ///
    /// # Safety
    /// As [`PubSet::head`].
    #[inline]
    pub unsafe fn listed(buf: usize) -> Listed {
        let at = PubSet::moving_block(buf);
        let (rows, stride) = layout(at);
        Listed {
            at: at + rows,
            stride,
            n: TableHead::count(at as *const u8),
            i: 0,
        }
    }
    /// The writer: slot number `i` of the list of `buf`.
    ///
    /// # Safety
    /// As [`PubSet::head`]; `i` < `cap`; the writer alone writes `buf`.
    #[inline]
    pub unsafe fn list_put(buf: usize, i: u32, slot: u32) {
        let at = PubSet::moving_block(buf);
        let (rows, stride) = layout(at);
        (*((at + rows + i as usize * stride) as *mut moving::Row)).slot = slot;
    }
    /// The writer: the list of `buf` holds `n` slots (Release: a reader that loads the count sees them).
    ///
    /// # Safety
    /// As [`PubSet::list_put`].
    #[inline]
    pub unsafe fn list_publish(buf: usize, n: u32) {
        moving::publish(PubSet::moving_block(buf) as *mut u8, n);
    }
    /// The row of `slot` in `buf`, at the stride the block announces.
    ///
    /// # Safety
    /// As [`PubSet::head`]; `slot` < `cap`.
    #[inline]
    pub unsafe fn rec(&self, buf: usize, slot: u32) -> &'static mut bodies::Row {
        let (rows, stride) = layout(buf);
        &mut *((buf + rows + slot as usize * stride) as *mut bodies::Row)
    }
    /// What `slot` is, at the stride the info block announces.
    ///
    /// # Safety
    /// `slot` < `cap`; a set checked or made by this build.
    #[inline]
    pub unsafe fn info(&self, slot: u32) -> &'static mut slots::Row {
        let info = self.info as usize;
        let (rows, stride) = layout(info);
        &mut *((info + rows + slot as usize * stride) as *mut slots::Row)
    }
}

/// The slots one publication listed, read in place at the stride of its moving block ([`PubSet::listed`]).
pub struct Listed {
    at: usize,
    stride: usize,
    n: u32,
    i: u32,
}

impl Iterator for Listed {
    type Item = u32;
    #[inline]
    fn next(&mut self) -> Option<u32> {
        (self.i < self.n).then(|| {
            // SAFETY: a row below the count the writer published (Acquire), in the block bound by `listed`.
            let s = unsafe {
                (*((self.at + self.i as usize * self.stride) as *const moving::Row)).slot
            };
            self.i += 1;
            s
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = (self.n - self.i) as usize;
        (n, Some(n))
    }
}

/// The shared contracts a side that serves the physics reads or writes (docs/decisions/2026-11-20-abi-par-contrat.md
/// §8): what a physics module declares (`contracts_section!`, kits/core_physics/rapier/module), checked by the host at
/// load against the core's — the published blocks it writes, the words of the orders it reads, the answers it packs.
pub const SERVE_CONTRACTS: &[(&[u8], u8)] = {
    use crate::contract::{BOTH, READ, WRITE};
    use gripforge_contracts as c;
    &[
        // The link itself and its sets, made by the user, checked once and served in place.
        (c::abi_physics_link::DECL, BOTH),
        (c::abi_physics_set::DECL, BOTH),
        // The frozen view of a body's intentions, made at each step and read by the base intentions.
        (c::abi_intent_view::DECL, BOTH),
        // The step block of every running intention: written before each call, its answers read after.
        (c::abi_intent_step::DECL, BOTH),
        // The parameters of the base intentions, read at each of their steps.
        (c::abi_intent_velocity::DECL, READ),
        (c::abi_intent_target::DECL, READ),
        (c::abi_intent_follow::DECL, READ),
        (c::abi_intent_force::DECL, READ),
        (c::abi_intent_jump::DECL, READ),
        (c::abi_intent_burst::DECL, READ),
        (c::abi_intent_set_velocity::DECL, READ),
        (c::abi_intent_turn::DECL, READ),
        (bodies::DECL, BOTH),
        (moving::DECL, WRITE),
        (slots::DECL, WRITE),
        (c::abi_physics_body_desc::DECL, READ),
        (c::abi_physics_body_state::DECL, BOTH),
        (c::abi_physics_shape_words::DECL, READ),
        (c::abi_physics_hit_words::DECL, WRITE),
        (c::abi_physics_joint::DECL, READ),
        (c::abi_physics_vehicle::DECL, READ),
        (c::abi_physics_vehicle_wheel::DECL, READ),
        (c::abi_physics_vehicle_input::DECL, READ),
        (c::abi_physics_vehicle_state::DECL, WRITE),
        (c::abi_physics_vehicle_wheel_state::DECL, WRITE),
        (c::abi_physics_buoyancy::DECL, READ),
        (c::abi_physics_buoyancy_cell::DECL, READ),
        (c::abi_physics_fluid::DECL, READ),
        (c::abi_physics_wave::DECL, READ),
        (c::abi_physics_shape_table::DECL, WRITE),
        (c::abi_physics_shapes::DECL, WRITE),
        // The blocks the module's entries answer (gfp_poses, gfp_events, gfp_overlap / gfp_raycast, gfp_query).
        (c::abi_physics_poses::DECL, WRITE),
        (c::abi_physics_events::DECL, WRITE),
        (c::abi_physics_names::DECL, WRITE),
        (c::abi_physics_hits::DECL, WRITE),
        // The answers of the tickets: taken by the user of the link, filled here.
        (c::abi_physics_answers::DECL, BOTH),
        // The blocks of active contacts of the groups of sensors a kit opened, written after a step.
        (c::abi_sensor_contacts::DECL, WRITE),
    ]
};
