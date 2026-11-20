//! The turn of a moving body, read IN PLACE in the shape table of the physics ([`super::shape_table`]): what the
//! reflection of an entity draws for a body that rolls or tips over.
//!
//! The entity keeps x, y, z and yaw only (its business state); the full rotation of a body is the physics' and stays
//! there. A host that draws a body (a gateway, a demo) asks it here for the entity's name: the records of the table
//! are indexed once per published array (the set of colliders changed), then each read is one lookup and four atomic
//! words — nothing copied, nothing allocated per frame. The pose of a body that is not fixed is rewritten in place at
//! each step (a read during a step may mix two steps: a path, not a contract, as for every reader of the table).
//!
//! Used by: `core/gateway` (`gfk_body_turn`, the engines' bound nodes) and the Bevy demos of the kits.

use super::shape_table::{pose_of, shape_kind, with_rows, ShapeTable, MAX_NAME};
use super::Quat;
use std::collections::HashMap;

/// The index of the moving bodies of one shape table: entity name → its first record that is not fixed.
#[derive(Default, Debug)]
pub struct BodyTurns {
    /// The block (its first row) and count the index was built from (the table swaps a new block when its set changes).
    recs: usize,
    count: usize,
    index: HashMap<Box<str>, usize>,
}

impl BodyTurns {
    /// The rotation (x, y, z, w) of the body of `entity` at the last step of the physics whose table is `t`; `None`
    /// when the entity has no moving body there (no body, a fixed one, mid swap). The index is built again only when
    /// the table published another array.
    ///
    /// # Safety
    /// `t` is a live table (`shape_table::table_at`): the world that published it is not dropped during the call.
    pub unsafe fn turn(&mut self, t: &ShapeTable, entity: &str) -> Option<Quat> {
        with_rows(t, |rows| self.read(rows?, entity))
    }

    /// Bodies indexed (a test, the debugger).
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// No body indexed.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    unsafe fn read(&mut self, rows: super::shape_table::Rows, entity: &str) -> Option<Quat> {
        // The block in service (`shape_table::with_rows`: `recs`, its count, `recs`); a block is never freed while a
        // reader is in. Indexed again only when another block was published.
        let first = rows.row(0) as usize;
        if (first, rows.count) != (self.recs, self.count) {
            self.index.clear();
            let mut i = 0;
            while i < rows.count {
                let r = &*rows.row(i);
                let skip = r.parts as usize;
                let (at, len) = (r.name.addr as usize, r.name.len as usize);
                if r.kind != shape_kind::FIXED
                    && r.kind <= shape_kind::DYNAMIC
                    && len != 0
                    && len <= MAX_NAME
                    && at != 0
                {
                    if let Ok(name) =
                        std::str::from_utf8(std::slice::from_raw_parts(at as *const u8, len))
                    {
                        self.index.entry(name.into()).or_insert(i);
                    }
                }
                // The parts of a compound are in its frame: never a body's turn.
                i += 1 + skip;
            }
            (self.recs, self.count) = (first, rows.count);
        }
        let i = *self.index.get(entity)?;
        let (_, q) = pose_of(rows.row(i));
        let len2: f64 = q.iter().map(|x| x * x).sum();
        (q.iter().all(|x| x.is_finite()) && len2 > 1e-12).then_some(q)
    }
}

#[cfg(test)]
mod tests {
    use super::super::shape_writer::ShapeWriter;
    use super::super::{BodyKind, Shape};
    use super::*;
    use std::sync::atomic::Ordering::SeqCst;

    fn ball() -> Shape {
        Shape::Sphere { radius: 0.5 }
    }

    #[test]
    fn a_rolling_body_answers_its_whole_turn_and_the_level_none() {
        let mut w = ShapeWriter::default();
        let id = [0.0, 0.0, 0.0, 1.0];
        w.add("e1", BodyKind::Dynamic, &ball(), None, [0.0; 3], id, true);
        w.add("e2", BodyKind::Static, &ball(), None, [0.0; 3], id, false);
        w.publish();
        // A quarter turn about X: a roll, no yaw at all.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        w.after_step(|e| (e == "e1").then_some(([1.0, 0.5, 0.0], [h, 0.0, 0.0, h])));
        let t = unsafe { &*(w.address() as *const ShapeTable) };
        let mut turns = BodyTurns::default();
        let q = unsafe { turns.turn(t, "e1") }.expect("the body of e1");
        assert!(
            (q[0] - h).abs() < 1e-12 && (q[3] - h).abs() < 1e-12,
            "{q:?}"
        );
        assert_eq!(
            unsafe { turns.turn(t, "e2") },
            None,
            "a fixed body is the level"
        );
        assert_eq!(unsafe { turns.turn(t, "e3") }, None);
        assert_eq!(turns.len(), 1);
        // Rewritten in place at the next step: the same index, the new words.
        w.after_step(|e| (e == "e1").then_some(([2.0, 0.5, 0.0], [0.0, 0.0, h, h])));
        let q = unsafe { turns.turn(t, "e1") }.unwrap();
        assert!((q[2] - h).abs() < 1e-12, "{q:?}");
        assert_eq!(t.readers().load(SeqCst), 0, "the reader counted itself out");
    }

    #[test]
    fn the_index_follows_a_new_array() {
        let mut w = ShapeWriter::default();
        let id = [0.0, 0.0, 0.0, 1.0];
        w.add("e1", BodyKind::Dynamic, &ball(), None, [0.0; 3], id, true);
        w.publish();
        let t = unsafe { &*(w.address() as *const ShapeTable) };
        let mut turns = BodyTurns::default();
        assert!(unsafe { turns.turn(t, "e1") }.is_some());
        w.forget("e1");
        w.add("e9", BodyKind::Kinematic, &ball(), None, [0.0; 3], id, true);
        w.end_step();
        assert_eq!(unsafe { turns.turn(t, "e1") }, None, "gone with its array");
        assert!(unsafe { turns.turn(t, "e9") }.is_some());
    }
}
