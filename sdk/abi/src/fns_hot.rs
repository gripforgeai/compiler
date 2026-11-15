//! `kithost.world@1`, the block 0x80–0x9f: the entity reads of a kit module that keeps its copies (docs/kit-modules.md
//! §7 step 4). Every entity of the core has a revision (a clock value, never 0); a module tells which revision its
//! copy has and gets only what is newer.
//!
//! `delta`: `u8` 0 absent · 1 unchanged · 2 whole (`rev u64, id, kind, owner string, tags list<string>,
//! components u32 × (name string, value), refs u32 × (key string, value)`) · 3 changes (`rev u64, tags bool
//! [list<string>], components u32 × (name string, value)`: each one set by name, a new name appended).

const B: u32 = crate::WORLD.base;
/// `get_if(id, have u64) -> delta`.
pub const GET_IF: u32 = B | 0x80;
/// `first_if(q, hint id string, have u64) -> delta` (`have` counts only when the first entity is `hint`).
pub const FIRST_IF: u32 = B | 0x81;
/// `query_if(q, hints u32 × (id string, have u64)) -> u32 × delta` in the order of the world (never `absent`).
pub const QUERY_IF: u32 = B | 0x82;
/// `patch(id, have u64, epoch u64, tags bool [list<string>], components u32 × (name string, value)) -> (u64, u64)`:
/// the write-back of `get_mut` as changes. First answer: the entity's revision after it when the copy was current
/// (`have`), else 0. Second: the world's epoch after it when `epoch` was the world's before (nothing else changed
/// since the module read it), else 0.
pub const PATCH: u32 = B | 0x83;
/// `epoch() -> u64`: the clock value of the last change of the world (also the direct link `hot_epoch`).
pub const EPOCH: u32 = B | 0x84;

/// `get_many_if(u32 × (id string, have u64)) -> that many delta`, in the order asked: these entities in one call.
pub const GET_MANY_IF: u32 = B | 0x85;
/// `structure() -> u64`: the clock value of the last change of what a query reads — the list and its order, a kind,
/// tags (also the direct link `hot_structure`). While it is the same a query selects the same entities.
pub const STRUCTURE: u32 = B | 0x86;

/// `set_if(id, component string, value, epoch u64) -> (u64, u64, u64)`: `set` of a module that keeps its copies. The
/// entity's revision before the write and after it (0, 0: no such entity), then the world's epoch after it when
/// `epoch` was the world's before (nothing else changed since the module read it: its copies stay current, the
/// one of this entity with the component set), else 0.
pub const SET_IF: u32 = B | 0x87;

pub const D_ABSENT: u8 = 0;
pub const D_SAME: u8 = 1;
pub const D_WHOLE: u8 = 2;
pub const D_CHANGES: u8 = 3;
