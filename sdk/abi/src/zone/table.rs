//! Blocks of the zone that follow a shared contract (docs/decisions/2026-11-20-abi-par-contrat.md §5), reached by their
//! OFFSET in the zone: what the host, the façade of the kits (an isolated kit reads the zone across two memories, word by
//! word) and the player share. A block begins with `TableHead`, written once by its writer before its offset is handed
//! to anyone; a reader binds it once ([`bind`]: the contract, its major, the fields it needs, the stride) and keeps
//! where its rows begin and their stride: every field is then read at `row(i) + a constant of the generated contract`,
//! never at a position written here. Written new for the lot 3 of the contracts.

pub use gripforge_contracts::table_head::{Refused, Spec, TableHead};

/// Bytes of `TableHead` (where the head of a contract and its rows may begin, never before).
pub const HEAD: u32 = core::mem::size_of::<TableHead>() as u32;
/// Where `count` is in `TableHead` (the one word a writer rewrites at each publication).
pub const COUNT: u32 = core::mem::offset_of!(TableHead, count) as u32;

/// A bound block: where its rows begin, their stride, the rows reserved and the fields its writer knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bound {
    /// The block (its `TableHead`), an offset in the zone.
    pub at: u32,
    /// Row 0.
    pub rows: u32,
    pub stride: u32,
    pub slots: u32,
    pub fields: u16,
    pub head_fields: u16,
}

impl Bound {
    /// The offset of row `i` (`i` < `slots`: the caller's to hold).
    #[inline]
    pub fn row(&self, i: u32) -> u32 {
        self.rows + i * self.stride
    }
    /// The offset of the word `count` of the block.
    #[inline]
    pub fn count_at(&self) -> u32 {
        self.at + COUNT
    }
    /// The bytes the block spans (its head and its reserved rows), as its head says.
    #[inline]
    pub fn bytes(&self) -> u32 {
        self.rows - self.at + self.slots * self.stride
    }
}

/// Reads the `TableHead` at `at` with `read_u32(offset)` and checks it against `spec` (what this build reads).
pub fn bind_or(spec: &Spec, at: u32, read_u32: impl Fn(u32) -> u32) -> Result<Bound, Refused> {
    let h = TableHead::from_words(core::array::from_fn(|i| read_u32(at + 4 * i as u32)));
    h.check(spec)?;
    Ok(Bound {
        at,
        rows: at + h.head_bytes,
        stride: h.stride,
        slots: h.slots,
        fields: h.fields,
        head_fields: h.head_fields,
    })
}

/// As [`bind_or`]; `None` at offset 0 or when refused (the check at load refuses such a module first: this is the net
/// for a block made after the load, or of another contract at this place).
pub fn bind(spec: &Spec, at: u32, read_u32: impl Fn(u32) -> u32) -> Option<Bound> {
    if at == 0 {
        return None;
    }
    bind_or(spec, at, read_u32).ok()
}

/// The words of the `TableHead` of a new block of `slots` rows of `spec`, `count` rows in service: what a writer
/// writes, word by word, before it hands the block's offset to anyone.
pub fn head_words(spec: &Spec, slots: u32, count: u32) -> [u32; 8] {
    TableHead {
        count,
        ..TableHead::new(spec, slots)
    }
    .to_words()
}

/// The bytes of a block of `slots` rows of `spec` as this build writes it.
pub const fn bytes(spec: &Spec, slots: u32) -> u32 {
    spec.head_bytes + slots * spec.stride
}

/// The writer of this build: the offset of row `i` of a block it made (its own head bytes and stride).
#[inline]
pub const fn row_of(spec: &Spec, at: u32, i: u32) -> u32 {
    at + spec.head_bytes + i * spec.stride
}

/// Writes the `TableHead` of a new block at `at` of the bytes `zone` (a writer that holds the zone as bytes: the host
/// when it lays the zone).
pub fn lay(zone: &mut [u8], at: usize, spec: &Spec, slots: u32, count: u32) {
    for (i, w) in head_words(spec, slots, count).into_iter().enumerate() {
        zone[at + 4 * i..at + 4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gripforge_contracts::abi_input_pointer as pointer;
    use gripforge_contracts::abi_zone_published as published;

    #[test]
    fn a_block_laid_binds_and_its_rows_follow_at_the_stride_of_its_writer() {
        let mut z = vec![0u8; 512];
        lay(&mut z, 64, &published::SPEC, 4, 2);
        let read = |o: u32| u32::from_le_bytes(z[o as usize..o as usize + 4].try_into().unwrap());
        let b = bind(&published::SPEC, 64, read).expect("its own writer");
        assert_eq!(
            (b.rows, b.stride, b.slots),
            (64 + published::HEAD_BYTES, published::STRIDE, 4)
        );
        assert_eq!(b.row(3), row_of(&published::SPEC, 64, 3));
        assert_eq!(read(b.count_at()), 2);
        assert_eq!(b.bytes(), bytes(&published::SPEC, 4));
        // Another contract at this place, or nothing laid: refused.
        assert!(bind(&pointer::SPEC, 64, read).is_none());
        assert!(bind(&published::SPEC, 0, read).is_none());
        assert!(bind(&published::SPEC, 128, read).is_none());
    }

    #[test]
    fn a_writer_of_a_longer_row_is_read_at_its_own_stride() {
        // Tomorrow's writer adds a field at the end: a wider stride, one field more; today's reader binds and reads row
        // 1 where that writer put it.
        let mut z = vec![0u8; 512];
        let mut w = head_words(&published::SPEC, 3, 3);
        let h = TableHead::from_words(w);
        w = TableHead {
            fields: h.fields + 1,
            stride: h.stride + 8,
            ..h
        }
        .to_words();
        for (i, v) in w.iter().enumerate() {
            z[8 + 4 * i..8 + 4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        let read = |o: u32| u32::from_le_bytes(z[o as usize..o as usize + 4].try_into().unwrap());
        let b = bind(&published::SPEC, 8, read).expect("an added field refuses nobody");
        assert_eq!(b.row(1) - b.row(0), published::STRIDE + 8);
    }
}
