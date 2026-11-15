//! The records of a physics module read in place (the names it found, the hits of a question) and the questions of
//! the layers (ABI 2.4: the optional entry `gfp_query`). Part of `abi.rs`, re-exported there: the copy only shrinks.

use super::abi::{Fns, NO_NAME};

/// A name found by a query, kept by the module ([`Fns::overlap`]).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NameRef {
    pub name: *const u8,
    pub len: usize,
}

/// The words of a body description from minor 4: [`DESC_WORDS`], then its layers and the layers it collides with.
pub const DESC_WORDS_4: usize = 25;
/// The words of a body description from minor 5: [`DESC_WORDS_4`], then whether it is a sensor (0 or 1).
pub const DESC_WORDS_5: usize = 26;
/// A hit of [`Query`], kept by the module: the entity's name ([`NO_NAME`]: none), the distance along the ray or the
/// cast (0 for an overlap), the point and the normal.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HitRec {
    pub name: *const u8,
    pub len: usize,
    pub toi: f64,
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

/// The `what` of [`Query`].
pub mod query {
    /// Every hit of a ray, nearest first. `args`: origin (3), direction (3), max toi, max hits.
    pub const RAY_ALL: u32 = 0;
    /// The first hit of a simple shape moved along a direction. `args`: [`SHAPE_WORDS`], direction (3), max toi.
    pub const SHAPE_CAST: u32 = 1;
    /// The entities touching a simple shape. `args`: [`SHAPE_WORDS`].
    pub const OVERLAP: u32 = 2;
    /// A simple shape: its code (0 box, 1 sphere, 2 capsule), its three numbers (`Shape::numbers`), position (3),
    /// rotation (4).
    pub const SHAPE_WORDS: usize = 11;

    use crate::ports::{Quat, Shape, V3};

    /// The words of a simple shape at a pose; `None` for a shape a question does not take (a mesh, a hull…).
    pub fn shape_words(shape: &Shape, position: V3, rotation: Quat) -> Option<[f64; SHAPE_WORDS]> {
        if shape.code() > 2 {
            return None;
        }
        let s = shape.numbers();
        let (p, q) = (position, rotation);
        Some([
            shape.code() as f64,
            s[0],
            s[1],
            s[2],
            p[0],
            p[1],
            p[2],
            q[0],
            q[1],
            q[2],
            q[3],
        ])
    }

    /// The shape and pose of [`shape_words`] (`w` at least [`SHAPE_WORDS`] long).
    pub fn shape_of(w: &[f64]) -> Option<(Shape, V3, Quat)> {
        let s = match (w.len() >= SHAPE_WORDS).then(|| w[0] as u32)? {
            0 => Shape::Box {
                size: [w[1], w[2], w[3]],
            },
            1 => Shape::Sphere { radius: w[1] },
            2 => Shape::Capsule {
                radius: w[1],
                height: w[2],
            },
            _ => return None,
        };
        Some((s, [w[4], w[5], w[6]], [w[7], w[8], w[9], w[10]]))
    }
}

/// 2.4, an optional entry (`gfp_query`; a module of an older minor does not have it, the core then answers by the
/// entries it has): a question through the layers of `mask` (the bits of the named layers), an entity left out. The
/// hits as records the module keeps (valid until the next call that changes the world), their number at `n`.
pub type Query = unsafe extern "C" fn(
    w: usize,
    what: u32,
    args: *const f64,
    mask: u32,
    ex: *const u8,
    ex_len: usize,
    n: *mut usize,
) -> *const HitRec;

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

/// 2.6, a required entry (`gfp_shapes`, in [`super::NAMES`]): the address of the world's shape table
/// ([`crate::ports::shape_table::ShapeTable`]), the same for the whole life of the world; 0: none.
pub type Shapes = unsafe extern "C" fn(w: usize) -> usize;

/// The optional entries, after [`NAMES`] (a loader hands their places when the module has them, 0 otherwise).
pub const OPTIONAL: [&str; 2] = ["gfp_query", "gfp_joint"];

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
        self
    }
}

/// The bytes a record names, read where they are.
///
/// # Safety
/// `ptr` is valid for `len` bytes of UTF-8 (a module's string), and stays so while the result is used.
pub unsafe fn name<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 || len == NO_NAME {
        return "";
    }
    std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
}
