//! The variables and the `$var` of the nodes of an instanced sub-scene (a `scene` node naming a file of
//! `data.scene_files`): one set per instance, named as game.engine names the instance's nodes (`<instance>_<id>.<name>`,
//! `shelf_book.open`), and the `visible` bound to them laid on the instance as an override (the file is shared by every
//! instance, so nothing per instance is written in it). Read by [`super::resolve_project_with`].

use super::{bind_node_visible, var_ref};
use crate::issue::Issue;
use crate::path::engine_id;
use crate::tree::merge;
use serde_json::{json, Map, Value as Json};
use std::collections::BTreeSet;

/// The deepest chain of instances read (a loop is the tree's `scene_cycle`, here simply cut).
const MAX_NESTING: usize = 16;

/// The paths of `data.scene_files` some `scene` node instances.
pub(crate) fn instanced_files(p: &Json) -> BTreeSet<String> {
    fn scan(nodes: &Json, out: &mut BTreeSet<String>) {
        for n in nodes.as_array().into_iter().flatten() {
            if n.get("type") == Some(&json!("scene")) {
                if let Some(s) = n.get("scene").and_then(Json::as_str) {
                    out.insert(s.to_string());
                }
            }
            scan(&n["children"], out);
        }
    }
    let mut out = BTreeSet::new();
    for s in p
        .pointer("/data/scenes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        scan(&s["nodes"], &mut out);
    }
    for d in p
        .pointer("/data/scene_files")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
        .map(|(_, d)| d)
    {
        scan(&d["nodes"], &mut out);
    }
    out
}

/// Read every instance of the project: its nodes' variables (given back, named per instance) and its bound `visible`
/// (laid in the instance node's `overrides`); then the instanced files lose their `variables` and bound `visible`.
pub(crate) fn instances(p: &mut Json, issues: &mut Vec<Issue>) -> Vec<(String, Json)> {
    let instanced = instanced_files(p);
    if instanced.is_empty() {
        return Vec::new();
    }
    let files = p
        .pointer("/data/scene_files")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let mut found = Vec::new();
    let mut top: Vec<&mut Json> = Vec::new();
    if let Some(data) = p.get_mut("data").and_then(Json::as_object_mut) {
        for (k, v) in data.iter_mut() {
            match k.as_str() {
                "scenes" => top.extend(
                    v.as_array_mut()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| s.get_mut("nodes")),
                ),
                "scene_files" => top.extend(
                    v.as_object_mut()
                        .into_iter()
                        .flatten()
                        .filter(|(k, _)| !instanced.contains(k.as_str()))
                        .filter_map(|(_, d)| d.get_mut("nodes")),
                ),
                _ => {}
            }
        }
    }
    for list in top {
        walk_instances(list, "", &files, &mut found, issues);
    }
    if let Some(sf) = p
        .pointer_mut("/data/scene_files")
        .and_then(Json::as_object_mut)
    {
        for (k, d) in sf.iter_mut() {
            if instanced.contains(k.as_str()) {
                if let Some(n) = d.get_mut("nodes") {
                    super::walk_nodes(n, &mut |o| {
                        o.remove("variables");
                        if o.get("visible").and_then(var_ref).is_some() {
                            o.remove("visible");
                        }
                    });
                }
            }
        }
    }
    found
}

/// The variables read per instance, added to the root `variables` (a name already there keeps its value).
pub(crate) fn add_variables(p: &mut Json, found: Vec<(String, Json)>) {
    if found.is_empty() {
        return;
    }
    if !p.get("variables").is_some_and(Json::is_object) {
        p["variables"] = json!({});
    }
    let vars = p["variables"].as_object_mut().unwrap();
    for (k, v) in found {
        vars.entry(k).or_insert(v);
    }
}

