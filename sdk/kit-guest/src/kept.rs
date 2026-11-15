//! The typed payload of an event (src/bus/event.rs): a block of the contract kept by this module.
pub use gripforge_abi::fixed::Kept;
use serde_json::Value as Json;

pub fn to_json(k: &Kept) -> Json {
    k.view().to_json().unwrap_or(Json::Null)
}
pub fn num(k: &Kept, key: &str) -> Option<f64> {
    k.view().get(key)?.as_f64()
}
pub fn flag(k: &Kept, key: &str) -> Option<bool> {
    k.view().get(key)?.as_bool()
}
pub fn text<'a>(k: &'a Kept, key: &str) -> Option<&'a str> {
    k.view().get(key)?.as_str()
}
pub fn has(k: &Kept, key: &str) -> bool {
    k.view().get(key).is_some()
}
