//! The records of a physics module read in place (the names it found, the hits of a question) and the questions of
//! the layers (ABI 2.4: the optional entry `gfp_query`). Part of `abi.rs`, re-exported there: the copy only shrinks.

use super::abi::Fns;

/// The hits of a [`Query`], kept by the module (the block; contract `abi.physics.hits`; until #1872 `HitRec`): the
/// entity's name (length [`super::abi::NO_NAME_LEN`]: none), the distance along the ray or the cast (0 for an
/// overlap), the point and the normal.
pub use gripforge_contracts::abi_physics_hits as hits;
/// The names an overlap found, kept by the module ([`Fns::overlap`]: the block; contract `abi.physics.names`,
/// core/abi/contracts/abi.physics.names.json; until #1872 `NameRef`, a pointer and a `usize`).
pub use gripforge_contracts::abi_physics_names as names;
/// A name of a row (`str`: the address and the length of bytes in the module's memory).
pub use gripforge_contracts::table_head::Str;

/// The `what` of [`Query`].
pub mod query {
    /// Every hit of a ray, nearest first. `args`: origin (3), direction (3), max toi, max hits.
    pub const RAY_ALL: u32 = 0;
    /// The first hit of a simple shape moved along a direction. `args`: [`SHAPE_WORDS`], direction (3), max toi.
    pub const SHAPE_CAST: u32 = 1;
    /// The entities touching a simple shape. `args`: [`SHAPE_WORDS`].
    pub const OVERLAP: u32 = 2;
    /// A simple shape: its code (0 box, 1 sphere, 2 capsule), its three numbers (`Shape::numbers`), position,
    /// rotation (contract `abi.physics.shape_words`, every index generated).
    pub use gripforge_contracts::abi_physics_shape_words as words;
    /// The words of a simple shape.
    pub const SHAPE_WORDS: usize = words::WORDS;

    use crate::ports::{Quat, Shape, V3};

    /// The words of a simple shape at a pose; `None` for a shape a question does not take (a mesh, a hull…).
    pub fn shape_words(shape: &Shape, position: V3, rotation: Quat) -> Option<[f64; SHAPE_WORDS]> {
        use words::at;
        if shape.code() > 2 {
            return None;
        }
        let mut w = [0.0; SHAPE_WORDS];
        w[at::CODE] = shape.code() as f64;
        w[at::NUMBERS..at::NUMBERS + 3].copy_from_slice(&shape.numbers());
        w[at::POSITION..at::POSITION + 3].copy_from_slice(&position);
        w[at::ROTATION..at::ROTATION + 4].copy_from_slice(&rotation);
        Some(w)
    }

    /// The shape and pose of [`shape_words`] (`None`: fewer words than a shape, or a shape a question does not take).
    pub fn shape_of(w: &[f64]) -> Option<(Shape, V3, Quat)> {
        words::check(w).ok()?;
        let n = words::numbers(w);
        let s = match words::code(w) as u32 {
            0 => Shape::Box { size: n },
            1 => Shape::Sphere { radius: n[0] },
            2 => Shape::Capsule {
                radius: n[0],
                height: n[1],
            },
            _ => return None,
        };
        Some((s, words::position(w), words::rotation(w)))
    }
}

/// 2.4, an optional entry (`gfp_query`; a module of an older minor does not have it, the core then answers by the
/// entries it has): a question through the layers of `mask` (the bits of the named layers), an entity left out. The
/// hits as the address of the `abi.physics.hits` block the module keeps (valid until the next call that changes the
/// world).
pub type Query = unsafe extern "C" fn(
    w: usize,
    what: u32,
    args: *const f64,
    mask: u32,
    ex: *const u8,
    ex_len: usize,
) -> *const u8;

/// 2.5, an optional entry (`gfp_joint`): the joint `id` between the bodies of `a` and `b`, `desc` its
/// [`super::JOINT_WORDS`] words (`JointDesc::words`); a null `desc` lets the joint go. 1 when taken. The names are
/// read during the call only.
pub type Joint = unsafe extern "C" fn(
    w: usize,
    id: *const u8,
    id_len: usize,
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
    desc: *const f64,
) -> u32;

