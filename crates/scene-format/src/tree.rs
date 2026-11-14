//! Read a scene file into its resolved tree: every node checked against the registries (tolerant: unknown fields kept
//! and reported), sub-scenes instanced with their overrides, transforms composed down the tree. Used by the validator,
//! by the lowering into game.engine documents and by the dev.builder kit.

use crate::issue::Issue;
use crate::math::Transform;
use crate::path::{is_name, join, project_path_error};
use crate::registry::{expected, fits, At, Registry, ELEMENT_KEY};
use crate::requires::{self, is_kit_id, Require};
use serde_json::{Map, Value as Json};
use std::collections::BTreeMap;

mod values;
pub use values::check_section;

/// The keys every node may have, besides the key named like its type.
const NODE_KEYS: &[&str] = &[
    "name", "id", "type", "position", "rotation", "scale", "visible", "children", "kits", "on",
    "bind", "entity", "reflect",
];
/// The keys of a scene file.
const FILE_KEYS: &[&str] = &["requires", "nodes", "kits", "on", "meta"];
/// How deep sub-scenes may nest before it is surely a loop the path check missed.
const MAX_DEPTH: usize = 32;

/// A node of the resolved tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// Its path from the root of the scene, through the instance nodes of sub-scenes (`Shelf/Book2`).
    pub path: String,
    /// The path of its parent, empty at the root.
    pub parent: String,
    /// Its type (`group` when the written one is unknown).
    pub ty: String,
    /// The node as written, overrides applied, `children` left out.
    pub raw: Map<String, Json>,
    /// Where it is in the scene.
    pub world: Transform,
    /// Whether it shows (its own `visible` and its ancestors').
    pub visible: bool,
    /// The project path of the file it was written in.
    pub file: String,
    /// Its stable identifier when it has a valid `id`, made unique through the instance nodes of sub-scenes
    /// (`shelf_` + the id written in the sub-scene): the identifier game.engine receives.
    pub id: Option<String>,
}

/// A scene file read and resolved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    /// The kits the file and its sub-scenes need.
    pub requires: Vec<Require>,
    /// Every node, parents before children, in the order of the files.
    pub nodes: Vec<Node>,
    /// The scene's own sections (`kits`).
    pub kits: Map<String, Json>,
    /// The scene's own bindings (`on`).
    pub on: Vec<Json>,
    /// What was found.
    pub issues: Vec<Issue>,
}

/// Where scene files come from: a project path → the parsed file (`None`: missing).
pub type Files<'a> = &'a dyn Fn(&str) -> Option<Json>;

struct Walk<'a> {
    reg: &'a Registry,
    files: Files<'a>,
    tree: Tree,
    stack: Vec<String>,
    /// The `id`s already given, with the path of their node: an `id` is unique in its scene.
    ids: Vec<(String, String)>,
}

