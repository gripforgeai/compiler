//! The writer's side of the shape table (ABI physique 2.6, [`super::shape_table`]): the colliders a physics holds,
//! published in its own memory for the readers that need them (ai.navigation, the debugger's `physics.colliders`),
//! read in place. One writer for every physics that publishes: the Rapier module (`kits/core_physics/rapier`) and the
//! bridge of an engine's own physics (`core/gateway/src/engine_bridge/physics.rs`, what it registered). In place of the
//! navigation records the core kept (NavRecords).
//!
//! The port keeps, for each collider, its name and its buffers (a mesh's vertices and indices). When the set changes,
//! a new array of records is built and swapped in (`count` 0, `recs`, `count`); the old array and the buffers of the
//! colliders that left go to a holding place, emptied only at a step where no reader is in. The poses of the bodies
//! that are not fixed are rewritten in place at each step.

use super::shape_table::{self as st, shape_kind, shapes, ShapeTable};
use super::{BodyKind, Quat, Shape, V3};
use gripforge_contracts::table_head::Str;
use std::sync::atomic::Ordering::SeqCst;

/// One collider as the port keeps it: what its record points at.
struct Entry {
    name: Box<str>,
    kind: u32,
    code: u32,
    numbers: [f64; 3],
    p: V3,
    q: Quat,
    floats: Box<[f32]>,
    indices: Box<[u32]>,
    /// The collider follows the body of its entity (its pose read at each step).
    follows: bool,
    /// A compound: the entries of its parts that follow it (their pose in its frame).
    parts: u32,
}

/// What a reader may still be reading: an array swapped out, the buffers of a collider that left.
enum Held {
    Recs(#[allow(dead_code, reason = "kept alive, never read")] Box<[u64]>),
    Entry(#[allow(dead_code, reason = "kept alive, never read")] Entry),
}

/// The table of one world and what it points at.
pub struct ShapeWriter {
    /// Boxed: its address never changes while the world lives (what `gfp_shapes` answers).
    table: Box<ShapeTable>,
    entries: Vec<Entry>,
    /// The `abi.physics.shapes` block the table points at (built from `entries`, in their order, while `dirty` is
    /// false).
    current: Box<[u64]>,
    /// `entries` changed since `current` was built.
    dirty: bool,
    held: Vec<Held>,
    /// A fixed collider moved during this step (the revision moves at its end).
    level_moved: bool,
}

// SAFETY: the records only name buffers this value owns (or holds until no reader is in); the table is read from other
// threads through its atomics alone, the poses through atomic words (`shape_table::store_f64`).
unsafe impl Send for ShapeWriter {}
unsafe impl Sync for ShapeWriter {}

impl Default for ShapeWriter {
    fn default() -> Self {
        ShapeWriter {
            table: Box::default(),
            entries: Vec::new(),
            current: Box::new([]),
            dirty: false,
            held: Vec::new(),
            level_moved: false,
        }
    }
}

fn kind_of(kind: BodyKind) -> u32 {
    match kind {
        BodyKind::Static => shape_kind::FIXED,
        BodyKind::Kinematic => shape_kind::KINEMATIC,
        BodyKind::Dynamic => shape_kind::DYNAMIC,
    }
}

impl ShapeWriter {
    /// The table's address (the same for the life of the world).
    pub fn address(&self) -> usize {
        &*self.table as *const ShapeTable as usize
    }

    fn level_changed(&self) {
        self.table.revision_word().fetch_add(1, SeqCst);
    }

    /// One more collider of `entity` (empty: nobody's). `streams`: the buffers in place of the shape's own (a mesh the
    /// caller keeps). `follows`: its pose is its body's, read at each step.
    #[allow(
        clippy::too_many_arguments,
        reason = "a collider's fields, as the port has them"
    )]
    pub fn add(
        &mut self,
        entity: &str,
        kind: BodyKind,
        shape: &Shape,
        streams: Option<(&[f32], &[u32])>,
        p: V3,
        q: Quat,
        follows: bool,
    ) {
        let kind = kind_of(kind);
        if let Shape::Compound { .. } = shape {
            // A compound: its row, then a row per part at its place in the compound (never rewritten).
            let parts = shape.parts();
            self.entries.push(Entry {
                name: entity.into(),
                kind,
                code: shape.code(),
                numbers: [0.0; 3],
                p,
                q,
                floats: Box::new([]),
                indices: Box::new([]),
                follows,
                parts: parts.len() as u32,
            });
            for (part, lp, lq) in parts {
                let (f, i) = part.streams().unwrap_or((&[], &[]));
                self.entries.push(Entry {
                    name: entity.into(),
                    kind,
                    code: part.code(),
                    numbers: part.numbers(),
                    p: lp,
                    q: lq,
                    floats: f.into(),
                    indices: i.into(),
                    follows: false,
                    parts: 0,
                });
            }
        } else {
            let (floats, indices) = streams.or_else(|| shape.streams()).unwrap_or((&[], &[]));
            self.entries.push(Entry {
                name: entity.into(),
                kind,
                code: shape.code(),
                numbers: shape.numbers(),
                p,
                q,
                floats: floats.into(),
                indices: indices.into(),
                follows,
                parts: 0,
            });
        }
        self.dirty = true;
        if kind == shape_kind::FIXED {
            self.level_changed();
        }
    }

