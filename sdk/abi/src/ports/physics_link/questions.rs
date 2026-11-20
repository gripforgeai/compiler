//! The questions of the link (a ticket each, docs/kit-modules.md §7.17): a character's move or cast, a raycast, the
//! overlaps, and since 2.4 the questions of the layers — every hit of a ray, a shape cast, a shape overlap — filtered
//! by a mask of named layers. The physics answers in the slab of answers, never waited for.

use super::order::*;
use super::unknown::answer;
use super::{PhysicsLink, Read, Sim, NO_SLOT};
use crate::ports::abi::query;
use crate::ports::link::AnswerRec;
use crate::ports::{PhysicsPort, QueryFilter, RayHit};

/// One hit in the answer of [`RAYCAST_ALL`] (contract `abi.physics.hit_words`): its slot, then toi, point and normal
/// (each `f64` as two `u32` words, low first); every index generated.
pub use gripforge_contracts::abi_physics_hit_words as hit_words;
/// The `u32` words of one hit.
pub const HIT_WORDS: usize = hit_words::WORDS;

/// The two words of an `f64`, low first.
fn split(x: f64) -> [u32; 2] {
    let b = x.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// An `f64` of its two words.
fn join(w: [u32; 2]) -> f64 {
    f64::from_bits(w[0] as u64 | (w[1] as u64) << 32)
}

/// The physics' side: the hits of [`RAYCAST_ALL`] in a record — their count, every hit past the record (`more`, the
/// allocator every module of the memory shares; the user frees it with the record).
pub fn pack_hits(r: &mut AnswerRec, hits: &[(u32, &RayHit)]) {
    use hit_words::at;
    r.found = hits.len() as u32;
    if hits.is_empty() {
        return;
    }
    let mut words: Box<[u32]> = vec![0u32; hits.len() * HIT_WORDS].into();
    for ((slot, h), w) in hits
        .iter()
        .zip(words.as_chunks_mut::<{ HIT_WORDS }>().0.iter_mut())
    {
        w[at::SLOT] = *slot;
        w[at::TOI..at::TOI + 2].copy_from_slice(&split(h.toi));
        for k in 0..3 {
            w[at::POINT + 2 * k..at::POINT + 2 * k + 2].copy_from_slice(&split(h.point[k]));
            w[at::NORMAL + 2 * k..at::NORMAL + 2 * k + 2].copy_from_slice(&split(h.normal[k]));
        }
    }
    r.more_len = words.len() as u32;
    r.more = Box::into_raw(words) as *mut u32 as usize as u64;
}

/// The user's side: the hits of [`pack_hits`], each its slot ([`NO_SLOT`]: an entity the link does not name) and
/// its numbers (toi, point, normal).
pub fn hits_of(r: &AnswerRec) -> impl Iterator<Item = (u32, [f64; 7])> + '_ {
    let words: &[u32] = if r.more != 0 {
        // SAFETY: `more_len` words at `more` when it is set (`pack_hits`).
        unsafe { std::slice::from_raw_parts(r.more as usize as *const u32, r.more_len as usize) }
    } else {
        &[]
    };
    words.as_chunks::<{ HIT_WORDS }>().0.iter().map(|c| {
        let (t, p, n) = (hit_words::toi(c), hit_words::point(c), hit_words::normal(c));
        let mut out = [join(t), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        for k in 0..3 {
            out[1 + k] = join([p[2 * k], p[2 * k + 1]]);
            out[4 + k] = join([n[2 * k], n[2 * k + 1]]);
        }
        (hit_words::slot(c), out)
    })
}

/// One hit in a record (a raycast, a shape cast): its numbers, its slot, and the name when the link does not know
/// it and it fits in the slots.
fn write_hit(r: &mut AnswerRec, h: &RayHit, slot: u32) {
    r.found = 1;
    r.first_slots[0] = slot;
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
            std::ptr::copy_nonoverlapping(
                e.as_ptr(),
                r.first_slots.as_mut_ptr() as *mut u8,
                e.len(),
            )
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
                        r.found = 1;
                        r.nums[..3].copy_from_slice(&d);
                        r.nums[3] = g as u8 as f64;
                    }
                });
                (k == MOVE_CHARACTER).then_some(o.slot)
            }
            RAYCAST => {
                use crate::ports::abi::ray;
                let ex = self.name(o.slot).map(str::to_string);
                let w = o.f64s();
                let hit = if ray::check(w).is_ok() {
                    p.raycast(ray::origin(w), ray::dir(w), ray::max_toi(w), ex.as_deref())
                } else {
                    None
                };
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
                use crate::ports::abi::{overlap_box as ob, overlap_sphere as os};
                let w = o.f64s();
                let found = if k == OVERLAP_SPHERE {
                    match os::check(w) {
                        Ok(()) => p.overlap_sphere(os::center(w), os::radius(w), ex.as_deref()),
                        Err(_) => Vec::new(),
                    }
                } else if ob::check(w).is_ok() {
                    let q = (ob::rotated(w) != 0.0).then(|| ob::rotation(w));
                    p.overlap_box(ob::center(w), ob::size(w), q, ex.as_deref())
                } else {
                    Vec::new()
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
                let hit = query::shape_of(o.f64s())
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
                let found = query::shape_of(o.f64s())
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
