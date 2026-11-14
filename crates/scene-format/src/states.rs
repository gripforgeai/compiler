//! The states of an element of a scene and its reflection, written as docs/entites-et-rendu.md and game.engine write
//! them (one writing): `entity.states` (game.engine's declarations: `{ values, default? }`, `{ default }`,
//! `{ derived: { var | component | expr | hook } }`), `entity.bind` (`[{ node?, property, from, factor? }]`: a render
//! value of a node of the subtree follows a number state) and `reflect` (`[{ when, set, transition? }]`: `when` names
//! states, `set` names nodes of the element's subtree, `""` the element itself, every rule that holds in order, the
//! last one wins on a property). Written new (no source to copy).
//!
//! The states live in the game.engine entity (`archetype.states`): the lowering puts `entity.states` as they are on
//! the archetype of the node's instance (game.engine checks them). The rules are resolved by game.engine's own
//! resolver (the crate `gfkit-game-engine-resolve`), node by node, in the dev.builder kit: no second resolver
//! here, only what reads and checks the file.
//!
//! Who uses it: the validator (`tree.rs`, [`check`]), the lowering (`lower.rs`), the dev.builder kit (`reflect.rs`).

use crate::issue::Issue;
use crate::tree::{Node, Tree};
use serde_json::{Map, Value as Json};

mod check;

pub use check::check;

/// What a rule's `set` may change on a node, besides the properties of the node's own type (`light`, `shape`...):
/// game.engine's fields (`visible`, the pose `rotation` / `scale` / `offset`, the model `material`, `anim`, `slot`,
/// `shape`, `morphs`, `fade`), and what only a node of a scene has (`light`, `play`, `playing`, `spin`, `on`).
pub const SET_PROPS: &[&str] = &[
    "visible", "rotation", "scale", "offset", "material", "anim", "slot", "shape", "morphs",
    "fade", "light", "play", "playing", "spin", "on",
];

/// The render values a node may bind to a number state (`entity.bind[].property`), game.engine's: degrees for a
/// rotation axis, a factor for a scale, metres for an offset; laid under the node's pose in the file.
pub const BIND_PROPS: &[&str] = &[
    "rotation.x",
    "rotation.y",
    "rotation.z",
    "scale",
    "scale.x",
    "scale.y",
    "scale.z",
    "offset.x",
    "offset.y",
    "offset.z",
];

/// The sources a derived state reads (game.engine's).
pub const DERIVED_SOURCES: &[&str] = &["var", "component", "expr", "hook"];

/// The states a node declares, when it declares any (`entity.states`, a non-empty object).
pub fn declared(n: &Node) -> Option<&Map<String, Json>> {
    n.raw
        .get("entity")
        .and_then(|e| e.get("states"))
        .and_then(Json::as_object)
        .filter(|s| !s.is_empty())
}

/// The rules of the node's reflection (`reflect`), empty when it has none.
pub fn rules(n: &Node) -> &[Json] {
    n.raw
        .get("reflect")
        .and_then(Json::as_array)
        .map_or(&[], Vec::as_slice)
}

