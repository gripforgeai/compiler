// The body description an order carries (contract `abi.physics.body_desc`), read back on the serving side: the inverse of
// `desc_words` (both moved out of `physics_link.rs` unchanged: size ceiling of the file). Part of `physics_link.rs`.

use super::Read;

/// A body description as the order carries it (the words of `abi.physics.body_desc`; a mesh's addresses in `extra`).
pub(super) fn desc_of(o: &Read) -> crate::ports::BodyDesc {
    use crate::ports::abi::body_desc as bd;
    use crate::ports::{BodyDesc, BodyKind, Shape};
    // The words the order carries (contract `abi.physics.body_desc`): a field past them keeps its default (an order of
    // fewer words: the first layer, colliding with all, not a sensor).
    let w = o.f64s();
    let kind = match bd::kind(w) as u32 {
        1 => BodyKind::Kinematic,
        2 => BodyKind::Static,
        _ => BodyKind::Dynamic,
    };
    let s = bd::numbers(w);
    let code = bd::shape(w) as u32;
    // A shape with streams (a mesh, a hull, a compound, a heightfield, a triangle's points): read where they are.
    let shape = if ((3..=6).contains(&code) || code == 10) && o.extra.len() >= 4 {
        let (vp, nv, ip, ni) = (
            o.extra[0] as usize,
            o.extra[1] as usize,
            o.extra[2] as usize,
            o.extra[3] as usize,
        );
        // SAFETY: the user keeps the streams where these say until the physics took the order.
        unsafe {
            let f: &[f32] = if nv == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(vp as *const f32, nv)
            };
            let i: &[u32] = if ni == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(ip as *const u32, ni)
            };
            Shape::from_wire(code, s, f, i)
        }
    } else {
        // By its numbers alone: a box, a sphere, a capsule, a plane, a cylinder, a cone.
        let by_numbers = code <= 2 || (7..=9).contains(&code);
        Shape::from_wire(if by_numbers { code } else { 0 }, s, &[], &[])
    };
    let mut d = BodyDesc::new(kind, shape, bd::position(w));
    d.rotation = bd::rotation(w);
    d.mass = bd::mass(w);
    d.friction = bd::friction(w);
    d.restitution = bd::restitution(w);
    d.linear_damping = bd::linear_damping(w);
    d.angular_damping = bd::angular_damping(w);
    d.ccd = bd::ccd(w) != 0.0;
    d.can_sleep = bd::can_sleep(w) != 0.0;
    d.lock_rotation = bd::lock_rotation(w) != 0.0;
    d.lock_tilt = bd::lock_tilt(w) != 0.0;
    d.gravity_scale = bd::gravity_scale(w);
    d.ground_probe = bd::ground_probe(w);
    d.layers = bd::layers(w) as u32;
    d.collides_with = bd::collides_with(w) as u32;
    d.sensor = bd::sensor(w) != 0.0;
    d
}

/// The words of a body description for [`order::ADD_BODY`] (the inverse of what the physics reads; contract
/// `abi.physics.body_desc`, every index generated).
pub fn desc_words(d: &crate::ports::BodyDesc) -> [f64; crate::ports::abi::DESC_WORDS] {
    use crate::ports::abi::body_desc::at;
    use crate::ports::BodyKind;
    let b = |x: bool| x as u8 as f64;
    let mut w = [0.0; crate::ports::abi::DESC_WORDS];
    w[at::KIND] = match d.kind {
        BodyKind::Dynamic => 0.0,
        BodyKind::Kinematic => 1.0,
        BodyKind::Static => 2.0,
    };
    w[at::SHAPE] = d.shape.code() as f64;
    w[at::NUMBERS..at::NUMBERS + 3].copy_from_slice(&d.shape.numbers());
    w[at::POSITION..at::POSITION + 3].copy_from_slice(&d.position);
    w[at::ROTATION..at::ROTATION + 4].copy_from_slice(&d.rotation);
    w[at::MASS] = d.mass;
    w[at::FRICTION] = d.friction;
    w[at::RESTITUTION] = d.restitution;
    w[at::LINEAR_DAMPING] = d.linear_damping;
    w[at::ANGULAR_DAMPING] = d.angular_damping;
    w[at::CCD] = b(d.ccd);
    w[at::CAN_SLEEP] = b(d.can_sleep);
    w[at::LOCK_ROTATION] = b(d.lock_rotation);
    w[at::LOCK_TILT] = b(d.lock_tilt);
    w[at::GRAVITY_SCALE] = d.gravity_scale;
    w[at::GROUND_PROBE] = d.ground_probe;
    w[at::LAYERS] = d.layers as f64;
    w[at::COLLIDES_WITH] = d.collides_with as f64;
    w[at::SENSOR] = b(d.sensor);
    w
}
