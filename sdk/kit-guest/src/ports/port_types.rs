//! The pure types and traits of the ports (src/ports.rs): physics (bodies, shapes, the `PhysicsPort` trait, the
//! navigation records) and the `AssetPort` trait. One file for the kit logic modules and the core (which
//! includes it with `#[path]`), so a kit sees the same types on both sides.

use super::{CharacterOptions, Shape};
use serde_json::{json, Value as Json};

pub type V3 = [f64; 3];
pub type Quat = [f64; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyKind {
    Dynamic,
    Kinematic,
    Static,
}

/// `PhysicsBodyDesc`, with its defaults (friction 0.5 combined with MIN, restitution 0 combined with MAX).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyDesc {
    pub kind: BodyKind,
    pub shape: Shape,
    pub position: V3,
    pub rotation: Quat,
    pub mass: f64,
    pub friction: f64,
    pub restitution: f64,
    pub linear_damping: f64,
    pub angular_damping: f64,
    pub ccd: bool,
    pub can_sleep: bool,
    pub lock_rotation: bool,
    /// Only rotation about +Y (`DynamicBoxOptions.lockTilt`).
    pub lock_tilt: bool,
    pub gravity_scale: f64,
    /// Downward reach of `grounded()` beyond the half height of the shape (`DynamicBoxOptions.groundProbe`,
    /// 0.1 by default). The null port does not read it: its bodies are grounded by their height over the floor.
    pub ground_probe: f64,
    /// The collision layers the body is on (bits of core.physics's named layers; 1, the first layer, by default).
    pub layers: u32,
    /// The layers it collides with (all by default). Two bodies touch when each is on a layer the other takes.
    pub collides_with: u32,
    /// A sensor (a trigger volume): it pushes nothing and is pushed by nothing, it only tells who enters and leaves
    /// (core.physics's events `core.physics.sensor.enter` / `.exit`). False by default.
    pub sensor: bool,
}

impl BodyDesc {
    pub fn new(kind: BodyKind, shape: Shape, position: V3) -> BodyDesc {
        BodyDesc {
            kind,
            shape,
            position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            mass: 1.0,
            friction: 0.5,
            restitution: 0.0,
            linear_damping: 0.0,
            angular_damping: 0.0,
            ccd: false,
            can_sleep: true,
            lock_rotation: false,
            lock_tilt: false,
            gravity_scale: 1.0,
            ground_probe: 0.1,
            layers: 1,
            collides_with: u32::MAX,
            sensor: false,
        }
    }
}

/// The filter of a question (brique 2): the layers it sees (`mask`, the bits of core.physics's named layers; every
/// layer by default) and one entity left out.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryFilter {
    pub mask: u32,
    pub exclude: Option<String>,
}

impl Default for QueryFilter {
    fn default() -> Self {
        QueryFilter {
            mask: u32::MAX,
            exclude: None,
        }
    }
}

impl QueryFilter {
    /// Whether a body on `layers` is seen.
    pub fn sees(&self, layers: u32) -> bool {
        self.mask & layers != 0
    }
}

/// `PhysicsBodyState`: `w` is the angular velocity (rad/s, world axes).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyState {
    pub entity: String,
    pub p: V3,
    pub q: Quat,
    pub v: V3,
    pub w: V3,
    pub sleeping: bool,
}

impl BodyState {
    pub fn to_json(&self) -> Json {
        json!({ "entity": self.entity, "p": self.p, "q": self.q, "v": self.v, "w": self.w, "sleeping": self.sleeping })
    }
    /// The yaw about +Y of the orientation (what `transform.yaw` mirrors).
    pub fn yaw(&self) -> f64 {
        let q = self.q;
        (2.0 * (q[3] * q[1] + q[0] * q[2])).atan2(1.0 - 2.0 * (q[1] * q[1] + q[2] * q[2]))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RayHit {
    pub entity: Option<String>,
    pub toi: f64,
    pub point: V3,
    pub normal: V3,
}

/// `PhysicsContactEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactEvent {
    pub begin: bool,
    pub a: String,
    pub b: String,
    pub impulse: f64,
}

/// A pair of `PhysicsPort.contacts()`: a collision pair of the last step and the speed of the pair.
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub a: String,
    pub b: String,
    pub speed: f64,
}

