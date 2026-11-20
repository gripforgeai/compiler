//! Lower a resolved scene into what game.engine already reads in an inline `data.scenes` document: its instances
//! (model and shape nodes, with an archetype made from the node when none is named), entries, the zones of world.zones (`data.zones`), its scene-level
//! keys (`world`, `data`, `layers`, `audio`, `ui`, `native`) and one script holding the scene's bindings. game.engine
//! then compiles it exactly as an inline scene: the same rules, hooks, saves and mods (docs/dev-builder.md).

use crate::issue::Issue;
use crate::path::{engine_id, is_engine_id, resolve};
use crate::registry::Registry;
use crate::tree::{Node, Tree};
use serde_json::{json, Map, Value as Json};

/// The game.engine section of a node or a scene.
pub const ENGINE: &str = "game.engine";
/// The scene-level keys of game.engine's scene document that a scene file holds (in `kits."game.engine"`).
pub const SCENE_KEYS: &[&str] = &["world", "data", "layers", "audio", "ui", "native"];

/// What game.engine receives from one scene file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lowered {
    /// The content keys of the scene document (`entries`, `instances`, `data` and the scene-level keys).
    pub content: Map<String, Json>,
    /// Archetypes made from model and shape nodes that name none (`gfscene_<id>`).
    pub archetypes: Vec<Json>,
    /// The script of the scene's bindings (`gfscene_<scene>`), when it has any.
    pub scripts: Vec<Json>,
    /// What was found while lowering.
    pub issues: Vec<Issue>,
}

fn round(x: f64) -> Json {
    // Keep integers integers (the documents game.engine compiles are compared by value in its traces).
    if x.fract() == 0.0 && x.abs() < 1e15 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

fn at(n: &Node) -> Json {
    Json::Array(n.world.position.iter().map(|v| round(*v)).collect())
}

/// The game.engine identifier of a node: its section's `id`, else its own `id`, else its path.
pub fn id_of(n: &Node) -> String {
    n.raw
        .get("kits")
        .and_then(|k| k.get(ENGINE))
        .and_then(|g| g.get("id"))
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| n.id.clone())
        .unwrap_or_else(|| engine_id(&n.path))
}

/// Whether a kit section on the node places it (so game.engine must not).
fn placed_by_kit(n: &Node, reg: &Registry) -> bool {
    n.raw
        .get("kits")
        .and_then(Json::as_object)
        .is_some_and(|k| {
            k.keys()
                .any(|kit| kit != ENGINE && reg.sections.get(kit).is_some_and(|s| s.places()))
        })
}

