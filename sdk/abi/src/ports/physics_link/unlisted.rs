//! What the port does not list and the writer publishes all the same (docs/decisions/2026-11-18-proprietaire-position.md):
//! the moving bodies the port left out (held out of context, carried, fixed for a while) and the static bodies an order
//! moved. Part of `physics_link.rs` (`mod unlisted;`). Used by: [`Sim::list_poses`](super::Sim).

use super::{kind, Sim};
use crate::ports::{PhysicsPort, V3};

/// A published pose: the position `p` and the quaternion `q` of a body (normalised; docs/design/pose-quaternion.md).
pub(super) fn pose_of(p: V3, q: [f64; 4]) -> [f64; 7] {
    let q = crate::turn::normalize(q);
    [p[0], p[1], p[2], q[0], q[1], q[2], q[3]]
}

/// The pose of a body the port listed (`for_each_pose` gives its cap only): a body's whole rotation, a character's cap
/// (its heading, the physics' own: `turn`).
pub(super) fn listed_pose(
    p: &dyn PhysicsPort,
    name: &str,
    at: V3,
    yaw: f64,
    character: bool,
) -> [f64; 7] {
    match (character, p.body(name)) {
        (false, Some(b)) => pose_of(at, b.q),
        _ => pose_of(at, crate::turn::quat_of_yaw(yaw)),
    }
}

impl Sim {
    /// Adds to `posed` the bodies the port's own list (`for_each_pose`, slots marked with `turn`) left out.
    pub(super) fn post_unlisted(
        &self,
        p: &dyn PhysicsPort,
        cap: u32,
        turn: u32,
        posed: &mut Vec<(u32, [f64; 7], bool)>,
    ) {
        // Every moving body the port did not list (held out of context, carried, fixed for a while): published all the
        // same, at its pose now, else at the last pose published — an owned pose never goes back to its entity.
        for &slot in &self.moving {
            if slot >= cap || self.listed.get(slot as usize) == Some(&turn) {
                continue;
            }
            let Some(name) = self.name(slot) else {
                continue;
            };
            let character = self.kinds[slot as usize] == kind::CHARACTER;
            // A character's heading is its body's (the physics turns it, `turn`), with its position.
            let now = if character {
                p.character_position(name)
                    .map(|c| pose_of(c, p.body(name).map_or(crate::turn::IDENTITY, |b| b.q)))
            } else {
                p.body(name).map(|b| pose_of(b.p, b.q))
            };
            let last = self.last_pose.get(slot as usize).map(|l| l.0);
            let pose = match (now, last) {
                (Some(n), _) => n,
                (None, Some(l)) if !l[0].is_nan() => l,
                _ => continue,
            };
            posed.push((slot, pose, character));
        }
        // The static bodies an order moved: the port lists only the moving ones, the writer publishes what it changed.
        for &slot in &self.placed {
            if slot >= cap || posed.iter().any(|e| e.0 == slot) {
                continue;
            }
            if let Some(b) = self.name(slot).and_then(|n| p.body(n)) {
                posed.push((slot, pose_of(b.p, b.q), false));
            }
        }
    }
}
