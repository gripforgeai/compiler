// The link's block of the contract `abi.physics.link` (core/abi/contracts/abi.physics.link.json): `PhysicsLink` from
// its `table` on, checked field by field against the generated positions, the check of a link another build made, and
// the physics' own state made at the first serve once the link was checked.
// Part of `physics_link.rs` (size ceiling of the file).

use super::{link_block, published, PhysicsLink, PubSet, Sim};
use crate::ports::link::{Answers, Head, Ring};
use gripforge_contracts::table_head::Refused;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};

const _: () = {
    use link_block::at;
    use std::mem::{offset_of, size_of};
    let row = offset_of!(PhysicsLink, table) + link_block::HEAD_BYTES as usize;
    let (o, e, a) = (
        offset_of!(PhysicsLink, orders),
        offset_of!(PhysicsLink, events),
        offset_of!(PhysicsLink, answers),
    );
    assert!(offset_of!(PhysicsLink, table) == size_of::<Head>());
    assert!(o + offset_of!(Ring, buf) == row + at::ORDERS_BUF);
    assert!(o + offset_of!(Ring, cap) == row + at::ORDERS_CAP);
    assert!(o + offset_of!(Ring, head) == row + at::ORDERS_HEAD);
    assert!(o + offset_of!(Ring, tail) == row + at::ORDERS_TAIL);
    assert!(e + offset_of!(Ring, buf) == row + at::EVENTS_BUF);
    assert!(e + offset_of!(Ring, cap) == row + at::EVENTS_CAP);
    assert!(e + offset_of!(Ring, head) == row + at::EVENTS_HEAD);
    assert!(e + offset_of!(Ring, tail) == row + at::EVENTS_TAIL);
    assert!(a + offset_of!(Answers, recs) == row + at::ANSWERS);
    assert!(a + offset_of!(Answers, cap) == row + at::ANSWERS_CAP);
    assert!(a + offset_of!(Answers, cursor) == row + at::ANSWERS_CURSOR);
    assert!(offset_of!(PhysicsLink, set) == row + at::SET);
    assert!(offset_of!(PhysicsLink, applied) == row + at::APPLIED);
    assert!(offset_of!(PhysicsLink, mode) == row + at::MODE);
    assert!(offset_of!(PhysicsLink, fixed_dt) == row + at::FIXED_DT);
    assert!(offset_of!(PhysicsLink, max_steps) == row + at::MAX_STEPS);
    assert!(offset_of!(PhysicsLink, events_dropped) == row + at::EVENTS_DROPPED);
    assert!(offset_of!(PhysicsLink, sim) == row + at::SIM);
    assert!(offset_of!(PhysicsLink, steps) == row + at::STEPS);
    assert!(offset_of!(PhysicsLink, skipped) == row + at::SKIPPED);
    assert!(offset_of!(PhysicsLink, busy_ms) == row + at::BUSY_MS);
    assert!(offset_of!(PhysicsLink, clearance_bits) == row + at::CLEARANCE);
    assert!(offset_of!(PhysicsLink, manual) == row + at::MANUAL);
    assert!(offset_of!(PhysicsLink, dropped_ms) == row + at::DROPPED_MS);
    assert!(offset_of!(PhysicsLink, shapes) == row + at::SHAPES);
    assert!(size_of::<PhysicsLink>() == row + link_block::STRIDE as usize);
};

impl PhysicsLink {
    /// Checks once, when a side takes a link it did not make (the side that serves, of another build), that it is a
    /// block of `abi.physics.link` laid as this build reads it, then its set and its answers: they are then read and
    /// written in place. A refusal names the contract and what does not fit (rebuild the side named).
    ///
    /// # Safety
    /// A link made by [`PhysicsLink::make`] of any build.
    pub unsafe fn check(&self) -> Result<(), Refused> {
        published::one_row(std::ptr::addr_of!(self.table) as usize, &link_block::SPEC)?;
        (*(self.set.load(Acquire) as usize as *const PubSet)).check()?;
        self.answers.check()
    }
}

pub(super) fn sim_of(link: &PhysicsLink) -> &'static mut Sim {
    let mut at = link.sim.load(Acquire);
    if at == 0 {
        // The link first (its fields are read in place, at this build's positions), then its set and its answers.
        // SAFETY: a link of `PhysicsLink::make` of any build, its block written before it was handed over.
        let refused = unsafe {
            published::one_row(std::ptr::addr_of!(link.table) as usize, &link_block::SPEC)
        }
        .err();
        let set = if refused.is_none() {
            link.set.load(Acquire) as usize
        } else {
            0
        };
        let mut sim = Sim::new(set, refused);
        if sim.refused.is_none() {
            // SAFETY: the block of the answers made by the user of the link (`Answers::block`), checked once here.
            sim.refused = unsafe { link.answers.check() }.err();
        }
        at = Box::into_raw(Box::new(sim)) as usize as u64;
        link.sim.store(at, Release);
    }
    // SAFETY: made above by this side, the only one that serves the link.
    unsafe { &mut *(at as usize as *mut Sim) }
}

/// Drops the physics' own state (the side that served the link, when it stops).
///
/// # Safety
/// The side that served `link`, which serves it no more.
pub unsafe fn drop_sim(link: &PhysicsLink) {
    let at = link.sim.swap(0, AcqRel);
    if at != 0 {
        drop(Box::from_raw(at as usize as *mut Sim));
    }
}
