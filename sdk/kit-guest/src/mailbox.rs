//! The mailboxes on a kit's side (docs/decisions/2026-11-19-canaux-directs.md): the directory the player laid in the
//! shared zone, read in place; a kit writes its messages **straight into** the box of the module it speaks to (no
//! call of the host, no passage through the core), and reads a state stream in place. A message sent before its box
//! is in the directory, or while the box is full, waits here and goes in order (nothing dropped, nothing reordered).
//! A kit module in its own memory (isolated) has no directory: its messages wait.

use gripforge_abi::ports::mailbox::{self as mb, Outbox, Resolve};
pub use gripforge_abi::ports::mailbox::{kind, MailboxHead, Table};
use std::cell::RefCell;

/// The directory where it is now (never kept: the zone may move). `None`: an isolated module, or no directory.
pub fn table() -> Option<Table> {
    #[cfg(feature = "common-memory")]
    {
        let s = crate::zone::state();
        if s.size < gripforge_abi::zone::state::MAILBOXES + 4 {
            return None;
        }
        let off = crate::zone::zone_read_u32(s.offset + gripforge_abi::zone::state::MAILBOXES);
        if off == 0 {
            return None;
        }
        // SAFETY: the table the core laid in the zone, in the memory this module shares with the core.
        Some(Table((crate::mem::zone_base() + off as usize) as *mut u8))
    }
    #[cfg(not(feature = "common-memory"))]
    None
}

/// No directory: nothing resolves (the messages wait).
struct Nowhere;
impl Resolve for Nowhere {
    fn find(&self, _: u32, _: &str) -> Option<(usize, u32)> {
        None
    }
}

thread_local! {
    static OUT: RefCell<Option<Outbox>> = const { RefCell::new(None) };
}

/// The sender's number of a kit: its id hashed (FNV-1a), the same on every run (the order is replayable).
pub fn sender(kit: &str) -> u32 {
    kit.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Writes one message for `tick` into the box `name` (`payload` from `gripforge_kit_common::bus::pack`).
pub fn send(kit: &str, name: &str, tick: u32, code: u32, payload: &[u64]) {
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        let out = o.get_or_insert_with(|| Outbox::new(sender(kit)));
        match table() {
            Some(t) => out.send(&t, 0, name, tick, code, payload),
            None => out.send(&Nowhere, 0, name, tick, code, payload),
        }
    })
}

/// Sends what waits (call it once a tick: a box opened late, a box that had no room).
pub fn flush() {
    OUT.with(|o| {
        if let (Some(out), Some(t)) = (o.borrow_mut().as_mut(), table()) {
            out.flush(&t);
        }
    })
}

/// The newest message of `code` in the state stream `name` whose payload starts with `key`, copied into `out`
/// (the words after the key). `None`: no box, or no such message yet.
pub fn newest(name: &str, code: u32, key: &[u64], out: &mut [u64]) -> Option<usize> {
    let (at, _) = table()?.find(0, name)?;
    // SAFETY: a block of the directory, alive with its module.
    unsafe { mb::newest(MailboxHead::at(at), code, key, out) }
}
