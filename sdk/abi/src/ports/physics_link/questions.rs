//! The questions of the link (a ticket each, docs/kit-modules.md §7.17): a character's move or cast, a raycast, the
//! overlaps, and since 2.4 the questions of the layers — every hit of a ray, a shape cast, a shape overlap — filtered
//! by a mask of named layers. The physics answers in the slab of answers, never waited for.

use super::order::*;
use super::{answer, PhysicsLink, Read, Sim, NO_SLOT};
use crate::ports::abi::query;
use crate::ports::link::AnswerRec;
use crate::ports::{PhysicsPort, QueryFilter, RayHit};

/// The `u32` words of one hit in the answer of [`RAYCAST_ALL`]: its slot, then toi, point and normal (7 `f64`, two
/// words each).
pub const HIT_WORDS: usize = 15;

/// The physics' side: the hits of [`RAYCAST_ALL`] in a record — their count, every hit past the record (`more`, the
/// allocator every module of the memory shares; the user frees it with the record).
pub fn pack_hits(r: &mut AnswerRec, hits: &[(u32, &RayHit)]) {
    r.count = hits.len() as u32;
    if hits.is_empty() {
        return;
    }
    let mut words: Vec<u32> = Vec::with_capacity(hits.len() * HIT_WORDS);
    for (slot, h) in hits {
        words.push(*slot);
        for x in [h.toi].iter().chain(h.point.iter()).chain(h.normal.iter()) {
            let b = x.to_bits();
            words.extend_from_slice(&[b as u32, (b >> 32) as u32]);
        }
    }
    let words: Box<[u32]> = words.into();
    r.more_len = words.len() as u32;
    r.more = Box::into_raw(words) as *mut u32 as usize;
}

/// The user's side: the hits of [`pack_hits`], each its slot ([`NO_SLOT`]: an entity the link does not name) and
/// its numbers (toi, point, normal).
pub fn hits_of(r: &AnswerRec) -> impl Iterator<Item = (u32, [f64; 7])> + '_ {
    let words: &[u32] = if r.more != 0 {
        // SAFETY: `more_len` words at `more` when it is set (`pack_hits`).
        unsafe { std::slice::from_raw_parts(r.more as *const u32, r.more_len as usize) }
    } else {
        &[]
    };
    words.as_chunks::<HIT_WORDS>().0.iter().map(|c| {
        let mut n = [0f64; 7];
        for (i, x) in n.iter_mut().enumerate() {
            *x = f64::from_bits(c[1 + 2 * i] as u64 | (c[2 + 2 * i] as u64) << 32);
        }
        (c[0], n)
    })
}

/// One hit in a record (a raycast, a shape cast): its numbers, its slot, and the name when the link does not know
/// it and it fits in the slots.
fn write_hit(r: &mut AnswerRec, h: &RayHit, slot: u32) {
    r.count = 1;
    r.slots[0] = slot;
    r.nums[0] = h.toi;
    r.nums[1..4].copy_from_slice(&h.point);
    r.nums[4..7].copy_from_slice(&h.normal);
    // A hit of a collider under a name this link does not know: the name in the record.
    if let Some(e) = h
        .entity
        .as_deref()
        .filter(|e| slot == NO_SLOT && e.len() <= 4 * AnswerRec::SLOTS)
    {
        r.nums[7] = e.len() as f64;
        // SAFETY: the slots of the record are `SLOTS` × 4 bytes, the name fits.
        unsafe {
            std::ptr::copy_nonoverlapping(e.as_ptr(), r.slots.as_mut_ptr() as *mut u8, e.len())
        };
    } else {
        r.nums[7] = 0.0;
    }
}

impl Sim {
    fn hit_slot(&self, h: &RayHit) -> u32 {
        h.entity.as_deref().map_or(NO_SLOT, |e| self.slot_of(e))
    }

    fn filter(&self, o: &Read, mask_at: usize) -> QueryFilter {
        QueryFilter {
            mask: o.num(mask_at) as u32,
            exclude: self.name(o.slot).map(str::to_string),
        }
    }

