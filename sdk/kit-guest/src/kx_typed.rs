//! The typed façade of `Kx` (docs/kit-modules.md §7.13): the methods a kit calls each frame without building a
//! JSON value. Shared word for word by the core (`#[path]` from gripforge-kithost) and the kit modules: it only uses
//! what both `Kx` have (`call_with`, `emit`, `emit_entity`, `set_var`, `host.entities`).
//!
//! Step 1 (contract, 2026-10-25): every method gives the same value as the JSON path it replaces, through that path.
//! Step 3 replaces the bodies — arguments lent as scalars and fixed-layout blocks to the provider's `call_typed`,
//! events written once as `repr(C)` records in the EVENTS ring, variables written in the STATE region — without
//! changing a signature.

use crate::entities::{Field, FieldVal};
use crate::host::Kx;
use crate::typed::{atoms, event_json, Arg, Method, Pose, TypedEvent};
use serde_json::Value as Json;

impl Kx<'_> {
    /// `kx.call(capability, method, args)` with typed arguments; the answer as a value (a caller that reads two
    /// fields of it: [`Kx::call_args_with`]).
    pub fn call_args(
        &mut self,
        capability: &str,
        method: &str,
        args: &[Arg],
    ) -> Result<Json, String> {
        self.call_args_with(capability, method, args, |v| v.to_json())
    }
    /// `kx.call_with(capability, method, args, f)` with typed arguments: `f` reads the answer where it is.
    pub fn call_args_with<R>(
        &mut self,
        capability: &str,
        method: &str,
        args: &[Arg],
        f: impl FnOnce(crate::val::Val) -> R,
    ) -> Result<R, String> {
        self.host
            .call_service_args_with(capability, method, args, f)
    }
    /// A method named once ([`Method`]) with typed arguments; the answer as a value.
    pub fn call_m(&mut self, m: &Method, args: &[Arg]) -> Result<Json, String> {
        self.call_args(m.capability, m.method, args)
    }
    /// A method named once with typed arguments; `f` reads the answer where it is.
    pub fn call_m_with<R>(
        &mut self,
        m: &Method,
        args: &[Arg],
        f: impl FnOnce(crate::val::Val) -> R,
    ) -> Result<R, String> {
        self.call_args_with(m.capability, m.method, args, f)
    }

    /// Moves an entity: `set_transform(id, &json!({ "x": num(x), … }))` by numbers (the fields of `pose` that are
    /// `Some`). Written in place in the transform and in the entity's row of the zone (§7.9).
    pub fn set_pose(&mut self, id: &str, pose: Pose) {
        self.host
            .entities
            .set_pose(id, pose.x, pose.y, pose.z, pose.yaw);
    }
    /// Writes fields of one component in place (`Entities::set_fields`, §7.12): what `set(id, component, {
    /// ...component, ...fields })` stored (`whole`: `set(id, component, fields)`). False: not written — the
    /// component lacks one of the fields, or no such entity: the caller sets it as a value, once.
    pub fn set_fields(&mut self, id: &str, fields: &[(&Field, FieldVal)], whole: bool) -> bool {
        self.host.entities.set_fields(id, fields, whole)
    }

    /// Whether an event of `topic` would be heard by anybody (a subscriber, a rule, a mission, a watcher). A kit
    /// that makes a frequent event (`locomotion.third_person.moved`) asks before building it: nothing is made for an event that goes
    /// nowhere.
    pub fn has_audience(&self, topic: &str) -> bool {
        self.host.has_audience(topic)
    }
    /// Whether a tap may listen to `hook` (game.engine.hooks): a kit that asks a frequent hook (each tick, each frame)
    /// checks it before building the payload — nothing is built nor sent for a hook nobody taps. `decide` and
    /// `game.engine.hooks` `run` / `decide` make the same check themselves, after the payload is built.
    pub fn hook_tapped(&mut self, hook: &str) -> bool {
        self.host.hook_tapped(hook)
    }
    /// `kx.emit(topic, json!({ k: v, … }))` with the fields typed, in their order (the same payload).
    pub fn emit_fields(&mut self, topic: &str, fields: &[(&str, Arg)]) {
        let id = self.kit_id();
        self.host.emit_fields_from(topic, fields, &id, None);
    }
    /// `kx.emit_entity(topic, json!({ … }), entity)` with the fields typed.
    pub fn emit_fields_entity(&mut self, topic: &str, fields: &[(&str, Arg)], entity: &str) {
        let id = self.kit_id();
        self.host.emit_fields_from(topic, fields, &id, Some(entity));
    }
    /// Emits a typed event declared in a `.gfi` (`E::TOPIC`): its fields written as the payload's block where the kit
    /// modules carry one (`emit_fields`), the same payload for the readers that read it by name — no JSON value.
    pub fn emit_ev<E: TypedEvent>(&mut self, e: &E) {
        self.emit_ev_at(e, None);
    }
    /// [`Kx::emit_ev`] about an entity.
    pub fn emit_ev_entity<E: TypedEvent>(&mut self, e: &E, entity: &str) {
        self.emit_ev_at(e, Some(entity));
    }
    fn emit_ev_at<E: TypedEvent>(&mut self, e: &E, entity: Option<&str>) {
        // The record's fields, copied out of the callback (a text field: the atom's name, the one string made).
        let mut own: [(&'static str, EvField); EV_FIELDS] = Default::default();
        let mut n = 0usize;
        let mut whole = E::FIELDS.len() <= EV_FIELDS;
        if whole {
            e.fields(&atoms::name, &mut |k, v| {
                if n >= EV_FIELDS {
                    whole = false;
                    return;
                }
                own[n] = (
                    k,
                    match v {
                        Arg::Null => EvField::Null,
                        Arg::Bool(b) => EvField::Bool(b),
                        Arg::Num(x) => EvField::Num(x),
                        Arg::F64(x) => EvField::F64(x),
                        Arg::Int(x) => EvField::Int(x),
                        Arg::Vec3(x) => EvField::Vec3(x),
                        Arg::Str(s) => EvField::Text(s.to_string()),
                        _ => {
                            whole = false;
                            EvField::Null
                        }
                    },
                );
                n += 1;
            });
        }
        let id = self.kit_id();
        if !whole {
            let payload = event_json(e, &atoms::name);
            self.host.emit_from(E::TOPIC, payload, &id, entity);
            return;
        }
        let mut args: [(&str, Arg); EV_FIELDS] = [("", Arg::Null); EV_FIELDS];
        for (slot, (k, f)) in args.iter_mut().zip(own.iter()).take(n) {
            *slot = (
                k,
                match f {
                    EvField::Null => Arg::Null,
                    EvField::Bool(b) => Arg::Bool(*b),
                    EvField::Num(x) => Arg::Num(*x),
                    EvField::F64(x) => Arg::F64(*x),
                    EvField::Int(x) => Arg::Int(*x),
                    EvField::Vec3(x) => Arg::Vec3(*x),
                    EvField::Text(s) => Arg::Str(s),
                },
            );
        }
        self.host
            .emit_fields_from(E::TOPIC, &args[..n], &id, entity);
    }

    /// `kx.set_var(path, json!(v))` with a typed value (a number, a flag, a text: the direct link of the STATE
    /// region, no value built).
    pub fn set_var_arg(&mut self, path: &str, value: Arg) {
        self.set_var(path, value.to_json());
    }
}

/// The fields of a typed event held on the stack by `emit_ev` (more: the JSON payload, as before).
const EV_FIELDS: usize = 12;

#[derive(Default)]
enum EvField {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    F64(f64),
    Int(i64),
    Vec3([f64; 3]),
    Text(String),
}