/// Lower the scene `scene` (its id in the project) resolved in `tree`.
pub fn lower(scene: &str, tree: &Tree, reg: &Registry) -> Lowered {
    let mut out = Lowered::default();
    let (mut instances, mut entries, mut zones, mut rules) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut ids: Vec<(String, String, String)> = Vec::new();
    // The game.engine id of each node lowered as an instance or an element, by path: what a child follows.
    let mut holders: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    if let Some(ge) = tree.kits.get(ENGINE).and_then(Json::as_object) {
        for k in SCENE_KEYS {
            if let Some(v) = ge.get(*k) {
                out.content.insert((*k).to_string(), v.clone());
            }
        }
    }
    for n in &tree.nodes {
        let mut lowers = reg
            .node_types
            .get(&n.ty)
            .map_or("none", |t| t.lowers.as_str());
        let states = crate::states::declared(n).map(|d| crate::states::to_engine(d).decls);
        if lowers == "none" && states.is_some() {
            // An element with states and no instance of its own (a group, a light...): a light entity that only holds
            // its states (docs/entites-et-rendu.md, rule 2); its render stays the node's.
            lowers = "element";
        }
        if lowers == "none" || !n.visible || placed_by_kit(n, reg) {
            continue;
        }
        let mut id = id_of(n);
        let iss = |i: Issue| i.in_file(&n.file).at(&n.path, "");
        if lowers != "zone" && !is_engine_id(&id) {
            let by_path = engine_id(&n.path);
            out.issues.push(iss(Issue::warning(
                "id_invalid",
                format!(
                    "the game.engine id {id} is not valid: {by_path}, made from the path, is used"
                ),
                "give kits.game.engine.id a lowercase id (letters, digits, _), or rename the node",
            )));
            id = by_path;
        }
        let kind = if lowers == "instance" || lowers == "element" {
            "instance"
        } else {
            lowers
        };
        if let Some((_, _, other)) = ids.iter().find(|(k, i, _)| *i == id && k == kind) {
            // Tolerant: the second node is still placed, under a suffixed id.
            let first = id.clone();
            let mut k = 2;
            while ids.iter().any(|(kk, i, _)| *i == id && kk == kind) {
                id = format!("{first}_{k}");
                k += 1;
            }
            out.issues.push(iss(Issue::warning(
                "id_collision",
                format!("this node and {other} both give the game.engine id {first}: this one gets {id}"),
                "rename one of the nodes, or give one an explicit id",
            )));
        }
        ids.push((kind.to_string(), id.clone(), n.path.clone()));
        let yaw = n.world.yaw_only();
        if lowers != "zone" && lowers != "element" && yaw.is_none() {
            out.issues.push(iss(Issue::warning("rotation_dropped", "game.engine keeps the yaw only: the pitch and roll of this node (or of its parents) are not applied", "rotate the node around Y only, or let a kit that reads full rotations place it")));
        }
        let mut ge = n
            .raw
            .get("kits")
            .and_then(|k| k.get(ENGINE))
            .and_then(Json::as_object)
            .cloned()
            .unwrap_or_default();
        ge.remove("id");
        match lowers {
            "instance" => {
                if !n.world.unscaled() {
                    out.issues.push(iss(Issue::warning("scale_dropped", "game.engine instances have no scale: the scale of this node (or of its parents) is not applied", "size the shape itself (size, radius, height), or scale the model file")));
                }
                let mut inst = Map::new();
                inst.insert("id".into(), json!(id));
                if !ge.contains_key("archetype") {
                    match archetype_of(n, &id) {
                        Ok(mut a) => {
                            if let Some(s) = &states {
                                a["states"] = Json::Object(s.clone());
                            }
                            inst.insert("archetype".into(), a["id"].clone());
                            out.archetypes.push(a);
                        }
                        Err(i) => {
                            out.issues.push(iss(*i));
                            continue;
                        }
                    }
                }
                if states.is_some() && ge.contains_key("archetype") {
                    out.issues.push(iss(Issue::warning("states_ignored", "this node names its archetype: the states it declares are not read, the archetype's are", "declare the states on the archetype, or remove kits.game.engine.archetype so the node's own archetype carries them")));
                }
                for (k, v) in ge {
                    if k != "at" && k != "yaw" {
                        inst.insert(k, v);
                    }
                }
                inst.insert("at".into(), at(n));
                if let Some(y) = yaw.filter(|y| *y != 0.0) {
                    inst.insert("yaw".into(), json!(y));
                }
                // The points others hang on (docs/attache-sur-un-os.md), as game.engine reads them on an instance.
                if let Some(s) = n.raw.get("entity").and_then(|e| e.get("sockets")) {
                    inst.insert("sockets".into(), s.clone());
                }
                // A child of a node that has its own entity hangs under it (docs/entites-et-rendu.md, parent /
                // child: at the render by default): drawn where its parent's view is, so a crate pushed by its
                // body takes its planks along. Its own entity keeps the place the file gives it.
                if let Some(p) = holder_of(n, &holders) {
                    inst.insert("parent".into(), json!(p));
                }
                holders.insert(n.path.as_str(), id.clone());
                instances.push(Json::Object(inst));
            }
            "element" => {
                let arch = format!("gfscene_{id}");
                let states = states.clone().unwrap_or_default();
                out.archetypes
                    .push(json!({ "id": arch, "kind": "prop", "states": states }));
                let mut inst = Map::new();
                inst.insert("id".into(), json!(id));
                inst.insert("archetype".into(), json!(arch));
                inst.insert("at".into(), at(n));
                // Its place on the ground, when the node says it (`kits.game.engine.place`: `none` keeps the place the file
                // gives a raised group, a vase on its pedestal); game.engine's default otherwise.
                if let Some(p) = ge.get("place") {
                    inst.insert("place".into(), p.clone());
                }
                if let Some(y) = yaw.filter(|y| *y != 0.0) {
                    inst.insert("yaw".into(), json!(y));
                }
                // The points others hang on (docs/attache-sur-un-os.md), as game.engine reads them on an instance.
                if let Some(s) = n.raw.get("entity").and_then(|e| e.get("sockets")) {
                    inst.insert("sockets".into(), s.clone());
                }
                holders.insert(n.path.as_str(), id.clone());
                instances.push(Json::Object(inst));
            }
            "entry" | "zone" if states.is_some() => {
                out.issues.push(iss(Issue::warning("states_not_kept", "an entry or a zone of game.engine holds no states: the states of this node are not read", "put the states on a group above it, or on a model or shape node")));
                if lowers == "zone" {
                    zones.push(zone_of(n, &id, ge));
                } else {
                    entries.push(entry_of(n, &id, yaw));
                }
            }
            "entry" => entries.push(entry_of(n, &id, yaw)),
            "zone" => zones.push(zone_of(n, &id, ge)),
            _ => {}
        }
        for (i, b) in n
            .raw
            .get("on")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            match rule_of(b, n, &id, tree) {
                Some(r) => rules.push(r),
                None => out.issues.push(iss(Issue::warning(
                    "binding_invalid",
                    format!("on[{i}] cannot be given to game.engine"),
                    "check its when and do (docs/ecrire-un-projet.md, the bindings)",
                ))),
            }
        }
    }
    for (i, b) in tree.on.iter().enumerate() {
        let when = b.get("when").and_then(Json::as_str).unwrap_or("");
        let cond = match when {
            "entered" => json!({ "scene": "entered" }),
            "left" => json!({ "scene": "left" }),
            "event" => json!({ "event": b.get("event").cloned().unwrap_or(Json::Null) }),
            _ => {
                out.issues.push(
                    Issue::warning(
                        "binding_invalid",
                        format!(
                            "the scene's on[{i}].when is \"{when}\", which a scene does not have"
                        ),
                        "use entered, left or event",
                    )
                    .at("", "on"),
                );
                continue;
            }
        };
        rules.push(rule(cond, b, None, tree));
    }
    for (k, v) in [("entries", entries), ("instances", instances)] {
        if !v.is_empty() {
            out.content.insert(k.into(), Json::Array(v));
        }
    }
    // The trigger zones are the collection `zones` of the kit world.zones, per scene (`data.zones`).
    if !zones.is_empty() {
        let data = out.content.entry("data").or_insert_with(|| json!({}));
        if let Some(d) = data.as_object_mut() {
            let list = d.entry("zones").or_insert_with(|| json!([]));
            if let Some(l) = list.as_array_mut() {
                l.extend(zones);
            }
        }
    }
    if !rules.is_empty() {
        out.scripts.push(json!({ "id": format!("gfscene_{scene}"), "on": format!("scene:{scene}"), "rules": rules }));
    }
    out
}

