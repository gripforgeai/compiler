//! The few JavaScript semantics the TypeScript runtime relies on (`Number(v)`, `String(v)`, loose `==`,
//! FNV-1a over UTF-16 code units), so the DSL behaves the same on both sides.

use serde_json::{Map, Value as Json};

/// `undefined` is `None`; everything else is plain JSON.
pub type JsVal = Option<Json>;

/// A JSON number from an `f64`: integral values stay integers (as `JSON.stringify` prints them).
pub fn num(n: f64) -> Json {
    if n.is_finite() && n == n.trunc() && n.abs() < 9.007_199_254_740_992e15 {
        if n == 0.0 {
            return Json::from(0i64);
        }
        Json::from(n as i64)
    } else if n.is_finite() {
        Json::from(n)
    } else {
        Json::Null
    }
}

/// `Number(v)`.
pub fn to_number(v: Option<&Json>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Json::Null) => 0.0,
        Some(Json::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Json::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Json::String(s)) => string_to_number(s),
        Some(Json::Array(a)) => match a.len() {
            0 => 0.0,
            1 => string_to_number(&to_js_string(Some(&a[0]))),
            _ => f64::NAN,
        },
        Some(Json::Object(_)) => f64::NAN,
    }
}

fn string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    let lower = t.to_ascii_lowercase();
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return u64::from_str_radix(rest, radix)
                .map(|v| v as f64)
                .unwrap_or(f64::NAN);
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if t.chars()
        .any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E')
    {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `numberOf(v, fallback)` of runtime/utils.ts.
pub fn number_of(v: Option<&Json>, fallback: f64) -> f64 {
    let n = match v {
        Some(Json::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        other => to_number(other),
    };
    if n.is_finite() {
        n
    } else {
        fallback
    }
}

/// `String(n)` for a number.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if n == 0.0 {
        return "0".into();
    }
    let a = n.abs();
    if !(1e-6..1e21).contains(&a) {
        let s = format!("{:e}", n);
        // Rust prints `1e21` / `1e-7`, JavaScript `1e+21` / `1e-7`.
        return match s.split_once('e') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
            _ => s,
        };
    }
    format!("{}", n)
}

/// `String(v)`.
pub fn to_js_string(v: Option<&Json>) -> String {
    match v {
        None => "undefined".into(),
        Some(Json::Null) => "null".into(),
        Some(Json::Bool(b)) => b.to_string(),
        Some(Json::Number(n)) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Json::String(s)) => s.clone(),
        Some(Json::Array(a)) => a
            .iter()
            .map(|x| match x {
                Json::Null => String::new(),
                other => to_js_string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Json::Object(_)) => "[object Object]".into(),
    }
}

/// `typeof v`.
pub fn type_of(v: Option<&Json>) -> &'static str {
    match v {
        None => "undefined",
        Some(Json::Null) | Some(Json::Array(_)) | Some(Json::Object(_)) => "object",
        Some(Json::Bool(_)) => "boolean",
        Some(Json::Number(_)) => "number",
        Some(Json::String(_)) => "string",
    }
}

/// `a === b`. Two objects are never the same reference here (state values are written, not shared).
pub fn strict_eq(a: Option<&Json>, b: Option<&Json>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Json::Null), Some(Json::Null)) => true,
        (Some(Json::Bool(x)), Some(Json::Bool(y))) => x == y,
        (Some(Json::Number(x)), Some(Json::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Json::String(x)), Some(Json::String(y))) => x == y,
        _ => false,
    }
}

/// `*`, `vehicle.*` or an exact topic.
pub fn topic_matches(pattern: &str, topic: &str) -> bool {
    if pattern == "*" || pattern == topic {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        if pattern.ends_with(".*") {
            return topic.starts_with(prefix);
        }
    }
    false
}

/// `matchesSubset(match, value)` of runtime/utils.ts.
pub fn matches_subset(m: &Json, value: Option<&Json>) -> bool {
    match m {
        Json::Object(mo) => match value {
            Some(Json::Object(vo)) => mo.iter().all(|(k, mv)| matches_subset(mv, vo.get(k))),
            Some(Json::Array(_)) => mo.iter().all(|(_, mv)| matches_subset(mv, None)),
            _ => false,
        },
        Json::Array(ma) => match value {
            Some(Json::Array(va)) => {
                va.len() == ma.len() && ma.iter().zip(va).all(|(a, b)| matches_subset(a, Some(b)))
            }
            _ => false,
        },
        prim => strict_eq(Some(prim), value),
    }
}

/// FNV-1a 32-bit over UTF-16 code units (`hash32`).
pub fn hash32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for unit in s.encode_utf16() {
        h ^= unit as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub fn hash_hex(s: &str) -> String {
    format!("{:08x}", hash32(s))
}

/// `JSON.stringify` of plain data (strings, booleans, numbers, arrays, objects in their key order).
pub fn stringify(v: &Json) -> String {
    fn append(v: &Json, out: &mut String) {
        use std::fmt::Write;
        match v {
            Json::Number(n) => out.push_str(&number_to_string(n.as_f64().unwrap_or(0.0))),
            Json::Array(a) => {
                out.push('[');
                for (i, value) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    append(value, out);
                }
                out.push(']');
            }
            Json::Object(o) => {
                out.push('{');
                for (i, (key, value)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    let _ = write!(out, "{}:", Json::String(key.clone()));
                    append(value, out);
                }
                out.push('}');
            }
            other => {
                let _ = write!(out, "{other}");
            }
        }
    }
    // One output buffer, without subtree strings and join copies at each depth.
    let mut out = String::new();
    append(v, &mut out);
    out
}

/// `v && typeof v === 'object'` as a record (arrays are not records here).
pub fn rec(v: Option<&Json>) -> Option<&Map<String, Json>> {
    match v {
        Some(Json::Object(o)) => Some(o),
        _ => None,
    }
}

pub fn str_of(v: Option<&Json>) -> Option<&str> {
    match v {
        Some(Json::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// A finite number, or the fallback (`typeof v === 'number' && Number.isFinite(v) ? v : fallback`).
pub fn finite_or(v: Option<&Json>, fallback: f64) -> f64 {
    match v {
        Some(Json::Number(n)) => n.as_f64().filter(|x| x.is_finite()).unwrap_or(fallback),
        _ => fallback,
    }
}

/// JavaScript truthiness.
pub fn truthy(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_f64().map(|x| x != 0.0 && !x.is_nan()).unwrap_or(false),
        Some(Json::String(s)) => !s.is_empty(),
        _ => true,
    }
}

/// `Math.round` (halves round up).
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `Math.max(a, b)` (NaN when either is NaN, as in JavaScript).
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Math.min(a, b)` (NaN when either is NaN, as in JavaScript).
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Math.hypot(...values)` as V8 computes it (scaled by the largest term, Kahan summation): not always the
/// correctly rounded `f64::hypot`, and a kit that compares a distance to a threshold it sits on needs the same bits.
#[inline]
pub fn js_hypot(values: &[f64]) -> f64 {
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

/// `Math.hypot(a, b)` (see `js_hypot`).
pub fn js_hypot2(a: f64, b: f64) -> f64 {
    js_hypot(&[a, b])
}

/// `String.prototype.trim`: the JavaScript white space and line terminators (not the Unicode `White_Space` set).
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\u{9}'..='\u{d}'
                | ' '
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
        )
    })
}
