//! The function numbers and the encoding of the port interfaces of the core (`kithost.physics@1`,
//! `kithost.input@2`, `kithost.assets@1`, `kithost.platform@1`), one file for the façade of the kit modules and
//! the core (which includes it with `#[path]`). The schema is in
//! kits/kitcall/interfaces/{physics,input,assets,platform}.gfi.
//!
//! Compound types (the `.gfi` comments name them): `v3` 3 × f64, `quat` 4 × f64, `opt<T>` a bool then `T` when
//! true, `shape` a u8 kind then its fields, `body_desc`, `body_state`, `ray_hit` as written below.

#![allow(dead_code)]

use crate::ports::{
    Answer, BodyDesc, BodyKind, BodyState, CharacterOptions, Contact, ContactEvent, JointDesc,
    Quat, QueryFilter, RayHit, Shape, JOINT_WORDS, V3,
};
use gripforge_abi::{Reader, Writer, ASSETS, INPUT, PHYSICS, PLATFORM, SCENE};

pub type R<T> = Result<T, String>;

/// `kithost.physics@1`: the methods of `PhysicsPort` (ports.rs), one function each.
pub mod physics_fn {
    use super::PHYSICS;
    const B: u32 = PHYSICS.base;
    pub const KIND: u32 = B | 1;
    pub const STEP: u32 = B | 2;
    pub const ADD_BODY: u32 = B | 3;
    pub const ADD_CHARACTER: u32 = B | 4;
    pub const REMOVE: u32 = B | 5;
    pub const HAS_BODY: u32 = B | 6;
    pub const HAS_CHARACTER: u32 = B | 7;
    pub const BODY: u32 = B | 8;
    pub const SET_BODY: u32 = B | 9;
    pub const SET_POSITION: u32 = B | 10;
    pub const MASS: u32 = B | 11;
    pub const GROUNDED: u32 = B | 12;
    pub const APPLY_IMPULSE: u32 = B | 13;
    pub const APPLY_FORCE: u32 = B | 14;
    pub const APPLY_TORQUE_IMPULSE: u32 = B | 15;
    pub const MOVE_KINEMATIC: u32 = B | 16;
    pub const MOVE_CHARACTER: u32 = B | 17;
    pub const CHARACTER_POSITION: u32 = B | 18;
    pub const SET_CHARACTER_POSITION: u32 = B | 19;
    pub const CHARACTER_GROUNDED: u32 = B | 20;
    pub const RAYCAST: u32 = B | 21;
    pub const OVERLAP_SPHERE: u32 = B | 22;
    pub const OVERLAP_BOX: u32 = B | 23;
    pub const CONTACT_EVENTS: u32 = B | 24;
    pub const CONTACTS: u32 = B | 25;
    pub const POSES: u32 = B | 26;
    pub const SET_MANUAL_STEPPING: u32 = B | 27;
    pub const MANUAL_STEPPING: u32 = B | 28;
    pub const SET_GRAVITY: u32 = B | 29;
    pub const SAVE_STATE: u32 = B | 30;
    pub const RESTORE_STATE: u32 = B | 31;
    pub const ADD_STATIC_BOX: u32 = B | 32;
    pub const ADD_DYNAMIC_BOX: u32 = B | 33;
    // B | 34 was `navigation_geometry` (retired with ABI 2.6: the shape table is read in place, `SHAPES_VIEW`).
    pub const GROUND_CLEARANCE: u32 = B | 35;
    pub const CAST_CHARACTER: u32 = B | 36;
    // 1.4: questions with a ticket, answers read without waiting (docs/kit-modules.md §7.17).
    pub const ASK_MOVE_CHARACTER: u32 = B | 37;
    pub const ASK_CAST_CHARACTER: u32 = B | 38;
    pub const ASK_RAYCAST: u32 = B | 39;
    pub const ASK_OVERLAP_SPHERE: u32 = B | 40;
    pub const ASK_OVERLAP_BOX: u32 = B | 41;
    pub const POLL: u32 = B | 42;
    pub const FORGET: u32 = B | 43;
    pub const COUNTERS: u32 = B | 44;
    pub const SET_COLLISION_SPACE: u32 = B | 45;
    /// Field-level writes (partial commands, no stale read-modify-write).
    pub const SET_LINVEL: u32 = B | 46;
    pub const SET_ANGVEL: u32 = B | 47;
    pub const SET_ROTATION: u32 = B | 48;
    // 1.8: the questions of the layers (a filter: mask u32, exclude opt<string>).
    pub const ASK_RAYCAST_ALL: u32 = B | 49;
    pub const ASK_SHAPE_CAST: u32 = B | 50;
    pub const ASK_OVERLAP_SHAPE: u32 = B | 51;
    // The offset of a character's frame, changed live (no physics order).
    pub const SET_CHARACTER_OFFSET: u32 = B | 52;
    // 1.9: the joints (id, a, b, joint) and letting one go (id).
    pub const ADD_JOINT: u32 = B | 53;
    pub const REMOVE_JOINT: u32 = B | 54;
    // 1.10: the address of the physics' shape table (u64, 0: none), read in place by the kit.
    pub const SHAPES_VIEW: u32 = B | 55;
}

