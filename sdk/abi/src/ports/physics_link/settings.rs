// The settings the physics applies to its own step, sent by the user through the order queue (as `SET_FIXED_DT`):
// the speed of the game time (F9's slow and fast motion) and the hold of a hidden page in a solo game
// (decision « horloges et boucles » of 19/11, correctifs 1 and 3, PR #1790). Never a clock the physics reads.
// Part of `physics_link.rs`.

use super::Sim;

impl Sim {
    /// `SET_SPEED`: the time a loop plays per second of wall time (0.05 to 8; anything else is ignored).
    pub(super) fn set_speed(&mut self, speed: f64) -> Option<u32> {
        if (0.05..=8.0).contains(&speed) {
            self.speed = speed;
        }
        None
    }

    /// `SET_HELD`: held, a loop plays no time; let go, it starts again from its next turn (no catch-up).
    pub(super) fn set_held(&mut self, held: bool) -> Option<u32> {
        if self.held && !held {
            self.last_ms = f64::NAN;
        }
        self.held = held;
        None
    }
}
