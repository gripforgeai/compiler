//! The register of the revision keys (names « à valider »: `revision_keys::Keys`, keys `action` and `physics`): each
//! owner of a part of an entity (the action tick, the physics, tomorrow an animation or a third-party kit, on any
//! Worker) enters its key once at its start and gets a slot number, the same on every thread that reads the block;
//! afterwards only the numbers travel (never a text per frame). Like the directory of the mailboxes
//! ([`super::mailbox::Table`]): a table of fixed layout, entered under a word lock, read in place without one.
//!
//! Layout: the contract `abi.physics.revisions` (core/abi/contracts/abi.physics.revisions.json): a `TableHead` (its
//! `count` the keys entered, its `slots` the capacity), the lock word in its head, then one row per key (`len`, `name`),
//! every position generated, read at the stride the block announces. [`Keys::lay`] enters `action` (slot 0) and
//! `physics` (slot 1).
//!
//! **A moddable register** (the common register type of the project, `docs/registres.md`): this table is only the
//! low-level storage shared by the Workers; the register itself is the collection `revisionKeys` of game.engine
//! (`engine.registries.revisionKeys` in core.kit-api's declaration), filled by kits and mods in data and read by
//! `gripforge_kit_api::revision_keys::declare`. `action` and `physics` are the base keys: always there, never removed
//! ([`Keys::remove`] refuses them). A removed key keeps its slot (bit 31 of `len`, the numbers held by the other
//! threads stay true); entering it again lifts the mark.
//!
//! History of the contract (the ABI value is unchanged, set at the tag; the contract's own lines are in its description):
//! - feat/registres-moddables (PR #1782 toward feat_refonte-kits): bit 31 of an entry's `len` = removed; base keys
//!   refused by `remove`. Recompile: the modules that read the table (kithost, physics Worker).
//! - feat/abi-contrat-lot2c: the table as the contract `abi.physics.revisions` (a `TableHead` instead of the four
//!   words written at hand, the lock in its head). Recompile: kithost.core and the native player.

use gripforge_contracts::abi_physics_revisions as rev;
use gripforge_contracts::table_head::{Refused, TableHead, Why};
use std::sync::atomic::{AtomicU32, Ordering::*};

/// Slots of a [`Keys`] table.
pub const KEYS: u32 = 32;
/// Bytes of a key's name (the contract's `name`).
pub const KEY_NAME: usize = 28;
/// Bytes of a [`Keys`] table laid by this build.
pub const KEYS_BYTES: usize = rev::bytes(KEYS);
// The name fills its row after `len` (the contract's `u8[28]`).
const _: () = assert!(rev::at::NAME + KEY_NAME == rev::STRIDE as usize);
/// The slot of `action`: what the action tick writes on the entity.
pub const ACTION: u16 = 0;
/// The slot of `physics`: the pose the physics owns, its published revision (`rev` of `abi.physics.bodies`).
pub const PHYSICS: u16 = 1;

/// Base keys: the first slots, laid by [`Keys::lay`], never removed.
pub const BASE: u16 = 2;
/// Bit of an entry's `len`: the key was removed (its slot is kept).
const REMOVED: u32 = 1 << 31;

/// Why [`Keys::remove`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveRefused {
    /// `action` or `physics`: base keys are fixed.
    Base,
    /// No such key.
    Unknown,
}

/// A register of revision keys over a block every thread reads in place.
#[derive(Clone, Copy)]
pub struct Keys(pub *mut u8);

// SAFETY: every write is under the table's lock word, every read after `len` covered it (Release / Acquire).
unsafe impl Send for Keys {}
unsafe impl Sync for Keys {}

impl Keys {
    fn word(&self, at: usize) -> &AtomicU32 {
        // SAFETY: inside the table, 4-aligned.
        unsafe { &*(self.0.add(at) as *const AtomicU32) }
    }