/// The physics a kit reaches through `Kx::physics()`.
#[allow(unused_variables)]
pub trait PhysicsPort {
    /// `null` or `rapier`.
    fn kind(&self) -> &'static str;
    fn step(&mut self, dt: f64);
    /// Any body kind with any shape; replaces the body the entity already had.
    fn add_body(&mut self, entity: &str, desc: &BodyDesc);
    /// Capsule character; `position` is the capsule centre, `height` the total height.
    fn add_character(
        &mut self,
        entity: &str,
        position: V3,
        radius: f64,
        height: f64,
        opts: CharacterOptions,
    );
    /// Scope kinematic queries to an explicit zone. Unscoped legacy bodies keep their behavior.
    /// Returns false when unsupported. Assign after creating the body; removal forgets it.
    fn set_collision_space(&mut self, entity: &str, space: &str) -> bool {
        false
    }
    fn remove(&mut self, entity: &str);
    fn has_body(&self, entity: &str) -> bool {
        self.body(entity).is_some()
    }
    fn has_character(&self, entity: &str) -> bool {
        false
    }
    /// Pose and velocities of a body (`RigidBodyHandle.state()`).
    fn body(&self, entity: &str) -> Option<BodyState>;
    /// Teleport, velocities and sleep flag (`RigidBodyHandle.setState`).
    fn set_body(&mut self, state: &BodyState);
    /// Teleports a body, its orientation and velocities kept (`RigidBodyHandle.setPosition`).
    fn set_position(&mut self, entity: &str, p: V3) {
        if let Some(mut state) = self.body(entity) {
            state.p = p;
            self.set_body(&state);
        }
    }
    /// Sets the linear velocity only: pose, spin and sleep flag kept. A field-level write: the port (which holds the
    /// freshest state) changes this one field, so it never carries a stale copy of the others.
    fn set_linvel(&mut self, entity: &str, v: V3) {
        if let Some(mut state) = self.body(entity) {
            state.v = v;
            state.sleeping = false;
            self.set_body(&state);
        }
    }
    /// Sets the angular velocity only (see [`PhysicsPort::set_linvel`]).
    fn set_angvel(&mut self, entity: &str, w: V3) {
        if let Some(mut state) = self.body(entity) {
            state.w = w;
            state.sleeping = false;
            self.set_body(&state);
        }
    }
    /// Sets the orientation only: position and velocities kept (see [`PhysicsPort::set_linvel`]).
    fn set_rotation(&mut self, entity: &str, q: Quat) {
        if let Some(mut state) = self.body(entity) {
            state.q = q;
            state.sleeping = false;
            self.set_body(&state);
        }
    }
    fn mass(&self, entity: &str) -> f64 {
        1.0
    }
    /// `RigidBodyHandle.grounded()`: the body rests on something. A port that cannot tell answers `false`.
    fn grounded(&self, entity: &str) -> bool {
        false
    }
    fn apply_impulse(&mut self, entity: &str, impulse: V3) {}
    /// Force (N) for the next step.
    fn apply_force(&mut self, entity: &str, force: V3) {}
    fn apply_torque_impulse(&mut self, entity: &str, torque: V3) {}
    /// Kinematic follow: reach this pose at the end of the next step.
    fn move_kinematic(&mut self, entity: &str, p: V3, q: Option<Quat>) {}
    /// Moves a character by `delta` with collisions, slopes and steps resolved: the displacement and grounded.
    fn move_character(&mut self, entity: &str, delta: V3) -> Option<(V3, bool)> {
        None
    }
    /// Query the character's real shape from an explicit centre, without moving any body,
    /// advancing time or applying impulses. No autostep or ground snap. Returns the
    /// collision-resolved displacement and grounded, or None if unsupported/missing.
    /// Used to validate every segment of a rail before committing its final pose.
    fn cast_character(&self, entity: &str, origin: V3, delta: V3) -> Option<(V3, bool)> {
        None
    }
    fn character_position(&self, entity: &str) -> Option<V3> {
        None
    }
    fn set_character_position(&mut self, entity: &str, p: V3) {}
    fn character_grounded(&self, entity: &str) -> bool {
        false
    }
    fn raycast(&self, origin: V3, dir: V3, max_toi: f64, exclude: Option<&str>) -> Option<RayHit> {
        None
    }
    /// Entities whose colliders overlap a sphere (sorted, unique).
    fn overlap_sphere(&self, center: V3, radius: f64, exclude: Option<&str>) -> Vec<String> {
        Vec::new()
    }
    /// Entities whose colliders overlap a box of full extents `size` (sorted, unique).
    fn overlap_box(
        &self,
        center: V3,
        size: V3,
        rotation: Option<Quat>,
        exclude: Option<&str>,
    ) -> Vec<String> {
        Vec::new()
    }
    /// Every hit of a ray, nearest first, at most `max_hits` (brique 2, filtered by layer). A port without it: its
    /// first hit, the layers not told apart.
    fn raycast_all(
        &self,
        origin: V3,
        dir: V3,
        max_toi: f64,
        filter: &QueryFilter,
        max_hits: usize,
    ) -> Vec<RayHit> {
        if max_hits == 0 {
            return Vec::new();
        }
        self.raycast(origin, dir, max_toi, filter.exclude.as_deref())
            .into_iter()
            .collect()
    }
    /// The first hit of a simple shape (box, sphere, capsule) moved from `position` along `dir` up to `max_toi`
    /// (`toi` the distance, `point` and `normal` of the contact). `None`: nothing, or a port without the cast.
    fn shape_cast(
        &self,
        shape: &Shape,
        position: V3,
        rotation: Quat,
        dir: V3,
        max_toi: f64,
        filter: &QueryFilter,
    ) -> Option<RayHit> {
        None
    }
    /// Entities whose colliders overlap a simple shape (sorted, unique). A port without it: the sphere or the box,
    /// a capsule by its box, the layers not told apart.
    fn overlap_shape(
        &self,
        shape: &Shape,
        position: V3,
        rotation: Quat,
        filter: &QueryFilter,
    ) -> Vec<String> {
        let ex = filter.exclude.as_deref();
        match shape {
            Shape::Sphere { radius } => self.overlap_sphere(position, *radius, ex),
            Shape::Box { size } => self.overlap_box(position, *size, Some(rotation), ex),
            Shape::Capsule { radius, height } => self.overlap_box(
                position,
                [2.0 * radius, *height, 2.0 * radius],
                Some(rotation),
                ex,
            ),
            _ => Vec::new(),
        }
    }
    /// Contact begin / end events of the last step.
    fn contact_events(&self) -> Vec<ContactEvent> {
        Vec::new()
    }
    /// Every event of [`Self::contact_events`] in turn (`a`, `b`, `begin`, `impulse`), read where the port keeps
    /// it: nothing made per step (a port that keeps its events in place gives them where they are).
    fn for_each_contact_event(&self, f: &mut dyn FnMut(&str, &str, bool, f64)) {
        for e in self.contact_events() {
            f(&e.a, &e.b, e.begin, e.impulse);
        }
    }
    /// The linear velocity of a body, its state not built (no name copied).
    fn body_velocity(&self, entity: &str) -> Option<V3> {
        self.body(entity).map(|state| state.v)
    }
    /// Every pair of [`Self::contacts`] in turn (`a`, `b`, `speed`). A port that keeps its events in place and
    /// the default `contacts` gives them with [`contacts_in_place`], nothing made per step.
    fn for_each_contact(&self, f: &mut dyn FnMut(&str, &str, f64)) {
        for c in self.contacts() {
            f(&c.a, &c.b, c.speed);
        }
    }
    /// `contacts()`: the collision pairs of the last step with their speed. By default the pairs whose contact
    /// began, with the relative speed of the two bodies after the step (what the web Rapier port reports).
    fn contacts(&self) -> Vec<Contact> {
        let linvel = |entity: &str| self.body(entity).map_or([0.0; 3], |state| state.v);
        self.contact_events()
            .into_iter()
            .filter(|e| e.begin)
            .map(|e| {
                let (va, vb) = (linvel(&e.a), linvel(&e.b));
                let speed = null_hypot(&[va[0] - vb[0], va[1] - vb[1], va[2] - vb[2]]);
                Contact {
                    a: e.a,
                    b: e.b,
                    speed,
                }
            })
            .collect()
    }
    /// Position and yaw of every moving body and character, for `transform` (`syncTransforms`).
    fn poses(&self) -> Vec<(String, V3, f64)> {
        Vec::new()
    }
    fn set_manual_stepping(&mut self, on: bool) {}
    fn manual_stepping(&self) -> bool {
        false
    }
    fn set_gravity(&mut self, g: V3) {}
    /// `addJoint`: holds bodies `a` and `b` (their entities) together under `id`, replacing a joint of that id.
    /// False: not taken (a body unknown, or a port without joints — the default).
    fn add_joint(&mut self, id: &str, a: &str, b: &str, joint: &super::JointDesc) -> bool {
        false
    }
    /// `removeJoint`: lets go of the joint `id` (nothing when there is none; a removed body takes its joints along).
    fn remove_joint(&mut self, id: &str) {}
    /// Every pose of [`Self::poses`] given in turn with whether it is a character's — nothing made for the list (a
    /// port that keeps its poses in place gives them where they are).
    fn for_each_pose(&self, f: &mut dyn FnMut(&str, V3, f64, bool)) {
        for (id, p, yaw) in self.poses() {
            let character = self.has_character(&id);
            f(&id, p, yaw, character);
        }
    }
    /// The port as itself, for a host that speaks to a kind of port it knows in another way (a physics module of
    /// its own: its entries are called with addresses, nothing copied).
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// `saveState(entities?)`: the state of the given bodies — every moving body by default, in the order of
    /// the port. Characters and static bodies are not part of a snapshot.
    fn save_state(&self, entities: Option<&[String]>) -> Vec<BodyState> {
        let ids: Vec<String> = match entities {
            Some(list) => list.to_vec(),
            None => {
                let mut ids: Vec<String> = Vec::new();
                for (id, _, _) in self.poses() {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                ids
            }
        };
        ids.iter().filter_map(|e| self.body(e)).collect()
    }
    /// `restoreState(snapshot)`: writes a snapshot back; bodies absent from the world are skipped.
    fn restore_state(&mut self, snapshot: &[BodyState]) {
        for state in snapshot {
            if self.has_body(&state.entity) {
                self.set_body(state);
            }
        }
    }

    fn add_static_box(&mut self, center: V3, size: V3, entity: &str) {
        self.add_body(
            entity,
            &BodyDesc::new(BodyKind::Static, Shape::Box { size }, center),
        );
    }
    fn add_dynamic_box(&mut self, entity: &str, center: V3, size: V3, mass: f64) {
        let mut d = BodyDesc::new(BodyKind::Dynamic, Shape::Box { size }, center);
        d.mass = mass;
        self.add_body(entity, &d);
    }

    /// The address of the world's shape table (ABI 2.6, `gripforge_abi::ports::shape_table`): the colliders the
    /// physics holds, read in place by ai.navigation and the debugger. The same address for the life of the world.
    /// `None`: a port that publishes none (the null physics, no world).
    fn shapes_view(&mut self) -> Option<usize> {
        None
    }
    /// `CharacterHandle.groundClearance`: the separation a character controller keeps from what it stands on.
    fn ground_clearance(&self) -> f64 {
        0.0
    }
}

// ─── the physics a kit reaches: orders, tickets, published state ─────────────

/// A question asked of the physics (docs/kit-modules.md §7.17): a character's move, a cast, a raycast, an overlap.
/// The physics answers as soon as it took the order — at once when it runs on the kit's thread, a little later
/// when it runs its own loop on a Worker; [`Physics::poll`] reads the answer without ever waiting. `Ticket::NONE`:
/// the question could not be asked (its answer is "nothing").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ticket(pub u32);

impl Ticket {
    pub const NONE: Ticket = Ticket(0);
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// The answer of a [`Ticket`]. A ready answer is read once: the ticket is free after it.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// The physics has not taken the order yet: ask again later (the ticket stays valid).
    Pending,
    /// `ask_move_character` / `ask_cast_character`: the displacement and whether grounded (`None`: no character).
    Moved(Option<(V3, bool)>),
    /// `ask_raycast`, `ask_shape_cast`.
    Hit(Option<RayHit>),
    /// `ask_raycast_all`: the hits, nearest first.
    Hits(Vec<RayHit>),
    /// `ask_overlap_sphere` / `ask_overlap_box`: the entities, sorted, unique.
    Found(Vec<String>),
    /// Not a ticket of this physics (already read, forgotten, or never given).
    Unknown,
}

/// The default [`PhysicsPort::contacts`] read in place: the pairs whose contact began in the last step, with the
/// relative speed of the two bodies ([`PhysicsPort::body_velocity`]), no name copied.
pub fn contacts_in_place<P: PhysicsPort + ?Sized>(port: &P, f: &mut dyn FnMut(&str, &str, f64)) {
    let linvel = |entity: &str| port.body_velocity(entity).unwrap_or([0.0; 3]);
    port.for_each_contact_event(&mut |a, b, begin, _| {
        if begin {
            let (va, vb) = (linvel(a), linvel(b));
            f(
                a,
                b,
                null_hypot(&[va[0] - vb[0], va[1] - vb[1], va[2] - vb[2]]),
            );
        }
    });
}

/// The physics a kit reaches through `Kx::physics()` — the same contract whether the physics runs on the kit's
/// thread, on a Worker of its own, or in a native engine (docs/kit-modules.md §7.17):
///
/// - **reads** (`body`, `has_body`, `grounded`, `character_position`, `contact_events`…) read the state the physics
///   published, where it is, at any time: they never wait;
/// - **changes** (`add_body`, `remove`, `apply_force`, `apply_impulse`, `set_position`, `move_kinematic`…) are
///   orders, fire and forget;
/// - **questions** (`ask_move_character`, `ask_cast_character`, `ask_raycast`, `ask_overlap_*`) are orders that
///   hand out a [`Ticket`]; [`Physics::poll`] reads its answer once the physics took it (at once on the kit's own
///   thread). A kit keeps a pending ticket and polls it on a later tick.
#[allow(unused_variables)]
pub trait Physics {
    /// The offset of a character's frame (from the origin of the followed object to the point the kits give and
    /// read), changed live: the link between the object and the capsule moves, the capsule does not (no order to the
    /// physics, no jump). A port that keeps no frame ignores it.
    fn set_character_offset(&mut self, entity: &str, offset: V3) {}
    /// `addJoint` / `removeJoint` (the same as [`PhysicsPort::add_joint`]): taken when both bodies are known.
    fn add_joint(&mut self, id: &str, a: &str, b: &str, joint: &super::JointDesc) -> bool {
        false
    }
    fn remove_joint(&mut self, id: &str) {}
    /// Queue zone assignment after body creation. False means unsupported or invalid.
    fn set_collision_space(&mut self, entity: &str, space: &str) -> bool {
        false
    }
    /// `null` or `rapier` (or the engine's).
    fn kind(&self) -> &'static str;
    /// A step asked by a kit that steps by hand (`set_manual_stepping`): an order.
    fn step(&mut self, dt: f64);
    fn add_body(&mut self, entity: &str, desc: &BodyDesc);
    fn add_character(
        &mut self,
        entity: &str,
        position: V3,
        radius: f64,
        height: f64,
        opts: CharacterOptions,
    );
    fn remove(&mut self, entity: &str);
    fn has_body(&self, entity: &str) -> bool;
    fn has_character(&self, entity: &str) -> bool;
    fn body(&self, entity: &str) -> Option<BodyState>;
    fn set_body(&mut self, state: &BodyState);
    fn set_position(&mut self, entity: &str, p: V3);
    /// Field-level writes, applied in order with the other orders at the next physics step on the freshest state:
    /// they never overwrite a just-issued command with a stale copy of the body (prefer them to
    /// `body()` + `set_body()`, which on a physics that publishes a tick late rewrites old data).
    fn set_linvel(&mut self, entity: &str, v: V3);
    fn set_angvel(&mut self, entity: &str, w: V3);
    fn set_rotation(&mut self, entity: &str, q: Quat);
    fn mass(&self, entity: &str) -> f64;
    /// Whether the body rests on something, as last published (the first read of a body starts its probe).
    fn grounded(&self, entity: &str) -> bool;
    fn apply_impulse(&mut self, entity: &str, impulse: V3);
    fn apply_force(&mut self, entity: &str, force: V3);
    fn apply_torque_impulse(&mut self, entity: &str, torque: V3);
    fn move_kinematic(&mut self, entity: &str, p: V3, q: Option<Quat>);
    fn character_position(&self, entity: &str) -> Option<V3>;
    fn set_character_position(&mut self, entity: &str, p: V3);
    fn character_grounded(&self, entity: &str) -> bool;
    /// Contact begin / end events since the physics last published (the last step on the kit's thread).
    fn contact_events(&self) -> Vec<ContactEvent>;
    fn contacts(&self) -> Vec<Contact>;
    /// Every event of [`Self::contact_events`] in turn (`a`, `b`, `begin`, `impulse`), where the names are.
    fn for_each_contact_event(&self, f: &mut dyn FnMut(&str, &str, bool, f64)) {
        for e in self.contact_events() {
            f(&e.a, &e.b, e.begin, e.impulse);
        }
    }
    /// Every pair of [`Self::contacts`] in turn (`a`, `b`, `speed`), where the names are.
    fn for_each_contact(&self, f: &mut dyn FnMut(&str, &str, f64)) {
        for c in self.contacts() {
            f(&c.a, &c.b, c.speed);
        }
    }
    fn poses(&self) -> Vec<(String, V3, f64)>;
    fn set_manual_stepping(&mut self, on: bool);
    fn manual_stepping(&self) -> bool;
    fn set_gravity(&mut self, g: V3);
    fn save_state(&self, entities: Option<&[String]>) -> Vec<BodyState>;
    fn restore_state(&mut self, snapshot: &[BodyState]);
    fn add_static_box(&mut self, center: V3, size: V3, entity: &str);
    fn add_dynamic_box(&mut self, entity: &str, center: V3, size: V3, mass: f64);
    /// The address of the physics' shape table ([`PhysicsPort::shapes_view`]), read in place with
    /// `shape_table::read_shapes`; `None` without one (the kit says why it has nothing to read).
    fn shapes_view(&mut self) -> Option<usize>;
    fn ground_clearance(&self) -> f64;

