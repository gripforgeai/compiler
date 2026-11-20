//! What the serving side does with an order of no known kind (said once, then counted: never swallowed without a
//! trace), and the record of a question's answer. The capabilities of the modules no longer pass here: a kit writes
//! them straight into the module's box (docs/decisions/2026-11-19-canaux-directs.md).

use super::{PhysicsLink, Sim};
use crate::ports::link::{AnswerRec, Answers};
use std::sync::atomic::Ordering::Acquire;

/// An order of no known kind: said once, then counted.
pub(super) fn unknown(count: &mut u32, k: u32) -> Option<u32> {
    if *count == 0 {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("physics link: an order of unknown kind {k} (counted, said once)");
    }
    *count += 1;
    let _ = k;
    None
}

/// The orders of no known kind the side serving `link` took (0 before it served).
///
/// # Safety
/// Read by the side that serves `link` (or after it stopped, before `drop_sim`).
pub unsafe fn unknown_orders(link: &PhysicsLink) -> u32 {
    match link.sim.load(Acquire) {
        0 => 0,
        // SAFETY: made by the serving side (`sim_of`), dropped only by `drop_sim`.
        at => (*(at as usize as *const Sim)).unknown,
    }
}

/// Writes the answer of `ticket` (every question of the link: its record filled, then ready).
pub(super) fn answer(link: &PhysicsLink, ticket: u32, fill: impl FnOnce(&mut AnswerRec)) {
    // SAFETY: the record of a ticket the user handed out with the order.
    if let Some(r) = unsafe { link.answers.to_fill(ticket) } {
        r.found = 0;
        fill(r);
        Answers::ready(r);
    }
}
