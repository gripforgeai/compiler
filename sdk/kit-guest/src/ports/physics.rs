//! `kx.physics()` of a kit module: the port of the core, one call per method (`kx.assets()`: assets.rs).

use super::codec::{physics_fn as f, *};
use super::{args, ask, tell, BodyDesc, BodyState, CharacterOptions, Contact, ContactEvent};
#[allow(unused_imports)]
use super::{Answer, Physics, Quat, QueryFilter, RayHit, Shape, Ticket, V3};
use gripforge_abi::Reader;
use std::cell::Cell;

/// The physics of the core (`kithost.physics@1`). Every method of `PhysicsPort` is a call, the defaults of the
/// trait included (the core's port may override them); only `kind()` is kept (a host never changes its port
/// once mounted).
#[derive(Default)]
pub struct PhysicsProxy {
    kind: Cell<Option<&'static str>>,
}

/// The direct links of `kithost.physics@1.1` (the core: kitcall/physics.rs `direct`): scalars, the entity packed
/// in two `u64`, the state of a body in 14 `f64` this module names by address — so only in the common memory
/// (feature `common-memory`); elsewhere, and for a long id, `None`: the same call by the bridge.
#[cfg(feature = "common-memory")]
#[path = "physics_direct.rs"]
mod direct;
#[cfg(not(feature = "common-memory"))]
mod direct {
    use super::{BodyState, V3};
    pub fn has_body(_: &str) -> Option<bool> {
        None
    }
    pub fn body(_: &str) -> Option<Option<BodyState>> {
        None
    }
    pub fn set_body(_: &BodyState) -> Option<()> {
        None
    }
    pub fn vector(_: &str, _: u32, _: V3) -> Option<()> {
        None
    }
    pub fn scalar(_: &str, _: u32) -> Option<f64> {
        None
    }
    pub fn character_position(_: &str) -> Option<Option<V3>> {
        None
    }
    pub fn contacts(_: u32) -> Option<Vec<(String, String, f64, bool)>> {
        None
    }
    pub fn ask(_: u32, _: &[f64], _: u32, _: Option<&str>) -> Option<u32> {
        None
    }
    pub fn poll(_: u32) -> Option<super::Answer> {
        None
    }
}

fn opt_state(r: &mut Reader) -> R<Option<BodyState>> {
    Ok(if r.bool()? { Some(r_state(r)?) } else { None })
}

/// `kx.host.physics.body(id)` without the trait in scope (the field of the core is a `Box<dyn PhysicsPort>`).
impl std::ops::Deref for PhysicsProxy {
    type Target = dyn Physics;
    fn deref(&self) -> &(dyn Physics + 'static) {
        self
    }
}
impl std::ops::DerefMut for PhysicsProxy {
    fn deref_mut(&mut self) -> &mut (dyn Physics + 'static) {
        self
    }
}

impl PhysicsProxy {
    /// A new entry of the core (nothing to drop: what is kept never changes, or carries its revision).
    pub fn enter(&mut self) {}

    /// Whether the physics runs on a loop of its own. A kit module cannot tell yet (the core does, `gf_physics_on_loop`):
    /// it answers `false`, so a kit that pre-asks for a loop asks at its own turn here, as it does on the thread.
    pub fn on_loop(&self) -> bool {
        false
    }
}

