// The physics time thrown away, counted (decision « horloges et boucles » of 19/11, correctif 2, PR #1790): throwing
// it stays wanted (the budget of the physics), it is only counted, for the debugger (`core.physics.counters`).
// Part of `physics_link.rs`.

use super::PhysicsLink;
use std::sync::atomic::Ordering::Relaxed;

impl PhysicsLink {
    /// Counts physics time thrown away (`s` seconds, nothing when not positive) and the fixed steps it held.
    pub(crate) fn throw(&self, s: f64, steps: u32) {
        if s > 0.0 {
            self.dropped_ms
                .fetch_add((s * 1000.0).round() as u32, Relaxed);
        }
        if steps > 0 {
            self.skipped.fetch_add(steps, Relaxed);
        }
    }
}
