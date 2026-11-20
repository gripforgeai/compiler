//! The entity types (runtime/entities.ts) without the registry — shared word for word by the core and the kit
//! logic modules (the file is the kit façade's, the core includes it).

use serde_json::{json, Map, Value as Json};

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: String,
    pub kind: String,
    pub tags: Vec<String>,
    /// Components in insertion order (a JS `Map`).
    pub components: Vec<(String, Json)>,
    pub owner: String,
    pub refs: Map<String, Json>,
}

impl Entity {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
    pub fn component(&self, name: &str) -> Option<&Json> {
        self.components
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }
    pub fn set_component(&mut self, name: &str, value: Json) {
        match self.components.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value,
            None => self.components.push((name.to_string(), value)),
        }
    }
    pub fn add_tag(&mut self, tag: &str) {
        if !self.has_tag(tag) {
            self.tags.push(tag.to_string());
        }
    }
    pub fn to_json(&self) -> Json {
        let mut components = Map::new();
        for (k, v) in &self.components {
            components.insert(k.clone(), v.clone());
        }
        json!({ "id": self.id, "kind": self.kind, "tags": self.tags, "components": components, "owner": self.owner, "refs": self.refs })
    }
}

/// Position of an entity (`transform` component).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
}

impl Pos {
    pub fn of(v: Option<&Json>) -> Option<Pos> {
        let o = v?.as_object()?;
        let f = |k: &str| o.get(k).and_then(Json::as_f64).unwrap_or(0.0);
        Some(Pos {
            x: f("x"),
            y: f("y"),
            z: f("z"),
            yaw: f("yaw"),
        })
    }
}

pub fn default_transform() -> Json {
    json!({ "x": 0, "y": 0, "z": 0, "yaw": 0 })
}

/// Writes the fields given (x, y, z, yaw; `None`: left as it is) in a transform object, as `{ ...transform,
/// ...patch }` with the numbers of the JS model would: a field that is there keeps its place, a new one is added
/// at the end in that order. `None` (nothing done) when `transform` is no object; `Some(true)` when only values
/// of fields that were there changed (no field added).
pub fn pose_into(transform: &mut Json, fields: [Option<f64>; 4]) -> Option<bool> {
    let Json::Object(o) = transform else {
        return None;
    };
    let mut values_only = true;
    for (k, v) in ["x", "y", "z", "yaw"].into_iter().zip(fields) {
        if let Some(v) = v {
            match o.get_mut(k) {
                Some(slot) => *slot = crate::js::num(v),
                None => {
                    values_only = false;
                    o.insert(k.to_string(), crate::js::num(v));
                }
            }
        }
    }
    Some(values_only)
}

/// [`pose_into`] for a patch given as a value, when the patch only gives new values to fields the transform
/// already has among x, y, z, yaw (what the physics writes after a step): the values are stored where they are —
/// the object `{ ...transform, ...patch }` would be, with nothing allocated. False (nothing done): any other patch.
pub fn pose_patch_into(transform: &mut Json, patch: &Json) -> bool {
    let (Json::Object(o), Json::Object(p)) = (transform, patch) else {
        return false;
    };
    if p.is_empty()
        || !p.iter().all(|(k, v)| {
            v.is_number()
                && matches!(k.as_str(), "x" | "y" | "z" | "yaw")
                && o.get(k).is_some_and(Json::is_number)
        })
    {
        return false;
    }
    for (k, v) in p {
        if let Some(slot) = o.get_mut(k) {
            *slot = v.clone();
        }
    }
    true
}