impl Physics for PhysicsProxy {
    fn kind(&self) -> &'static str {
        if let Some(k) = self.kind.get() {
            return k;
        }
        let k = ask(crate::func!(f::KIND), &args(|_| {}), |r| r.string());
        let k: &'static str = match k.as_str() {
            "null" => "null",
            "rapier" => "rapier",
            _ => Box::leak(k.into_boxed_str()),
        };
        self.kind.set(Some(k));
        k
    }
    fn step(&mut self, dt: f64) {
        tell(
            crate::func!(f::STEP),
            &args(|w| {
                w.f64(dt);
            }),
        );
    }
    fn add_body(&mut self, entity: &str, desc: &BodyDesc) {
        tell(
            crate::func!(f::ADD_BODY),
            &args(|w| {
                w.str(entity);
                w_desc(w, desc);
            }),
        );
    }
    fn add_character(
        &mut self,
        entity: &str,
        position: V3,
        radius: f64,
        height: f64,
        opts: CharacterOptions,
    ) {
        tell(
            crate::func!(f::ADD_CHARACTER),
            &args(|w| {
                w.str(entity);
                w_v3(w, position);
                w.f64(radius).f64(height);
                w_opts(w, opts);
            }),
        );
    }
    fn remove(&mut self, entity: &str) {
        tell(
            crate::func!(f::REMOVE),
            &args(|w| {
                w.str(entity);
            }),
        );
    }
    fn has_body(&self, entity: &str) -> bool {
        if let Some(b) = direct::has_body(entity) {
            return b;
        }
        ask(
            crate::func!(f::HAS_BODY),
            &args(|w| {
                w.str(entity);
            }),
            |r| r.bool(),
        )
    }
    fn has_character(&self, entity: &str) -> bool {
        if let Some(v) = direct::scalar(entity, 2) {
            return v != 0.0;
        }
        ask(
            crate::func!(f::HAS_CHARACTER),
            &args(|w| {
                w.str(entity);
            }),
            |r| r.bool(),
        )
    }
    fn body(&self, entity: &str) -> Option<BodyState> {
        if let Some(b) = direct::body(entity) {
            return b;
        }
        ask(
            crate::func!(f::BODY),
            &args(|w| {
                w.str(entity);
            }),
            opt_state,
        )
    }
    fn set_body(&mut self, state: &BodyState) {
        if direct::set_body(state).is_some() {
            return;
        }
        tell(crate::func!(f::SET_BODY), &args(|w| w_state(w, state)));
    }
    fn set_position(&mut self, entity: &str, p: V3) {
        if direct::vector(entity, 0, p).is_some() {
            return;
        }
        tell_v3(crate::func!(f::SET_POSITION), entity, p);
    }
    fn set_linvel(&mut self, entity: &str, v: V3) {
        tell_v3(crate::func!(f::SET_LINVEL), entity, v);
    }
    fn set_angvel(&mut self, entity: &str, v: V3) {
        tell_v3(crate::func!(f::SET_ANGVEL), entity, v);
    }
    fn set_rotation(&mut self, entity: &str, q: Quat) {
        tell(
            crate::func!(f::SET_ROTATION),
            &args(|w| {
                w.str(entity);
                w_quat(w, q);
            }),
        );
    }
    fn mass(&self, entity: &str) -> f64 {
        if let Some(v) = direct::scalar(entity, 0) {
            return v;
        }
        ask(
            crate::func!(f::MASS),
            &args(|w| {
                w.str(entity);
            }),
            |r| r.f64(),
        )
    }
    fn grounded(&self, entity: &str) -> bool {
        if let Some(v) = direct::scalar(entity, 1) {
            return v != 0.0;
        }
        ask(
            crate::func!(f::GROUNDED),
            &args(|w| {
                w.str(entity);
            }),
            |r| r.bool(),
        )
    }
    fn apply_impulse(&mut self, entity: &str, impulse: V3) {
        // An event for the physics: a record of its inbox in the zone, taken in order before anything reads it.
        if crate::columns::push_impulse(gripforge_abi::rows::inbox::IMPULSE, entity, impulse) {
            return;
        }
        if direct::vector(entity, 1, impulse).is_some() {
            return;
        }
        tell_v3(crate::func!(f::APPLY_IMPULSE), entity, impulse);
    }
    fn apply_force(&mut self, entity: &str, force: V3) {
        // A state for the next step: added to the entity's force column, where the kits' forces of a tick add up
        // (in the precision the port adds in: single for Rapier). A port of another kind gets the call.
        let kind = self.kind();
        if kind == "null" || kind == "rapier" {
            match crate::columns::add_force(
                crate::host::module().id(),
                entity,
                force,
                kind == "rapier",
            ) {
                Ok(true) => return,
                Ok(false) => {}
                Err(refused) => panic!("{refused}"),
            }
        }
        if direct::vector(entity, 2, force).is_some() {
            return;
        }
        tell_v3(crate::func!(f::APPLY_FORCE), entity, force);
    }
    fn apply_torque_impulse(&mut self, entity: &str, torque: V3) {
        if crate::columns::push_impulse(gripforge_abi::rows::inbox::TORQUE_IMPULSE, entity, torque)
        {
            return;
        }
        if direct::vector(entity, 3, torque).is_some() {
            return;
        }
        tell_v3(crate::func!(f::APPLY_TORQUE_IMPULSE), entity, torque);
    }
    fn move_kinematic(&mut self, entity: &str, p: V3, q: Option<Quat>) {
        tell(
            crate::func!(f::MOVE_KINEMATIC),
            &args(|w| {
                w.str(entity);
                w_v3(w, p);
                w_opt_quat(w, q);
            }),
        );
    }
    fn hold_body(&mut self, entity: &str, mode: u32) -> bool {
        ask(
            crate::func!(f::HOLD_BODY),
            &args(|w| {
                w.str(entity);
                w.u32(mode);
            }),
            |r| r.bool(),
        )
    }
    fn set_collision_space(&mut self, entity: &str, space: &str) -> bool {
        ask(
            crate::func!(f::SET_COLLISION_SPACE),
            &args(|w| {
                w.str(entity);
                w.str(space);
            }),
            |r| r.bool(),
        )
    }
    fn character_position(&self, entity: &str) -> Option<V3> {
        if let Some(at) = direct::character_position(entity) {
            return at;
        }
        ask(
            crate::func!(f::CHARACTER_POSITION),
            &args(|w| {
                w.str(entity);
            }),
            r_opt_v3,
        )
    }
    fn set_character_position(&mut self, entity: &str, p: V3) {
        tell_v3(crate::func!(f::SET_CHARACTER_POSITION), entity, p);
    }
    fn set_character_offset(&mut self, entity: &str, offset: V3) {
        // In place, in the game's column when it has one (the front follows it): no call.
        let column = "physics.character_offset@1";
        if crate::columns::write("core.physics", entity, column, offset, false) == Ok(true) {
            return;
        }
        tell_v3(crate::func!(f::SET_CHARACTER_OFFSET), entity, offset);
    }
    fn add_joint(&mut self, id: &str, a: &str, b: &str, joint: &super::JointDesc) -> bool {
        let given = args(|w| {
            w.str(id).str(a).str(b);
            w_joint(w, joint);
        });
        ask(crate::func!(f::ADD_JOINT), &given, |r| r.bool())
    }
    fn remove_joint(&mut self, id: &str) {
        tell(
            crate::func!(f::REMOVE_JOINT),
            &args(|w| {
                w.str(id);
            }),
        );
    }
    fn character_grounded(&self, entity: &str) -> bool {
        if let Some(v) = direct::scalar(entity, 3) {
            return v != 0.0;
        }
        ask(
            crate::func!(f::CHARACTER_GROUNDED),
            &args(|w| {
                w.str(entity);
            }),
            |r| r.bool(),
        )
    }
    fn contact_events(&self) -> Vec<ContactEvent> {
        if let Some(list) = direct::contacts(0) {
            return list
                .into_iter()
                .map(|(a, b, impulse, begin)| ContactEvent {
                    begin,
                    a,
                    b,
                    impulse,
                })
                .collect();
        }
        ask(
            crate::func!(f::CONTACT_EVENTS),
            &args(|_| {}),
            r_contact_events,
        )
    }
    fn contacts(&self) -> Vec<Contact> {
        if let Some(list) = direct::contacts(1) {
            return list
                .into_iter()
                .map(|(a, b, speed, _)| Contact { a, b, speed })
                .collect();
        }
        ask(crate::func!(f::CONTACTS), &args(|_| {}), r_contacts)
    }
    fn poses(&self) -> Vec<(String, V3, f64)> {
        ask(crate::func!(f::POSES), &args(|_| {}), r_poses)
    }
    fn set_manual_stepping(&mut self, on: bool) {
        tell(
            crate::func!(f::SET_MANUAL_STEPPING),
            &args(|w| {
                w.bool(on);
            }),
        );
    }
    fn manual_stepping(&self) -> bool {
        ask(crate::func!(f::MANUAL_STEPPING), &args(|_| {}), |r| {
            r.bool()
        })
    }
    fn set_gravity(&mut self, g: V3) {
        tell(crate::func!(f::SET_GRAVITY), &args(|w| w_v3(w, g)));
    }
    fn save_state(&self, entities: Option<&[String]>) -> Vec<BodyState> {
        ask(
            crate::func!(f::SAVE_STATE),
            &args(|w| w_opt_strs(w, entities)),
            r_states,
        )
    }
    fn restore_state(&mut self, snapshot: &[BodyState]) {
        tell(
            crate::func!(f::RESTORE_STATE),
            &args(|w| w_states(w, snapshot)),
        );
    }
    fn add_static_box(&mut self, center: V3, size: V3, entity: &str) {
        tell(
            crate::func!(f::ADD_STATIC_BOX),
            &args(|w| {
                w_v3(w, center);
                w_v3(w, size);
                w.str(entity);
            }),
        );
    }
    fn add_dynamic_box(&mut self, entity: &str, center: V3, size: V3, mass: f64) {
        tell(
            crate::func!(f::ADD_DYNAMIC_BOX),
            &args(|w| {
                w.str(entity);
                w_v3(w, center);
                w_v3(w, size);
                w.f64(mass);
            }),
        );
    }
    fn shapes_view(&mut self) -> Option<usize> {
        // An isolated module owns its memory: an address of the core's means nothing in it.
        if cfg!(feature = "isolated") {
            return None;
        }
        let at = ask(crate::func!(f::SHAPES_VIEW), &args(|_| {}), |r| r.u64());
        (at != 0).then_some(at as usize)
    }
    fn ground_clearance(&self) -> f64 {
        ask(crate::func!(f::GROUND_CLEARANCE), &args(|_| {}), |r| {
            r.f64()
        })
    }

    // 1.4: the questions with a ticket (docs/kit-modules.md §7.17).
    fn ask_move_character(&mut self, entity: &str, delta: V3) -> Ticket {
        if let Some(t) = direct::ask(0, &delta, 0, Some(entity)) {
            return Ticket(t);
        }
        Ticket(ask(
            crate::func!(f::ASK_MOVE_CHARACTER),
            &args(|w| {
                w.str(entity);
                w_v3(w, delta);
            }),
            |r| r.u32(),
        ))
    }
    fn ask_cast_character(&mut self, entity: &str, origin: V3, delta: V3) -> Ticket {
        if let Some(t) = direct::ask(
            1,
            &[
                origin[0], origin[1], origin[2], delta[0], delta[1], delta[2],
            ],
            0,
            Some(entity),
        ) {
            return Ticket(t);
        }
        Ticket(ask(
            crate::func!(f::ASK_CAST_CHARACTER),
            &args(|w| {
                w.str(entity);
                w_v3(w, origin);
                w_v3(w, delta);
            }),
            |r| r.u32(),
        ))
    }
    fn ask_raycast(&mut self, origin: V3, dir: V3, max_toi: f64, exclude: Option<&str>) -> Ticket {
        if let Some(t) = direct::ask(
            2,
            &[
                origin[0], origin[1], origin[2], dir[0], dir[1], dir[2], max_toi,
            ],
            0,
            exclude,
        ) {
            return Ticket(t);
        }
        Ticket(ask(
            crate::func!(f::ASK_RAYCAST),
            &args(|w| {
                w_v3(w, origin);
                w_v3(w, dir);
                w.f64(max_toi).opt_str(exclude);
            }),
            |r| r.u32(),
        ))
    }
    fn ask_overlap_sphere(&mut self, center: V3, radius: f64, exclude: Option<&str>) -> Ticket {
        if let Some(t) = direct::ask(3, &[center[0], center[1], center[2], radius], 0, exclude) {
            return Ticket(t);
        }
        Ticket(ask(
            crate::func!(f::ASK_OVERLAP_SPHERE),
            &args(|w| {
                w_v3(w, center);
                w.f64(radius).opt_str(exclude);
            }),
            |r| r.u32(),
        ))
    }
    fn ask_overlap_box(
        &mut self,
        center: V3,
        size: V3,
        rotation: Option<Quat>,
        exclude: Option<&str>,
    ) -> Ticket {
        let q = rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let nums = [
            center[0], center[1], center[2], size[0], size[1], size[2], q[0], q[1], q[2], q[3],
        ];
        if let Some(t) = direct::ask(4, &nums, (rotation.is_some() as u32) << 1, exclude) {
            return Ticket(t);
        }
        Ticket(ask(
            crate::func!(f::ASK_OVERLAP_BOX),
            &args(|w| {
                w_v3(w, center);
                w_v3(w, size);
                w_opt_quat(w, rotation);
                w.opt_str(exclude);
            }),
            |r| r.u32(),
        ))
    }
    fn ask_raycast_all(
        &mut self,
        origin: V3,
        dir: V3,
        max_toi: f64,
        filter: &QueryFilter,
        max_hits: u32,
    ) -> Ticket {
        Ticket(ask(
            crate::func!(f::ASK_RAYCAST_ALL),
            &args(|w| {
                w_v3(w, origin);
                w_v3(w, dir);
                w.f64(max_toi);
                w_filter(w, filter);
                w.u32(max_hits);
            }),
            |r| r.u32(),
        ))
    }
    fn ask_shape_cast(
        &mut self,
        shape: &Shape,
        position: V3,
        rotation: Quat,
        dir: V3,
        max_toi: f64,
        filter: &QueryFilter,
    ) -> Ticket {
        Ticket(ask(
            crate::func!(f::ASK_SHAPE_CAST),
            &args(|w| {
                w_shaped(w, shape, position, rotation);
                w_v3(w, dir);
                w.f64(max_toi);
                w_filter(w, filter);
            }),
            |r| r.u32(),
        ))
    }
    fn ask_overlap_shape(
        &mut self,
        shape: &Shape,
        position: V3,
        rotation: Quat,
        filter: &QueryFilter,
    ) -> Ticket {
        Ticket(ask(
            crate::func!(f::ASK_OVERLAP_SHAPE),
            &args(|w| {
                w_shaped(w, shape, position, rotation);
                w_filter(w, filter);
            }),
            |r| r.u32(),
        ))
    }
    fn poll(&mut self, ticket: Ticket) -> Answer {
        if ticket.is_none() {
            return Answer::Unknown;
        }
        if let Some(a) = direct::poll(ticket.0) {
            return a;
        }
        ask(
            crate::func!(f::POLL),
            &args(|w| {
                w.u32(ticket.0);
            }),
            r_answer,
        )
    }
    fn forget(&mut self, ticket: Ticket) {
        if !ticket.is_none() {
            tell(
                crate::func!(f::FORGET),
                &args(|w| {
                    w.u32(ticket.0);
                }),
            );
        }
    }
    fn counters_json(&self) -> String {
        ask(crate::func!(f::COUNTERS), &args(|_| {}), |r| r.string())
    }
}

/// An order naming an entity and a vector (a character's position, its offset).
fn tell_v3(func: crate::uses::Func, entity: &str, v: V3) {
    tell(
        func,
        &args(|w| {
            w.str(entity);
            w_v3(w, v);
        }),
    );
}