/// The game.engine id of the nearest ancestor of `n` that was lowered as an instance or an element.
fn holder_of(n: &Node, holders: &std::collections::HashMap<&str, String>) -> Option<String> {
    let mut path = n.parent.as_str();
    while !path.is_empty() {
        if let Some(id) = holders.get(path) {
            return Some(id.clone());
        }
        path = path.rsplit_once('/').map_or("", |(up, _)| up);
    }
    None
}

/// An entry of game.engine: where it is and its yaw.
fn entry_of(n: &Node, id: &str, yaw: Option<f64>) -> Json {
    let mut e = Map::new();
    e.insert("id".into(), json!(id));
    e.insert("at".into(), at(n));
    if let Some(y) = yaw.filter(|y| *y != 0.0) {
        e.insert("yaw".into(), json!(y));
    }
    Json::Object(e)
}

/// An archetype made from a model or shape node that names none.
fn archetype_of(n: &Node, id: &str) -> Result<Json, Box<Issue>> {
    let props = n
        .raw
        .get(&n.ty)
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let mut asset = Map::new();
    if n.ty == "model" {
        let Some(file) = props
            .get("asset")
            .and_then(Json::as_str)
            .filter(|s| !s.is_empty())
        else {
            return Err(Box::new(Issue::warning("prop_invalid", "a model node has no asset: it is left out, the rest of the scene loads", "write \"model\": { \"asset\": \"models/<file>.glb\" }, the path of the file in the project").at(&n.path, "model.asset")));
        };
        asset.insert("slot".into(), json!(file));
    } else {
        asset.insert(
            "shape".into(),
            props.get("shape").cloned().unwrap_or(json!("box")),
        );
        for k in ["size", "radius", "height", "tube", "color"] {
            if let Some(v) = props.get(k).filter(|v| !v.is_null()) {
                asset.insert(k.into(), v.clone());
            }
        }
    }
    if let Some(m) = props.get("material").filter(|v| !v.is_null()) {
        asset.insert("material".into(), m.clone());
    }
    let mut a = json!({ "id": format!("gfscene_{id}"), "kind": "prop", "asset": asset });
    if n.ty == "model" {
        // A model of a scene file is drawn as authored: the origin and the units of its file (a tree, a house),
        // never centred on its box like a loose prop (player/engine/src/views/fit.rs, `native`).
        a["components"] = json!({ "model.placement": { "mode": "native" } });
    }
    Ok(a)
}

