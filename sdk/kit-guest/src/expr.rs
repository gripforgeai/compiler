//! Computed values of the rule language (`runtime/expr.ts`, game.engine 1.3, docs/mods-engine.md §13 C). An
//! object with exactly one key among the operators below is an expression; anything else is a literal. `None`: no
//! value (missing field, operand of the wrong type, division by zero, nesting deeper than `MAX_DEPTH`).
//!
//! game.engine 1.31 (`calc`, docs/regles-calculees.md) adds comparisons, logic, the conditional, `clamp`, `abs`,
//! `floor`, `ceil`, `mod` and `coalesce`: game.engine's compiler turns a formula written by the author
//! (`clamp(hp - event.amount, 0, maxHp)`) into this tree once, at load; the host walks it when the action runs.
//! Pure: the same event and the same vars give the same value (replays).
//! Used by: the trigger engine of the core (`triggers.rs`: action values, the `calc` condition), the hooks of
//! game.engine (`hooks.rs`) and its derived states (`computed.rs`).

use crate::js::{num, truthy};
use serde_json::{Map, Value as Json};

const OPS: [&str; 25] = [
    "$field",
    "$var",
    "$add",
    "$sub",
    "$mul",
    "$div",
    "$min",
    "$max",
    "$round",
    // game.engine 1.31
    "$mod",
    "$abs",
    "$floor",
    "$ceil",
    "$clamp",
    "$eq",
    "$ne",
    "$lt",
    "$le",
    "$gt",
    "$ge",
    "$and",
    "$or",
    "$not",
    "$if",
    "$coalesce",
];
/// Deep enough for a formula with a few derived states inlined (game.engine inlines a derived state it reads).
const MAX_DEPTH: usize = 32;

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

/// The operands of an operator: a list, or one value.
fn items(arg: &Json) -> Vec<&Json> {
    match arg {
        Json::Array(a) => a.iter().collect(),
        other => vec![other],
    }
}

/// `==` of the calc language: two numbers by value (1 == 1.0), anything else by JSON equality.
fn same(a: &Json, b: &Json) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
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
    let eval = |x: &Json| eval_expr(x, payload, get, depth + 1);
    match op {
        "$field" => return arg.as_str().and_then(|p| at(payload, p)).cloned(),
        "$var" => return arg.as_str().and_then(get),
        // Logic: lazy, left to right; a value is true unless null, false, 0 or "" (truthy of the rules).
        "$and" | "$or" => {
            let want = op == "$or";
            for x in items(arg) {
                if truthy(Some(&eval(x)?)) == want {
                    return Some(Json::Bool(want));
                }
            }
            return Some(Json::Bool(!want));
        }
        "$not" => return Some(Json::Bool(!truthy(Some(&eval(arg)?)))),
        "$if" => {
            let [c, a, b] = items(arg)[..] else {
                return None;
            };
            return if truthy(Some(&eval(c)?)) {
                eval(a)
            } else {
                eval(b)
            };
        }
        "$coalesce" => {
            let list = items(arg);
            // A last term written `null` is a value: `event.by ?? null` is null when the field is missing, so an
            // optional field (`by`) does not skip the whole action (game.engine 1.31).
            let last_null = list.last().is_some_and(|x| x.is_null());
            return list
                .into_iter()
                .find_map(|x| eval(x).filter(|v| !v.is_null()))
                .or(last_null.then_some(Json::Null));
        }
        "$eq" | "$ne" => {
            let [a, b] = items(arg)[..] else {
                return None;
            };
            let eq = same(&eval(a)?, &eval(b)?);
            return Some(Json::Bool(eq == (op == "$eq")));
        }
        _ => {}
    }
    let mut n = Vec::with_capacity(4);
    for a in items(arg) {
        match eval(a).and_then(|x| x.as_f64()) {
            Some(x) if x.is_finite() => n.push(x),
            _ => return None,
        }
    }
    if n.is_empty() {
        return None;
    }
    let pair = || (n.len() == 2).then(|| (n[0], n[1]));
    let out = match op {
        "$lt" | "$le" | "$gt" | "$ge" => {
            let (a, b) = pair()?;
            let r = match op {
                "$lt" => a < b,
                "$le" => a <= b,
                "$gt" => a > b,
                _ => a >= b,
            };
            return Some(Json::Bool(r));
        }
        "$add" => n.iter().sum(),
        "$sub" => n[1..].iter().fold(n[0], |a, b| a - b),
        "$mul" => n.iter().product(),
        "$div" => {
            if n[1..].contains(&0.0) {
                return None;
            }
            n[1..].iter().fold(n[0], |a, b| a / b)
        }
        "$mod" => {
            let (a, b) = pair()?;
            if b == 0.0 {
                return None;
            }
            a % b
        }
        "$min" => n.iter().copied().fold(f64::INFINITY, f64::min),
        "$max" => n.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "$abs" => n[0].abs(),
        "$floor" => n[0].floor(),
        "$ceil" => n[0].ceil(),
        "$clamp" => {
            let [x, lo, hi] = n[..] else {
                return None;
            };
            if lo > hi {
                return None;
            }
            x.max(lo).min(hi)
        }
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

/// The vars an expression reads (`$var` paths), each once, in order: what a derived state depends on.
pub fn vars_read(v: &Json, out: &mut Vec<String>) {
    match v {
        Json::Object(o) => {
            if let Some(p) = o
                .get("$var")
                .and_then(Json::as_str)
                .filter(|_| o.len() == 1)
            {
                if !out.iter().any(|x| x == p) {
                    out.push(p.to_string());
                }
                return;
            }
            o.values().for_each(|x| vars_read(x, out));
        }
        Json::Array(a) => a.iter().for_each(|x| vars_read(x, out)),
        _ => {}
    }
}
