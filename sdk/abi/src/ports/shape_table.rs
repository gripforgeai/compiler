//! The shape table of a physics module (ABI 2.6, the optional entry `gfp_shapes`): every collision shape the physics
//! made, published in its own memory and read IN PLACE by whoever needs them (ai.navigation bakes from them, the
//! `physics.colliders` view of the debugger draws them, the gateways read a body's turn) — no copy across an interface,
//! no question asked. Two shared contracts (core/abi/contracts/): the anchor `abi.physics.shape_table` (one row:
//! revision, readers, the block in service) and the rows `abi.physics.shapes` (one per shape: its genre, its size
//! fields, its pose, its buffers; a compound followed by its parts). Every position is generated.
//!
//! **Best effort, never a dangling pointer.** The physics keeps one anchor per world, at an address that never moves
//! while the world lives. When its set of shapes changes it builds a new block of rows, swaps it in (`recs`), and
//! keeps the old block and the buffers of the shapes that left in a holding place, emptied only at a step where no
//! reader is in (`readers` read after the swap). A reader counts itself in, reads `recs`, that block's count, `recs`
//! again (the same block both times), reads, and counts itself out ([`read_shapes`]). The poses of the shapes that
//! are not fixed are rewritten in place at each step, word by word through atomics: a pose read during a step may mix
//! two steps — that is a path, not a contract. A cheap guard drops what cannot be a shape (an unknown code, numbers
//! that are not finite, lengths beyond what a level holds, a null buffer with a length).
//!
//! History: until feat/abi-contrat-lot2 (20/11) `ShapeTable` and `ShapeRec` were laid out here by hand (pointers and
//! `usize`: 112 bytes in wasm32, 136 natively), the characters' capsules were missing, a compound was one record of
//! packed parts, and no cylinder, cone nor triangle; rebuild the physics modules, kithost.core, the native player and the kits
//! that read shapes.

use super::{Quat, Shape, V3};
pub use gripforge_contracts::abi_physics_shape_table as anchor;
pub use gripforge_contracts::abi_physics_shapes as shapes;
use gripforge_contracts::table_head::TableHead;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::SeqCst};

/// The kind of a record: a fixed body (the level), a kinematic one, a dynamic one.
pub mod shape_kind {
    pub const FIXED: u32 = 0;
    pub const KINEMATIC: u32 = 1;
    pub const DYNAMIC: u32 = 2;
}

/// The genres of a shape (`code` of `abi.physics.shapes`; `Shape::code`).
pub mod shape_code {
    pub const BOX: u32 = 0;
    pub const SPHERE: u32 = 1;
    pub const CAPSULE: u32 = 2;
    pub const MESH: u32 = 3;
    pub const CONVEX: u32 = 4;
    pub const COMPOUND: u32 = 5;
    pub const HEIGHTFIELD: u32 = 6;
    pub const PLANE: u32 = 7;
    pub const CYLINDER: u32 = 8;
    pub const CONE: u32 = 9;
    pub const TRIANGLE: u32 = 10;
    /// The last code a reader of this build knows.
    pub const LAST: u32 = TRIANGLE;
}

/// Records a table holds at most for a reader (more: the table is read as empty).
pub const MAX_RECORDS: usize = 1 << 22;
/// Floats or indices a record carries at most for a reader (more: the record is dropped).
pub const MAX_STREAM: usize = 1 << 27;
/// Bytes of a name at most for a reader (more: the record is dropped).
pub const MAX_NAME: usize = 1 << 12;

/// The anchor of a world (`gfp_shapes`): a block of the contract `abi.physics.shape_table`, one row, at an address that
/// never changes while the world lives. Made by the writer ([`ShapeTable::new`]); every word read as an atomic.
#[repr(C, align(8))]
pub struct ShapeTable {
    block: [u64; ShapeTable::WORDS],
}

impl Default for ShapeTable {
    fn default() -> Self {
        ShapeTable::new()
    }
}

impl ShapeTable {
    const WORDS: usize = anchor::bytes(1).div_ceil(8);