    // ─── questions ───
    /// Moves a character by `delta` with collisions, slopes and steps resolved: answers the displacement and
    /// grounded.
    fn ask_move_character(&mut self, entity: &str, delta: V3) -> Ticket;
    /// The character's shape cast from an explicit centre, nothing moved (no autostep, no snap).
    fn ask_cast_character(&mut self, entity: &str, origin: V3, delta: V3) -> Ticket;
    fn ask_raycast(&mut self, origin: V3, dir: V3, max_toi: f64, exclude: Option<&str>) -> Ticket;
    /// Every hit of a ray through the layers of `filter`, nearest first, at most `max_hits` (1: the first hit,
    /// filtered). Answers [`Answer::Hits`].
    fn ask_raycast_all(
        &mut self,
        origin: V3,
        dir: V3,
        max_toi: f64,
        filter: &QueryFilter,
        max_hits: u32,
    ) -> Ticket;
    /// The first hit of a simple shape (box, sphere, capsule) moved along `dir`: [`Answer::Hit`]. Another shape:
    /// [`Ticket::NONE`].
    fn ask_shape_cast(
        &mut self,
        shape: &Shape,
        position: V3,
        rotation: Quat,
        dir: V3,
        max_toi: f64,
        filter: &QueryFilter,
    ) -> Ticket;
    /// The entities touching a simple shape: [`Answer::Found`]. Another shape: [`Ticket::NONE`].
    fn ask_overlap_shape(
        &mut self,
        shape: &Shape,
        position: V3,
        rotation: Quat,
        filter: &QueryFilter,
    ) -> Ticket;
    fn ask_overlap_sphere(&mut self, center: V3, radius: f64, exclude: Option<&str>) -> Ticket;
    fn ask_overlap_box(
        &mut self,
        center: V3,
        size: V3,
        rotation: Option<Quat>,
        exclude: Option<&str>,
    ) -> Ticket;
    /// The answer of a ticket: `Pending` until the physics took it; a ready answer is read once.
    fn poll(&mut self, ticket: Ticket) -> Answer;
    /// Gives a ticket back without reading it (its answer, when it comes, is dropped).
    fn forget(&mut self, ticket: Ticket);
    /// The counters of the physics as JSON text (`{ thread, steps, loopMs, loops, eventsDropped, asked, answered,
    /// inFlight, backlog }`); empty when the physics keeps none. For the debug panel.
    fn counters_json(&self) -> String;

