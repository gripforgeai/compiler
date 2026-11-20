//! Binding a block of the zone that follows a shared contract (docs/decisions/2026-11-20-abi-par-contrat.md §5): the
//! façade reads the block's `TableHead` once, word by word through the zone's own reads (an isolated module reads the
//! zone across two memories), checks it against the contract this build knows, and keeps where row 0 begins. The rows
//! are then read at constant positions (`at` of the generated contract), never by a position written here. Used by the
//! drawn poses (drawn_pose.rs) and the render yaws (render_yaw_claim.rs).

use crate::zone::zone_read_u32;
use gripforge_contracts::table_head::{Spec, TableHead};

/// The offset of row 0 of the block at `at` (in the zone) of the contract `spec`; `None` when the head says another
/// contract, or a writer that lays out fewer fields than this build reads with no default (the check at load refuses
/// such a module first; this is the net for a block made after the load).
pub(super) fn bind(spec: &Spec, at: u32) -> Option<u32> {
    let words = core::array::from_fn(|i| zone_read_u32(at + 4 * i as u32));
    let head = TableHead::from_words(words);
    head.check(spec).ok()?;
    Some(at + head.head_bytes)
}