    fn shape_args(o: &Read) -> Vec<f64> {
        (0..query::SHAPE_WORDS).map(|i| o.num(i)).collect()
    }

    /// Answers a question. Whether it changed what is published (a character's move).
    pub(super) fn question(
        &mut self,
        p: &mut dyn PhysicsPort,
        link: &PhysicsLink,
        k: u32,
        o: &Read,
    ) -> Option<u32> {
        match k {
            MOVE_CHARACTER | CAST_CHARACTER => {
                let n = self.name(o.slot).map(str::to_string);
                let got = n.and_then(|n| {
                    if k == MOVE_CHARACTER {
                        p.move_character(&n, o.v3(0))
                    } else {
                        p.cast_character(&n, o.v3(0), o.v3(3))
                    }
                });
                answer(link, o.ticket, |r| {
                    if let Some((d, g)) = got {
                        r.count = 1;
                        r.nums[..3].copy_from_slice(&d);
                        r.nums[3] = g as u8 as f64;
                    }
                });
                (k == MOVE_CHARACTER).then_some(o.slot)
            }
            RAYCAST => {
                let ex = self.name(o.slot).map(str::to_string);
                let hit = p.raycast(o.v3(0), o.v3(3), o.num(6), ex.as_deref());
                let slot = hit.as_ref().map_or(NO_SLOT, |h| self.hit_slot(h));
                answer(link, o.ticket, |r| {
                    if let Some(h) = &hit {
                        write_hit(r, h, slot);
                    }
                });
                None
            }
            OVERLAP_SPHERE | OVERLAP_BOX => {
                let ex = self.name(o.slot).map(str::to_string);
                let found = if k == OVERLAP_SPHERE {
                    p.overlap_sphere(o.v3(0), o.num(3), ex.as_deref())
                } else {
                    let q = (o.num(10) != 0.0).then(|| [o.num(6), o.num(7), o.num(8), o.num(9)]);
                    p.overlap_box(o.v3(0), o.v3(3), q, ex.as_deref())
                };
                self.answer_found(link, o.ticket, &found);
                None
            }
            RAYCAST_ALL => {
                // origin, dir, max toi, max hits, mask
                let f = self.filter(o, 8);
                let hits = p.raycast_all(o.v3(0), o.v3(3), o.num(6), &f, o.num(7) as usize);
                let slotted: Vec<(u32, &RayHit)> =
                    hits.iter().map(|h| (self.hit_slot(h), h)).collect();
                answer(link, o.ticket, |r| pack_hits(r, &slotted));
                None
            }
            SHAPE_CAST => {
                // shape words, dir, max toi, mask
                let w = query::SHAPE_WORDS;
                let f = self.filter(o, w + 4);
                let hit = query::shape_of(&Self::shape_args(o))
                    .and_then(|(s, at, q)| p.shape_cast(&s, at, q, o.v3(w), o.num(w + 3), &f));
                let slot = hit.as_ref().map_or(NO_SLOT, |h| self.hit_slot(h));
                answer(link, o.ticket, |r| {
                    if let Some(h) = &hit {
                        write_hit(r, h, slot);
                    }
                });
                None
            }
            OVERLAP_SHAPE => {
                // shape words, mask
                let f = self.filter(o, query::SHAPE_WORDS);
                let found = query::shape_of(&Self::shape_args(o))
                    .map(|(s, at, q)| p.overlap_shape(&s, at, q, &f))
                    .unwrap_or_default();
                self.answer_found(link, o.ticket, &found);
                None
            }
            _ => None,
        }
    }

    fn answer_found(&self, link: &PhysicsLink, ticket: u32, found: &[String]) {
        let slots: Vec<u32> = found
            .iter()
            .map(|e| self.slot_of(e))
            .filter(|s| *s != NO_SLOT)
            .collect();
        answer(link, ticket, |r| r.set_slots(&slots));
    }
}