/// `kithost.input@2`: the neutral transport of the devices (the core holds no table of actions: core.input does).
pub mod input_fn {
    use super::INPUT;
    const B: u32 = INPUT.base;
    /// () → list<(kind u8, index u32, code string, x f64, y f64, link u8)>: the raw device records since the last
    /// take (core.input, once per tick).
    pub const RAW_TAKE: u32 = B | 25;
    /// (kind u8, index u32, code string, x f64, y f64, link u8) → (): one record from a kit that is a device (an
    /// on-screen pad).
    pub const PUSH_RAW: u32 = B | 26;
}

/// `kithost.assets@1`: `AssetPort`.
pub mod assets_fn {
    use super::ASSETS;
    const B: u32 = ASSETS.base;
    pub const URL: u32 = B | 1;
    pub const BOUND: u32 = B | 2;
    pub const VARIANTS: u32 = B | 3;
    pub const BYTES: u32 = B | 4;
}

/// `kithost.platform@1`: the machine (platform.rs): clock, language, target, save stores, audio, display, pointer.
pub mod platform_fn {
    use super::PLATFORM;
    const B: u32 = PLATFORM.base;
    pub const INFO: u32 = B | 1;
    pub const NOW: u32 = B | 2;
    pub const TZ_OFFSET: u32 = B | 3;
    pub const STORE_HAS: u32 = B | 16;
    pub const STORE_KIND: u32 = B | 17;
    pub const STORE_GET: u32 = B | 18;
    pub const STORE_SET: u32 = B | 19;
    pub const STORE_REMOVE: u32 = B | 20;
    pub const STORE_KEYS: u32 = B | 21;
    pub const AUDIO_START: u32 = B | 32;
    pub const AUDIO_UPDATE: u32 = B | 33;
    pub const AUDIO_STOP: u32 = B | 34;
    pub const AUDIO_BUS: u32 = B | 35;
    pub const AUDIO_RECORDED: u32 = B | 36;
    pub const AUDIO_ACOUSTICS: u32 = B | 37;
    pub const DISPLAY_AVAILABLE: u32 = B | 48;
    pub const DISPLAY_NEEDS_GESTURE: u32 = B | 49;
    pub const DISPLAY_REQUEST: u32 = B | 50;
    pub const DISPLAY_EXIT: u32 = B | 51;
    pub const DISPLAY_RECORDED: u32 = B | 52;
    pub const DISPLAY_DRAIN: u32 = B | 53;
    pub const POINTER_AVAILABLE: u32 = B | 64;
    pub const POINTER_NEEDS_GESTURE: u32 = B | 65;
    pub const POINTER_LOCK: u32 = B | 66;
    pub const POINTER_UNLOCK: u32 = B | 67;
    pub const POINTER_RECORDED: u32 = B | 68;
    pub const POINTER_DRAIN: u32 = B | 69;
}

/// `kithost.scene@1`: `ScenePort` (the engine's tree, core/abi/interfaces/scene.gfi).
pub mod scene_fn {
    use super::SCENE;
    const B: u32 = SCENE.base;
    pub const ENGINE: u32 = B | 1;
    pub const ROOT: u32 = B | 2;
    pub const FIND: u32 = B | 3;
    pub const TAGGED: u32 = B | 4;
    pub const TRANSFORM: u32 = B | 5;
    pub const SET_TRANSFORM: u32 = B | 6;
    pub const VISIBLE: u32 = B | 7;
    pub const SET_VISIBLE: u32 = B | 8;
    pub const CREATE: u32 = B | 9;
    pub const DESTROY: u32 = B | 10;
    pub const ADOPT: u32 = B | 11;
    pub const PARENT: u32 = B | 12;
    pub const WATCH_POSE: u32 = B | 13;
    pub const UNWATCH_POSE: u32 = B | 14;
    pub const CLAIM_RENDER_YAW: u32 = B | 15;
    pub const RELEASE_RENDER_YAW: u32 = B | 16;
    pub const ENTITY_NODE: u32 = B | 17;
    pub const SET_NODE_TURN: u32 = B | 18;
}

