//! A yaw applied at the render (`kithost.scene@1.2` `claim_render_yaw`, layout: gripforge-abi `ports::render_yaw`): a
//! kit claims once that an entity is drawn with `anchor - look × sensitivity`, the look read in place at the render.
//! The record is the kit's to write in place afterwards (sensitivity, a yaw the game sets); the tick reads the yaw the
//! frame drew back with `watch_pose(ENTITY, …)` of the same entity. Who uses it: a first-person body (the character
//! kits), any kit whose entity turns with the look in the frame the mouse moved.

use super::codec::scene_fn as f;
use super::{args, ask, SceneProxy};
use crate::zone::{look_now, zone_read_f64, zone_read_u32, zone_write_f64};
pub use gripforge_abi::ports::render_yaw::LOOK;
use gripforge_abi::ports::render_yaw::{ANCHOR, SENSITIVITY, SOURCE};

/// A claimed yaw: where its record is in the zone. Given back with [`SceneProxy::release_render_yaw`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderYaw {
    pub at: u32,
}

impl RenderYaw {
    /// The look of the record's source now (x), 0 while none is published.
    fn look(&self) -> f64 {
        match zone_read_u32(self.at + SOURCE) {
            LOOK => look_now().map_or(0.0, |l| l.0),
            _ => 0.0,
        }
    }
    /// Radians per unit of the look.
    pub fn sensitivity(&self) -> f64 {
        zone_read_f64(self.at + SENSITIVITY)
    }
    /// The yaw the render draws now (`anchor - look × sensitivity`, not wrapped).
    pub fn yaw(&self) -> f64 {
        zone_read_f64(self.at + ANCHOR) - self.look() * self.sensitivity()
    }
    /// The entity is drawn at `yaw` from now on, the look turning it from there (a respawn, a yaw the game sets).
    pub fn set_yaw(&self, yaw: f64) {
        zone_write_f64(self.at + ANCHOR, yaw + self.look() * self.sensitivity());
    }
    /// Another sensitivity, the yaw drawn now kept (no jump).
    pub fn set_sensitivity(&self, sensitivity: f64) {
        if !sensitivity.is_finite() {
            return;
        }
        let yaw = self.yaw();
        zone_write_f64(self.at + SENSITIVITY, sensitivity);
        self.set_yaw(yaw);
    }
}

impl SceneProxy {
    /// Claims the yaw of the kit entity numbered `entity` (`e12` → 12) for the render, from `source` ([`LOOK`]: the
    /// look of player 0 of core.input) at `sensitivity` radians per unit of the look. Anchored on the entity's yaw
    /// now; claimed again, anchored again. `None`: refused (no such entity, unknown source, zone full).
    pub fn claim_render_yaw(
        &self,
        entity: u64,
        source: u32,
        sensitivity: f64,
    ) -> Option<RenderYaw> {
        let at = ask(
            crate::func!(f::CLAIM_RENDER_YAW),
            &args(|w| {
                w.u64(entity).u32(source).f64(sensitivity);
            }),
            |r| r.u32(),
        );
        (at != 0).then_some(RenderYaw { at })
    }
    /// The entity is drawn with its ticks' yaw again.
    pub fn release_render_yaw(&self, claim: RenderYaw) -> bool {
        ask(
            crate::func!(f::RELEASE_RENDER_YAW),
            &args(|w| {
                w.u32(claim.at);
            }),
            |r| r.bool(),
        )
    }
}
