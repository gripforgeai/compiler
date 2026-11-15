//! The neighbour search by a direct link (`w_near`, `kithost.world@1.3`; docs/kit-modules.md §7.11): a kit module of
//! the common memory names a block of 64-bit words that says what it asks, and a buffer the core writes the answer
//! in — scalars both ways, nothing encoded, nothing allocated by the call.
//!
//! ## The question (`spec`, 4 words then 3 per condition)
//!
//! | word | field |
//! |---|---|
//! | 0, 1, 2 | `x`, `z`, `radius` (`f64` bits) |
//! | 3 | how many conditions follow (at most [`MAX_CONDS`]) |
//! | 4 + 3i | the kind of condition `i` (low half) and the atom of its component or tag (high half) |
//! | 5 + 3i | the atom of its field, or of the string it compares with |
//! | 6 + 3i | its number (`f64` bits) |
//!
//! ## The answer (`out`, 2 words per neighbour, in the order of the registry)
//!
//! The number of the entity's id (`e12` → 12, as its row of the zone) and its distance (`f64` bits). The link
//! answers how many neighbours there are — more than the buffer holds: ask again with that much room — or
//! `u32::MAX` when it cannot answer this way (an id that is no `e<number>`): the caller walks its own copies.

/// Conditions a question holds at most.
pub const MAX_CONDS: usize = 8;
/// Words of the question before its conditions, per condition, per neighbour of the answer.
pub const HEAD: usize = 4;
pub const COND: usize = 3;
pub const HIT: usize = 2;
/// "Not answered by this link."
pub const UNANSWERED: u32 = u32::MAX;

/// The kinds of condition (`Cond` of the entity types).
pub const ABOVE: u32 = 1;
pub const NUMBER: u32 = 2;
pub const TEXT: u32 = 3;
pub const TEXT_NOT: u32 = 4;
pub const TAG: u32 = 5;
pub const NO_TAG: u32 = 6;
