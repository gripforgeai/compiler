//! The edit commands of a scene, in **the format of the site's scene patch** (`gripforge_scene_patch`, the
//! `commands` of `/api/v1/scenes/schema`): `insert`, `remove`, `transform`, `replace`, `update`. The debugging editor
//! (`debug.inspector.edit`) emits them with their `baseRevision` (`changes()`) and keeps no file; the CLI, the MCP, an
//! AI and the editor app use the same. Here they are applied to the
//! project's file and the sub-scenes it names with the smallest diff (crate::jsontext): only the changed values move,
//! the order, the layout and the unknown fields stay (docs/dev-builder.md « L'édition »).
//!
//! Two readings of `id`, so a command of the debugging editor lands in the file: a **node path** (`Hall/Crate`), its
//! transform local to the parent, as the site's nodes; or **the game.engine id of an instance** (`hall_crate`, what the
//! running game knows), its transform in the world, turned back into the node's local transform. A node inside an
//! instanced sub-scene (`Shelf/Book2`) is changed through an **override** of the instance node, as in a Godot scene;
//! inserting or removing there is refused.

#![allow(
    clippy::result_large_err,
    reason = "a refusal is rare and handed to a person or an AI as it is: boxing it buys nothing"
)]

use crate::issue::Issue;
use crate::jsontext::{self, find, Seg, Span};
use crate::lower::id_of;
use crate::math::{to_euler_deg, Transform};
use crate::path::is_name;
use crate::registry::Registry;
use crate::tree::resolve_doc;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

/// The schema of a file of patches (`room`, `baseRevision`, `commands`), read as the file of edits. The debugging
/// editor wrote one until 17/11; `debug.inspector.edit` writes none.
pub const PATCHES_SCHEMA: &str = "gripforge.editor-patches/1";

/// Where a node path lands in a scene file.
enum Located {
    /// The node itself, written in this file.
    Direct(Vec<Seg>),
    /// A node of an instanced sub-scene: the instance node, and the path inside the sub-scene.
    InInstance(Vec<Seg>, String),
}

