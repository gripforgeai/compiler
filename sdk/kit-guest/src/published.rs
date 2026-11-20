//! The regions published by kits (gripforge-abi `zone::published`, #1766, docs/design/entrees-hors-abi.md): a kit
//! lays a block of its own in the zone and publishes it under a name; any module finds it by name and reads it in
//! place. The core and this crate know no name and no layout: a published region's layout is its kit's.
//! The directory is a block of the contract `abi.zone.published`; a converted region begins with the `TableHead` of its
//! kit's contract ([`find_bound`]). Also the neutral [`follow_now`] of a `PointerFollow` row (contract `abi.input.pointer`).
//! Used by: core.input (publishes), core.camera, core.control, locomotion.first_person (read).

use crate::zone::{self, layout};
use layout::published as p;
use layout::table::{self, Bound};

/// The directory (contract `abi.zone.published`), bound; `None` while nobody published.
fn directory() -> Option<Bound> {
    let f = zone::input_fields()?;
    table::bind(
        &p::SPEC,
        zone::zone_read_u32(f.published),
        zone::zone_read_u32,
    )
}

/// Lays the directory (the first publisher), its place in the head of `INPUT`.
fn lay_directory(published: u32) -> Option<Bound> {
    let dir = zone::zone_region(p::bytes(p::DEFAULT_CAPACITY));
    if dir == 0 {
        return None;
    }
    for (i, w) in table::head_words(&p::SPEC, p::DEFAULT_CAPACITY, 0)
        .into_iter()
        .enumerate()
    {
        zone::zone_write_u32(dir + 4 * i as u32, w);
    }
    // The head is whole before the directory's place is handed to anyone.
    zone::zone_write_u32(published, dir);
    table::bind(&p::SPEC, dir, zone::zone_read_u32)
}

/// Publishes (or moves) the region `name`: `offset` in the zone, `bytes` long (a block that begins with the
/// `TableHead` of its kit's contract). The first publisher lays the directory. False without a zone, or when the
/// directory is full.
pub fn publish(name: &str, offset: u32, bytes: u32) -> bool {
    let Some(f) = zone::input_fields() else {
        return false;
    };
    let Some(dir) = directory().or_else(|| lay_directory(f.published)) else {
        return false;
    };
    let key = p::hash(name);
    let count = zone::zone_read_u32(dir.count_at()).min(dir.slots);
    let mut slot = (0..count).find(|i| zone::zone_read_u64(dir.row(*i) + p::E_KEY) == key);
    if slot.is_none() {
        if count >= dir.slots {
            return false;
        }
        slot = Some(count);
    }
    let e = dir.row(slot.unwrap_or(0));
    zone::zone_write_u64(e + p::E_KEY, key);
    zone::zone_write_u32(e + p::E_OFFSET, offset);
    zone::zone_write_u32(e + p::E_BYTES, bytes);
    if slot == Some(count) {
        zone::zone_write_u32(dir.count_at(), count + 1);
    }
    true
}

/// The region `name` now: `(offset, bytes)`; `None` while nobody published it (its kit absent: the reader keeps its
/// default).
pub fn find(name: &str) -> Option<(u32, u32)> {
    let dir = directory()?;
    let key = p::hash(name);
    let count = zone::zone_read_u32(dir.count_at()).min(dir.slots);
    (0..count)
        .map(|i| dir.row(i))
        .find(|e| zone::zone_read_u64(e + p::E_KEY) == key)
        .map(|e| {
            (
                zone::zone_read_u32(e + p::E_OFFSET),
                zone::zone_read_u32(e + p::E_BYTES),
            )
        })
        .filter(|(o, _)| *o != 0)
}

/// The region `name`, bound as a block of the contract `spec` (its kit's): where its rows are and their stride, the
/// head read once. `None` while nobody published it, or when it is not a block this build reads.
pub fn find_bound(name: &str, spec: &layout::table::Spec) -> Option<Bound> {
    let (at, _) = find(name)?;
    table::bind(spec, at, zone::zone_read_u32)
}

/// The row of a `PointerFollow` block (contract `abi.input.pointer`) at `block`, its head read; `None` otherwise.
pub fn follow_row(block: u32) -> Option<u32> {
    table::bind(&layout::pointer_follow::SPEC, block, zone::zone_read_u32).map(|b| b.row(0))
}

/// The sum a `PointerFollow` row holds (what its writer's ticks summed).
pub fn follow_sum(row: u32) -> (f64, f64) {
    use layout::pointer_follow as f;
    (
        zone::zone_read_f64(row + f::SUM),
        zone::zone_read_f64(row + f::SUM + 8),
    )
}

/// A `PointerFollow` row now, for the render: `sum + (pointer - taken) × gain` (the pointer total read in place,
/// moved by the devices since the writer's tick; no frame late).
pub fn follow_now(row: u32) -> (f64, f64) {
    use layout::pointer_follow as f;
    let pointer = zone::input_fields().map_or(0, |f| f.pointer);
    let r = |o: u32| zone::zone_read_f64(row + o);
    let ptr = |o: u32| {
        if pointer == 0 {
            0.0
        } else {
            zone::zone_read_f64(pointer + o)
        }
    };
    let sum = follow_sum(row);
    (
        sum.0 + (ptr(0) - r(f::TAKEN)) * r(f::GAIN),
        sum.1 + (ptr(8) - r(f::TAKEN + 8)) * r(f::GAIN + 8),
    )
}

#[cfg(all(test, not(feature = "common-memory")))]
mod tests {
    use super::*;

    #[test]
    fn a_region_is_found_by_its_name_and_a_follow_extrapolates_the_pointer() {
        use layout::pointer_follow as f;
        assert_eq!(find("kit.a.region"), None);
        let a = zone::zone_region(32);
        let b = zone::zone_region(f::BYTES);
        for (i, w) in table::head_words(&f::SPEC, 1, 1).into_iter().enumerate() {
            zone::zone_write_u32(b + 4 * i as u32, w);
        }
        assert!(publish("kit.a.region", a, 32) && publish("kit.b.follow", b, f::BYTES));
        assert_eq!(find("kit.a.region"), Some((a, 32)));
        assert!(publish("kit.a.region", a + 8, 24));
        assert_eq!(
            find("kit.a.region"),
            Some((a + 8, 24)),
            "published again: moved, not doubled"
        );
        assert!(
            find_bound("kit.a.region", &f::SPEC).is_none(),
            "not a block of that contract"
        );
        let row = find_bound("kit.b.follow", &f::SPEC)
            .expect("a PointerFollow block")
            .row(0);
        assert_eq!(follow_row(b), Some(row));
        let pointer = zone::input_fields().expect("the head of INPUT").pointer;
        zone::zone_write_f64(pointer, 10.0);
        zone::zone_write_f64(row + f::SUM, 1.0);
        zone::zone_write_f64(row + f::TAKEN, 4.0);
        zone::zone_write_f64(row + f::GAIN, 0.5);
        assert_eq!(follow_now(row).0, 1.0 + (10.0 - 4.0) * 0.5);
        assert_eq!(follow_sum(row).0, 1.0);
    }
}