    /// [`Physics::poll`] of a move or a cast.
    fn poll_moved(&mut self, ticket: Ticket) -> std::task::Poll<Option<(V3, bool)>> {
        match self.poll(ticket) {
            Answer::Pending => std::task::Poll::Pending,
            Answer::Moved(m) => std::task::Poll::Ready(m),
            _ => std::task::Poll::Ready(None),
        }
    }
    /// [`Physics::poll`] of a raycast.
    fn poll_hit(&mut self, ticket: Ticket) -> std::task::Poll<Option<RayHit>> {
        match self.poll(ticket) {
            Answer::Pending => std::task::Poll::Pending,
            Answer::Hit(h) => std::task::Poll::Ready(h),
            _ => std::task::Poll::Ready(None),
        }
    }
    /// [`Physics::poll`] of [`Physics::ask_raycast_all`].
    fn poll_hits(&mut self, ticket: Ticket) -> std::task::Poll<Vec<RayHit>> {
        match self.poll(ticket) {
            Answer::Pending => std::task::Poll::Pending,
            Answer::Hits(h) => std::task::Poll::Ready(h),
            Answer::Hit(h) => std::task::Poll::Ready(h.into_iter().collect()),
            _ => std::task::Poll::Ready(Vec::new()),
        }
    }
    /// [`Physics::poll`] of an overlap.
    fn poll_found(&mut self, ticket: Ticket) -> std::task::Poll<Vec<String>> {
        match self.poll(ticket) {
            Answer::Pending => std::task::Poll::Pending,
            Answer::Found(f) => std::task::Poll::Ready(f),
            _ => std::task::Poll::Ready(Vec::new()),
        }
    }
}

/// `Math.hypot` as V8 computes it: scaled by the largest term, Kahan summation.
pub fn null_hypot(values: &[f64]) -> f64 {
    let mut nan = false;
    let mut max = 0.0_f64;
    for v in values {
        if v.is_nan() {
            nan = true;
        } else if v.abs() > max {
            max = v.abs();
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let (mut sum, mut compensation) = (0.0_f64, 0.0_f64);
    for v in values {
        let n = v.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

// ─── assets ──────────────────────────────────────────────────────────────────

/// `AssetPort`: where the file bound to a slot is, for the kits that ask (most only name the slot in `model`).
pub trait AssetPort {
    fn url(&self, slot: &str) -> Option<String>;
    fn bound(&self, slot: &str) -> bool {
        self.url(slot).is_some()
    }
    fn variants(&self, slot: &str) -> Vec<String> {
        let _ = slot;
        Vec::new()
    }
    /// `bytes?(slot)`: the bytes of a bound data file the host loaded before mounting (a baked terrain map);
    /// `None` on a host that does not preload, as a TypeScript host without the method.
    fn bytes(&self, slot: &str) -> Option<Vec<u8>> {
        let _ = slot;
        None
    }
}