    /// A new anchor: its `TableHead` written, no block in service.
    pub fn new() -> ShapeTable {
        let mut t = ShapeTable {
            block: [0; ShapeTable::WORDS],
        };
        // SAFETY: the block holds `anchor::bytes(1)` bytes, aligned on 8, read by nobody yet.
        unsafe {
            anchor::init(t.block.as_mut_ptr() as *mut u8, 1);
            anchor::publish(t.block.as_mut_ptr() as *mut u8, 1);
        }
        t
    }
    /// A word of the row at its generated position (`head_bytes` read in the anchor's own head).
    fn word(&self, at: usize) -> *mut u8 {
        let base = self.block.as_ptr() as *mut u8;
        // SAFETY: the anchor's head was written at its making; `at` is a position of its row.
        unsafe { base.add((*(base as *const TableHead)).head_bytes as usize + at) }
    }
    fn u64_at(&self, at: usize) -> &AtomicU64 {
        // SAFETY: a field of 8 bytes, aligned on 8 (the tool aligns every field on its size), alive with `self`.
        unsafe { AtomicU64::from_ptr(self.word(at) as *mut u64) }
    }
    /// Moves when a FIXED collider arrives, leaves or moves (the level changed); the others move every step.
    pub fn revision_word(&self) -> &AtomicU64 {
        self.u64_at(anchor::at::REVISION)
    }
    /// The revision of the fixed colliders.
    pub fn revision(&self) -> u64 {
        self.revision_word().load(SeqCst)
    }
    /// Readers in: a reader adds 1, reads, takes 1 away. The physics frees a swapped-out block only while it is 0.
    pub fn readers(&self) -> &AtomicU32 {
        // SAFETY: a field of 4 bytes, aligned on 4, alive with `self`.
        unsafe { AtomicU32::from_ptr(self.word(anchor::at::READERS) as *mut u32) }
    }
    /// The address of the `abi.physics.shapes` block in service (0: none).
    pub fn recs(&self) -> &AtomicU64 {
        self.u64_at(anchor::at::RECS)
    }
}

/// A block of `abi.physics.shapes` checked for this build: its rows at the stride it announces.
#[derive(Clone, Copy)]
pub struct Rows {
    rows: usize,
    stride: usize,
    /// The rows in service.
    pub count: usize,
}

impl Rows {
    /// The block at `at`, or `None` (no block, another contract, rows that do not hold this build's, too many).
    ///
    /// # Safety
    /// `at` is 0 or a block the writer published and does not free while the reader is in.
    pub unsafe fn of(at: usize) -> Option<Rows> {
        if at == 0 {
            return None;
        }
        let h = TableHead::read(at as *const u8);
        h.check(&shapes::SPEC).ok()?;
        let count = h.count as usize;
        ((h.stride as usize) >= std::mem::size_of::<shapes::Row>() && count <= MAX_RECORDS)
            .then_some(Rows {
                rows: at + h.head_bytes as usize,
                stride: h.stride as usize,
                count,
            })
    }
    /// Row `i` (< `count`), in place.
    ///
    /// # Safety
    /// The block is still in service for the reader (counted in).
    pub unsafe fn row(&self, i: usize) -> *const shapes::Row {
        (self.rows + i * self.stride) as *const shapes::Row
    }
}

/// A record that passed the guard, read where it is (its pose read once, word by word).
#[derive(Clone, Copy)]
pub struct ShapeView<'a> {
    pub name: &'a str,
    pub kind: u32,
    /// The genre ([`shape_code`]).
    pub code: u32,
    /// The size fields of its genre (the doc of `abi.physics.shapes`).
    pub numbers: [f64; 3],
    /// The pose in the world (a part's: in its compound's frame).
    pub p: V3,
    pub q: Quat,
    pub floats: &'a [f32],
    pub indices: &'a [u32],
    /// A compound: its parts, the rows that follow it (none otherwise).
    pub parts: Parts<'a>,
}

impl std::fmt::Debug for ShapeView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapeView")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("code", &self.code)
            .field("numbers", &self.numbers)
            .field("p", &self.p)
            .field("q", &self.q)
            .field("parts", &self.parts.len())
            .finish()
    }
}

/// The parts of a compound, rows of the block read in place ([`ShapeView::parts`]).
#[derive(Clone, Copy)]
pub struct Parts<'a> {
    rows: Option<Rows>,
    first: usize,
    n: usize,
    _m: std::marker::PhantomData<&'a ()>,
}

impl<'a> Parts<'a> {
    const NONE: Parts<'static> = Parts {
        rows: None,
        first: 0,
        n: 0,
        _m: std::marker::PhantomData,
    };
    /// The parts.
    pub fn len(&self) -> usize {
        self.n
    }
    /// No part (not a compound).
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    /// Each part that passes the guard, its pose in the compound's frame.
    pub fn iter(&self) -> impl Iterator<Item = ShapeView<'a>> + '_ {
        (0..self.n).filter_map(move |k| {
            let rows = self.rows?;
            // SAFETY: a row of the block the view was read from, below its count (checked when the view was made).
            unsafe { view_of(rows.row(self.first + k), Parts::NONE) }
        })
    }
}