fn bad(code: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Issue {
    Issue::error(code, message, fix)
}

fn locate(doc: &Json, path: &str) -> Result<Located, Issue> {
    if path.is_empty() {
        return Ok(Located::Direct(Vec::new()));
    }
    let names: Vec<&str> = path.split('/').collect();
    let mut segs: Vec<Seg> = Vec::new();
    let mut node: Option<&Json> = None;
    for (i, name) in names.iter().enumerate() {
        if let Some(n) = node {
            if n.get("type").and_then(Json::as_str) == Some("scene") {
                return Ok(Located::InInstance(segs, names[i..].join("/")));
            }
        }
        let list_key = if node.is_some() { "children" } else { "nodes" };
        let list = node.unwrap_or(doc).get(list_key).and_then(Json::as_array);
        let Some((idx, n)) = list
            .into_iter()
            .flatten()
            .enumerate()
            .find(|(_, n)| n.get("name").and_then(Json::as_str) == Some(name))
        else {
            return Err(bad(
                "node_not_found",
                format!("no node {path} in the file ({name} is not there)"),
                "check the path: the names from the root of the scene, joined by /",
            ));
        };
        segs.push(Seg::Key(list_key.into()));
        segs.push(Seg::Index(idx));
        node = Some(n);
    }
    Ok(Located::Direct(segs))
}

/// Set the value at `keys` under the object at `base`, creating the missing objects on the way.
fn set_path(
    text: &str,
    root: &Span,
    base: &[Seg],
    keys: &[String],
    v: &Json,
) -> Result<String, Issue> {
    let mut at: Vec<Seg> = base.to_vec();
    for (i, k) in keys.iter().enumerate() {
        let here = find(root, &at).ok_or_else(|| {
            bad(
                "op_invalid",
                "the target is gone",
                "apply the edits in order",
            )
        })?;
        if !matches!(here.kind, jsontext::Kind::Object(_)) {
            return Err(bad(
                "op_invalid",
                format!(
                    "{} is not an object: {k} cannot be set in it",
                    keys[..i].join("/")
                ),
                "set the whole value one level up",
            ));
        }
        let mut next = at.clone();
        next.push(Seg::Key(k.clone()));
        if find(root, &next).is_none() || i + 1 == keys.len() {
            let mut value = v.clone();
            for k in keys[i + 1..].iter().rev() {
                value = json!({ k.clone(): value });
            }
            return jsontext::set_key(text, root, &at, k, &value).ok_or_else(|| {
                bad(
                    "op_invalid",
                    "the value cannot be written",
                    "check the file is valid JSON",
                )
            });
        }
        at = next;
    }
    jsontext::replace(text, root, &at, v).ok_or_else(|| {
        bad(
            "op_invalid",
            "the value cannot be written",
            "check the file is valid JSON",
        )
    })
}

fn keys_of(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// One low-level edit (`set`, `move`, `add`, `remove` on a node path) applied to a scene file's text; the new text,
/// or why it is refused (nothing changed). The commands of the site are made of these.
fn edit(text: &str, op: &Json) -> Result<String, Issue> {
    let doc: Json = serde_json::from_str(text).map_err(|e| {
        bad(
            "file_invalid",
            format!("the file is not valid JSON ({e})"),
            "fix the JSON first",
        )
    })?;
    let root = jsontext::parse(text).map_err(|e| bad("file_invalid", e, "fix the JSON first"))?;
    let kind = op.get("op").and_then(Json::as_str).unwrap_or("");
    let node = op.get("node").and_then(Json::as_str).unwrap_or("");
    match kind {
        "set" => {
            let keys = keys_of(op.get("path").and_then(Json::as_str).unwrap_or(""));
            let Some(value) = op.get("value") else {
                return Err(bad("op_invalid", "set has no value", "write \"value\": the new value (null removes nothing: it writes null)"));
            };
            if keys.is_empty() || (!node.is_empty() && matches!(keys[0].as_str(), "name" | "children")) {
                return Err(bad("op_invalid", "set needs a path inside the node, other than name and children", "write \"path\": \"light/intensity\", \"kits/<kit id>/<property>\" or \"visible\"; add and remove change the children"));
            }
            match locate(&doc, node)? {
                Located::Direct(segs) => set_path(text, &root, &segs, &keys, value),
                Located::InInstance(inst, rest) => {
                    let mut k = vec!["overrides".to_string(), rest];
                    k.extend(keys);
                    set_path(text, &root, &inst, &k, value)
                }
            }
        }
        "move" => {
            let mut out = text.to_string();
            let mut any = false;
            for k in ["position", "rotation", "scale"] {
                if let Some(v) = op.get(k) {
                    any = true;
                    out = edit(&out, &json!({ "op": "set", "node": node, "path": k, "value": v }))?;
                }
            }
            if !any || node.is_empty() {
                return Err(bad("op_invalid", "move needs a node and at least one of position, rotation, scale", "write { \"op\": \"move\", \"node\": \"Hall/Crate\", \"position\": [x, y, z] }"));
            }
            Ok(out)
        }
        "add" => {
            let parent = op.get("parent").and_then(Json::as_str).unwrap_or("");
            let Some(new) = op.get("node").filter(|n| n.is_object()) else {
                return Err(bad("op_invalid", "add needs the node to add, an object", "write \"node\": { \"name\": \"Crate\", \"type\": \"model\", ... } and \"parent\": the path of its parent (empty: the root)"));
            };
            let name = new.get("name").and_then(Json::as_str).unwrap_or("");
            if !is_name(name) {
                return Err(bad("name_invalid", format!("the added node's name \"{name}\" is not valid"), "use ASCII letters, digits, _ and -"));
            }
            let (container, owner) = match locate(&doc, parent)? {
                Located::Direct(segs) if segs.is_empty() => (vec![Seg::Key("nodes".into())], segs),
                Located::Direct(segs) => {
                    let mut c = segs.clone();
                    c.push(Seg::Key("children".into()));
                    (c, segs)
                }
                Located::InInstance(..) => return Err(bad("op_in_instance", format!("{parent} is inside an instanced sub-scene: a node cannot be added there"), "add it in the sub-scene's own file, or under the instance node itself")),
            };
            let siblings = find(&root, &container).map(|_| doc_at(&doc, &container)).unwrap_or(Json::Null);
            if siblings.as_array().into_iter().flatten().any(|n| n.get("name").and_then(Json::as_str) == Some(name)) {
                return Err(bad("name_duplicate", format!("{parent}/{name} already exists"), "choose a name unique among its siblings"));
            }
            match find(&root, &container) {
                Some(_) => {
                    let index = op.get("index").and_then(Json::as_u64).map_or(usize::MAX, |i| i as usize);
                    jsontext::insert_item(text, &root, &container, index, new).ok_or_else(|| bad("op_invalid", "the children are not a list", "fix children in the file"))
                }
                None => {
                    let key = if owner.is_empty() { "nodes" } else { "children" };
                    jsontext::set_key(text, &root, &owner, key, &json!([new])).ok_or_else(|| bad("op_invalid", "the parent is not an object", "fix the file"))
                }
            }
        }
        "remove" => match locate(&doc, node)? {
            Located::Direct(segs) if !segs.is_empty() => {
                let (Seg::Index(i), container) = (segs[segs.len() - 1].clone(), &segs[..segs.len() - 1]) else { unreachable!("a node path ends with its index") };
                jsontext::remove_item(text, &root, container, i).ok_or_else(|| bad("op_invalid", "the node cannot be removed", "fix the file"))
            }
            Located::Direct(_) => Err(bad("op_invalid", "remove needs a node", "write \"node\": the path of the node to remove")),
            Located::InInstance(..) => Err(bad("op_in_instance", format!("{node} is inside an instanced sub-scene: it cannot be removed from here"), "hide it with { \"op\": \"set\", \"path\": \"visible\", \"value\": false }, or remove it in the sub-scene's own file")),
        },
        other => Err(bad("op_invalid", format!("the operation \"{other}\" is not known"), "use add, remove, move or set")),
    }
}

fn doc_at(doc: &Json, segs: &[Seg]) -> Json {
    let mut v = doc;
    for s in segs {
        v = match s {
            Seg::Key(k) => &v[k.as_str()],
            Seg::Index(i) => &v[*i],
        };
    }
    v.clone()
}

/// What a command needs besides the file's text: the other scene files (sub-scenes), to find a game.engine id.
pub type Read<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Where a command's `id` is: the node's path, and the transform of its parent in the scene when `id` was a
/// game.engine id (then the command's transform is in the world).
fn target(
    doc: &Json,
    file: &str,
    id: &str,
    read: Read,
) -> Result<(String, Option<Transform>), Issue> {
    if locate(doc, id).is_ok() {
        return Ok((id.to_string(), None));
    }
    let files = |p: &str| read(p).and_then(|t| serde_json::from_str(&t).ok());
    let tree = resolve_doc(file, doc, &Registry::base(), &files);
    let Some(n) = tree.nodes.iter().find(|n| id_of(n) == id) else {
        return Err(bad(
            "node_not_found",
            format!("no node {id} in {file}, neither as a path nor as a game.engine id"),
            "name the node by its path (Hall/Crate) or by the id game.engine gives it (hall_crate)",
        ));
    };
    let parent = tree
        .nodes
        .iter()
        .find(|p| p.path == n.parent)
        .map_or_else(Transform::default, |p| p.world);
    Ok((n.path.clone(), Some(parent)))
}

fn xyz(v: Option<&Json>) -> Option<[f64; 3]> {
    let v = v?;
    Some([
        v.get("x")?.as_f64()?,
        v.get("y")?.as_f64()?,
        v.get("z")?.as_f64()?,
    ])
}

/// A number rounded to a micro-unit, so a file does not get `0.30000000000000004`.
fn tidy(x: f64) -> Json {
    let r = (x * 1e6).round() / 1e6;
    if r.fract() == 0.0 && r.abs() < 1e15 {
        json!(r as i64)
    } else {
        json!(r)
    }
}

/// Two values equal, numbers within a micro-unit (a scale written `2` is the same as `[2, 2, 2]`).
fn same(a: &Json, b: &Json) -> bool {
    let v = |x: &Json| -> Option<[f64; 3]> {
        match x {
            Json::Number(n) => n.as_f64().map(|n| [n; 3]),
            Json::Array(a) if a.len() == 3 => {
                Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
            }
            _ => None,
        }
    };
    matches!((v(a), v(b)), (Some(x), Some(y)) if x.iter().zip(y).all(|(p, q)| (p - q).abs() < 1e-6))
}

fn arr3(v: [f64; 3]) -> Json {
    Json::Array(v.iter().map(|x| tidy(*x)).collect())
}

/// Flatten a partial node into `(path in the node, value)`: objects are merged key by key, the rest replaced.
fn leaves(prefix: &str, v: &Json, out: &mut Vec<(String, Json)>) {
    match v.as_object() {
        Some(o) if !o.is_empty() && !prefix.is_empty() => {
            for (k, x) in o {
                leaves(&format!("{prefix}/{k}"), x, out);
            }
        }
        Some(o) if prefix.is_empty() => {
            for (k, x) in o {
                leaves(k, x, out);
            }
        }
        _ => out.push((prefix.to_string(), v.clone())),
    }
}

/// Apply one command of the site's format (`insert`, `remove`, `transform`, `replace`, `update`) to a scene file.
pub fn apply_command(text: &str, file: &str, cmd: &Json, read: Read) -> Result<String, Issue> {
    let doc: Json = serde_json::from_str(text).map_err(|e| {
        bad(
            "file_invalid",
            format!("the file is not valid JSON ({e})"),
            "fix the JSON first",
        )
    })?;
    let kind = cmd.get("type").and_then(Json::as_str).unwrap_or("");
    let id = cmd.get("id").and_then(Json::as_str).unwrap_or("");
    match kind {
        "transform" => {
            let t = cmd.get("transform").ok_or_else(|| bad("op_invalid", "transform has no transform", "write \"transform\": { \"translation\": {x,y,z}, \"rotation\": {x,y,z,w}, \"scale\": {x,y,z} }"))?;
            let (path, parent) = target(&doc, file, id, read)?;
            let r = t.get("rotation");
            let q = r.and_then(|r| {
                Some([
                    r.get("x")?.as_f64()?,
                    r.get("y")?.as_f64()?,
                    r.get("z")?.as_f64()?,
                    r.get("w")?.as_f64()?,
                ])
            });
            let given = Transform {
                position: xyz(t.get("translation")).unwrap_or_default(),
                rotation: q.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                scale: xyz(t.get("scale")).unwrap_or([1.0; 3]),
            };
            let local = parent.map_or(given, |p| p.local_of(&given));
            // What the node holds now (its defaults when not written): an unchanged key is not written, for the diff.
            let now = match locate(&doc, &path) {
                Ok(Located::Direct(segs)) => doc_at(&doc, &segs),
                _ => Json::Null,
            };
            let mut m = json!({ "op": "move", "node": path });
            for (key, given, value, default) in [
                (
                    "position",
                    t.get("translation").is_some(),
                    arr3(local.position),
                    json!([0, 0, 0]),
                ),
                (
                    "rotation",
                    q.is_some(),
                    arr3(to_euler_deg(local.rotation)),
                    json!([0, 0, 0]),
                ),
                (
                    "scale",
                    t.get("scale").is_some(),
                    arr3(local.scale),
                    json!([1, 1, 1]),
                ),
            ] {
                let current = now.get(key).cloned().unwrap_or(default);
                if given && !same(&current, &value) {
                    m[key] = value;
                }
            }
            if m.as_object().is_some_and(|o| o.len() == 2) {
                return Ok(text.to_string());
            }
            edit(text, &m)
        }
        "update" => {
            let (path, _) = target(&doc, file, id, read)?;
            let Some(node) = cmd.get("node").filter(|n| n.is_object()) else {
                return Err(bad("op_invalid", "update has no node", "write \"node\": the keys to change, { \"visible\": false, \"light\": { \"intensity\": 2 } }"));
            };
            let mut list = Vec::new();
            leaves("", node, &mut list);
            let mut out = text.to_string();
            for (k, v) in list {
                out = edit(
                    &out,
                    &json!({ "op": "set", "node": path, "path": k, "value": v }),
                )?;
            }
            Ok(out)
        }
        "insert" => {
            let Some(mut node) = cmd.get("node").and_then(Json::as_object).cloned() else {
                return Err(bad("op_invalid", "insert has no node", "write \"node\": { \"name\": \"Crate\", \"type\": \"model\", \"parentId\": \"Hall\" }"));
            };
            let parent = node
                .remove("parentId")
                .and_then(|p| p.as_str().map(str::to_string))
                .unwrap_or_default();
            if !node.contains_key("name") {
                if let Some(i) = node.remove("id") {
                    node.insert("name".into(), i);
                }
            }
            let mut op = json!({ "op": "add", "parent": parent, "node": node });
            if let Some(i) = cmd.get("index") {
                op["index"] = i.clone();
            }
            edit(text, &op)
        }
        "remove" => {
            let (path, _) = target(&doc, file, id, read)?;
            edit(text, &json!({ "op": "remove", "node": path }))
        }
        "replace" => {
            let (path, _) = target(&doc, file, id, read)?;
            let asset = cmd.get("asset");
            let Some(p) =
                asset.and_then(|a| a.as_str().or_else(|| a.get("path").and_then(Json::as_str)))
            else {
                return Err(bad("asset_ref_unsupported", "replace names the asset by a Library reference: a project file is named by its path here", "write \"asset\": \"models/<file>.glb\" (the passport of that file stays its identity)"));
            };
            edit(
                text,
                &json!({ "op": "set", "node": path, "path": "model/asset", "value": p }),
            )
        }
        other => Err(bad(
            "op_invalid",
            format!("the command type \"{other}\" is not known"),
            "use insert, remove, transform, replace or update (the site's scene patch commands)",
        )),
    }
}

/// Apply a file of patches (`{ "schema": "gripforge.editor-patches/1", "room", "commands": [...] }`, or a bare list of
/// commands) to the scene files of `project`. Each command goes to its `file` (an extension of the site's format), else
/// to the file of the scene `room` names in `data.scenes`. Returns the new text of every changed file, and the issues
/// of the commands that were refused (each skipped, the others applied in order).
pub fn apply_patches(
    patches: &Json,
    project: &Json,
    read: Read,
) -> (BTreeMap<String, String>, Vec<Issue>) {
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut issues = Vec::new();
    let list = patches
        .get("commands")
        .unwrap_or(patches)
        .as_array()
        .cloned()
        .unwrap_or_default();
    let room = patches.get("room").and_then(Json::as_str);
    let room_file = room.and_then(|r| {
        project
            .pointer("/data/scenes")?
            .as_array()?
            .iter()
            .find(|s| s.get("id").and_then(Json::as_str) == Some(r))?
            .get("file")?
            .as_str()
            .map(str::to_string)
    });
    if list.is_empty() {
        issues.push(Issue::warning("edits_empty", "the file of patches holds no command", format!("write {{ \"schema\": \"{PATCHES_SCHEMA}\", \"room\": \"<scene id>\", \"commands\": [{{ \"type\": \"transform\", \"id\": \"Hall/Crate\", \"transform\": {{ \"translation\": {{ \"x\": 1, \"y\": 0, \"z\": 2 }} }} }}] }}")));
    }
    for (i, cmd) in list.iter().enumerate() {
        let field = format!("commands[{i}]");
        let Some(file) = cmd
            .get("file")
            .and_then(Json::as_str)
            .map(str::to_string)
            .or_else(|| room_file.clone())
        else {
            issues.push(bad("op_invalid", "the command names no file and the room is not a scene of the project with a file", "add \"file\": the path of the sub-scene, or set \"room\" to a scene id of data.scenes").at("", &field));
            continue;
        };
        let Some(current) = texts.get(&file).cloned().or_else(|| read(&file)) else {
            issues.push(
                bad(
                    "scene_file_missing",
                    format!("{file} is not in the project"),
                    "check the path, relative to the project file",
                )
                .in_file(&file)
                .at("", &field),
            );
            continue;
        };
        let lookup = |p: &str| texts.get(p).cloned().or_else(|| read(p));
        match apply_command(&current, &file, cmd, &lookup) {
            Ok(t) => {
                texts.insert(file, t);
            }
            Err(e) => issues.push(
                e.in_file(&file)
                    .at(cmd.get("id").and_then(Json::as_str).unwrap_or(""), &field),
            ),
        }
    }
    (texts, issues)
}
