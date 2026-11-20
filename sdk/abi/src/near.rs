//! The neighbour search by a direct link (`w_near`, `kithost.world@1.3`; docs/kit-modules.md §7.11): a kit module of
//! the common memory names a block of 64-bit words that says what it asks, and a buffer the core writes the answer
//! in — scalars both ways, nothing encoded, nothing allocated by the call.
//!
//! ## The question (contract `abi.near.question`, then `abi.near.cond` per condition; words of u64)
//!
//! `x`, `z`, `radius` (`f64` bits), how many conditions follow (at most [`MAX_CONDS`]); each condition: its kind (low
//! half) and the atom of its component or tag (high half), the atom of its field or of the string it compares with,
//! its number (`f64` bits). The index of each word is generated (core/abi/contracts/abi.near.*.json).
//!
//! ## The answer (`out`, `abi.near.hit` per neighbour, in the order of the registry)
//!
//! The number of the entity's id (`e12` → 12, as its row of the zone) and its distance (`f64` bits). The link
//! answers how many neighbours there are — more than the buffer holds: ask again with that much room — or
//! `u32::MAX` when it cannot answer this way (an id that is no `e<number>`): the caller walks its own copies.

/// The question (`abi.near.question`), a condition (`abi.near.cond`), a neighbour of the answer (`abi.near.hit`).
pub use gripforge_contracts::{
    abi_near_cond as cond, abi_near_hit as hit, abi_near_question as question,
};

/// Conditions a question holds at most.
pub const MAX_CONDS: usize = 8;
/// Words of the question before its conditions, per condition, per neighbour of the answer (the contracts' lengths).
pub const HEAD: usize = question::WORDS;
pub const COND: usize = cond::WORDS;
pub const HIT: usize = hit::WORDS;
/// "Not answered by this link."
pub const UNANSWERED: u32 = u32::MAX;

/// The kinds of condition (`Cond` of the entity types).
pub const ABOVE: u32 = 1;
pub const NUMBER: u32 = 2;
pub const TEXT: u32 = 3;
pub const TEXT_NOT: u32 = 4;
pub const TAG: u32 = 5;
pub const NO_TAG: u32 = 6;
