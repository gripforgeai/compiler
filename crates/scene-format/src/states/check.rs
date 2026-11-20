//! The validator's part for the states of an element (`entity.states`), its reflection (`reflect`) and the render
//! values bound to a state (`entity.bind`): tolerant, every problem a warning with its fix, the fields kept as written
//! (docs/dev-builder.md « Un validateur tolérant »). Run once the tree is resolved, so a rule may name any node of
//! the element's subtree, sub-scenes included. Written new.

use super::{declared, target, to_engine, under, warn, BIND_PROPS, SET_PROPS};
use crate::issue::Issue;
use crate::tree::{Node, Tree};
use serde_json::{Map, Value as Json};

/// The keys of `entity` on a scene node.
const ENTITY_KEYS: &[&str] = &["states", "bind", "sockets"];

/// Adds to `tree.issues` what is wrong in the states, the reflections and the bound values of its nodes.
pub fn check(tree: &mut Tree) {
    let mut found = Vec::new();
    for n in &tree.nodes {
        if let Some(e) = n.raw.get("entity") {
            entity(n, e, &mut found);
        }
        if let Some(v) = n.raw.get("reflect") {
            rules(tree, n, v, &mut found);
        }
        if let Some(b) = n.raw.get("entity").and_then(|e| e.get("bind")) {
            binds(tree, n, b, &mut found);
        }
        entity_links(n, &mut found);
    }
    tree.issues.extend(found);
}

fn entity(n: &Node, raw: &Json, out: &mut Vec<Issue>) {
    let Some(e) = raw.as_object() else {
        out.push(warn(
            n,
            "entity",
            "state_invalid",
            "entity is not an object: no state is read".into(),
            "write \"entity\": { \"states\": { \"door\": { \"values\": [\"closed\", \"open\"] } } }",
        ));
        return;
    };
    for k in e.keys().filter(|k| !ENTITY_KEYS.contains(&k.as_str())) {
        out.push(warn(
            n,
            &format!("entity.{k}"),
            "state_invalid",
            format!("entity.{k} is not read: an entity of a scene node holds its states and bound values"),
            "keep states and bind in entity; the render goes in the node and in reflect",
        ));
    }
    let Some(raw) = e.get("states") else { return };
    let Some(all) = raw.as_object() else {
        out.push(warn(
            n,
            "entity.states",
            "state_invalid",
            "entity.states is not an object of named states: none is read".into(),
            "write \"states\": { \"open\": { \"values\": [false, true] } }",
        ));
        return;
    };
    for (name, why) in to_engine(all).left {
        out.push(warn(n, &format!("entity.states.{name}"), "state_invalid", format!("the state {name} is left out: {why}"),
            "write { \"values\": [...] }, { \"default\": ... } or { \"derived\": { \"var\" | \"component\" | \"expr\" | \"hook\": ... } } (kits/game_engine/README.md)"));
    }
}