    /// The one collider of `entity`, in place of any it had, following its body (no rotation at first).
    pub fn replace_moving(&mut self, entity: &str, kind: BodyKind, shape: &Shape, p: V3) {
        self.forget(entity);
        self.add(entity, kind, shape, None, p, [0.0, 0.0, 0.0, 1.0], true);
    }

    /// Drops every collider of `entity`; their buffers are held until no reader is in.
    pub fn forget(&mut self, entity: &str) {
        if !self.entries.iter().any(|e| &*e.name == entity) {
            return;
        }
        let (gone, kept): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|e| &*e.name == entity);
        self.entries = kept;
        if gone.iter().any(|e| e.kind == shape_kind::FIXED) {
            self.level_changed();
        }
        self.held.extend(gone.into_iter().map(Held::Entry));
        self.dirty = true;
    }

    /// The entity has a collider that follows a body.
    pub fn has_pose(&self, entity: &str) -> bool {
        self.entries.iter().any(|e| e.follows && &*e.name == entity)
    }

    /// Builds and swaps in a new array when the set changed since the last one.
    pub fn publish(&mut self) {
        if !self.dirty {
            return;
        }
        let n = self.entries.len() as u32;
        let mut block: Box<[u64]> = vec![0u64; shapes::bytes(n).div_ceil(8)].into();
        let at = block.as_mut_ptr() as *mut u8;
        // SAFETY: the block just made, `bytes(n)` long, aligned on 8; nobody reads it before `recs` names it.
        unsafe {
            shapes::init(at, n);
            for (i, e) in self.entries.iter().enumerate() {
                *shapes::row_mut(at, i as u32) = shapes::Row {
                    name: Str {
                        addr: e.name.as_ptr() as usize as u64,
                        len: e.name.len() as u32,
                        zero: 0,
                    },
                    kind: e.kind,
                    code: e.code,
                    numbers: e.numbers,
                    p: e.p,
                    q: e.q,
                    floats: e.floats.as_ptr() as usize as u64,
                    indices: e.indices.as_ptr() as usize as u64,
                    floats_len: e.floats.len() as u32,
                    indices_len: e.indices.len() as u32,
                    parts: e.parts,
                    _pad0: [0; 4],
                };
            }
            shapes::publish(at, n);
        }
        self.table.recs().store(at as usize as u64, SeqCst);
        let old = std::mem::replace(&mut self.current, block);
        self.held.push(Held::Recs(old));
        self.dirty = false;
    }

    /// How many colliders are kept (the range of [`ShapeWriter::follower`]).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No collider kept.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entity of collider `i` when its pose is its body's (read again at each step), else `None`.
    pub fn follower(&self, i: usize) -> Option<&str> {
        let e = self.entries.get(i)?;
        (e.follows && !e.name.is_empty()).then_some(&*e.name)
    }

    /// The pose of collider `i` now: rewritten in place in the published array when it moved (a fixed one that moved
    /// moves the revision at [`ShapeWriter::end_step`]).
    pub fn set_pose(&mut self, i: usize, p: V3, q: Quat) {
        let in_place = !self.dirty;
        let Some(e) = self.entries.get_mut(i) else {
            return;
        };
        if p == e.p && q == e.q {
            return;
        }
        e.p = p;
        e.q = q;
        self.level_moved |= e.kind == shape_kind::FIXED;
        if in_place {
            // SAFETY: row `i` of the block in service (built from `entries` in their order), at this build's stride.
            let r = unsafe {
                shapes::row_mut(self.current.as_mut_ptr() as *mut u8, i as u32) as *mut shapes::Row
            };
            // SAFETY: a record of the published block, rewritten word by word (readers load the same words).
            unsafe {
                let pw = std::ptr::addr_of_mut!((*r).p) as *mut f64;
                for (k, x) in p.iter().enumerate() {
                    st::store_f64(pw.add(k), *x);
                }
                let qw = std::ptr::addr_of_mut!((*r).q) as *mut f64;
                for (k, x) in q.iter().enumerate() {
                    st::store_f64(qw.add(k), *x);
                }
            }
        }
    }

    /// The step's end, once the poses are set: the revision moved if a fixed collider moved, the array swapped if
    /// the set changed, what was held freed when no reader is in.
    pub fn end_step(&mut self) {
        if std::mem::take(&mut self.level_moved) {
            self.level_changed();
        }
        self.publish();
        // After the swap: a reader that came in later reads the new array only.
        if !self.held.is_empty() && self.table.readers().load(SeqCst) == 0 {
            self.held.clear();
        }
    }

    /// [`ShapeWriter::set_pose`] for every follower from `body`, then [`ShapeWriter::end_step`]. A port whose `body`
    /// is a method of its own loops over `follower` itself (nothing taken out of it per step).
    pub fn after_step(&mut self, body: impl Fn(&str) -> Option<(V3, Quat)>) {
        for i in 0..self.len() {
            if let Some((p, q)) = self.follower(i).and_then(&body) {
                self.set_pose(i, p, q);
            }
        }
        self.end_step();
    }

    /// Arrays and buffers held for readers.
    pub fn held(&self) -> usize {
        self.held.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &ShapeWriter) -> Vec<(String, u32, V3)> {
        let mut out = Vec::new();
        // SAFETY: the table of `s`, alive through the read.
        unsafe {
            st::read_shapes(st::table_at(s.address()).unwrap(), |v| {
                out.push((v.name.to_string(), v.kind, v.p))
            })
        };
        out
    }

    #[test]
    fn the_revision_moves_with_the_level_only() {
        let mut s = ShapeWriter::default();
        let box1 = Shape::Box { size: [1.0; 3] };
        let id = [0.0, 0.0, 0.0, 1.0];
        s.add("wall", BodyKind::Static, &box1, None, [0.0; 3], id, false);
        let r0 = s.table.revision();
        s.add("crate", BodyKind::Dynamic, &box1, None, [1.0; 3], id, true);
        assert_eq!(
            s.table.revision(),
            r0,
            "a moving body is no change of the level"
        );
        s.after_step(|e| (e == "crate").then_some(([2.0, 1.0, 1.0], id)));
        assert_eq!(s.table.revision(), r0);
        assert_eq!(
            read(&s),
            vec![
                ("wall".into(), shape_kind::FIXED, [0.0; 3]),
                ("crate".into(), shape_kind::DYNAMIC, [2.0, 1.0, 1.0])
            ]
        );
        // Moved in place: the array is the same.
        let at = s.table.recs().load(SeqCst);
        s.after_step(|e| (e == "crate").then_some(([3.0, 1.0, 1.0], id)));
        assert_eq!(s.table.recs().load(SeqCst), at);
        assert_eq!(read(&s)[1].2, [3.0, 1.0, 1.0]);
        s.forget("wall");
        assert_ne!(s.table.revision(), r0, "a fixed collider left");
        s.after_step(|_| None);
        assert_eq!(read(&s).len(), 1);
    }

    #[test]
    fn what_a_reader_may_read_is_held_while_it_is_in() {
        let mut s = ShapeWriter::default();
        let mesh = Shape::Mesh {
            vertices: vec![0.0; 9],
            indices: vec![0, 1, 2],
        };
        let id = [0.0, 0.0, 0.0, 1.0];
        s.add("floor", BodyKind::Static, &mesh, None, [0.0; 3], id, false);
        s.after_step(|_| None);
        s.held.clear();
        // A reader is in (another thread, mid read): nothing it may hold is freed.
        s.table.readers().fetch_add(1, SeqCst);
        s.forget("floor");
        s.after_step(|_| None);
        assert_eq!(s.held(), 2, "the buffers that left and the old array");
        s.table.readers().fetch_sub(1, SeqCst);
        s.after_step(|_| None);
        assert_eq!(s.held(), 0, "freed at the first step with no reader");
        assert!(read(&s).is_empty());
    }

    #[test]
    fn a_compound_is_published_as_its_parts_at_their_place() {
        let mut s = ShapeWriter::default();
        let id = [0.0, 0.0, 0.0, 1.0];
        let compound = Shape::compound(&[
            (Shape::Box { size: [1.0; 3] }, [0.0, 0.5, 0.0], id),
            (
                Shape::Capsule {
                    radius: 0.25,
                    height: 1.0,
                },
                [0.0, 1.5, 0.0],
                id,
            ),
        ]);
        s.add(
            "cart",
            BodyKind::Dynamic,
            &compound,
            None,
            [4.0, 0.0, 0.0],
            id,
            true,
        );
        s.after_step(|e| (e == "cart").then_some(([5.0, 0.0, 0.0], id)));
        let mut seen = Vec::new();
        // SAFETY: the table of `s`, alive through the read.
        unsafe {
            st::read_shapes(st::table_at(s.address()).unwrap(), |v| {
                let parts: Vec<(u32, V3, V3)> =
                    v.parts.iter().map(|p| (p.code, p.numbers, p.p)).collect();
                seen.push((v.code, v.p, parts, v.shape()));
            })
        };
        assert_eq!(seen.len(), 1, "one compound, its parts with it");
        let (code, p, parts, shape) = &seen[0];
        assert_eq!(
            (*code, *p),
            (st::shape_code::COMPOUND, [5.0, 0.0, 0.0]),
            "the compound follows its body"
        );
        assert_eq!(
            parts,
            &vec![
                (st::shape_code::BOX, [1.0; 3], [0.0, 0.5, 0.0]),
                (st::shape_code::CAPSULE, [0.25, 1.0, 0.0], [0.0, 1.5, 0.0])
            ],
            "each part with its size, at its place in the compound"
        );
        assert_eq!(
            shape, &compound,
            "a reader that keeps it gets the same compound back"
        );
    }
}
