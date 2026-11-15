//! A value a kit reads where it is (`Kx::call_with`): the answer of another kit module of the common memory stays
//! in the fixed-layout block it was given in (gripforge-kitcall src/fixed.rs) — a kit that reads two fields of a
//! large answer rebuilds nothing; it copies (`to_json`) only what it keeps. Elsewhere (the core answered, a kit in
//! its own memory, a scalar) the value is a `serde_json` one, read the same way. The core's twin: src/val.rs of
//! gripforge-kithost (the native host reads the provider's value itself).

use gripforge_abi::fixed::{Owned, View};
use serde_json::Value as Json;

/// What a service answered, owned by the caller for as long as it reads it.
pub(crate) enum Answer {
    Value(Json),
    Block(Owned),
}

impl Answer {
    pub(crate) fn val(&self) -> Val<'_> {
        match self {
            Answer::Value(v) => Val::Json(v),
            Answer::Block(b) => Val::Fixed(b.view()),
        }
    }
}

/// A value read in place. The accessors answer what `serde_json::Value`'s answer.
#[derive(Clone, Copy, Debug)]
pub enum Val<'a> {
    Json(&'a Json),
    Fixed(View<'a>),
}

impl<'a> Val<'a> {
    /// `value[key]` of a dictionary (`None`: not a dictionary, no such key).
    pub fn get(&self, key: &str) -> Option<Val<'a>> {
        match self {
            Val::Json(v) => v.as_object()?.get(key).map(Val::Json),
            Val::Fixed(v) => v.get(key).map(Val::Fixed),
        }
    }
    /// Item `i` of a list.
    pub fn at(&self, i: usize) -> Option<Val<'a>> {
        match self {
            Val::Json(v) => v.as_array()?.get(i).map(Val::Json),
            Val::Fixed(v) => v.index(i).map(Val::Fixed),
        }
    }
    /// Items of a list or entries of a dictionary (0 for the others).
    pub fn len(&self) -> usize {
        match self {
            Val::Json(Json::Array(a)) => a.len(),
            Val::Json(Json::Object(o)) => o.len(),
            Val::Json(_) => 0,
            Val::Fixed(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn is_null(&self) -> bool {
        match self {
            Val::Json(v) => v.is_null(),
            Val::Fixed(v) => v.is_null(),
        }
    }
    /// A dictionary / a list.
    pub fn is_object(&self) -> bool {
        match self {
            Val::Json(v) => v.is_object(),
            Val::Fixed(v) => v.tag() == gripforge_abi::fixed::tag::DICT,
        }
    }
    pub fn is_array(&self) -> bool {
        match self {
            Val::Json(v) => v.is_array(),
            Val::Fixed(v) => v.tag() == gripforge_abi::fixed::tag::LIST,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Val::Json(v) => v.as_bool(),
            Val::Fixed(v) => v.as_bool(),
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Val::Json(v) => v.as_f64(),
            Val::Fixed(v) => v.as_f64(),
        }
    }
    pub fn as_str(&self) -> Option<&'a str> {
        match self {
            Val::Json(v) => v.as_str(),
            Val::Fixed(v) => v.as_str(),
        }
    }
    /// A copy the kit keeps.
    pub fn to_json(&self) -> Json {
        match self {
            Val::Json(v) => (*v).clone(),
            Val::Fixed(v) => v.to_json().unwrap_or(Json::Null),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_block_reads_as_the_value_it_holds() {
        let v = json!({ "camera": { "min": [-90, -90.5], "max": [90, 90] }, "name": "map", "lanes": [{ "id": "mid" }], "open": true, "none": null, "big": 18446744073709551615u64 });
        let block = Answer::Block(Owned::from_json(&v).unwrap());
        let plain = Answer::Value(v.clone());
        for a in [&block, &plain] {
            let m = a.val();
            assert_eq!(m.to_json(), v);
            assert_eq!(m.len(), 6);
            let camera = m.get("camera").unwrap();
            assert_eq!(camera.to_json(), v["camera"]);
            assert_eq!(
                camera
                    .get("min")
                    .and_then(|p| p.at(1))
                    .and_then(|n| n.as_f64()),
                Some(-90.5)
            );
            assert_eq!(camera.get("min").map(|p| p.len()), Some(2));
            assert_eq!(m.get("name").and_then(|n| n.as_str()), Some("map"));
            assert_eq!(m.get("open").and_then(|n| n.as_bool()), Some(true));
            assert!(m.get("none").is_some_and(|n| n.is_null()));
            assert!(
                m.get("absent").is_none()
                    && m.at(0).is_none()
                    && m.get("name").unwrap().get("x").is_none()
            );
            assert!(
                m.is_object()
                    && !m.is_array()
                    && m.get("lanes").unwrap().is_array()
                    && !m.get("name").unwrap().is_object()
            );
            assert_eq!(
                m.get("lanes")
                    .and_then(|l| l.at(0))
                    .and_then(|l| l.get("id"))
                    .and_then(|i| i.as_str()),
                Some("mid")
            );
        }
    }
}