/// The render values bound to a state (`entity.bind`), empty when it has none.
pub fn binds(n: &Node) -> &[Json] {
    n.raw
        .get("entity")
        .and_then(|e| e.get("bind"))
        .and_then(Json::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Whether the node is an element: it declares states, a reflection or bound values.
pub fn is_element(n: &Node) -> bool {
    declared(n).is_some() || !rules(n).is_empty() || !binds(n).is_empty()
}

/// The states of a node in game.engine's form, from [`to_engine`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Engine {
    /// The declarations game.engine reads (`archetype.states`), as written.
    pub decls: Map<String, Json>,
    /// The game variables the derived states read (`derived.var`, the `var` of `derived.expr`), to watch them.
    pub vars: Vec<String>,
    /// The states left out, with the reason.
    pub left: Vec<(String, String)>,
}

/// The declarations of `entity.states` as game.engine reads them (the same writing, kept as written), and the game
/// variables the derived ones read. A declaration that game.engine would refuse is left out with the reason (the
/// validator says it, [`check`]).
pub fn to_engine(states: &Map<String, Json>) -> Engine {
    let mut out = Engine::default();
    for (name, decl) in states {
        match engine_state(name, decl) {
            Ok(vars) => {
                out.decls.insert(name.clone(), decl.clone());
                for v in vars {
                    if !out.vars.contains(&v) {
                        out.vars.push(v);
                    }
                }
            }
            Err(e) => out.left.push((name.clone(), e)),
        }
    }
    out
}

/// A name of state: letters, digits and `_` (game.engine's rule).
pub fn is_state_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Checks one declaration as game.engine does (`sanitize_state`), and gives the game variables it reads.
fn engine_state(name: &str, decl: &Json) -> Result<Vec<String>, String> {
    if !is_state_name(name) {
        return Err("a state name is made of letters, digits and _".into());
    }
    let Some(o) = decl.as_object() else {
        return Err("a state is an object: { values }, { default } or { derived }".into());
    };
    if let Some(d) = o.get("derived") {
        if o.contains_key("values") || o.contains_key("default") {
            return Err("a derived state has no values nor default: its source gives it".into());
        }
        let src = d.as_object().filter(|s| s.len() == 1);
        let Some((k, v)) = src
            .and_then(|s| s.iter().next())
            .filter(|(k, _)| DERIVED_SOURCES.contains(&k.as_str()))
        else {
            return Err(
                "derived takes one source: { var }, { component }, { expr } or { hook: true }"
                    .into(),
            );
        };
        let mut vars = Vec::new();
        let ok = match k.as_str() {
            "var" => v
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| vars.push(s.to_string()))
                .is_some(),
            "component" => {
                v.as_str().is_some_and(|s| !s.is_empty())
                    || v.get("name")
                        .and_then(Json::as_str)
                        .is_some_and(|s| !s.is_empty())
            }
            "expr" => expr_vars(v, 0, &mut vars),
            _ => v == &Json::Bool(true),
        };
        return if ok {
            Ok(vars)
        } else {
            Err(format!("derived.{k} is not valid"))
        };
    }
    if let Some(values) = o.get("values") {
        let list = values
            .as_array()
            .filter(|l| !l.is_empty())
            .ok_or("values is a non-empty list")?;
        if let Some(d) = o.get("default").filter(|d| !list.contains(d)) {
            return Err(format!("default {d} is not one of its values"));
        }
    }
    Ok(Vec::new())
}

/// Whether `e` is an expression game.engine computes (`{ var, op?, value? }`, `{ all }`, `{ any }`, `{ not }`, a
/// constant), collecting the variables it reads.
fn expr_vars(e: &Json, depth: usize, vars: &mut Vec<String>) -> bool {
    const OPS: [&str; 6] = ["==", "!=", "<", "<=", ">", ">="];
    if depth > 8 {
        return false;
    }
    let Some(o) = e.as_object() else {
        return !e.is_null();
    };
    if let Some(v) = o.get("var") {
        let op_ok = o
            .get("op")
            .is_none_or(|op| op.as_str().is_some_and(|op| OPS.contains(&op)));
        let Some(v) = v.as_str().filter(|s| !s.is_empty()) else {
            return false;
        };
        if !vars.iter().any(|x| x == v) {
            vars.push(v.to_string());
        }
        return op_ok;
    }
    for k in ["all", "any"] {
        if let Some(list) = o.get(k) {
            return list
                .as_array()
                .is_some_and(|l| l.iter().all(|x| expr_vars(x, depth + 1, vars)));
        }
    }
    o.get("not").is_some_and(|n| expr_vars(n, depth + 1, vars))
}

/// The value of each declared state when the game has not set it: its `default`, else the first of its `values`
/// (derived states: none, their source gives them).
pub fn defaults(states: &Map<String, Json>) -> Map<String, Json> {
    states
        .iter()
        .filter(|(_, d)| d.get("derived").is_none())
        .map(|(k, d)| {
            let v = d
                .get("default")
                .cloned()
                .or_else(|| d.get("values").and_then(|v| v.get(0)).cloned())
                .unwrap_or(Json::Null);
            (k.clone(), v)
        })
        .collect()
}

/// The path in the scene of a node a rule's `set` (or a bound value's `node`) names, relative to the element at
/// `element`.
pub fn target(element: &str, rel: &str) -> String {
    match (element.is_empty(), rel.is_empty()) {
        (_, true) => element.to_string(),
        (true, false) => rel.to_string(),
        _ => format!("{element}/{rel}"),
    }
}

/// Whether `path` is the element at `element` or under it.
pub fn under(element: &str, path: &str) -> bool {
    path == element
        || path
            .strip_prefix(element)
            .is_some_and(|r| r.starts_with('/'))
}

/// A warning about the node `n`, at its `field` (used by [`check`]).
pub(crate) fn warn(n: &Node, field: &str, code: &'static str, message: String, fix: &str) -> Issue {
    Issue::warning(code, message, fix)
        .at(&n.path, field)
        .in_file(&n.file)
}

/// The index of each shown element of a tree, in its order (parents before children).
pub fn elements(tree: &Tree) -> Vec<usize> {
    tree.nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.visible && is_element(n))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests;