fn walk_instances(
    nodes: &mut Json,
    base: &str,
    files: &Map<String, Json>,
    found: &mut Vec<(String, Json)>,
    issues: &mut Vec<Issue>,
) {
    for n in nodes.as_array_mut().into_iter().flatten() {
        let name = n
            .get("name")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        let path = if base.is_empty() {
            name
        } else {
            format!("{base}/{name}")
        };
        if let Some(o) = n.as_object_mut() {
            if o.get("type") == Some(&json!("scene")) {
                let patches = instance(
                    o.get("scene").and_then(Json::as_str),
                    &path,
                    files,
                    found,
                    issues,
                    0,
                );
                lay(o, patches);
            }
            if let Some(c) = o.get_mut("children") {
                walk_instances(c, &path, files, found, issues);
            }
        }
    }
}

/// Merge override patches into a `scene` node's `overrides`.
fn lay(o: &mut Map<String, Json>, patches: Map<String, Json>) {
    if patches.is_empty() {
        return;
    }
    let ov = o.entry("overrides").or_insert(json!({}));
    if let Some(ov) = ov.as_object_mut() {
        for (k, v) in patches {
            match (ov.get_mut(&k), v) {
                (Some(Json::Object(a)), Json::Object(b)) => merge(a, &b),
                (_, v) => {
                    ov.insert(k, v);
                }
            }
        }
    }
}

/// One instance at `path` of the file `scene`: the overrides it needs (keyed by the node's id, else its path in the
/// file), its variables pushed to `found`.
fn instance(
    scene: Option<&str>,
    path: &str,
    files: &Map<String, Json>,
    found: &mut Vec<(String, Json)>,
    issues: &mut Vec<Issue>,
    depth: usize,
) -> Map<String, Json> {
    let mut out = Map::new();
    let Some(doc) = scene.and_then(|s| files.get(s)) else {
        return out;
    };
    if depth > MAX_NESTING {
        return out;
    }
    let pre = engine_id(path);
    let mut ids = BTreeSet::new();
    collect_ids(&doc["nodes"], &mut ids);
    let rename = |r: &str| -> String {
        match r.rsplit_once('.') {
            Some((head, tail)) if ids.contains(head) => format!("{pre}_{head}.{tail}"),
            _ => r.to_string(),
        }
    };
    let mut stack: Vec<(String, &Json)> = doc["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| (String::new(), n))
        .collect();
    while let Some((base, n)) = stack.pop() {
        let name = n.get("name").and_then(Json::as_str).unwrap_or("");
        let rel = if base.is_empty() {
            name.to_string()
        } else {
            format!("{base}/{name}")
        };
        let key = n
            .get("id")
            .and_then(Json::as_str)
            .map_or_else(|| rel.clone(), str::to_string);
        let mut patch = Map::new();
        if let Some(vars) = n.get("variables").and_then(Json::as_object) {
            match n.get("id").and_then(Json::as_str) {
                Some(id) => {
                    for (k, v) in vars {
                        let v =
                            var_ref(v).map_or_else(|| v.clone(), |r| json!({ "$var": rename(r) }));
                        found.push((format!("{pre}_{id}.{k}"), v));
                    }
                }
                None => issues.push(
                    Issue::error(
                        "node_variables_without_id",
                        format!("the node {rel} has variables but no id"),
                        "give the node an id: its variables are named <id>.<name>",
                    )
                    .in_file(scene.unwrap_or(""))
                    .at(&rel, "variables"),
                ),
            }
        }
        if let Some(r) = n.get("visible").and_then(var_ref) {
            bind_node_visible(&mut patch, &rename(r));
            patch.insert("visible".into(), json!(true));
        }
        if n.get("type") == Some(&json!("scene")) {
            let inner = instance(
                n.get("scene").and_then(Json::as_str),
                &format!("{path}/{rel}"),
                files,
                found,
                issues,
                depth + 1,
            );
            if !inner.is_empty() {
                patch.insert("overrides".into(), Json::Object(inner));
            }
        }
        if !patch.is_empty() {
            out.insert(key, Json::Object(patch));
        }
        for c in n["children"].as_array().into_iter().flatten() {
            stack.push((rel.clone(), c));
        }
    }
    out
}

fn collect_ids(nodes: &Json, out: &mut BTreeSet<String>) {
    for n in nodes.as_array().into_iter().flatten() {
        if let Some(id) = n.get("id").and_then(Json::as_str) {
            out.insert(id.to_string());
        }
        collect_ids(&n["children"], out);
    }
}
