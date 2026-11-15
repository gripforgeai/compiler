//! The zone allocator a host keeps beside the zone bytes (the bytes themselves are the host's: a `Vec<u8>`
//! natively, an `ArrayBuffer` / `SharedArrayBuffer` on the web — `web/module-calls.mjs` is the same algorithm).
//!
//! First fit, blocks rounded up to 8 bytes, offsets `0..8` never given (`0` is "none"), freed neighbours merged.
//! No metadata in the zone: a module can only damage the bytes of blocks it may write, never the allocator.
use std::collections::BTreeMap;
use std::vec::Vec;

/// Alignment and granule of a block.
pub const ALIGN: u32 = 8;

#[derive(Debug, Clone)]
pub struct ZoneAlloc {
    size: u32,
    /// Free ranges `(start, len)`, sorted by start, never adjacent.
    free: Vec<(u32, u32)>,
    /// Blocks given: start → (rounded length, owner module id; 0 = the host).
    used: BTreeMap<u32, (u32, u32)>,
}

impl ZoneAlloc {
    pub fn new(size: u32) -> Self {
        let size = size & !(ALIGN - 1);
        ZoneAlloc {
            size,
            free: std::vec![(ALIGN, size.saturating_sub(ALIGN))],
            used: BTreeMap::new(),
        }
    }
    pub fn size(&self) -> u32 {
        self.size
    }
    /// A block of at least `len` bytes (`len` 0 counts as 1) owned by `owner`, or `None` when the zone has no room.
    pub fn alloc(&mut self, len: u32, owner: u32) -> Option<u32> {
        let want = len.max(1).checked_add(ALIGN - 1)? & !(ALIGN - 1);
        let i = self.free.iter().position(|&(_, l)| l >= want)?;
        let (start, l) = self.free[i];
        if l == want {
            self.free.remove(i);
        } else {
            self.free[i] = (start + want, l - want);
        }
        self.used.insert(start, (want, owner));
        Some(start)
    }
    /// Gives a block back; `false` when `ptr` is not a block in use (freed twice, never given).
    pub fn free(&mut self, ptr: u32) -> bool {
        let Some((len, _)) = self.used.remove(&ptr) else {
            return false;
        };
        let i = self.free.partition_point(|&(s, _)| s < ptr);
        self.free.insert(i, (ptr, len));
        if i + 1 < self.free.len() && self.free[i].0 + self.free[i].1 == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == self.free[i].0 {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
        }
        true
    }
    /// The length of the block in use at `ptr`.
    pub fn block_len(&self, ptr: u32) -> Option<u32> {
        self.used.get(&ptr).map(|b| b.0)
    }
    /// The owner of the block in use at `ptr`.
    pub fn owner(&self, ptr: u32) -> Option<u32> {
        self.used.get(&ptr).map(|b| b.1)
    }
    /// Hands the block at `ptr` to `owner` (a posted call becomes the host's: its writer cannot change it any more).
    pub fn set_owner(&mut self, ptr: u32, owner: u32) -> bool {
        self.used.get_mut(&ptr).map(|b| b.1 = owner).is_some()
    }
    /// The owner of the block that holds all of `[ptr, ptr+len)`, if one does (what a module may write).
    pub fn owner_of_range(&self, ptr: u32, len: u32) -> Option<u32> {
        let (&start, &(blen, owner)) = self.used.range(..=ptr).next_back()?;
        (ptr.checked_add(len)? <= start + blen).then_some(owner)
    }
    /// Whether `[ptr, ptr+len)` lies inside the zone.
    pub fn in_bounds(&self, ptr: u32, len: u32) -> bool {
        ptr >= ALIGN && ptr.checked_add(len).is_some_and(|e| e <= self.size)
    }
    /// Bytes in use.
    pub fn used_bytes(&self) -> u32 {
        self.used.values().map(|b| b.0).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_free_merge() {
        let mut z = ZoneAlloc::new(128);
        let a = z.alloc(10, 1).unwrap();
        let b = z.alloc(16, 2).unwrap();
        let c = z.alloc(1, 1).unwrap();
        assert_eq!(z.owner_of_range(26, 14), Some(2));
        assert_eq!(z.owner_of_range(26, 15), None);
        assert_eq!((a, b, c), (8, 24, 40));
        assert!(z.free(b));
        assert!(!z.free(b));
        assert_eq!(z.alloc(16, 3), Some(24));
        assert!(z.free(a) && z.free(24) && z.free(c));
        assert_eq!(z.used_bytes(), 0);
        assert_eq!(z.alloc(120, 0), Some(8));
        assert_eq!(z.alloc(1, 0), None);
    }
}