/// The save store a `StoreHandle` of a kit module names.
pub const STORE_LOCAL: u8 = 0;
pub const STORE_CLOUD: u8 = 1;

// ---- compound values ---------------------------------------------------------------------------------------------

pub fn w_v3(w: &mut Writer, v: V3) {
    w.f64(v[0]).f64(v[1]).f64(v[2]);
}
pub fn r_v3(r: &mut Reader) -> R<V3> {
    Ok([r.f64()?, r.f64()?, r.f64()?])
}
pub fn w_quat(w: &mut Writer, q: Quat) {
    w.f64(q[0]).f64(q[1]).f64(q[2]).f64(q[3]);
}
pub fn r_quat(r: &mut Reader) -> R<Quat> {
    Ok([r.f64()?, r.f64()?, r.f64()?, r.f64()?])
}
pub fn w_opt_v3(w: &mut Writer, v: Option<V3>) {
    w.bool(v.is_some());
    if let Some(v) = v {
        w_v3(w, v);
    }
}
pub fn r_opt_v3(r: &mut Reader) -> R<Option<V3>> {
    Ok(if r.bool()? { Some(r_v3(r)?) } else { None })
}
pub fn w_opt_quat(w: &mut Writer, q: Option<Quat>) {
    w.bool(q.is_some());
    if let Some(q) = q {
        w_quat(w, q);
    }
}
pub fn r_opt_quat(r: &mut Reader) -> R<Option<Quat>> {
    Ok(if r.bool()? { Some(r_quat(r)?) } else { None })
}
pub fn w_opt_strs(w: &mut Writer, v: Option<&[String]>) {
    w.bool(v.is_some());
    if let Some(v) = v {
        w.strs(v);
    }
}
pub fn r_opt_strs(r: &mut Reader) -> R<Option<Vec<String>>> {
    Ok(if r.bool()? { Some(r.strs()?) } else { None })
}
pub fn r_opt_string(r: &mut Reader) -> R<Option<String>> {
    Ok(r.opt_str()?.map(str::to_string))
}

pub fn w_kind(w: &mut Writer, k: BodyKind) {
    w.u8(match k {
        BodyKind::Dynamic => 0,
        BodyKind::Kinematic => 1,
        BodyKind::Static => 2,
    });
}
pub fn r_kind(r: &mut Reader) -> R<BodyKind> {
    Ok(match r.u8()? {
        0 => BodyKind::Dynamic,
        1 => BodyKind::Kinematic,
        2 => BodyKind::Static,
        k => return Err(format!("kitcall: unknown body kind {k}")),
    })
}

pub fn w_shape(w: &mut Writer, s: &Shape) {
    match s {
        Shape::Box { size } => {
            w.u8(0);
            w_v3(w, *size);
        }
        Shape::Sphere { radius } => {
            w.u8(1).f64(*radius);
        }
        Shape::Capsule { radius, height } => {
            w.u8(2).f64(*radius).f64(*height);
        }
        Shape::Mesh { .. } | Shape::Convex { .. } | Shape::Compound { .. } => {
            let (vertices, indices) = s.streams().unwrap_or((&[], &[]));
            w.u8(s.code() as u8).u32(vertices.len() as u32);
            for v in vertices {
                w.f32(*v);
            }
            w.u32(indices.len() as u32);
            for i in indices {
                w.u32(*i);
            }
        }
        Shape::HeightField {
            cols,
            cell,
            heights,
        } => {
            w.u8(6).u32(*cols).f64(cell[0]).f64(cell[1]);
            w.u32(heights.len() as u32);
            for h in heights {
                w.f32(*h);
            }
        }
        Shape::Plane { normal } => {
            w.u8(7);
            w_v3(w, *normal);
        }
    }
}
pub fn r_shape(r: &mut Reader) -> R<Shape> {
    Ok(match r.u8()? {
        0 => Shape::Box { size: r_v3(r)? },
        1 => Shape::Sphere { radius: r.f64()? },
        2 => Shape::Capsule {
            radius: r.f64()?,
            height: r.f64()?,
        },
        code @ 3..=5 => {
            let n = r.u32()? as usize;
            let mut vertices = Vec::with_capacity(n.min(1 << 20));
            for _ in 0..n {
                vertices.push(r.f32()?);
            }
            let n = r.u32()? as usize;
            let mut indices = Vec::with_capacity(n.min(1 << 20));
            for _ in 0..n {
                indices.push(r.u32()?);
            }
            match code {
                3 => Shape::Mesh { vertices, indices },
                4 => Shape::Convex { vertices, indices },
                _ => Shape::Compound {
                    floats: vertices,
                    indices,
                },
            }
        }
        6 => {
            let (cols, cell) = (r.u32()?, [r.f64()?, r.f64()?]);
            let n = r.u32()? as usize;
            let mut heights = Vec::with_capacity(n.min(1 << 20));
            for _ in 0..n {
                heights.push(r.f32()?);
            }
            Shape::HeightField {
                cols,
                cell,
                heights,
            }
        }
        7 => Shape::Plane { normal: r_v3(r)? },
        k => return Err(format!("kitcall: unknown shape {k}")),
    })
}

