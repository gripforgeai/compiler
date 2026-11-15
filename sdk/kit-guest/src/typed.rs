//! The typed façade on the zone (docs/kit-modules.md §7.13) — shared word for word by the core (`#[path]` from
//! gripforge-kithost) and the kit modules.
//!
//! What a kit passes **each frame** to the façade is made of these types, never of `serde_json::Value`s built for
//! the call: arguments borrowed where the kit holds them ([`Arg`]), methods named once ([`Method`]), answers of a
//! typed provider as scalars ([`Ret`]), poses as numbers ([`Pose`]) and events as `repr(C)` records declared in a
//! `.gfi` ([`TypedEvent`]). JSON is kept for a real change of state (an event emitted when something changed, a
//! save, the debug snapshot) and the on-demand API.
//!
//! **Contract frozen on 2026-10-25** (signatures of this file and of the `Kx` methods that take them: `call_args`,
//! `call_args_with`, `call_m`, `call_m_with`, `set_pose`, `set_fields`, `emit_fields`, `emit_fields_entity`,
//! `emit_ev`, `emit_ev_entity`, `set_var_arg`; `KitModule::call_typed`). The implementation behind them may move
//! (step 3 of the lot: arguments lent as `kitcall::fast::Scalar`s and fixed-layout blocks, events written once in
//! the EVENTS ring) — what a kit writes does not. Today every one of them gives **the same value** as the JSON
//! path it replaces (the parity fixtures do not move); what changes first is that no `json!` is left in the kit.

use serde_json::{Map, Value as Json};

/// One argument of a call, one field of a payload or a variable, **borrowed** where the kit holds it.
///
/// - `Num`: a number as JavaScript has it (the `num()` of src/js.rs: an integral value is written as an integer) —
///   the default for a kit's `f64`s, the same bytes as `num(x)`.
/// - `F64`: a float written as a float (what `json!(x)` of an `f64` gave). Use it only where the kit wrote
///   `json!(x)` of an `f64`: the parity tells the two apart.
/// - `Int`: an integer (`json!(n)` of an integer type).
/// - `Str`: a text (`json!(id)`, `json!("exit")`).
/// - `Vec3`: `[num(x), num(y), num(z)]` (the `vec_json` of the kits).
/// - `List`, `Dict`: a list, a dictionary of arguments, in this order (`json!([..])`, `json!({..})`).
/// - `Value`: a JSON value the kit already holds (a config, a component it read) — lent, not cloned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg<'a> {
    Null,
    Bool(bool),
    Num(f64),
    F64(f64),
    Int(i64),
    Str(&'a str),
    Vec3([f64; 3]),
    List(&'a [Arg<'a>]),
    Dict(&'a [(&'a str, Arg<'a>)]),
    Value(&'a Json),
}

impl From<bool> for Arg<'_> {
    fn from(b: bool) -> Self {
        Arg::Bool(b)
    }
}
impl From<f64> for Arg<'_> {
    fn from(n: f64) -> Self {
        Arg::Num(n)
    }
}
impl From<i64> for Arg<'_> {
    fn from(n: i64) -> Self {
        Arg::Int(n)
    }
}
impl From<u32> for Arg<'_> {
    fn from(n: u32) -> Self {
        Arg::Int(n as i64)
    }
}
impl<'a> From<&'a str> for Arg<'a> {
    fn from(s: &'a str) -> Self {
        Arg::Str(s)
    }
}
impl<'a> From<&'a String> for Arg<'a> {
    fn from(s: &'a String) -> Self {
        Arg::Str(s)
    }
}
impl<'a> From<&'a Json> for Arg<'a> {
    fn from(v: &'a Json) -> Self {
        Arg::Value(v)
    }
}
impl<'a, T: Into<Arg<'a>>> From<Option<T>> for Arg<'a> {
    fn from(v: Option<T>) -> Self {
        v.map_or(Arg::Null, Into::into)
    }
}

/// `num()` of src/js.rs, kept here so that this file stands alone.
fn js_num(n: f64) -> Json {
    if n.is_finite() && n == n.trunc() && n.abs() < 9.007_199_254_740_992e15 {
        Json::from(if n == 0.0 { 0i64 } else { n as i64 })
    } else if n.is_finite() {
        Json::from(n)
    } else {
        Json::Null
    }
}