    /// The place of row `slot`, at the stride the block announces (its `TableHead`, written once at the lay).
    fn row(&self, slot: usize) -> usize {
        // SAFETY: a table laid by `lay` (its TableHead written before the table was handed out).
        let h = unsafe { &*(self.0 as *const TableHead) };
        h.head_bytes as usize + slot * h.stride as usize
    }

    /// Lays a table over a zeroed block of [`KEYS_BYTES`] (8-aligned), `action` and `physics` entered.
    ///
    /// # Safety
    /// `p` valid for [`KEYS_BYTES`], nobody reads it yet.
    pub unsafe fn lay(p: *mut u8) -> Keys {
        rev::init(p, KEYS);
        let k = Keys(p);
        k.register("action");
        k.register("physics");
        k
    }

    /// Checks once a table laid by another build (the player's, shared by its Workers): of its contract, and holding
    /// this build's rows and lock. A refusal names the contract and what does not fit (rebuild the side named).
    ///
    /// # Safety
    /// `self.0` a block aligned on 8, its `TableHead` written.
    pub unsafe fn check(&self) -> Result<(), Refused> {
        let h = TableHead::read(self.0);
        h.check(&rev::SPEC)?;
        if (h.stride as usize) < rev::at::NAME + KEY_NAME {
            return Err(Refused {
                contract: rev::NAME,
                why: Why::Stride(h.stride),
            });
        }
        Ok(())
    }