/// A joint: its [`JointDesc::words`] (17 f64).
pub fn w_joint(w: &mut Writer, j: &JointDesc) {
    for x in j.words() {
        w.f64(x);
    }
}
pub fn r_joint(r: &mut Reader) -> R<JointDesc> {
    let mut words = [0.0; JOINT_WORDS];
    for x in &mut words {
        *x = r.f64()?;
    }
    JointDesc::of_words(&words).ok_or_else(|| "kitcall: unknown joint".into())
}

pub fn w_desc(w: &mut Writer, d: &BodyDesc) {
    w_kind(w, d.kind);
    w_shape(w, &d.shape);
    w_v3(w, d.position);
    w_quat(w, d.rotation);
    w.f64(d.mass)
        .f64(d.friction)
        .f64(d.restitution)
        .f64(d.linear_damping)
        .f64(d.angular_damping);
    w.bool(d.ccd)
        .bool(d.can_sleep)
        .bool(d.lock_rotation)
        .bool(d.lock_tilt);
    w.f64(d.gravity_scale).f64(d.ground_probe);
    // 1.8: the layers, last (a description of an older module ends before them).
    w.u32(d.layers).u32(d.collides_with);
    // Whether it is a sensor, last again (a description that ends before it is not one).
    w.bool(d.sensor);
}
pub fn r_desc(r: &mut Reader) -> R<BodyDesc> {
    let kind = r_kind(r)?;
    let shape = r_shape(r)?;
    let position = r_v3(r)?;
    let mut d = BodyDesc::new(kind, shape, position);
    d.rotation = r_quat(r)?;
    d.mass = r.f64()?;
    d.friction = r.f64()?;
    d.restitution = r.f64()?;
    d.linear_damping = r.f64()?;
    d.angular_damping = r.f64()?;
    d.ccd = r.bool()?;
    d.can_sleep = r.bool()?;
    d.lock_rotation = r.bool()?;
    d.lock_tilt = r.bool()?;
    d.gravity_scale = r.f64()?;
    d.ground_probe = r.f64()?;
    if !r.done() {
        d.layers = r.u32()?;
        d.collides_with = r.u32()?;
    }
    if !r.done() {
        d.sensor = r.bool()?;
    }
    Ok(d)
}

pub fn w_state(w: &mut Writer, s: &BodyState) {
    w.str(&s.entity);
    w_v3(w, s.p);
    w_quat(w, s.q);
    w_v3(w, s.v);
    w_v3(w, s.w);
    w.bool(s.sleeping);
}
pub fn r_state(r: &mut Reader) -> R<BodyState> {
    Ok(BodyState {
        entity: r.string()?,
        p: r_v3(r)?,
        q: r_quat(r)?,
        v: r_v3(r)?,
        w: r_v3(r)?,
        sleeping: r.bool()?,
    })
}
pub fn w_states(w: &mut Writer, list: &[BodyState]) {
    w.u32(list.len() as u32);
    for s in list {
        w_state(w, s);
    }
}
pub fn r_states(r: &mut Reader) -> R<Vec<BodyState>> {
    let n = r.u32()? as usize;
    (0..n).map(|_| r_state(r)).collect()
}

pub fn w_opts(w: &mut Writer, o: CharacterOptions) {
    w.f64(o.max_slope_deg)
        .f64(o.step_height)
        .f64(o.snap_distance)
        .f64(o.skin_ratio);
    // 1.8: the frame, last (the options of an older module end before it).
    // A former word of the frame (a preset), never set: kept so the wire of 1.8 is unchanged.
    w.bool(false);
    w.bool(o.origin_offset.is_some());
    if let Some(v) = o.origin_offset {
        w_v3(w, v);
    }
}
pub fn r_opts(r: &mut Reader) -> R<CharacterOptions> {
    let (max_slope_deg, step_height, snap_distance, skin_ratio) =
        (r.f64()?, r.f64()?, r.f64()?, r.f64()?);
    // The former preset word (see `w_opts`): read and dropped.
    if !r.done() && r.bool()? {
        r.bool()?;
    }
    let origin_offset = if !r.done() && r.bool()? {
        Some(r_v3(r)?)
    } else {
        None
    };
    Ok(CharacterOptions {
        max_slope_deg,
        step_height,
        snap_distance,
        skin_ratio,
        origin_offset,
    })
}