fn vec3(v: Option<&Json>) -> Option<[f64; 3]> {
    let a = v?.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

/// Merge a correction into a node, key by key: objects merged, the rest replaced.
pub fn merge(into: &mut Map<String, Json>, patch: &Map<String, Json>) {
    for (k, v) in patch {
        match (into.get_mut(k), v) {
            (Some(Json::Object(a)), Json::Object(b)) => merge(a, b),
            _ => {
                into.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Read and resolve the scene file at `file` (a project path).
pub fn resolve(file: &str, reg: &Registry, files: Files) -> Tree {
    let mut w = Walk {
        reg,
        files,
        tree: Tree::default(),
        stack: Vec::new(),
        ids: Vec::new(),
    };
    let Some(doc) = files(file) else {
        w.tree.issues.push(
            Issue::warning(
                "scene_file_missing",
                format!("the scene file {file} is not in the project"),
                "check the path (relative to the project file, with /) or create the file",
            )
            .in_file(file),
        );
        return w.tree;
    };
    w.file(
        file,
        &doc,
        "",
        &Transform::default(),
        true,
        &BTreeMap::new(),
        true,
    );
    crate::states::check(&mut w.tree);
    w.tree
}

/// Resolve a scene given as a document (the project path names it in the issues).
pub fn resolve_doc(file: &str, doc: &Json, reg: &Registry, files: Files) -> Tree {
    let mut w = Walk {
        reg,
        files,
        tree: Tree::default(),
        stack: Vec::new(),
        ids: Vec::new(),
    };
    w.file(
        file,
        doc,
        "",
        &Transform::default(),
        true,
        &BTreeMap::new(),
        true,
    );
    crate::states::check(&mut w.tree);
    w.tree
}

impl Walk<'_> {
    fn issue(&mut self, i: Issue, file: &str) {
        self.tree.issues.push(i.in_file(file));
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the walk's state down one file: kept flat, read in one place"
    )]
    fn file(
        &mut self,
        file: &str,
        doc: &Json,
        prefix: &str,
        base: &Transform,
        visible: bool,
        overrides: &BTreeMap<String, Map<String, Json>>,
        top: bool,
    ) -> Vec<String> {
        let Some(o) = doc.as_object() else {
            self.issue(
                Issue::warning(
                    "file_invalid",
                    "the scene file is not a JSON object",
                    "write the scene as { \"requires\": [...], \"nodes\": [...] }",
                ),
                file,
            );
            return Vec::new();
        };
        self.stack.push(file.to_string());
        for k in o.keys().filter(|k| !FILE_KEYS.contains(&k.as_str())) {
            if k != "id" {
                self.issue(
                    Issue::warning(
                        "field_unknown",
                        format!("the scene file has an unknown key {k}: it is kept and not read"),
                        format!("remove {k}, or move it under meta if it is a note for the editor"),
                    )
                    .at("", k),
                    file,
                );
            }
        }
        let mut found = Vec::new();
        let reqs = requires::read(o.get("requires"), &mut found);
        for i in found {
            self.issue(i, file);
        }
        for r in reqs {
            if !self.tree.requires.iter().any(|x| x.id == r.id) {
                self.tree.requires.push(r);
            }
        }
        if top {
            if o.get("kits").is_some() {
                // The scene's own sections, checked like a node's against the elements exposed on the scene.
                let mut holder = Map::new();
                holder.insert("kits".into(), o["kits"].clone());
                self.sections(file, &mut holder, "", "", At::Scene);
                if let Some(Json::Object(k)) = holder.remove("kits") {
                    self.tree.kits = k;
                }
            }
            self.tree.on = o
                .get("on")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default();
        }
        let mut used = Vec::new();
        match o.get("nodes") {
            None => {}
            Some(Json::Array(nodes)) => self.nodes(file, nodes, prefix, "", base, visible, overrides, &mut used),
            Some(_) => self.issue(Issue::warning("nodes_invalid", "nodes is not a list", "write nodes as a list of nodes: [{ \"name\": \"Crate\", \"type\": \"model\" }]").at("", "nodes"), file),
        }
        self.stack.pop();
        used
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the walk's state down one list of siblings"
    )]
    fn nodes(
        &mut self,
        file: &str,
        nodes: &[Json],
        prefix: &str,
        rel: &str,
        base: &Transform,
        visible: bool,
        overrides: &BTreeMap<String, Map<String, Json>>,
        used: &mut Vec<String>,
    ) {
        let mut names: Vec<String> = Vec::new();
        for (i, raw) in nodes.iter().enumerate() {
            let Some(written) = raw.as_object() else {
                self.issue(Issue::warning("node_invalid", format!("node {i} of {} is not an object", if rel.is_empty() { "the scene" } else { rel }), "write each node as an object: { \"name\": \"Crate\", \"type\": \"model\" }").at(&join(prefix, rel), "nodes"), file);
                continue;
            };
            let written_name = written.get("name").and_then(Json::as_str).unwrap_or("");
            let under = if rel.is_empty() { "the root" } else { rel };
            // Tolerant: a node with a missing, invalid or repeated name is still placed, under a name made from its
            // place, so the scene loads with what is readable (docs/dev-builder.md, the tolerant validator).
            let mut name = if written_name.is_empty() {
                format!("node{i}")
            } else {
                crate::path::to_name(written_name)
            };
            let (code, message, fix) = if written_name.is_empty() {
                (
                    "name_missing",
                    format!("node {i} under {under} has no name: it is placed as {name}"),
                    "give the node a name, unique among its siblings: \"name\": \"Crate\""
                        .to_string(),
                )
            } else if !is_name(written_name) {
                ("name_invalid", format!("the name \"{written_name}\" has signs a path cannot hold: it is placed as {name}"), format!("use ASCII letters, digits, _ and - only, for example \"{name}\""))
            } else {
                ("", String::new(), String::new())
            };
            let mut n = 2;
            let first = name.clone();
            while names.contains(&name) {
                name = format!("{first}_{n}");
                n += 1;
            }
            let (code, message, fix) = if code.is_empty() && name != first {
                (
                    "name_duplicate",
                    format!("two siblings are named {first}: the second is placed as {name}"),
                    "rename one of them: a name is unique among its siblings".to_string(),
                )
            } else {
                (code, message, fix)
            };
            let rel_path = join(rel, &name);
            let path = join(prefix, &rel_path);
            if !code.is_empty() {
                self.issue(Issue::warning(code, message, fix).at(&path, "name"), file);
            }
            names.push(name.clone());
            let mut node = written.clone();
            if let Some(patch) = overrides.get(&rel_path) {
                merge(&mut node, patch);
                used.push(rel_path.clone());
            }
            self.node(
                file, node, &path, prefix, &rel_path, base, visible, overrides, used,
            );
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the walk's state at one node")]
    fn node(
        &mut self,
        file: &str,
        mut node: Map<String, Json>,
        path: &str,
        prefix: &str,
        rel_path: &str,
        base: &Transform,
        visible: bool,
        overrides: &BTreeMap<String, Map<String, Json>>,
        used: &mut Vec<String>,
    ) {
        let written_ty = node
            .get("type")
            .and_then(Json::as_str)
            .unwrap_or("group")
            .to_string();
        let ty = if self.reg.node_types.contains_key(&written_ty) {
            written_ty
        } else {
            let known: Vec<&str> = self.reg.node_types.keys().map(String::as_str).collect();
            self.issue(
                Issue::warning(
                    "node_type_unknown",
                    format!("the node type {written_ty} is not known: the node is kept as a group"),
                    format!(
                        "use one of {}, or install the kit that adds {written_ty}",
                        known.join(", ")
                    ),
                )
                .at(path, "type"),
                file,
            );
            "group".to_string()
        };
        let mut allowed: Vec<&str> = NODE_KEYS.to_vec();
        allowed.push(ty.as_str());
        if ty == "scene" {
            allowed.push("overrides");
        }
        for k in node.keys().filter(|k| !allowed.contains(&k.as_str())) {
            self.tree.issues.push(
                Issue::warning(
                    "field_unknown",
                    format!("the node has an unknown key {k}: it is kept and not read"),
                    format!(
                        "remove {k}, or check its spelling (a {ty} node reads: {})",
                        allowed.join(", ")
                    ),
                )
                .at(path, k)
                .in_file(file),
            );
        }
        let transform = self.transform(file, &node, path);
        let world = base.then(&transform);
        let shown = visible && node.get("visible") != Some(&Json::Bool(false));
        if node.get("visible").is_some_and(|v| !v.is_boolean()) {
            // A warning: the default (shown) is used, the scene loads (decision of the owner, dev.builder step c).
            node.remove("visible");
            self.issue(
                Issue::warning(
                    "field_invalid",
                    "visible is not true or false: the default (true) is used",
                    "write \"visible\": true or false",
                )
                .at(path, "visible"),
                file,
            );
        }
        let id = self.node_id(file, &node, path, prefix);
        self.props(file, &mut node, path, &ty);
        self.sections(file, &mut node, path, &ty, At::Node);
        self.bindings(file, &node, path, &ty);
        let children = node.remove("children");
        let parent = path
            .rfind('/')
            .map_or(String::new(), |i| path[..i].to_string());
        self.tree.nodes.push(Node {
            path: path.to_string(),
            parent,
            ty: ty.clone(),
            raw: node.clone(),
            world,
            visible: shown,
            file: file.to_string(),
            id,
        });
        if ty == "scene" {
            self.instance(file, &node, path, rel_path, &world, shown, overrides, used);
        }
        match children {
            None => {}
            Some(Json::Array(c)) => {
                self.nodes(file, &c, prefix, rel_path, &world, shown, overrides, used)
            }
            Some(_) => self.issue(
                Issue::warning(
                    "field_invalid",
                    "children is not a list: the node is loaded without children",
                    "write children as a list of nodes",
                )
                .at(path, "children"),
                file,
            ),
        }
    }

    /// The node's resolved `id`, checked: a valid identifier, unique in the scene (else a warning, and the path).
    fn node_id(
        &mut self,
        file: &str,
        node: &Map<String, Json>,
        path: &str,
        prefix: &str,
    ) -> Option<String> {
        let written = node.get("id")?;
        let Some(own) = written.as_str().filter(|s| crate::path::is_engine_id(s)) else {
            self.issue(Issue::warning("id_invalid", format!("the id {written} is not a lowercase identifier: the node is named by its path"), "write \"id\": lowercase letters, digits and _, starting with a letter, like \"crate_1\"").at(path, "id"), file);
            return None;
        };
        let id = if prefix.is_empty() {
            own.to_string()
        } else {
            format!("{}_{own}", crate::path::engine_id(prefix))
        };
        if let Some((_, other)) = self.ids.iter().find(|(i, _)| *i == id) {
            let other = other.clone();
            self.issue(
                Issue::warning(
                    "id_duplicate",
                    format!(
                        "the id {id} is already the id of {other}: this node is named by its path"
                    ),
                    "give each node its own id in the scene",
                )
                .at(path, "id"),
                file,
            );
            return None;
        }
        self.ids.push((id.clone(), path.to_string()));
        Some(id)
    }

    fn transform(&mut self, file: &str, node: &Map<String, Json>, path: &str) -> Transform {
        let mut read = |k: &str, default: [f64; 3]| {
            match node.get(k) {
            None => default,
            Some(Json::Number(n)) if k == "scale" => [n.as_f64().unwrap_or(1.0); 3],
            Some(v @ Json::Object(_)) if k == "rotation" => crate::math::quat_of(v)
                .map(crate::math::to_euler_deg)
                .unwrap_or_else(|| {
                    self.tree.issues.push(Issue::warning("transform_invalid", "rotation is neither three angles nor a quaternion: the default is used", "write \"rotation\": [x, y, z] in degrees, or { \"x\", \"y\", \"z\", \"w\" }").at(path, k).in_file(file));
                    default
                }),
            Some(v) => vec3(Some(v)).unwrap_or_else(|| {
                self.tree.issues.push(
                    Issue::warning(
                        "transform_invalid",
                        format!("{k} is not three numbers: the default is used"),
                        format!(
                            "write \"{k}\": [x, y, z]{}",
                            if k == "rotation" { " in degrees" } else { "" }
                        ),
                    )
                    .at(path, k)
                    .in_file(file),
                );
                default
            }),
        }
        };
        let p = read("position", [0.0; 3]);
        let r = read("rotation", [0.0; 3]);
        let s = read("scale", [1.0; 3]);
        Transform::local(p, r, s)
    }

    fn bindings(&mut self, file: &str, node: &Map<String, Json>, path: &str, ty: &str) {
        let Some(on) = node.get("on") else { return };
        let mut when: Vec<String> = self
            .reg
            .node_types
            .get(ty)
            .map(|t| t.when.clone())
            .unwrap_or_default();
        // A node that declares states has its entity (a group too, docs/entites-et-rendu.md): it can be used.
        let has_states = node
            .get("entity")
            .and_then(|e| e.get("states"))
            .and_then(Json::as_object)
            .is_some_and(|s| !s.is_empty());
        if has_states && !when.iter().any(|w| w == "interact") {
            when.push("interact".into());
        }
        for i in check_bindings(on, &when, ty) {
            self.issue(i.at(path, "on"), file);
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the walk's state at an instance node"
    )]
    fn instance(
        &mut self,
        file: &str,
        node: &Map<String, Json>,
        path: &str,
        rel_path: &str,
        world: &Transform,
        shown: bool,
        outer: &BTreeMap<String, Map<String, Json>>,
        used: &mut Vec<String>,
    ) {
        let Some(sub) = node.get("scene").and_then(Json::as_str) else {
            self.issue(Issue::warning("scene_invalid", "a scene node has no scene file", "write \"scene\": \"scenes/<file>.gfscene\", the path of the instanced scene in the project").at(path, "scene"), file);
            return;
        };
        if let Some(why) = project_path_error(sub) {
            self.issue(
                Issue::warning(
                    "path_invalid",
                    format!("scene {why}"),
                    "write the path relative to the project file, with /",
                )
                .at(path, "scene"),
                file,
            );
            return;
        }
        if self.stack.iter().any(|f| f == sub) || self.stack.len() >= MAX_DEPTH {
            self.issue(
                Issue::warning(
                    "scene_cycle",
                    format!(
                        "{sub} instances itself (through {}): the branch is left out",
                        self.stack.join(" → ")
                    ),
                    "remove the scene node that closes the loop",
                )
                .at(path, "scene"),
                file,
            );
            return;
        }
        let Some(doc) = (self.files)(sub) else {
            self.issue(
                Issue::warning(
                    "scene_file_missing",
                    format!("the instanced scene {sub} is not in the project"),
                    "check the path (relative to the project file, with /) or create the file",
                )
                .at(path, "scene"),
                file,
            );
            return;
        };
        // This node's own overrides, then the outer scene's that reach through it (the outer one wins).
        let mut over: BTreeMap<String, Map<String, Json>> = BTreeMap::new();
        let mut own_keys: Vec<String> = Vec::new();
        match node.get("overrides") {
            None => {}
            Some(Json::Object(o)) => {
                for (k, v) in o {
                    match v.as_object() {
                        Some(p) => {
                            let mut p = p.clone();
                            for bad in ["children", "name"] {
                                if p.remove(bad).is_some() {
                                    self.issue(Issue::warning("override_invalid", format!("overrides.{k} changes {bad}, which an override cannot change: ignored"), format!("change {bad} in {sub} itself")).at(path, &format!("overrides.{k}")), file);
                                }
                            }
                            // A key names a node of the sub-scene by its path, else by its id.
                            let k = crate::path::id_path(&doc, k).unwrap_or_else(|| k.clone());
                            own_keys.push(k.clone());
                            over.insert(k, p);
                        }
                        None => self.issue(Issue::warning("override_invalid", format!("overrides.{k} is not an object"), "write the correction as an object of the node's keys: { \"visible\": false }").at(path, &format!("overrides.{k}")), file),
                    }
                }
            }
            Some(_) => self.issue(
                Issue::warning(
                    "override_invalid",
                    "overrides is not an object",
                    "write overrides as { \"<path in the sub-scene>\": { ... } }",
                )
                .at(path, "overrides"),
                file,
            ),
        }
        let lead = format!("{rel_path}/");
        for (k, p) in outer
            .iter()
            .filter_map(|(k, p)| k.strip_prefix(&lead).map(|k| (k.to_string(), p)))
        {
            merge(over.entry(k).or_default(), p);
        }
        let inner_used = self.file(sub, &doc, path, world, shown, &over, false);
        for k in own_keys.iter().filter(|k| !inner_used.contains(k)) {
            self.issue(
                Issue::warning(
                    "override_orphan",
                    format!("overrides.{k} names no node of {sub}: ignored"),
                    format!("check the path of the node inside {sub} (names joined by /)"),
                )
                .at(path, &format!("overrides.{k}")),
                file,
            );
        }
        for k in inner_used {
            used.push(format!("{lead}{k}"));
        }
    }
}

/// The issues of a list of bindings (`on`) of a node of type `ty` that accepts the `when` in `allowed`.
pub fn check_bindings(on: &Json, allowed: &[String], ty: &str) -> Vec<Issue> {
    let mut out = Vec::new();
    let Some(list) = on.as_array() else {
        return vec![Issue::warning("binding_invalid", "on is not a list", "write on as a list: [{ \"when\": \"enter\", \"do\": [{ \"emit\": { \"topic\": \"room.door\" } }] }]")];
    };
    for (i, b) in list.iter().enumerate() {
        let when = b.get("when").and_then(Json::as_str).unwrap_or("");
        if !allowed.iter().any(|a| a == when) {
            out.push(Issue::warning(
                "binding_invalid",
                format!("on[{i}].when is \"{when}\", which a {ty} node does not have"),
                format!("use one of {}", allowed.join(", ")),
            ));
            continue;
        }
        if when == "event"
            && b.get("event")
                .and_then(Json::as_str)
                .is_none_or(str::is_empty)
        {
            out.push(Issue::warning(
                "binding_invalid",
                format!("on[{i}] waits for an event but does not name it"),
                "add \"event\": \"<topic>\", the event that runs the binding",
            ));
        }
        let ok = b
            .get("do")
            .and_then(Json::as_array)
            .is_some_and(|d| !d.is_empty() && d.iter().all(Json::is_object));
        if !ok {
            out.push(Issue::warning("binding_invalid", format!("on[{i}].do is not a list of game.engine verbs"), "write do as a list of verbs: [{ \"emit\": { \"topic\": \"room.door\" } }], [{ \"load_scene\": { \"scene\": \"level\" } }]"));
        }
    }
    out
}