/// x, y, z, yaw of a transform when each is there as a plain number a reader can write again exactly from its
/// `f64`: the values, and per field whether it is a float (bit `i`) or an integer. `None`: a field is missing, is
/// no number, or is an integer an `f64` does not hold exactly.
pub fn pose_exact(transform: Option<&Json>) -> Option<([f64; 4], u32)> {
    let o = transform?.as_object()?;
    let (mut out, mut floats) = ([0f64; 4], 0u32);
    for (i, k) in ["x", "y", "z", "yaw"].into_iter().enumerate() {
        let Json::Number(n) = o.get(k)? else {
            return None;
        };
        if n.is_f64() {
            floats |= 1 << i;
            out[i] = n.as_f64()?;
        } else {
            let v = n.as_i64()?;
            if (v as f64) as i64 != v || v.unsigned_abs() > 1 << 53 {
                return None;
            }
            out[i] = v as f64;
        }
    }
    Some((out, floats))
}

/// The inverse of [`pose_exact`] on a copy of the transform: each of x, y, z, yaw that is not already that number
/// of that kind becomes it. False (the copy may be half written: read it again) when a field is missing.
pub fn pose_exact_into(transform: &mut Json, fields: [f64; 4], floats: u32) -> bool {
    let Json::Object(o) = transform else {
        return false;
    };
    for (i, k) in ["x", "y", "z", "yaw"].into_iter().enumerate() {
        let Some(slot) = o.get_mut(k) else {
            return false;
        };
        let float = floats & 1 << i != 0;
        let same = matches!(&*slot, Json::Number(n) if n.is_f64() == float && n.as_f64().map(f64::to_bits) == Some(fields[i].to_bits()));
        if !same {
            *slot = if float {
                Json::from(fields[i])
            } else {
                Json::from(fields[i] as i64)
            };
        }
    }
    true
}

/// `{ ...a, ...b }` for two JSON objects.
pub fn merge(a: &Json, b: Option<&Json>) -> Json {
    let mut out = a.as_object().cloned().unwrap_or_default();
    if let Some(Json::Object(o)) = b {
        for (k, v) in o {
            out.insert(k.clone(), v.clone());
        }
    }
    Json::Object(out)
}

/// Which entities a query selects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub kind: Option<String>,
    pub all: Vec<String>,
    pub any: Vec<String>,
    pub none: Vec<String>,
}

impl Query {
    pub fn tag(tag: &str) -> Query {
        Query {
            all: vec![tag.to_string()],
            ..Query::default()
        }
    }
}

/// One condition of a neighbour search (`Entities::near`) on the entity itself. Names are borrowed: a search
/// builds nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cond<'a> {
    /// `component[field]` is a finite number above `min` (`field` empty: the component itself).
    Above {
        component: &'a str,
        field: &'a str,
        min: f64,
    },
    /// `component[field]` is a finite number.
    Number {
        component: &'a str,
        field: &'a str,
    },
    /// The component is a string that is not empty.
    Text {
        component: &'a str,
    },
    /// The component is not the string `value` (an entity without the component passes).
    TextNot {
        component: &'a str,
        value: &'a str,
    },
    /// The entity has the tag / has not.
    Tag(&'a str),
    NoTag(&'a str),
}

impl Cond<'_> {
    fn number(e: &Entity, component: &str, field: &str) -> Option<f64> {
        let c = e.component(component)?;
        let v = if field.is_empty() { c } else { c.get(field)? };
        match v {
            Json::Number(n) => n.as_f64().filter(|x| x.is_finite()),
            _ => None,
        }
    }

    pub fn holds(&self, e: &Entity) -> bool {
        match *self {
            Cond::Above {
                component,
                field,
                min,
            } => Self::number(e, component, field).is_some_and(|v| v > min),
            Cond::Number { component, field } => Self::number(e, component, field).is_some(),
            Cond::Text { component } => {
                matches!(e.component(component), Some(Json::String(s)) if !s.is_empty())
            }
            Cond::TextNot { component, value } => {
                !matches!(e.component(component), Some(Json::String(s)) if s == value)
            }
            Cond::Tag(t) => e.has_tag(t),
            Cond::NoTag(t) => !e.has_tag(t),
        }
    }
}