impl ShapeView<'_> {
    /// A fixed collider (the level).
    pub fn fixed(&self) -> bool {
        self.kind == shape_kind::FIXED
    }
    /// The shape, made again from the record: a copy (the buffers of a mesh included; a compound packed again from
    /// its parts), for a reader that keeps it (a bake, made when the level changed — never per tick).
    pub fn shape(&self) -> Shape {
        if self.code == shape_code::COMPOUND {
            let parts: Vec<(Shape, V3, Quat)> =
                self.parts.iter().map(|v| (v.shape(), v.p, v.q)).collect();
            return Shape::compound(&parts);
        }
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

/// The pose of a row, read word by word (the writer may be rewriting it).
///
/// # Safety
/// `r` a row of a block in service.
pub unsafe fn pose_of(r: *const shapes::Row) -> (V3, Quat) {
    let pb = std::ptr::addr_of!((*r).p) as *const f64;
    let qb = std::ptr::addr_of!((*r).q) as *const f64;
    (
        std::array::from_fn(|k| load_f64(pb.add(k))),
        std::array::from_fn(|k| load_f64(qb.add(k))),
    )
}

/// The table at `address` (what `shapes_view` answers), `None` for 0 or an anchor of another contract (a refusal by
/// the contract's name: rebuild the physics or the reader).
///
/// # Safety
/// `address` is 0 or the address of a live table (the world that published it is not dropped while the result is
/// used: a reader asks for it again at each read).
pub unsafe fn table_at<'a>(address: usize) -> Option<&'a ShapeTable> {
    if address == 0 {
        return None;
    }
    let h = TableHead::read(address as *const u8);
    (h.check(&anchor::SPEC).is_ok() && h.stride as usize >= std::mem::size_of::<anchor::Row>())
        .then(|| &*(address as *const ShapeTable))
}

/// Counts the reader in, reads the block in service (`recs`, its count, `recs` again), calls `f`, counts it out.
///
/// # Safety
/// `t` is a live table.
pub unsafe fn with_rows<R>(t: &ShapeTable, f: impl FnOnce(Option<Rows>) -> R) -> R {
    t.readers().fetch_add(1, SeqCst);
    // The same block both times: its count is the one read (a block never comes back while a reader is in).
    let first = t.recs().load(SeqCst) as usize;
    let rows = Rows::of(first).filter(|_| t.recs().load(SeqCst) as usize == first);
    let out = f(rows);
    t.readers().fetch_sub(1, SeqCst);
    out
}

/// Calls `f` for each record of `t` that passes the guard, in the table's order (a compound with its parts, the parts
/// not given again on their own), the reader counted in meanwhile. Answers the revision read before the records.
///
/// # Safety
/// `t` is a live table (see [`table_at`]); `f` keeps nothing of a view past its call.
pub unsafe fn read_shapes(t: &ShapeTable, mut f: impl FnMut(ShapeView<'_>)) -> u64 {
    let revision = t.revision();
    with_rows(t, |rows| {
        let Some(rows) = rows else { return };
        let mut i = 0;
        while i < rows.count {
            let r = rows.row(i);
            let n = ((*r).parts as usize).min(rows.count - i - 1);
            let parts = Parts {
                rows: Some(rows),
                first: i + 1,
                n,
                _m: std::marker::PhantomData,
            };
            if let Some(view) = view_of(r, parts) {
                f(view);
            }
            i += 1 + n;
        }
    });
    revision
}

/// One record through the guard.
///
/// # Safety
/// `r` points at a record of a published block.
unsafe fn view_of<'a>(r: *const shapes::Row, parts: Parts<'a>) -> Option<ShapeView<'a>> {
    let rec = &*r;
    let (code, kind) = (rec.code, rec.kind);
    if code > shape_code::LAST || kind > shape_kind::DYNAMIC {
        return None;
    }
    let (name_at, name_len) = (rec.name.addr as usize, rec.name.len as usize);
    let (floats_len, indices_len) = (rec.floats_len as usize, rec.indices_len as usize);
    let plausible = |ptr: usize, len: usize, max: usize| len <= max && (len == 0 || ptr != 0);
    if !plausible(name_at, name_len, MAX_NAME)
        || !plausible(rec.floats as usize, floats_len, MAX_STREAM)
        || !plausible(rec.indices as usize, indices_len, MAX_STREAM)
    {
        return None;
    }
    let numbers = rec.numbers;
    let (p, q) = pose_of(r);
    if !numbers.iter().chain(&p).chain(&q).all(|x| x.is_finite()) {
        return None;
    }
    let name = if name_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(name_at as *const u8, name_len)).ok()?
    };
    let floats: &'a [f32] = if floats_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(rec.floats as usize as *const f32, floats_len)
    };
    let indices: &'a [u32] = if indices_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(rec.indices as usize as *const u32, indices_len)
    };
    Some(ShapeView {
        name,
        kind,
        code,
        numbers,
        p,
        q,
        floats,
        indices,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gripforge_contracts::table_head::Str;

    /// A block of rows, as a writer publishes it.
    fn block(rows: &[shapes::Row]) -> Box<[u64]> {
        let n = rows.len() as u32;
        let mut b: Box<[u64]> = vec![0u64; shapes::bytes(n).div_ceil(8)].into();
        let at = b.as_mut_ptr() as *mut u8;
        // SAFETY: the block just made, `bytes(n)` long, read by nobody yet.
        unsafe {
            shapes::init(at, n);
            for (i, r) in rows.iter().enumerate() {
                *shapes::row_mut(at, i as u32) = *r;
            }
            shapes::publish(at, n);
        }
        b
    }

    fn rec(name: &'static str, code: u32, numbers: [f64; 3]) -> shapes::Row {
        shapes::Row {
            name: Str {
                addr: name.as_ptr() as u64,
                len: name.len() as u32,
                zero: 0,
            },
            kind: shape_kind::FIXED,
            code,
            numbers,
            p: [1.0, 2.0, 3.0],
            q: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        }
    }

    fn read(t: &ShapeTable) -> Vec<(String, u32, [f64; 3], usize)> {
        let mut seen = Vec::new();
        // SAFETY: the table and its block live through the read.
        unsafe {
            read_shapes(t, |v| {
                seen.push((v.name.to_string(), v.code, v.numbers, v.parts.len()))
            })
        };
        seen
    }

    #[test]
    fn the_guard_drops_what_cannot_be_a_shape() {
        let mut bad_len = rec("b", shape_code::MESH, [0.0; 3]);
        bad_len.floats_len = 9; // a length with no buffer
        let b = block(&[
            rec("a", shape_code::BOX, [1.0, 2.0, 3.0]),
            rec("x", 99, [1.0; 3]),
            rec("n", shape_code::SPHERE, [f64::NAN, 0.0, 0.0]),
            bad_len,
        ]);
        let t = ShapeTable::new();
        t.recs().store(b.as_ptr() as u64, SeqCst);
        t.revision_word().store(7, SeqCst);
        let mut shapes = Vec::new();
        // SAFETY: as `read`.
        let revision = unsafe { read_shapes(&t, |v| shapes.push(v.shape())) };
        assert_eq!(revision, 7);
        assert_eq!(
            shapes,
            vec![Shape::Box {
                size: [1.0, 2.0, 3.0]
            }]
        );
        assert_eq!(t.readers().load(SeqCst), 0, "the reader counted itself out");
    }

    #[test]
    fn a_cylinder_a_cone_and_a_compound_read_with_their_sizes() {
        // Codes 8 and 9 (the classic shapes): the table carries their size fields whatever the reader's enum knows.
        let mut compound = rec("c", shape_code::COMPOUND, [0.0; 3]);
        compound.parts = 2;
        let mut part_a = rec("c", shape_code::BOX, [1.0, 1.0, 1.0]);
        part_a.p = [0.0, 0.5, 0.0];
        let mut part_b = rec("c", shape_code::SPHERE, [0.25, 0.0, 0.0]);
        part_b.p = [0.0, 1.5, 0.0];
        let b = block(&[
            rec("cyl", shape_code::CYLINDER, [0.4, 2.0, 0.0]),
            rec("cone", shape_code::CONE, [0.5, 1.0, 0.0]),
            compound,
            part_a,
            part_b,
        ]);
        let t = ShapeTable::new();
        t.recs().store(b.as_ptr() as u64, SeqCst);
        assert_eq!(
            read(&t),
            vec![
                ("cyl".into(), shape_code::CYLINDER, [0.4, 2.0, 0.0], 0),
                ("cone".into(), shape_code::CONE, [0.5, 1.0, 0.0], 0),
                ("c".into(), shape_code::COMPOUND, [0.0; 3], 2),
            ],
            "the parts come with their compound, not again on their own"
        );
        let mut parts = Vec::new();
        // SAFETY: as `read`.
        unsafe {
            read_shapes(&t, |v| {
                for p in v.parts.iter() {
                    parts.push((p.code, p.numbers, p.p));
                }
            })
        };
        assert_eq!(
            parts,
            vec![
                (shape_code::BOX, [1.0, 1.0, 1.0], [0.0, 0.5, 0.0]),
                (shape_code::SPHERE, [0.25, 0.0, 0.0], [0.0, 1.5, 0.0])
            ]
        );
    }

    #[test]
    fn an_empty_table_or_a_block_of_another_contract_reads_as_nothing() {
        let t = ShapeTable::new();
        assert!(read(&t).is_empty());
        // A block that is not of `abi.physics.shapes`: refused at its head, nothing read.
        let other = [0u64; 16];
        t.recs().store(other.as_ptr() as u64, SeqCst);
        assert!(read(&t).is_empty());
        assert!(unsafe { table_at(0) }.is_none());
        assert!(unsafe { table_at(other.as_ptr() as usize) }.is_none());
        assert!(unsafe { table_at(&t as *const ShapeTable as usize) }.is_some());
    }
}