pub fn w_hit(w: &mut Writer, h: &RayHit) {
    w.opt_str(h.entity.as_deref()).f64(h.toi);
    w_v3(w, h.point);
    w_v3(w, h.normal);
}
pub fn r_hit(r: &mut Reader) -> R<RayHit> {
    Ok(RayHit {
        entity: r_opt_string(r)?,
        toi: r.f64()?,
        point: r_v3(r)?,
        normal: r_v3(r)?,
    })
}

pub fn w_contact_events(w: &mut Writer, list: &[ContactEvent]) {
    w.u32(list.len() as u32);
    for e in list {
        w.bool(e.begin).str(&e.a).str(&e.b).f64(e.impulse);
    }
}
pub fn r_contact_events(r: &mut Reader) -> R<Vec<ContactEvent>> {
    let n = r.u32()? as usize;
    (0..n)
        .map(|_| {
            Ok(ContactEvent {
                begin: r.bool()?,
                a: r.string()?,
                b: r.string()?,
                impulse: r.f64()?,
            })
        })
        .collect()
}
pub fn w_contacts(w: &mut Writer, list: &[Contact]) {
    w.u32(list.len() as u32);
    for c in list {
        w.str(&c.a).str(&c.b).f64(c.speed);
    }
}
pub fn r_contacts(r: &mut Reader) -> R<Vec<Contact>> {
    let n = r.u32()? as usize;
    (0..n)
        .map(|_| {
            Ok(Contact {
                a: r.string()?,
                b: r.string()?,
                speed: r.f64()?,
            })
        })
        .collect()
}
pub fn w_poses(w: &mut Writer, list: &[(String, V3, f64)]) {
    w.u32(list.len() as u32);
    for (id, p, yaw) in list {
        w.str(id);
        w_v3(w, *p);
        w.f64(*yaw);
    }
}
pub fn r_poses(r: &mut Reader) -> R<Vec<(String, V3, f64)>> {
    let n = r.u32()? as usize;
    (0..n)
        .map(|_| Ok((r.string()?, r_v3(r)?, r.f64()?)))
        .collect()
}

/// `answer` (1.4): the answer of a ticket.
pub fn w_answer(w: &mut Writer, a: &Answer) {
    match a {
        Answer::Pending => {
            w.u8(0);
        }
        Answer::Moved(m) => {
            w.u8(1).bool(m.is_some());
            if let Some((d, g)) = m {
                w_v3(w, *d);
                w.bool(*g);
            }
        }
        Answer::Hit(h) => {
            w.u8(2).bool(h.is_some());
            if let Some(h) = h {
                w_hit(w, h);
            }
        }
        Answer::Found(f) => {
            w.u8(3).strs(f);
        }
        Answer::Unknown => {
            w.u8(4);
        }
        Answer::Hits(h) => {
            w.u8(5).u32(h.len() as u32);
            for h in h {
                w_hit(w, h);
            }
        }
    }
}

/// A shape at a pose (1.8: the shape cast and overlap of the layers).
pub fn w_shaped(w: &mut Writer, shape: &Shape, position: V3, rotation: Quat) {
    w_shape(w, shape);
    w_v3(w, position);
    w_quat(w, rotation);
}

/// A question's filter (1.8): the layers' mask, the entity left out.
pub fn w_filter(w: &mut Writer, f: &QueryFilter) {
    w.u32(f.mask).opt_str(f.exclude.as_deref());
}
pub fn r_filter(r: &mut Reader) -> R<QueryFilter> {
    Ok(QueryFilter {
        mask: r.u32()?,
        exclude: r.opt_str()?.map(str::to_string),
    })
}
pub fn r_answer(r: &mut Reader) -> R<Answer> {
    Ok(match r.u8()? {
        0 => Answer::Pending,
        1 => Answer::Moved(if r.bool()? {
            Some((r_v3(r)?, r.bool()?))
        } else {
            None
        }),
        2 => Answer::Hit(if r.bool()? { Some(r_hit(r)?) } else { None }),
        3 => Answer::Found(r.strs()?),
        5 => {
            let n = r.u32()? as usize;
            Answer::Hits((0..n).map(|_| r_hit(r)).collect::<R<Vec<_>>>()?)
        }
        _ => Answer::Unknown,
    })
}