/// Where `e` stands on the ground: x and z of its transform (`Pos::of`, less the fields a distance on the ground
/// does not read). `None`: no transform object.
pub fn ground_of(e: &Entity) -> Option<(f64, f64)> {
    let o = e.component("transform")?.as_object()?;
    let f = |k: &str| o.get(k).and_then(Json::as_f64).unwrap_or(0.0);
    Some((f("x"), f("z")))
}

/// The distance on the ground (`Math.hypot` as the JS model computes it) from the point to an entity that stands
/// at `ground`, when it lies within `radius` and every condition holds for `e`.
#[inline]
pub fn near_from(
    e: &Entity,
    ground: (f64, f64),
    x: f64,
    z: f64,
    radius: f64,
    conds: &[Cond],
) -> Option<f64> {
    let d = crate::js::js_hypot(&[ground.0 - x, ground.1 - z]);
    (d <= radius && conds.iter().all(|c| c.holds(e))).then_some(d)
}

/// What `Entities::near` answers for one entity: its distance when it has a transform, lies within `radius` and
/// every condition holds. The one definition of that search — the core's registry and a kit module that walks its
/// copies both call it.
pub fn near_one(e: &Entity, x: f64, z: f64, radius: f64, conds: &[Cond]) -> Option<f64> {
    near_from(e, ground_of(e)?, x, z, radius, conds)
}

/// A scalar field of a component that a kit writes and reads **in place** (`Entities::set_fields`, `field`):
/// `static SPEED: Field = Field::new("car.state", "speed");`. The component stays the truth; when the kit
/// declared a column that mirrors the field (`of: { component, field }` in its `<kit>.schema.json`), a kit module
/// of the common memory reads it as a load in the entity's row and writes it by scalars — no value is built,
/// encoded or copied (docs/kit-modules.md §7.12).
pub struct Field {
    pub component: &'static str,
    /// The key in the component (empty: the component itself).
    pub name: &'static str,
    /// Where the game schema put the column that mirrors it, as a kit module last found it (the zone's generation
    /// and a place in the mirror table; unused by the core).
    #[allow(dead_code)]
    pub(crate) memo: std::sync::atomic::AtomicU64,
}

