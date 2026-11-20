//! The neutral vector, yaw and timing helpers every moving kit shares (the "vectors, yaw, timing" part of
//! `kits/shared-runtime.ts`): one definition for the kits, no copy per kit and no `#[path]` between kits.
//!
//! Moved here unchanged from `shared_runtime.rs` of the former controller kit (D1 of
//! docs/design/core-control-locomotion-animation.md, #658: only the neutral functions go to the core; what is about
//! combat stays with the kit until `combat_core` is ported). Pure: no host, no state, every target.

use crate::js::{js_hypot, js_max, js_min};
use serde_json::Value as Json;

/// The orientation helpers of a pose (a quaternion, the cap computed from it), one definition for the core and the kits.
pub use gripforge_abi::turn::{
    quat_of_yaw, tilt_of, turn_about_up, with_yaw, yaw_of_quat, Quat, IDENTITY,
};

/// A position or a direction in the world frame: `[x, y, z]`, metres (y up).
pub type Vec3 = [f64; 3];

/// `typeof v === 'number' && Number.isFinite(v)`.
pub fn finite(v: Option<&Json>) -> Option<f64> {
    match v {
        Some(Json::Number(n)) => n.as_f64().filter(|x| x.is_finite()),
        _ => None,
    }
}

/// A `[x, y, z]` of three finite numbers.
pub fn vec3_of(v: Option<&Json>) -> Option<Vec3> {
    let a = v?.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some([finite(a.first())?, finite(a.get(1))?, finite(a.get(2))?])
}

/// `distXZ(a, b)`: the distance on the ground plane.
pub fn dist_xz(a: Vec3, b: Vec3) -> f64 {
    js_hypot(&[a[0] - b[0], a[2] - b[2]])
}

/// `dist3(a, b)`.
pub fn dist3(a: Vec3, b: Vec3) -> f64 {
    let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    js_hypot(&[x, y, z])
}

/// `lenXZ(v)`: the length on the ground plane.
pub fn len_xz(v: Vec3) -> f64 {
    js_hypot(&[v[0], v[2]])
}

/// `wrapAngle(a)`: wrapped to (-π, π].
pub fn wrap_angle(a: f64) -> f64 {
    use std::f64::consts::PI;
    let r = a % (PI * 2.0);
    if r <= -PI {
        r + PI * 2.0
    } else if r > PI {
        r - PI * 2.0
    } else {
        r
    }
}

/// `angleTowards(from, to, k)`: along the shortest arc by the blend factor `k` in [0, 1].
pub fn angle_towards(from: f64, to: f64, k: f64) -> f64 {
    let d = wrap_angle(to - from);
    wrap_angle(from + d * js_min(1.0, js_max(0.0, k)))
}

/// `yawFromDir(dx, dz)`: the yaw that faces the direction (0 faces +z).
pub fn yaw_from_dir(dx: f64, dz: f64) -> f64 {
    dx.atan2(dz)
}

/// `forwardOf(yaw)`: the unit direction a yaw faces, on the ground plane.
pub fn forward_of(yaw: f64) -> Vec3 {
    [yaw.sin(), 0.0, yaw.cos()]
}

/// `screenRightOf(yaw)`: the right of a view facing `yaw`, on the ground plane.
pub fn screen_right_of(yaw: f64) -> Vec3 {
    [-yaw.cos(), 0.0, yaw.sin()]
}

/// `follow(dt, tau)`: the per-tick blend factor of an exponential follow (`tau` ≤ 0 snaps).
pub fn follow(dt: f64, tau: f64) -> f64 {
    if tau <= 1e-6 {
        1.0
    } else {
        1.0 - (-dt / tau).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn wrap_angle_lands_in_the_half_open_turn() {
        assert_eq!(wrap_angle(0.0), 0.0);
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-12);
        assert!((wrap_angle(-PI) - PI).abs() < 1e-12);
        assert!((wrap_angle(PI / 2.0 + 4.0 * PI) - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn angle_towards_takes_the_short_arc_and_clamps_k() {
        let a = angle_towards(PI - 0.1, -PI + 0.1, 0.5);
        assert!((a.abs() - PI).abs() < 1e-9, "{a}");
        assert_eq!(angle_towards(0.0, 1.0, 2.0), 1.0);
        assert_eq!(angle_towards(0.0, 1.0, -1.0), 0.0);
    }

    #[test]
    fn yaw_and_forward_agree() {
        for yaw in [0.0, 0.7, -2.1, PI] {
            let f = forward_of(yaw);
            assert!((wrap_angle(yaw_from_dir(f[0], f[2]) - yaw)).abs() < 1e-12);
            assert!((len_xz(f) - 1.0).abs() < 1e-12);
            let r = screen_right_of(yaw);
            assert!(
                (f[0] * r[0] + f[2] * r[2]).abs() < 1e-12,
                "right is square to forward"
            );
        }
    }

    #[test]
    fn follow_snaps_without_a_time_constant() {
        assert_eq!(follow(0.016, 0.0), 1.0);
        let k = follow(0.1, 0.1);
        assert!((k - (1.0 - (-1f64).exp())).abs() < 1e-12);
    }

    #[test]
    fn vectors_read_only_three_finite_numbers() {
        assert_eq!(
            vec3_of(Some(&serde_json::json!([1, 2, 3]))),
            Some([1.0, 2.0, 3.0])
        );
        assert_eq!(vec3_of(Some(&serde_json::json!([1, 2]))), None);
        assert_eq!(vec3_of(Some(&serde_json::json!([1, "a", 3]))), None);
        assert_eq!(finite(Some(&serde_json::json!(2.5))), Some(2.5));
        assert_eq!(dist_xz([0.0, 5.0, 0.0], [3.0, -1.0, 4.0]), 5.0);
        assert_eq!(dist3([0.0, 0.0, 0.0], [2.0, 3.0, 6.0]), 7.0);
    }
}