/// The module's directory side (dev, no change of the ABI's value; docs/decisions/2026-11-19-canaux-directs.md), an
/// optional entry (`gfp_bus`). It carries no message: a kit writes its orders straight into the module's box.
/// - [`bus::CAPABILITIES`]: the address and the length of the module's list of capabilities (JSON, in its memory,
///   valid for the world's life) written at `out[0]` and `out[1]`; 1 when written.
///
/// The names and the words are read during the call only.
pub type Bus = unsafe extern "C" fn(
    w: usize,
    what: u32,
    code: u32,
    body: *const u8,
    body_len: usize,
    name: *const u8,
    name_len: usize,
    words: *const u64,
    n: usize,
    out: *mut u64,
    cap: usize,
) -> usize;

/// The `what` of [`Bus`].
pub mod bus {
    // 0 (an order) and 1 (a question) are gone (#1761): the messages go into the module's boxes.
    pub const CAPABILITIES: u32 = 2;
    /// The module's mailboxes (docs/decisions/2026-11-19-canaux-directs.md): 4 words each in `out` — the block's
    /// address, the kind, the name's address and bytes (in the common memory, alive with the world); answers how many.
    pub const MAILBOXES: u32 = 3;
}

/// 2.6, a required entry (`gfp_shapes`, in [`super::NAMES`]): the address of the world's shape table
/// ([`crate::ports::shape_table::ShapeTable`]), the same for the whole life of the world; 0: none.
pub type Shapes = unsafe extern "C" fn(w: usize) -> usize;

/// The optional entries, after [`NAMES`] (a loader hands their places when the module has them, 0 otherwise).
pub const OPTIONAL: [&str; 3] = ["gfp_query", "gfp_joint", "gfp_bus"];

impl Fns {
    /// The optional entries from their places (in the order of [`OPTIONAL`]; 0: the module does not have it).
    ///
    /// # Safety
    /// Each place not 0 is the module's export of that name, with the signature of its type here.
    pub unsafe fn with_optional(mut self, p: &[usize]) -> Fns {
        if let Some(&q) = p.first().filter(|q| **q != 0) {
            self.query = Some(std::mem::transmute::<usize, Query>(q));
        }
        if let Some(&j) = p.get(1).filter(|j| **j != 0) {
            self.joint = Some(std::mem::transmute::<usize, Joint>(j));
        }
        if let Some(&b) = p.get(2).filter(|b| **b != 0) {
            self.bus = Some(std::mem::transmute::<usize, Bus>(b));
        }
        self
    }
}

/// The bytes a name of a call names (address, length), read where they are ("" for none).
///
/// # Safety
/// `ptr` is valid for `len` bytes of UTF-8, and stays so while the result is used.
pub unsafe fn name<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 || len == super::abi::NO_NAME {
        return "";
    }
    std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
}

/// The bytes a row names (`str`), read where they are ("" for no name).
///
/// # Safety
/// `s` names bytes of UTF-8 (a module's string) valid while the result is used, or has length 0 or
/// [`super::abi::NO_NAME_LEN`].
pub unsafe fn row_name<'a>(s: Str) -> &'a str {
    if s.len == 0 || s.len == super::abi::NO_NAME_LEN {
        return "";
    }
    name(s.addr as usize as *const u8, s.len as usize)
}

/// A row's name of the module's bytes `s` (`None`: [`super::abi::NO_NAME_LEN`]).
pub fn str_of(s: Option<&str>) -> Str {
    s.map_or(
        Str {
            addr: 0,
            len: super::abi::NO_NAME_LEN,
            ..Str::default()
        },
        |s| Str {
            addr: s.as_ptr() as usize as u64,
            len: s.len() as u32,
            ..Str::default()
        },
    )
}
