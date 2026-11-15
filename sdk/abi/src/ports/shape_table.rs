//! The shape table of a physics module (ABI 2.6, the optional entry `gfp_shapes`): the collision shapes the physics
//! holds, published in its own memory and read IN PLACE by whoever needs them (ai.navigation bakes from them, the
//! `physics.colliders` view of the debugger draws them) — no copy across an interface, no question asked.
//!
//! **Best effort, never a dangling pointer.** The physics keeps one table per world, at an address that never moves
//! while the world lives. When its set of shapes changes it builds a new array of [`ShapeRec`], swaps it in
//! (`count` set to 0, then `recs`, then `count`), and keeps the old array and the buffers of the shapes that left in
//! a holding place, emptied only at a step where no reader is in ([`ShapeTable::readers`] read after the swap). A
//! reader counts itself in, reads `recs`, `count`, `recs` again (the same array both times: `count` is that array's
//! or 0), reads, and counts itself out ([`read_shapes`]). The poses of the bodies that are not fixed are rewritten in
//! place at each step, word by word through atomics: a pose read during a step may mix two steps — that is a path,
//! not a contract. A cheap guard drops what cannot be a shape (an unknown code, numbers that are not finite, lengths
//! beyond what a level holds, a null buffer with a length).
//!
//! No dependency: the physics module (`kits/core_physics/rapier`) writes it, the kit modules read it.

use super::{Quat, Shape, V3};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering::SeqCst};

/// The kind of a record: a fixed body (the level), a kinematic one, a dynamic one.
pub mod shape_kind {
    pub const FIXED: u32 = 0;
    pub const KINEMATIC: u32 = 1;
    pub const DYNAMIC: u32 = 2;
}

/// Records a table holds at most for a reader (more: the table is read as empty).
pub const MAX_RECORDS: usize = 1 << 22;
/// Floats or indices a record carries at most for a reader (more: the record is dropped).
pub const MAX_STREAM: usize = 1 << 27;
/// Bytes of a name at most for a reader (more: the record is dropped).
pub const MAX_NAME: usize = 1 << 12;

/// The table of a world (`gfp_shapes`): one per world, at an address that never changes while the world lives.
#[repr(C)]
#[derive(Default)]
pub struct ShapeTable {
    /// Moves when a FIXED collider arrives, leaves or moves (the level changed); the others move every step.
    pub revision: AtomicU64,
    /// Readers in: a reader adds 1, reads, takes 1 away. The physics frees a swapped-out array only while it is 0.
    pub readers: AtomicU32,
    pub _pad: u32,
    /// The address of the array of [`ShapeRec`] (0: none).
    pub recs: AtomicUsize,
    /// How many records the array at `recs` holds (0 while it is being swapped).
    pub count: AtomicUsize,
}

/// One collider of the physics, as the table publishes it. Every field but the pose is written once, before the
/// array is published; the pose (`p`, `q`) of a body that is not fixed is rewritten in place at each step.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ShapeRec {
    /// The entity (empty: a collider of nobody), bytes the physics keeps.
    pub name: *const u8,
    pub name_len: usize,
    /// [`shape_kind`]: 0 fixed, 1 kinematic, 2 dynamic.
    pub kind: u32,
    /// `Shape::code()` (0 box … 7 plane).
    pub code: u32,
    /// `Shape::numbers()`.
    pub numbers: [f64; 3],
    /// The pose in the world: position, rotation (x, y, z, w).
    pub p: [f64; 3],
    pub q: [f64; 4],
    /// `Shape::streams()`: buffers the physics keeps (a mesh's vertices, a heightfield's heights…).
    pub floats: *const f32,
    pub floats_len: usize,
    pub indices: *const u32,
    pub indices_len: usize,
}

/// A record that passed the guard, read where it is (its pose read once, word by word).
#[derive(Clone, Copy, Debug)]
pub struct ShapeView<'a> {
    pub name: &'a str,
    pub kind: u32,
    pub code: u32,
    pub numbers: [f64; 3],
    pub p: V3,
    pub q: Quat,
    pub floats: &'a [f32],
    pub indices: &'a [u32],
}

impl ShapeView<'_> {
    /// A fixed collider (the level).
    pub fn fixed(&self) -> bool {
        self.kind == shape_kind::FIXED
    }
    /// The shape, made again from the record: a copy (the buffers of a mesh included), for a reader that keeps it
    /// (a bake, made when the level changed — never per tick).
    pub fn shape(&self) -> Shape {
        Shape::from_wire(self.code, self.numbers, self.floats, self.indices)
    }
}

/// Reads an `f64` the physics may be rewriting (a pose word), as one atomic word.
///
/// # Safety
/// `at` is valid and aligned for 8 bytes.
pub unsafe fn load_f64(at: *const f64) -> f64 {
    f64::from_bits(AtomicU64::from_ptr(at as *mut u64).load(std::sync::atomic::Ordering::Relaxed))
}

/// Writes an `f64` a reader may be reading (a pose word), as one atomic word.
///
/// # Safety
/// `at` is valid and aligned for 8 bytes.
pub unsafe fn store_f64(at: *mut f64, x: f64) {
    AtomicU64::from_ptr(at as *mut u64).store(x.to_bits(), std::sync::atomic::Ordering::Relaxed)
}

impl ShapeTable {
    /// The revision of the fixed colliders.
    pub fn revision(&self) -> u64 {
        self.revision.load(SeqCst)
    }
}