impl Field {
    pub const fn new(component: &'static str, name: &'static str) -> Field {
        Field {
            component,
            name,
            memo: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

/// What `set_fields` writes in one field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldVal<'a> {
    /// A number, stored as the JS model stores it (`js::num`).
    Num(f64),
    Flag(bool),
    Text(&'a str),
    /// An array of three numbers.
    Vec3([f64; 3]),
}

/// Whether storing `num(v)` in `slot` is a change a reader can redo from `v` alone: the number stored is a number,
/// and `slot` did not already hold that value written another way (an integer for a float).
fn num_into(slot: &mut Json, v: f64) -> bool {
    let new = crate::js::num(v);
    let plain = new.is_number()
        && !matches!(&*slot, Json::Number(n) if n.as_f64().map(f64::to_bits) == Some(v.to_bits()) && *slot != new);
    *slot = new;
    plain
}

/// Writes fields of a component object where they are — what `{ ...component, ...patch }` would store, with
/// nothing allocated: every field named must be a key of the object (a `Vec3` an array of three), and with
/// `whole` the object must have exactly those keys in that order (what replacing the component by the patch would
/// store). `None`: not so — nothing is written, the caller sets the component as a value. `Some(true)`: only
/// numbers and flags changed, each a value a reader can redo from the number alone (a copy of the entity is
/// brought up to date from its row); `Some(false)`: a text changed, or a number changed the way it is written.
///
/// The one definition of that write: the core's registry and the copies a kit module keeps both call it.
pub fn fields_into<'a>(
    component: &mut Json,
    fields: impl Iterator<Item = (&'a str, FieldVal<'a>)> + Clone,
    whole: bool,
) -> Option<bool> {
    let Json::Object(o) = component else {
        return None;
    };
    let (mut count, mut keys) = (0, o.keys());
    for (name, v) in fields.clone() {
        if whole && keys.next().is_none_or(|k| k != name) {
            return None;
        }
        let slot = o.get(name)?;
        if matches!(v, FieldVal::Vec3(_)) && !matches!(slot, Json::Array(a) if a.len() == 3) {
            return None;
        }
        count += 1;
    }
    if count == 0 || (whole && o.len() != count) {
        return None;
    }
    let mut plain = true;
    for (name, v) in fields {
        let Some(slot) = o.get_mut(name) else {
            continue;
        };
        match v {
            FieldVal::Num(v) => plain &= num_into(slot, v),
            FieldVal::Flag(b) => *slot = Json::Bool(b),
            FieldVal::Text(s) => match slot {
                Json::String(had) if had == s => {}
                Json::String(had) => {
                    had.clear();
                    had.push_str(s);
                    plain = false;
                }
                _ => {
                    *slot = Json::String(s.to_string());
                    plain = false;
                }
            },
            FieldVal::Vec3(v) => {
                if let Json::Array(a) = slot {
                    for (slot, x) in a.iter_mut().zip(v) {
                        plain &= num_into(slot, x);
                    }
                }
            }
        }
    }
    Some(plain)
}

/// Writes fields of a component object where it is: what `{ ...component, ...patch }` would store, a field given
/// `None` deleted (`delete component[name]`) — a field it holds keeps its place, a new one goes last. False: the
/// component is no object, nothing is written. Where [`fields_into`] needs every field to be there already, this
/// one also adds and removes keys (the shape of the component may change). The core's `Entities::merge_fields`
/// and the copies a kit module keeps both call it.
pub fn fields_merge<'a>(
    component: &mut Json,
    fields: impl Iterator<Item = (&'a str, Option<FieldVal<'a>>)>,
) -> bool {
    let Json::Object(o) = component else {
        return false;
    };
    for (name, v) in fields {
        let new = match v {
            None => {
                o.shift_remove(name);
                continue;
            }
            Some(FieldVal::Text(s)) => {
                if let Some(Json::String(had)) = o.get_mut(name) {
                    if had != s {
                        had.clear();
                        had.push_str(s);
                    }
                    continue;
                }
                Json::String(s.to_string())
            }
            Some(FieldVal::Num(v)) => crate::js::num(v),
            Some(FieldVal::Flag(b)) => Json::Bool(b),
            Some(FieldVal::Vec3(v)) => Json::Array(v.iter().map(|x| crate::js::num(*x)).collect()),
        };
        match o.get_mut(name) {
            Some(slot) => *slot = new,
            None => {
                o.insert(name.to_string(), new);
            }
        }
    }
    true
}

/// A field as a number / a flag / three numbers, read in the component (`None`: not of that kind).
pub fn field_num(component: Option<&Json>, name: &str) -> Option<f64> {
    let c = component?;
    (if name.is_empty() {
        Some(c)
    } else {
        c.get(name)
    })?
    .as_f64()
}
/// The field `name` of a component as it is (`None`: no component, no such field).
pub fn field_value<'a>(component: Option<&'a Json>, name: &str) -> Option<&'a Json> {
    let c = component?;
    if name.is_empty() {
        Some(c)
    } else {
        c.get(name)
    }
}
/// The field `name` of a component when it is a text.
pub fn field_text<'a>(component: Option<&'a Json>, name: &str) -> Option<&'a str> {
    field_value(component, name)?.as_str()
}
pub fn field_flag(component: Option<&Json>, name: &str) -> Option<bool> {
    let c = component?;
    (if name.is_empty() {
        Some(c)
    } else {
        c.get(name)
    })?
    .as_bool()
}
pub fn field_vec3(component: Option<&Json>, name: &str) -> Option<[f64; 3]> {
    let c = component?;
    match (if name.is_empty() {
        Some(c)
    } else {
        c.get(name)
    })? {
        Json::Array(a) if a.len() == 3 => Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?]),
        _ => None,
    }
}