fn rules(tree: &Tree, n: &Node, raw: &Json, out: &mut Vec<Issue>) {
    let Some(list) = raw.as_array() else {
        out.push(warn(n, "reflect", "reflect_invalid", "reflect is not a list: no rule is read".into(),
            "write \"reflect\": [{ \"when\": { \"open\": true }, \"set\": { \"Leaf\": { \"rotation\": [0, 90, 0] } } }]"));
        return;
    };
    let empty = Map::new();
    let decls = declared(n).unwrap_or(&empty);
    for (i, rule) in list.iter().enumerate() {
        let at = format!("reflect[{i}]");
        let Some(r) = rule.as_object() else {
            out.push(warn(
                n,
                &at,
                "reflect_invalid",
                format!("{at} is not an object: it is left out"),
                "write { \"when\": {...}, \"set\": {...} }",
            ));
            continue;
        };
        for (k, want) in r
            .get("when")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let Some(d) = decls.get(k) else {
                out.push(warn(n, &format!("{at}.when.{k}"), "reflect_state_unknown",
                    format!("{at} names the state {k}, which this element does not declare: the rule never holds"),
                    "declare the state in entity.states, or fix its name"));
                continue;
            };
            if let Some(vs) = d.get("values").and_then(Json::as_array) {
                if !vs.contains(want) {
                    out.push(warn(
                        n,
                        &format!("{at}.when.{k}"),
                        "reflect_value_unknown",
                        format!("{at} waits for {k} = {want}, which is not one of its values"),
                        &format!(
                            "use one of {} (one value per state: write a rule per value)",
                            Json::Array(vs.clone())
                        ),
                    ));
                }
            }
        }
        let Some(set) = r.get("set").and_then(Json::as_object) else {
            if r.get("transition").is_none() {
                out.push(warn(n, &at, "reflect_invalid", format!("{at} has no set object: it changes nothing"),
                    "write \"set\": { \"<node path under the element, \\\"\\\" for itself>\": { ... } }"));
            }
            continue;
        };
        for (rel, props) in set {
            let path = target(&n.path, rel);
            let Some(node) = tree
                .nodes
                .iter()
                .find(|x| x.path == path && under(&n.path, &x.path))
            else {
                out.push(warn(n, &format!("{at}.set.{rel}"), "reflect_node_unknown",
                    format!("{at} sets the node {rel}, which is not under this element: it is left out"),
                    "write the path of a node under the element (Hinge/Leaf), or \"\" for the element itself"));
                continue;
            };
            for k in props.as_object().into_iter().flatten().map(|p| p.0) {
                if !SET_PROPS.contains(&k.as_str()) && *k != node.ty {
                    out.push(warn(
                        n,
                        &format!("{at}.set.{rel}.{k}"),
                        "reflect_prop_unknown",
                        format!(
                            "{at} sets {k} on {rel}, which a {} node does not have: it is left out",
                            node.ty
                        ),
                        &format!("use one of {}, {}", SET_PROPS.join(", "), node.ty),
                    ));
                }
            }
        }
    }
}

fn binds(tree: &Tree, n: &Node, raw: &Json, out: &mut Vec<Issue>) {
    let Some(list) = raw.as_array() else {
        out.push(warn(n, "entity.bind", "bind_invalid", "entity.bind is not a list: no value is bound".into(),
            "write \"bind\": [{ \"node\": \"Wheel\", \"property\": \"rotation.z\", \"from\": \"angle\" }]"));
        return;
    };
    let empty = Map::new();
    let decls = declared(n).unwrap_or(&empty);
    for (i, b) in list.iter().enumerate() {
        let field = format!("entity.bind[{i}]");
        let property = b.get("property").and_then(Json::as_str).unwrap_or("");
        let from = b.get("from").and_then(Json::as_str).unwrap_or("");
        let rel = b.get("node").and_then(Json::as_str).unwrap_or("");
        let path = target(&n.path, rel);
        let why = if !BIND_PROPS.contains(&property) {
            Some((
                format!("{property:?} cannot follow a state: it is left out"),
                format!("bind one of {}", BIND_PROPS.join(", ")),
            ))
        } else if !decls.contains_key(from) {
            Some((format!("{property} follows the state {from:?}, which this element does not declare: it is left out"),
                "declare the state in entity.states, or fix from".to_string()))
        } else if !tree
            .nodes
            .iter()
            .any(|x| x.path == path && under(&n.path, &x.path))
        {
            Some((format!("{property} is bound on the node {rel}, which is not under this element: it is left out"),
                "write the path of a node under the element (Hinge/Leaf), or leave node out for the element itself".to_string()))
        } else {
            None
        };
        if let Some((message, fix)) = why {
            out.push(warn(n, &field, "bind_invalid", message, &fix));
        }
    }
}

/// The node's own `bind` links its render to its entity (`position`, `yaw` → `"entity"`); a value that follows a
/// state is written in `entity.bind` of its element (the one writing).
fn entity_links(n: &Node, out: &mut Vec<Issue>) {
    let Some(b) = n.raw.get("bind").and_then(Json::as_object) else {
        return;
    };
    for k in b.keys().filter(|k| *k != "position" && *k != "yaw") {
        out.push(warn(
            n,
            &format!("bind.{k}"),
            "bind_invalid",
            format!("bind.{k} is not read: a node's bind links only position and yaw to its entity"),
            "bind a render value to a state in entity.bind of the element: [{ \"node\": \"<path under it>\", \"property\": \"rotation.z\", \"from\": \"angle\" }]",
        ));
    }
}
