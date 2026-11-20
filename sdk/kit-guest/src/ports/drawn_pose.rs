//! The drawn pose of an entity, read in place at the tick (`kithost.scene@1.1` `watch_pose`, contract
//! `abi.scene.drawn`: core/abi/contracts/abi.scene.drawn.json): a kit watches an entity once, then reads at every tick the pose the last frame drew — from the
//! zone where the render side wrote it, no call, nothing copied. Who uses it: a kit that moves or aims relative to
//! what is on screen (the walking character reads the heading of the camera it is given).

use super::codec::scene_fn as f;
use super::{args, ask, SceneProxy};
use crate::zone::{zone_read_f64, zone_read_u64};
pub use gripforge_abi::ports::drawn::{CAMERA, ENTITY, OBJECT};
use gripforge_contracts::abi_scene_drawn::{at as field, SPEC};

/// A watched entity: where its block is in the zone, and where its row begins (bound once at the watch). Read with
/// [`DrawnPoseView::read`] at any tick; given back with [`SceneProxy::unwatch_pose`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawnPoseView {
    /// The block (`TableHead`, then the row), what `watch_pose` answered.
    pub at: u32,
    row: u32,
}

/// The pose as one frame drew it: world space of the kits (metres, right-handed, +Y up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawnPose {
    /// The drawn frame that wrote it (counted from 1 since the first watch).
    pub frame: u64,
    pub position: [f64; 3],
    /// Quaternion xyzw.
    pub rotation: [f64; 4],
}

impl DrawnPoseView {
    /// The pose of the last frame that drew the entity; `None` before the first.
    pub fn read(&self) -> Option<DrawnPose> {
        let frame = zone_read_u64(self.row + field::FRAME as u32);
        (frame != 0).then(|| {
            let v = |i: u32| zone_read_f64(self.row + field::POSITION as u32 + 8 * i);
            let r = |i: u32| zone_read_f64(self.row + field::ROTATION as u32 + 8 * i);
            DrawnPose {
                frame,
                position: [v(0), v(1), v(2)],
                rotation: [r(0), r(1), r(2), r(3)],
            }
        })
    }
}

impl DrawnPose {
    /// Where the local `axis` points in the world (the rotation applied to it).
    pub fn along(&self, axis: [f64; 3]) -> [f64; 3] {
        let [x, y, z, w] = self.rotation;
        let t = [
            2. * (y * axis[2] - z * axis[1]),
            2. * (z * axis[0] - x * axis[2]),
            2. * (x * axis[1] - y * axis[0]),
        ];
        [
            axis[0] + w * t[0] + (y * t[2] - z * t[1]),
            axis[1] + w * t[1] + (z * t[0] - x * t[2]),
            axis[2] + w * t[2] + (x * t[1] - y * t[0]),
        ]
    }
    /// Yaw and pitch of the local `axis` in the kits' convention (yaw 0 along +Z, pitch up positive): `[0, 0, -1]`
    /// for a camera (it looks along its −Z), `[0, 0, 1]` for an entity facing +Z.
    pub fn heading(&self, axis: [f64; 3]) -> (f64, f64) {
        let d = self.along(axis);
        (d[0].atan2(d[2]), d[1].atan2(d[0].hypot(d[2])))
    }
}

impl SceneProxy {
    /// Watches the drawn pose of `handle` of `kind` ([`OBJECT`] an engine object, [`ENTITY`] a kit entity by the
    /// number of its id, [`CAMERA`] the camera the engine draws with, handle 0). `None`: refused.
    pub fn watch_pose(&self, kind: u32, handle: u64) -> Option<DrawnPoseView> {
        let at = ask(
            crate::func!(f::WATCH_POSE),
            &args(|w| {
                w.u32(kind).u64(handle);
            }),
            |r| r.u32(),
        );
        if at == 0 {
            return None;
        }
        // Bound once: a block of another contract, or of a writer that lays out fewer fields, is given back.
        match super::zone_table::bind(&SPEC, at) {
            Some(row) => Some(DrawnPoseView { at, row }),
            None => {
                self.unwatch_pose(DrawnPoseView { at, row: at });
                None
            }
        }
    }
    /// One watcher less; the record is freed with its last watcher.
    pub fn unwatch_pose(&self, view: DrawnPoseView) -> bool {
        ask(
            crate::func!(f::UNWATCH_POSE),
            &args(|w| {
                w.u32(view.at);
            }),
            |r| r.bool(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::DrawnPose;

    #[test]
    fn the_heading_follows_the_kits_convention() {
        // A camera turned by `quat_neg_z(yaw 0.3, pitch 0.2)`: −Z yawed by 0.3 + π, then pitched up by 0.2.
        let (yaw, pitch) = (0.3f64, 0.2f64);
        let q = |axis: [f64; 3], a: f64| {
            let (s, c) = (a / 2.).sin_cos();
            [axis[0] * s, axis[1] * s, axis[2] * s, c]
        };
        let (a, b) = (
            q([0., 1., 0.], yaw + std::f64::consts::PI),
            q([1., 0., 0.], pitch),
        );
        let rotation = [
            a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
            a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
            a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
            a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
        ];
        let pose = DrawnPose {
            rotation,
            ..Default::default()
        };
        let (y, p) = pose.heading([0., 0., -1.]);
        assert!(
            (y - yaw).abs() < 1e-9 && (p - pitch).abs() < 1e-9,
            "{y} {p}"
        );
        // An entity turned about +Y, facing +Z: its yaw.
        let turned = DrawnPose {
            rotation: q([0., 1., 0.], 1.1),
            ..Default::default()
        };
        assert!((turned.heading([0., 0., 1.]).0 - 1.1).abs() < 1e-9);
    }
}
