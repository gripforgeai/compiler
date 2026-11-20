//! The contact events a step writes in the events ring of the link (contract `abi.physics.link_event`).

use super::{PhysicsLink, Sim, NO_SLOT};
use std::sync::atomic::Ordering::Relaxed;

impl Sim {
    /// A record of the events ring (contract `abi.physics.link_event`: the two slots, the flag, the value, the bytes
    /// of the two names) then the names of the slots this link does not know (a collider under a name nobody added
    /// through it).
    pub(super) fn push_pair(
        &self,
        link: &PhysicsLink,
        k: u32,
        a: &str,
        b: &str,
        flag: u64,
        value: f64,
    ) {
        use gripforge_contracts::abi_physics_link_event as ev;
        let (sa, sb) = (self.slot_of(a), self.slot_of(b));
        let na = if sa == NO_SLOT { a.as_bytes() } else { &[] };
        let nb = if sb == NO_SLOT { b.as_bytes() } else { &[] };
        // The ring's own header word, the event's words, the names.
        let words = 1 + ev::WORDS as u32 + (na.len() + nb.len()).div_ceil(8) as u32;
        // SAFETY: the physics is the one producer of the events ring.
        let pushed = unsafe {
            link.events.push(k, words, |w| {
                w[ev::at::PAIR] = sa as u64 | (sb as u64) << 32;
                w[ev::at::FLAG] = flag;
                w[ev::at::VALUE] = value.to_bits();
                w[ev::at::NAMES] = na.len() as u64 | (nb.len() as u64) << 32;
                let bytes = std::slice::from_raw_parts_mut(
                    w[ev::WORDS..].as_mut_ptr() as *mut u8,
                    (w.len() - ev::WORDS) * 8,
                );
                bytes[..na.len()].copy_from_slice(na);
                bytes[na.len()..na.len() + nb.len()].copy_from_slice(nb);
            })
        };
        if !pushed {
            link.events_dropped.fetch_add(1, Relaxed);
        }
    }
}
