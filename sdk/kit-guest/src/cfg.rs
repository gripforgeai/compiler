//! The config readers of kits/shared-runtime.ts (`cfgNumber`, `cfgInt`, `cfgBool`, `cfgString`), so a ported
//! kit reads its config exactly as its web runtime does.

use crate::js::{js_round, to_number};
use serde_json::Value as Json;

/// `Math.min(max, Math.max(min, v))`.
pub fn clamp(v: f64, min: f64, max: f64) -> f64 {
    max.min(min.max(v))
}

/// `cfgNumber(config, key, fallback, min, max)`: a number, or what `Number(raw)` makes of it when finite.
pub fn cfg_number(config: &Json, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    let n = to_number(config.get(key));
    clamp(if n.is_finite() { n } else { fallback }, min, max)
}

pub fn cfg_int(config: &Json, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    js_round(cfg_number(config, key, fallback, min, max))
}

pub fn cfg_bool(config: &Json, key: &str, fallback: bool) -> bool {
    config.get(key).and_then(Json::as_bool).unwrap_or(fallback)
}

/// `cfgString`: the trimmed string, or the fallback when absent or blank.
pub fn cfg_string(config: &Json, key: &str, fallback: &str) -> String {
    match config.get(key).and_then(Json::as_str).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => fallback.to_string(),
    }
}

/// The strings of an array config value (`[]` for anything else).
pub fn cfg_strings(v: Option<&Json>) -> Vec<String> {
    v.and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect()
}