impl Arg<'_> {
    /// The JSON value this argument stands for (the path of today, and of a provider that is not typed).
    pub fn to_json(&self) -> Json {
        match *self {
            Arg::Null => Json::Null,
            Arg::Bool(b) => Json::Bool(b),
            Arg::Num(n) => js_num(n),
            Arg::F64(n) => serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number),
            Arg::Int(n) => Json::from(n),
            Arg::Str(s) => {
                made();
                Json::String(s.to_string())
            }
            Arg::Vec3([x, y, z]) => {
                made();
                Json::Array(vec![js_num(x), js_num(y), js_num(z)])
            }
            Arg::List(l) => {
                made();
                Json::Array(l.iter().map(Arg::to_json).collect())
            }
            Arg::Dict(d) => dict_json(d),
            Arg::Value(v) => v.clone(),
        }
    }
    /// A number (`Num`, `F64`, `Int`, or a JSON number lent).
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Arg::Num(n) | Arg::F64(n) => Some(n),
            Arg::Int(n) => Some(n as f64),
            Arg::Value(v) => v.as_f64(),
            _ => None,
        }
    }
    /// A text (`Str`, or a JSON text lent).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Arg::Str(s) => Some(s),
            Arg::Value(v) => v.as_str(),
            _ => None,
        }
    }
    /// A flag (`Bool`, or a JSON boolean lent).
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Arg::Bool(b) => Some(b),
            Arg::Value(v) => v.as_bool(),
            _ => None,
        }
    }
    /// A vector (`Vec3`, or a JSON list of three numbers lent).
    pub fn as_vec3(&self) -> Option<[f64; 3]> {
        match *self {
            Arg::Vec3(v) => Some(v),
            Arg::Value(Json::Array(a)) if a.len() == 3 => {
                Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
            }
            _ => None,
        }
    }
}

