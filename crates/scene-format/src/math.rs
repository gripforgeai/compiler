//! The transforms of the nodes: local position, rotation (Euler angles in degrees, applied Y then X then Z: yaw, pitch,
//! roll) and scale, composed down the tree. Kits' space: metres, right-handed, +Y up. Quaternions are `[x, y, z, w]`,
//! as game.engine writes them (`[0, sin(yaw / 2), 0, cos(yaw / 2)]`).

/// A resolved transform: where a node is in its scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Position in metres.
    pub position: [f64; 3],
    /// Rotation, a unit quaternion `[x, y, z, w]`.
    pub rotation: [f64; 4],
    /// Scale per axis.
    pub scale: [f64; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

fn axis(a: [f64; 3], deg: f64) -> [f64; 4] {
    let h = deg.to_radians() / 2.0;
    let s = h.sin();
    [a[0] * s, a[1] * s, a[2] * s, h.cos()]
}

/// The product `a * b` (b applied first).
pub fn mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// A quaternion written as an object `{ "x", "y", "z", "w" }` (the form of the editor and of the site's commands),
/// normalised; `None` when a component is missing or it has no length.
pub fn quat_of(v: &serde_json::Value) -> Option<[f64; 4]> {
    let c = |k: &str| v.get(k)?.as_f64();
    let q = [c("x")?, c("y")?, c("z")?, c("w")?];
    let len = q.iter().map(|x| x * x).sum::<f64>().sqrt();
    (len > 1e-12).then(|| q.map(|x| x / len))
}

/// The quaternion of Euler angles in degrees `[x, y, z]`, applied Y, then X, then Z.
pub fn from_euler_deg(e: [f64; 3]) -> [f64; 4] {
    mul(
        mul(axis([0.0, 1.0, 0.0], e[1]), axis([1.0, 0.0, 0.0], e[0])),
        axis([0.0, 0.0, 1.0], e[2]),
    )
}

/// Rotate a vector by a unit quaternion.
pub fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let p = mul(mul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [p[0], p[1], p[2]]
}

impl Transform {
    /// A local transform from the node's fields.
    pub fn local(position: [f64; 3], rotation_deg: [f64; 3], scale: [f64; 3]) -> Self {
        Self {
            position,
            rotation: from_euler_deg(rotation_deg),
            scale,
        }
    }

    /// `self` (the parent) followed by `child`: where the child is in the parent's space.
    pub fn then(&self, child: &Transform) -> Transform {
        let scaled = [
            child.position[0] * self.scale[0],
            child.position[1] * self.scale[1],
            child.position[2] * self.scale[2],
        ];
        let r = rotate(self.rotation, scaled);
        Transform {
            position: [
                self.position[0] + r[0],
                self.position[1] + r[1],
                self.position[2] + r[2],
            ],
            rotation: mul(self.rotation, child.rotation),
            scale: [
                self.scale[0] * child.scale[0],
                self.scale[1] * child.scale[1],
                self.scale[2] * child.scale[2],
            ],
        }
    }

    /// The yaw in radians when the rotation is around Y only, else `None` (game.engine keeps a yaw only).
    pub fn yaw_only(&self) -> Option<f64> {
        let [x, y, z, w] = self.rotation;
        if x.abs() > 1e-9 || z.abs() > 1e-9 {
            return None;
        }
        Some(2.0 * y.atan2(w))
    }

    /// Whether the scale is 1 on every axis.
    pub fn unscaled(&self) -> bool {
        self.scale.iter().all(|s| (s - 1.0).abs() < 1e-12)
    }
}

/// The Euler angles in degrees `[x, y, z]` (applied Y, X, Z) of a unit quaternion: the inverse of [`from_euler_deg`].
pub fn to_euler_deg(q: [f64; 4]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let m12 = 2.0 * (y * z - w * x);
    let pitch = (-m12).clamp(-1.0, 1.0).asin();
    let yaw = (2.0 * (x * z + w * y)).atan2(1.0 - 2.0 * (x * x + y * y));
    let roll = (2.0 * (x * y + w * z)).atan2(1.0 - 2.0 * (x * x + z * z));
    [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()]
}

impl Transform {
    /// The transform that, followed by `self`'s parent (`self`), gives `world`: `world` expressed in `self`'s space.
    pub fn local_of(&self, world: &Transform) -> Transform {
        let inv = [
            -self.rotation[0],
            -self.rotation[1],
            -self.rotation[2],
            self.rotation[3],
        ];
        let d = [
            world.position[0] - self.position[0],
            world.position[1] - self.position[1],
            world.position[2] - self.position[2],
        ];
        let r = rotate(inv, d);
        let div = |a: f64, b: f64| if b.abs() < 1e-12 { a } else { a / b };
        Transform {
            position: [
                div(r[0], self.scale[0]),
                div(r[1], self.scale[1]),
                div(r[2], self.scale[2]),
            ],
            rotation: mul(inv, world.rotation),
            scale: [
                div(world.scale[0], self.scale[0]),
                div(world.scale[1], self.scale[1]),
                div(world.scale[2], self.scale[2]),
            ],
        }
    }
}