    /// Keys entered.
    pub fn len(&self) -> usize {
        // SAFETY: the TableHead of a laid table; its count moves (Release) after the row it covers.
        let h = unsafe { &*(self.0 as *const TableHead) };
        (unsafe { TableHead::count(self.0) } as usize).min(h.slots as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The name of `slot`, read in place (F9 says which owner changed what).
    pub fn name(&self, slot: u16) -> Option<&str> {
        let i = slot as usize;
        if i >= self.len() {
            return None;
        }
        let e = self.row(i);
        let len = ((self.word(e + rev::at::LEN).load(Relaxed) & !REMOVED) as usize).min(KEY_NAME);
        // SAFETY: an entry written before `len` moved past it.
        let b = unsafe { std::slice::from_raw_parts(self.0.add(e + rev::at::NAME), len) };
        std::str::from_utf8(b).ok()
    }

    /// Whether `slot` was removed (its number stays reserved).
    pub fn is_removed(&self, slot: u16) -> bool {
        (slot as usize) < self.len()
            && self
                .word(self.row(slot as usize) + rev::at::LEN)
                .load(Acquire)
                & REMOVED
                != 0
    }

    fn slot_of(&self, key: &str) -> Option<u16> {
        (0..self.len())
            .find(|&i| self.name(i as u16) == Some(key))
            .map(|i| i as u16)
    }

    /// The slot of `key`, without entering it (`None` when removed).
    pub fn find(&self, key: &str) -> Option<u16> {
        self.slot_of(key).filter(|&s| !self.is_removed(s))
    }

    /// Removes `key` (a kit or a mod takes back what it entered); the base keys are refused.
    pub fn remove(&self, key: &str) -> Result<(), RemoveRefused> {
        let s = self.find(key).ok_or(RemoveRefused::Unknown)?;
        if s < BASE {
            return Err(RemoveRefused::Base);
        }
        self.word(self.row(s as usize) + rev::at::LEN)
            .fetch_or(REMOVED, AcqRel);
        Ok(())
    }

    /// The slot of `key`, entered now when it is new (once, at the start of its owner). `None`: the table is full or
    /// the name longer than [`KEY_NAME`].
    pub fn register(&self, key: &str) -> Option<u16> {
        if key.is_empty() || key.len() > KEY_NAME {
            return None;
        }
        if let Some(s) = self.slot_of(key) {
            self.word(self.row(s as usize) + rev::at::LEN)
                .fetch_and(!REMOVED, AcqRel);
            return Some(s);
        }
        while self
            .word(rev::at::head::LOCK)
            .compare_exchange_weak(0, 1, Acquire, Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        let n = self.len();
        let slot = match self.slot_of(key) {
            Some(s) => Some(s),
            // SAFETY: the TableHead of a laid table (its slots never change).
            None if n < unsafe { (*(self.0 as *const TableHead)).slots } as usize => {
                let e = self.row(n);
                // SAFETY: an entry nobody reads before the count covers it.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        key.as_ptr(),
                        self.0.add(e + rev::at::NAME),
                        key.len(),
                    )
                };
                self.word(e + rev::at::LEN).store(key.len() as u32, Relaxed);
                // SAFETY: as above; Release: a reader that loads the count sees the row.
                unsafe { rev::publish(self.0, n as u32 + 1) };
                Some(n as u16)
            }
            None => None,
        };
        self.word(rev::at::head::LOCK).store(0, Release);
        slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two Workers (two threads) on the same block enter keys: the same numbers on both, `action` 0 and `physics` 1.
    #[test]
    fn two_workers_on_one_block_get_the_same_slots() {
        let mut block = vec![0u64; KEYS_BYTES / 8].into_boxed_slice();
        let keys = unsafe { Keys::lay(block.as_mut_ptr() as *mut u8) };
        let a = std::thread::spawn(move || {
            (0..8)
                .map(|i| keys.register(&format!("worker.k{i}")).unwrap())
                .collect::<Vec<_>>()
        });
        let b = std::thread::spawn(move || {
            (0..8)
                .rev()
                .map(|i| keys.register(&format!("worker.k{i}")).unwrap())
                .collect::<Vec<_>>()
        });
        let (a, mut b) = (a.join().unwrap(), b.join().unwrap());
        b.reverse();
        assert_eq!(a, b, "the same slot for a key on both threads");
        assert_eq!(keys.find("action"), Some(ACTION));
        assert_eq!(keys.find("physics"), Some(PHYSICS));
        assert_eq!(keys.len(), 10);
        assert_eq!(keys.name(a[3]), Some("worker.k3"));
        assert_eq!(keys.register(&"x".repeat(KEY_NAME + 1)), None);
        drop(block);
    }

    /// The table is a block of its contract: bound at the stride it announces, the keys entered are its count.
    #[test]
    fn the_table_is_a_block_of_its_contract() {
        let mut block = vec![0u64; KEYS_BYTES / 8].into_boxed_slice();
        let keys = unsafe { Keys::lay(block.as_mut_ptr() as *mut u8) };
        assert!(unsafe { keys.check() }.is_ok());
        let t = unsafe { rev::Table::bind(keys.0) }.expect("a block of abi.physics.revisions");
        assert_eq!((t.slots(), t.count()), (KEYS, 2));
        let s = keys.register("anim").unwrap();
        assert_eq!((t.count(), t.len(u32::from(s))), (3, 4));
        assert_eq!(&t.name(u32::from(s))[..4], b"anim");
        // Not a table of keys: refused by name.
        let mut other = vec![0u64; KEYS_BYTES / 8].into_boxed_slice();
        assert!(unsafe { Keys(other.as_mut_ptr() as *mut u8).check() }.is_err());
        drop(block);
    }

    /// Base keys are refused by `remove`; a removed key keeps its slot and gets it back when entered again.
    #[test]
    fn base_keys_cannot_be_removed_and_a_removed_key_keeps_its_slot() {
        let mut block = vec![0u64; KEYS_BYTES / 8].into_boxed_slice();
        let keys = unsafe { Keys::lay(block.as_mut_ptr() as *mut u8) };
        assert_eq!(keys.remove("action"), Err(RemoveRefused::Base));
        assert_eq!(keys.remove("physics"), Err(RemoveRefused::Base));
        assert_eq!(keys.remove("nope"), Err(RemoveRefused::Unknown));
        let s = keys.register("mod.anim").unwrap();
        assert_eq!(keys.remove("mod.anim"), Ok(()));
        assert_eq!(keys.find("mod.anim"), None);
        assert!(keys.is_removed(s));
        assert_eq!(keys.register("mod.anim"), Some(s));
        assert_eq!(keys.find("mod.anim"), Some(s));
        drop(block);
    }
}