thread_local! {
    static MADE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The JSON dictionaries, lists and texts made from typed arguments by this thread so far (the counter the tests
/// read: a typed write made none).
pub fn json_made() -> u64 {
    MADE.with(|m| m.get())
}

fn made() {
    MADE.with(|m| m.set(m.get() + 1));
}

/// The JSON dictionary of fields, in their order (what `json!({ .. })` gave).
pub fn dict_json(fields: &[(&str, Arg)]) -> Json {
    made();
    let mut m = Map::with_capacity(fields.len());
    for (k, v) in fields {
        m.insert((*k).to_string(), v.to_json());
    }
    Json::Object(m)
}

/// The JSON values of arguments (the path of today: `Kx::call` of a provider that is not typed).
pub fn args_json(args: &[Arg]) -> Vec<Json> {
    args.iter().map(Arg::to_json).collect()
}

/// A method of a service, named once: `static NEXT_STEP: Method = Method::new("world.navigation", "nextStep");`.
/// The core resolves the two names to its atoms the first time the method is called (step 3); a kit never builds
/// the names again.
#[derive(Debug)]
pub struct Method {
    pub capability: &'static str,
    pub method: &'static str,
    /// The atoms of the two names + 1 (0: not resolved yet). Owned by the façade.
    pub atoms: std::sync::atomic::AtomicU64,
}

impl Method {
    pub const fn new(capability: &'static str, method: &'static str) -> Method {
        Method {
            capability,
            method,
            atoms: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

/// What a typed provider answers ([`crate::kit::KitModule::call_typed`]): a scalar, given by value. A list or a
/// dictionary is not a `Ret`: such a method stays a `call` answering a value (read in place by `call_with`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Ret {
    #[default]
    Null,
    Bool(bool),
    /// A number as JavaScript has it (`num()`).
    Num(f64),
    Int(i64),
    Vec3([f64; 3]),
    Text(String),
}

impl Ret {
    /// The JSON value the same answer had from `call` (the answer a caller of `call` receives).
    pub fn to_json(&self) -> Json {
        match self {
            Ret::Null => Json::Null,
            Ret::Bool(b) => Json::Bool(*b),
            Ret::Num(n) => js_num(*n),
            Ret::Int(n) => Json::from(*n),
            Ret::Vec3([x, y, z]) => Json::Array(vec![js_num(*x), js_num(*y), js_num(*z)]),
            Ret::Text(s) => Json::String(s.clone()),
        }
    }
}

/// A move of an entity: the fields of its transform that change (`None`: left as it is). The same write as
/// `set_transform(id, &json!({ "x": num(x), .. }))` with the fields given, by numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    pub yaw: Option<f64>,
}

impl Pose {
    /// x, y, z.
    pub const fn at(p: [f64; 3]) -> Pose {
        Pose {
            x: Some(p[0]),
            y: Some(p[1]),
            z: Some(p[2]),
            yaw: None,
        }
    }
    /// x, y, z and yaw.
    pub const fn at_yaw(p: [f64; 3], yaw: f64) -> Pose {
        Pose {
            x: Some(p[0]),
            y: Some(p[1]),
            z: Some(p[2]),
            yaw: Some(yaw),
        }
    }
    /// The yaw alone.
    pub const fn yaw(yaw: f64) -> Pose {
        Pose {
            x: None,
            y: None,
            z: None,
            yaw: Some(yaw),
        }
    }
    pub const fn with_yaw(self, yaw: f64) -> Pose {
        Pose {
            yaw: Some(yaw),
            ..self
        }
    }
    /// The four fields, in the order of `Entities::set_pose`.
    pub const fn fields(&self) -> [Option<f64>; 4] {
        [self.x, self.y, self.z, self.yaw]
    }
    /// The JSON patch of `set_transform` (for a path that still takes one).
    pub fn to_json(&self) -> Json {
        let mut m = Map::new();
        for (k, v) in ["x", "y", "z", "yaw"].into_iter().zip(self.fields()) {
            if let Some(v) = v {
                m.insert(k.to_string(), js_num(v));
            }
        }
        Json::Object(m)
    }
}

/// The type of one field of a typed event (`.gfi`: `event <n> <topic>(field: type, …)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    F64,
    I64,
    U32,
    Bool,
    Vec3,
    /// A text: an entity id, a name. In the `repr(C)` record an [`Atom`]; in the payload the text.
    Atom,
}

/// A text as a number of the core's names table (`kithost.world` atoms, docs/kit-modules.md §7.1). In a typed
/// event a text field is an `Atom`: the record stays fixed in size. Never saved, never sent over the network.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Atom(pub u32);

/// An event declared in a `.gfi` (`event <n> <topic>(…)`), whose Rust record `tools/modcall/gfi.mjs --rust`
/// generates: `#[repr(C)]`, `Copy`, fields of fixed size, texts as [`Atom`]s. The emitter writes it once
/// (`Kx::emit_ev`); a reader reads it with [`TypedEvent::read`] — or reads the payload's fields by name
/// (`ev.payload.num("damage")`) as before: a subscriber that is not migrated sees the same payload.
pub trait TypedEvent: Copy + 'static {
    /// The topic (`room.entered`).
    const TOPIC: &'static str;
    /// Its interface and number in that interface (the `.gfi`): what the EVENTS ring records as its kind.
    const INTERFACE: &'static str;
    const NUMBER: u32;
    /// The fields in their order, with their type (the payload's keys).
    const FIELDS: &'static [(&'static str, FieldType)];
    /// The payload as the fields of a dictionary, in the order of `FIELDS` (a text: its name).
    fn fields(&self, names: &dyn Fn(Atom) -> String, out: &mut dyn FnMut(&'static str, Arg));
    /// The record of a payload (`None`: a field is missing or of another type).
    fn read(payload: &Json, atom: &dyn Fn(&str) -> Atom) -> Option<Self>;
}

/// The JSON payload of a typed event (the path of today and of a reader that reads JSON).
pub fn event_json<E: TypedEvent>(e: &E, names: &dyn Fn(Atom) -> String) -> Json {
    let mut m = Map::with_capacity(E::FIELDS.len());
    e.fields(names, &mut |k, v| {
        m.insert(k.to_string(), v.to_json());
    });
    Json::Object(m)
}

/// [`TypedEvent::read`] of an event's payload (the subscriber side of a typed event).
pub fn read_event<E: TypedEvent>(ev: &crate::bus::GameEvent) -> Option<E> {
    if ev.topic != E::TOPIC {
        return None;
    }
    E::read(&ev.payload, &atoms::of)
}

/// The names of the atoms of the typed events in this module (stub of step 1: a table of the module; step 3 binds
/// it to the core's atoms). Thread local: a module runs on one thread.
pub mod atoms {
    use super::Atom;
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static NAMES: RefCell<(Vec<String>, HashMap<String, u32>)> = RefCell::new((Vec::new(), HashMap::new()));
    }

    /// The atom of a text.
    pub fn of(name: &str) -> Atom {
        NAMES.with(|t| {
            let mut t = t.borrow_mut();
            if let Some(&n) = t.1.get(name) {
                return Atom(n);
            }
            let n = t.0.len() as u32;
            t.0.push(name.to_string());
            t.1.insert(name.to_string(), n);
            Atom(n)
        })
    }
    /// The text of an atom (empty: not one of this table).
    pub fn name(a: Atom) -> String {
        NAMES.with(|t| t.borrow().0.get(a.0 as usize).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn args_give_the_values_of_json() {
        assert_eq!(Arg::Num(2.0).to_json(), json!(2));
        assert_eq!(Arg::Num(2.5).to_json(), json!(2.5));
        assert_eq!(Arg::F64(2.0).to_json(), json!(2.0f64));
        assert_eq!(Arg::Vec3([1.0, 0.5, -0.0]).to_json(), json!([1, 0.5, 0]));
        let d = [
            ("entity", Arg::Str("e1")),
            ("at", Arg::Vec3([0.0, 1.0, 2.0])),
            ("ok", Arg::Bool(true)),
        ];
        assert_eq!(
            dict_json(&d).to_string(),
            json!({ "entity": "e1", "at": [0, 1, 2], "ok": true }).to_string()
        );
        assert_eq!(
            Pose::at([1.0, 2.0, 3.5]).to_json(),
            json!({ "x": 1, "y": 2, "z": 3.5 })
        );
        assert_eq!(atoms::name(atoms::of("room.a")), "room.a");
    }
}