fn zone_of(n: &Node, id: &str, ge: Map<String, Json>) -> Json {
    let props = n
        .raw
        .get("trigger")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let boxed = props.get("shape").and_then(Json::as_str) == Some("box");
    let mut z = Map::new();
    z.insert("id".into(), json!(id));
    z.insert("shape".into(), json!(if boxed { "box" } else { "sphere" }));
    z.insert("center".into(), at(n));
    if boxed {
        z.insert(
            "size".into(),
            props.get("size").cloned().unwrap_or(json!([10, 4, 10])),
        );
    } else {
        z.insert(
            "radius".into(),
            props.get("radius").cloned().unwrap_or(json!(5)),
        );
    }
    for k in ["tags", "watch"] {
        if let Some(v) = props
            .get(k)
            .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
        {
            z.insert(k.into(), v.clone());
        }
    }
    for (k, v) in ge {
        z.entry(k).or_insert(v);
    }
    Json::Object(z)
}

fn rule_of(b: &Json, n: &Node, id: &str, tree: &Tree) -> Option<Json> {
    let cond = match b.get("when").and_then(Json::as_str)? {
        "enter" => json!({ "event": "world.zones.entered", "match": { "zone": id } }),
        "leave" => json!({ "event": "world.zones.left", "match": { "zone": id } }),
        "interact" => json!({ "interact": id }),
        "event" => json!({ "event": b.get("event")?.as_str()? }),
        _ => return None,
    };
    Some(rule(cond, b, Some((n, id)), tree))
}

/// One game.engine rule: the condition, the verbs (their `on` paths turned into game.engine ids), `if`, `else`, `once`.
fn rule(cond: Json, b: &Json, from: Option<(&Node, &str)>, tree: &Tree) -> Json {
    let verbs = |k: &str| -> Option<Json> {
        let list = b.get(k)?.as_array()?;
        Some(Json::Array(
            list.iter().map(|v| retarget(v, from, tree)).collect(),
        ))
    };
    let mut r = json!({ "when": cond, "then": verbs("do").unwrap_or(json!([])) });
    for k in ["if", "else"] {
        if let Some(v) = verbs(k) {
            r[k] = v;
        }
    }
    if b.get("once") == Some(&Json::Bool(true)) {
        r["once"] = json!(true);
    }
    r
}

/// A verb whose `on` names a node by path (`../Crate`, `Hall/Crate`) gets that node's game.engine id. On a node's own
/// binding, `set_state` and the condition `state` aim at the node's entity when they name none, and the short
/// condition `{ "state": "door", "eq": "closed" }` (docs/entites-et-rendu.md) is written as game.engine reads it.
fn retarget(verb: &Json, from: Option<(&Node, &str)>, tree: &Tree) -> Json {
    let mut v = verb.clone();
    if let (Some((_, own)), Some(o)) = (from, v.as_object_mut()) {
        if let Some(name) = o.get("state").and_then(Json::as_str).map(str::to_string) {
            let value = o.remove("eq").unwrap_or(Json::Bool(true));
            o.insert("state".into(), json!({ name: value }));
        }
        if (o.contains_key("set_state") || o.get("state").is_some_and(Json::is_object))
            && !o.contains_key("on")
        {
            o.insert("on".into(), json!(own));
        }
    }
    let from = from.map(|(n, _)| n);
    if let Some(target) = v.get("on").and_then(Json::as_str) {
        let base = from.map_or("", |n| n.path.as_str());
        // By path first, else by the node's id (its own, or the one game.engine gets).
        let by_path = resolve(base, target).and_then(|p| tree.nodes.iter().find(|n| n.path == p));
        if let Some(n) = by_path.or_else(|| {
            tree.nodes
                .iter()
                .find(|n| n.id.as_deref() == Some(target) || id_of(n) == target)
        }) {
            v["on"] = json!(id_of(n));
        }
    }
    v
}
