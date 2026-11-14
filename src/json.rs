//! JSON written the way the players and the tools of GripForge read it: one space of indentation, keys in their
//! order, a number that is a whole value written without a fraction (`1`, not `1.0`). What `game.json`, `build.json`,
//! `kit.api.json` and `module.json` of a build are written with.

use serde_json::Value as Json;
use std::fmt::Write as _;

/// `v` indented by one space per level.
pub fn pretty(v: &Json) -> String {
    let mut out = String::new();
    write(&mut out, v, 0);
    out
}

/// A number as JavaScript writes it: whole values without a fraction, others the shortest that reads back.
pub fn number(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() && f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 => {
            format!("{}", f as i64)
        }
        _ => n.to_string(),
    }
}

fn write(out: &mut String, v: &Json, depth: usize) {
    match v {
        Json::Null | Json::Bool(_) | Json::String(_) => out.push_str(&v.to_string()),
        Json::Number(n) => out.push_str(&number(n)),
        Json::Array(a) if a.is_empty() => out.push_str("[]"),
        Json::Object(o) if o.is_empty() => out.push_str("{}"),
        Json::Array(a) => {
            out.push('[');
            for (k, x) in a.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write(out, x, depth + 1);
            }
            newline(out, depth);
            out.push(']');
        }
        Json::Object(o) => {
            out.push('{');
            for (k, (key, x)) in o.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                let _ = write!(out, "{}: ", Json::String(key.clone()));
                write(out, x, depth + 1);
            }
            newline(out, depth);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push(' ');
    }
}
