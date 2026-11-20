//! The orientation of a pose (docs/design/pose-quaternion.md): a unit quaternion `[x, y, z, w]`, right handed, Y up —
//! the only orientation an entity stores. The cap (the yaw about +Y, the kits' forward `(sin yaw, ·, cos yaw)`) is
//! never stored: these helpers compute it from the quaternion, or the quaternion of a cap. Pure, every target, shared
//! by the core (the physics owner, the frame) and the modules (re-exported by `gripforge_kit_guest::motion`).

/// A rotation: `[x, y, z, w]`.
pub type Quat = [f64; 4];

/// No rotation.
pub const IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

/// The rotation of a cap alone: `yaw` radians about +Y.
#[inline]
pub fn quat_of_yaw(yaw: f64) -> Quat {
    let (s, c) = (yaw * 0.5).sin_cos();
    [0.0, s, 0.0, c]
}

/// The cap of a rotation: the yaw about +Y of where its local +Z points, projected on the ground plane (0 when it
/// points straight up or down).
#[inline]
pub fn yaw_of_quat(q: Quat) -> f64 {
    let [x, y, z, w] = q;
    (2.0 * (w * y + x * z)).atan2(1.0 - 2.0 * (y * y + x * x))
}

/// `q` turned by `d` radians about the world's +Y (`(0, sin d/2, 0, cos d/2) × q`): the tilt kept, the cap moved.
#[inline]
pub fn turn_about_up(q: Quat, d: f64) -> Quat {
    let (s, c) = (d * 0.5).sin_cos();
    [
        c * q[0] + s * q[2],
        c * q[1] + s * q[3],
        c * q[2] - s * q[0],
        c * q[3] - s * q[1],
    ]
}

/// `q` with the cap `yaw`, its tilt (pitch, roll) kept: what writing a cap means on an orientation that has one.
#[inline]
pub fn with_yaw(q: Quat, yaw: f64) -> Quat {
    turn_about_up(q, yaw - yaw_of_quat(q))
}

/// The angle, in radians, between the rotation's local +Y and the world's +Y: 0 upright, π upside down.
#[inline]
pub fn tilt_of(q: Quat) -> f64 {
    let [x, _, z, _] = q;
    // The Y component of the rotated +Y.
    let up_y = 1.0 - 2.0 * (x * x + z * z);
    up_y.clamp(-1.0, 1.0).acos()
}

/// The rotation `a` to `b` at `k` (0 `a`, 1 `b`) by the shortest arc, at a constant angular speed (the slerp: what the
/// render draws between two states, as Godot's physics interpolation). `k` is held within 0..1 (never extrapolated);
/// two rotations nearly the same are mixed straight then normalised.
#[inline]
pub fn slerp(a: Quat, b: Quat, k: f64) -> Quat {
    let k = if k.is_nan() { 1.0 } else { k.clamp(0.0, 1.0) };
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    // q and −q are the same rotation: the short way.
    let b = if dot < 0.0 {
        dot = -dot;
        [-b[0], -b[1], -b[2], -b[3]]
    } else {
        b
    };
    let (s0, s1) = if dot > 0.9995 {
        (1.0 - k, k)
    } else {
        let th = dot.min(1.0).acos();
        let s = th.sin();
        (((1.0 - k) * th).sin() / s, (k * th).sin() / s)
    };
    normalize([
        a[0] * s0 + b[0] * s1,
        a[1] * s0 + b[1] * s1,
        a[2] * s0 + b[2] * s1,
        a[3] * s0 + b[3] * s1,
    ])
}

/// `q` of length 1 (the identity for a null or not finite one).
#[inline]
pub fn normalize(q: Quat) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n.is_finite() && n > 1e-12 {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    } else {
        IDENTITY
    }
}

/// The angle, in radians, of the turn from `a` to `b` (0 the same rotation, `q` and `−q` alike).
#[inline]
pub fn angle_between(a: Quat, b: Quat) -> f64 {
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
    2.0 * dot.min(1.0).acos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn wrap(a: f64) -> f64 {
        (a + PI).rem_euclid(2.0 * PI) - PI
    }

    #[test]
    fn a_cap_written_is_the_cap_read_back() {
        for yaw in [0.0, 0.3, -1.2, 2.9, -PI + 1e-9, PI / 2.0] {
            let q = quat_of_yaw(yaw);
            assert!(wrap(yaw_of_quat(q) - yaw).abs() < 1e-12, "{yaw}");
            assert!(tilt_of(q) < 1e-12, "a cap alone stays upright");
        }
    }

    #[test]
    fn a_cap_written_on_a_tilted_rotation_keeps_its_tilt() {
        // Rolled 0.4 rad about +Z, then capped at 0.9.
        let (s, c) = (0.2f64).sin_cos();
        let rolled = [0.0, 0.0, s, c];
        let q = with_yaw(rolled, 0.9);
        assert!((yaw_of_quat(q) - 0.9).abs() < 1e-12);
        assert!((tilt_of(q) - 0.4).abs() < 1e-12);
        let q2 = turn_about_up(q, -0.2);
        assert!((yaw_of_quat(q2) - 0.7).abs() < 1e-12);
    }

    #[test]
    fn a_rolled_rotation_is_not_upright() {
        let (s, c) = (PI / 4.0).sin_cos();
        assert!((tilt_of([s, 0.0, 0.0, c]) - PI / 2.0).abs() < 1e-12);
        assert_eq!(tilt_of(IDENTITY), 0.0);
    }

    /// A rotation of `angle` about the unit `axis`.
    fn about(axis: [f64; 3], angle: f64) -> Quat {
        let (s, c) = (angle * 0.5).sin_cos();
        [axis[0] * s, axis[1] * s, axis[2] * s, c]
    }

    #[test]
    fn the_slerp_turns_at_a_constant_speed_about_any_axis() {
        let n = 1.0 / 3f64.sqrt();
        for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [n, n, n]] {
            let (a, b) = (about(axis, 0.2), about(axis, 1.4));
            for k in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let q = slerp(a, b, k);
                let want = about(axis, 0.2 + 1.2 * k);
                assert!(angle_between(q, want) < 1e-6, "{axis:?} at {k}");
            }
            // Never past either end.
            assert!(angle_between(slerp(a, b, 2.0), b) < 1e-6);
            assert!(angle_between(slerp(a, b, -1.0), a) < 1e-6);
        }
    }

    #[test]
    fn the_slerp_takes_the_short_way() {
        // 170° to −170° about X: 20° the short way, through 180°.
        let (a, b) = (
            about([1.0, 0.0, 0.0], 170f64.to_radians()),
            about([1.0, 0.0, 0.0], -170f64.to_radians()),
        );
        let mid = slerp(a, b, 0.5);
        assert!(angle_between(mid, about([1.0, 0.0, 0.0], PI)) < 1e-6);
        assert!((angle_between(a, mid) - 10f64.to_radians()).abs() < 1e-6);
    }
}