/// The table at `address` (what `shapes_view` answers), `None` for 0.
///
/// # Safety
/// `address` is 0 or the address of a live table (the world that published it is not dropped while the result is
/// used: a reader asks for it again at each read).
pub unsafe fn table_at<'a>(address: usize) -> Option<&'a ShapeTable> {
    (address != 0).then(|| &*(address as *const ShapeTable))
}

/// Calls `f` for each record of `t` that passes the guard, in the table's order, the reader counted in meanwhile.
/// Answers the revision read before the records.
///
/// # Safety
/// `t` is a live table (see [`table_at`]); `f` keeps nothing of a view past its call.
pub unsafe fn read_shapes(t: &ShapeTable, mut f: impl FnMut(ShapeView<'_>)) -> u64 {
    t.readers.fetch_add(1, SeqCst);
    let revision = t.revision.load(SeqCst);
    // `recs`, `count`, `recs`: the same array both times means `count` is that array's (or 0, mid swap) — an
    // array never comes back while a reader is in (it is freed only once none is).
    let first = t.recs.load(SeqCst);
    let count = t.count.load(SeqCst);
    let again = t.recs.load(SeqCst);
    if first == again && first != 0 && count <= MAX_RECORDS {
        let recs = first as *const ShapeRec;
        for i in 0..count {
            if let Some(view) = view_of(recs.add(i)) {
                f(view);
            }
        }
    }
    t.readers.fetch_sub(1, SeqCst);
    revision
}

/// One record through the guard.
///
/// # Safety
/// `r` points at a record of a published array.
unsafe fn view_of<'a>(r: *const ShapeRec) -> Option<ShapeView<'a>> {
    let rec = &*r;
    let (code, kind) = (rec.code, rec.kind);
    if code > 7 || kind > shape_kind::DYNAMIC {
        return None;
    }
    let plausible = |ptr: bool, len: usize, max: usize| len <= max && (len == 0 || ptr);
    if !plausible(!rec.name.is_null(), rec.name_len, MAX_NAME)
        || !plausible(!rec.floats.is_null(), rec.floats_len, MAX_STREAM)
        || !plausible(!rec.indices.is_null(), rec.indices_len, MAX_STREAM)
    {
        return None;
    }
    let numbers = rec.numbers;
    let base = std::ptr::addr_of!((*r).p) as *const f64;
    let p = [load_f64(base), load_f64(base.add(1)), load_f64(base.add(2))];
    let qb = std::ptr::addr_of!((*r).q) as *const f64;
    let q = [
        load_f64(qb),
        load_f64(qb.add(1)),
        load_f64(qb.add(2)),
        load_f64(qb.add(3)),
    ];
    if !numbers.iter().chain(&p).chain(&q).all(|x| x.is_finite()) {
        return None;
    }
    let name = if rec.name_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(rec.name, rec.name_len)).ok()?
    };
    let slice = |ptr: *const f32, len: usize| -> &'a [f32] {
        if len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(ptr, len)
        }
    };
    let indices: &'a [u32] = if rec.indices_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(rec.indices, rec.indices_len)
    };
    Some(ShapeView {
        name,
        kind,
        code,
        numbers,
        p,
        q,
        floats: slice(rec.floats, rec.floats_len),
        indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &'static str, code: u32, numbers: [f64; 3]) -> ShapeRec {
        ShapeRec {
            name: name.as_ptr(),
            name_len: name.len(),
            kind: shape_kind::FIXED,
            code,
            numbers,
            p: [1.0, 2.0, 3.0],
            q: [0.0, 0.0, 0.0, 1.0],
            floats: std::ptr::null(),
            floats_len: 0,
            indices: std::ptr::null(),
            indices_len: 0,
        }
    }

    #[test]
    fn the_guard_drops_what_cannot_be_a_shape() {
        let mut bad_len = rec("b", 3, [0.0; 3]);
        bad_len.floats_len = 9; // a length with no buffer
        let recs = [
            rec("a", 0, [1.0, 2.0, 3.0]),
            rec("x", 9, [1.0; 3]),
            rec("n", 1, [f64::NAN, 0.0, 0.0]),
            bad_len,
        ];
        let t = ShapeTable::default();
        t.recs.store(recs.as_ptr() as usize, SeqCst);
        t.count.store(recs.len(), SeqCst);
        t.revision.store(7, SeqCst);
        let mut seen = Vec::new();
        // SAFETY: the table and its records live through the read.
        let revision = unsafe { read_shapes(&t, |v| seen.push((v.name.to_string(), v.shape()))) };
        assert_eq!(revision, 7);
        assert_eq!(
            seen,
            vec![(
                "a".to_string(),
                Shape::Box {
                    size: [1.0, 2.0, 3.0]
                }
            )]
        );
        assert_eq!(t.readers.load(SeqCst), 0, "the reader counted itself out");
    }

    #[test]
    fn an_empty_or_swapping_table_reads_as_nothing() {
        let t = ShapeTable::default();
        let mut n = 0;
        // SAFETY: an empty table.
        unsafe { read_shapes(&t, |_| n += 1) };
        let recs = [rec("a", 0, [1.0; 3])];
        t.recs.store(recs.as_ptr() as usize, SeqCst);
        // `count` still 0: mid swap.
        unsafe { read_shapes(&t, |_| n += 1) };
        assert_eq!(n, 0);
        assert!(unsafe { table_at(0) }.is_none());
    }
}
