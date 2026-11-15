//! Computed values of the rule language (`runtime/expr.ts`, game.engine 1.3, docs/mods-engine.md §13 C). An
//! object with exactly one key among `$field $var $add $sub $mul $div $min $max $round` is an expression; anything
//! else is a literal. `None`: no value (missing field, non-numeric operand, division by zero, nesting deeper than 8).

use crate::js::num;
use serde_json::{Map, Value as Json};

const OPS: [&str; 9] = [
    "$field", "$var", "$add", "$sub", "$mul", "$div", "$min", "$max", "$round",
];
const MAX_DEPTH: usize = 8;

pub fn expr_op(v: &Json) -> Option<&str> {
    let o = v.as_object()?;
    if o.len() != 1 {
        return None;
    }
    let k = o.keys().next()?;
    OPS.iter().find(|op| **op == k).copied()
}

/// A value at a dotted path of an object.
pub fn at<'a>(root: &'a Json, path: &str) -> Option<&'a Json> {
    let mut cur = root;
    for k in path.split('.') {
        cur = cur.as_object()?.get(k)?;
    }
    Some(cur)
}

fn round(n: f64, d: f64) -> f64 {
    let f = 10f64.powi(d.trunc().clamp(0.0, 6.0) as i32);
    // Math.round: half up towards +∞.
    (n * f + 0.5).floor() / f
}

pub fn eval_expr(
    v: &Json,
    payload: &Json,
    get: &dyn Fn(&str) -> Option<Json>,
    depth: usize,
) -> Option<Json> {
    let Some(op) = expr_op(v) else {
        return Some(v.clone());
    };
    if depth >= MAX_DEPTH {
        return None;
    }
    let arg = &v[op];
    match op {
        "$field" => return arg.as_str().and_then(|p| at(payload, p)).cloned(),
        "$var" => return arg.as_str().and_then(get),
        _ => {}
    }
    let items: Vec<&Json> = match arg {
        Json::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    let mut n = Vec::with_capacity(items.len());
    for a in items {
        match eval_expr(a, payload, get, depth + 1).and_then(|x| x.as_f64()) {
            Some(x) if x.is_finite() => n.push(x),
            _ => return None,
        }
    }
    if n.is_empty() {
        return None;
    }
    let out = match op {
        "$add" => n.iter().sum(),
        "$sub" => n[1..].iter().fold(n[0], |a, b| a - b),
        "$mul" => n.iter().product(),
        "$div" => {
            if n[1..].contains(&0.0) {
                return None;
            }
            n[1..].iter().fold(n[0], |a, b| a / b)
        }
        "$min" => n.iter().copied().fold(f64::INFINITY, f64::min),
        "$max" => n.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        _ => round(n[0], n.get(1).copied().unwrap_or(0.0)),
    };
    out.is_finite().then(|| num(out))
}

/// Every expression inside a value resolved; `None` when one has no value.
pub fn resolve_exprs(
    v: &Json,
    payload: &Json,
    get: &dyn Fn(&str) -> Option<Json>,
    depth: usize,
) -> Option<Json> {
    if expr_op(v).is_some() {
        return eval_expr(v, payload, get, 0);
    }
    if depth >= 32 {
        return Some(v.clone());
    }
    match v {
        Json::Array(a) => a
            .iter()
            .map(|x| resolve_exprs(x, payload, get, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(Json::Array),
        Json::Object(o) => {
            let mut out = Map::new();
            for (k, x) in o {
                out.insert(k.clone(), resolve_exprs(x, payload, get, depth + 1)?);
            }
            Some(Json::Object(out))
        }
        _ => Some(v.clone()),
    }
}

pub fn has_expr(v: &Json, depth: usize) -> bool {
    if expr_op(v).is_some() {
        return true;
    }
    if depth >= 32 {
        return false;
    }
    match v {
        Json::Array(a) => a.iter().any(|x| has_expr(x, depth + 1)),
        Json::Object(o) => o.values().any(|x| has_expr(x, depth + 1)),
        _ => false,
    }
}
