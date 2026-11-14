//! What a scene asks of the kits when it is entered, decided once for both ways of posing it
//! (docs/design/build-gripforge.md): the dev.builder kit at run time (the « not built » mode of development) and the
//! Rust code `gfscene code` writes ahead of time (the built game). One source, so both give the same scene.
//!
//! - the light and camera nodes: what core.light and core.camera receive ([`light_params`], [`camera_look`],
//!   [`camera_params`]);
//! - the kits' sections: which are written ([`wanted`]), the element and its complete properties ([`resolve_section`]),
//!   the order of the kits ([`ordered`]) and the `spec` of `create` ([`element_spec`]).
//!
//! Moved here from kits/dev_builder/logic (place.rs, sections.rs) unchanged in behaviour.

use crate::issue::Issue;
use crate::math::rotate;
use crate::registry::{At, Element, Registry};
use crate::tree::{check_section, Node, Tree};
use serde_json::{json, Map, Value as Json};

/// The node's -Z axis in the scene (where a light shines and a camera looks).
pub fn forward(n: &Node) -> [f64; 3] {
    rotate(n.world.rotation, [0.0, 0.0, -1.0])
}

/// The params core.light reads for a light node, in its units (only what the node writes, plus the defaults of the
/// node type for range and angle): what a kind does not carry is not given (core.light refuses it). `(kind, params)`.
pub fn light_params(n: &Node) -> (String, Json) {
    let empty = Map::new();
    let p = n
        .raw
        .get("light")
        .and_then(Json::as_object)
        .unwrap_or(&empty);
    let kind = p
        .get("kind")
        .and_then(Json::as_str)
        .unwrap_or("point")
        .to_string();
    let mut out = Map::new();
    for k in ["color", "intensity", "shadows"] {
        if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
            out.insert(k.into(), v.clone());
        }
    }
    let at = json!(n.world.position);
    let toward = json!(forward(n));
    let range = p.get("range").cloned().unwrap_or(json!(20));
    match kind.as_str() {
        "directional" => {
            out.insert("direction".into(), toward);
        }
        "spot" => {
            out.insert("position".into(), at);
            out.insert("direction".into(), toward);
            out.insert("range".into(), range);
            out.insert("outer".into(), p.get("angle").cloned().unwrap_or(json!(45)));
        }
        _ => {
            out.insert("position".into(), at);
            out.insert("range".into(), range);
        }
    }
    (kind, Json::Object(out))
}

/// Where a camera node is and the point it looks at, `ahead` metres along its -Z axis (dev.builder's `lookAhead`).
pub fn camera_look(n: &Node, ahead: f64) -> ([f64; 3], [f64; 3]) {
    let (at, f) = (n.world.position, forward(n));
    (
        at,
        [
            at[0] + f[0] * ahead,
            at[1] + f[1] * ahead,
            at[2] + f[2] * ahead,
        ],
    )
}

/// The field of view of a camera node (60 when not written) and whether it is the one that draws (`current`).
pub fn camera_params(n: &Node) -> (Json, bool) {
    let p = n.raw.get("camera");
    let fov = p.and_then(|p| p.get("fov")).cloned().unwrap_or(json!(60));
    let current = p.and_then(|p| p.get("current")) == Some(&Json::Bool(true));
    (fov, current)
}

/// The last segment of a node's path: its name in the engine's tree.
pub fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// One section written in a scene: where, which kit, as written.
#[derive(Clone, Debug, PartialEq)]
pub struct Wanted {
    /// The node's path; empty for a section of the scene itself.
    pub node: String,
    /// The node's type; empty for the scene.
    pub ty: String,
    /// The project path of the file it was written in.
    pub file: String,
    /// On the scene or on a node.
    pub at: At,
    /// The kit's id.
    pub kit: String,
    /// The section as written.
    pub written: Json,
}

/// The sections of a resolved scene (its own `kits`, then every shown node's, parents first), as written; `file` is
/// the scene's file (the project path its own sections carry in their issues).
pub fn wanted(tree: &Tree, file: &str) -> Vec<Wanted> {
    let mut out: Vec<Wanted> = tree
        .kits
        .iter()
        .map(|(kit, w)| Wanted {
            node: String::new(),
            ty: String::new(),
            file: file.to_string(),
            at: At::Scene,
            kit: kit.clone(),
            written: w.clone(),
        })
        .collect();
    for n in tree.nodes.iter().filter(|n| n.visible) {
        for (kit, w) in n
            .raw
            .get("kits")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            out.push(Wanted {
                node: n.path.clone(),
                ty: n.ty.clone(),
                file: n.file.clone(),
                at: At::Node,
                kit: kit.clone(),
                written: w.clone(),
            });
        }
    }
    out
}

/// The element a section stands for and its properties (defaults of the declaration, then what is written and fits),
/// with the issues of the check; `None` when the kit exposes nothing there.
pub fn resolve_section(reg: &Registry, w: &Wanted) -> Option<(Element, Json, Vec<Issue>)> {
    let (issues, dropped, bad) = check_section(reg, &w.kit, &w.written, &w.ty, w.at);
    let issues: Vec<Issue> = issues
        .into_iter()
        .map(|i| {
            let field = i.field.clone();
            i.in_file(&w.file).at(&w.node, &field)
        })
        .collect();
    let written = w.written.as_object().filter(|_| !dropped)?;
    let el = reg.sections.get(&w.kit)?.pick(w.at, written).ok()?.clone();
    let mut props = Map::new();
    for (k, f) in &el.props {
        let v = written
            .get(k)
            .filter(|_| !bad.contains(k))
            .filter(|v| !v.is_null())
            .unwrap_or(&f.default);
        props.insert(k.clone(), v.clone());
    }
    Some((el, Json::Object(props), issues))
}

/// The kits in the order their elements are made: a kit after those its elements name in `after` (a scenery after its
/// terrain), else in the order of the file.
pub fn ordered(reg: &Registry, kits: &[String]) -> Vec<String> {
    let after = |k: &str| -> Vec<String> {
        reg.sections
            .get(k)
            .into_iter()
            .flat_map(|s| s.elements.iter().flat_map(|e| e.after.clone()))
            .filter(|a| kits.contains(a))
            .collect()
    };
    let mut out: Vec<String> = Vec::new();
    let mut left: Vec<String> = kits.to_vec();
    while !left.is_empty() {
        let i = left
            .iter()
            .position(|k| after(k).iter().all(|a| out.contains(a) || a == k))
            .unwrap_or(0); // a loop of `after`: the file's order breaks it
        out.push(left.remove(i));
    }
    out
}

/// The kits of `all` in the order of their first section.
pub fn kits_of(all: &[Wanted]) -> Vec<String> {
    let mut kits: Vec<String> = Vec::new();
    for w in all {
        if !kits.contains(&w.kit) {
            kits.push(w.kit.clone());
        }
    }
    kits
}

/// What a kit receives to make an element (`create(spec)`): the element, where it is, its properties, the node's
/// resolved transform and the engine node to make it under (`parent`: `null` for the scene's root or an element of
/// the scene).
pub fn element_spec(
    scene: &str,
    w: &Wanted,
    el: &Element,
    props: &Json,
    node: Option<&Node>,
    parent: Json,
) -> Json {
    let transform = node.map_or(Json::Null, |n| {
        json!({ "position": n.world.position, "rotation": n.world.rotation, "scale": n.world.scale })
    });
    let id = node.and_then(|n| n.id.clone());
    json!({ "element": el.name, "scene": scene, "node": w.node, "id": id, "props": props,
        "transform": transform, "parent": parent })
}
