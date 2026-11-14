//! The two forms of a project and the passage between them (docs/dev-builder.md, « Éditer en un fichier, publier
//! découpé »): **one file** to edit (scenes written inline in the project, `data.scenes[].nodes`, sub-scene files
//! carried in `data.scene_files`), or **one file per scene** (`data.scenes[].file`). `build` turns either into the form
//! that is published: validated, sub-scenes and overrides resolved, asset ids turned into their library paths, one
//! resolved scene file (`<name>.gfscene`) per scene and a manifest for asset.preload. `bundle` goes the other way, into one file.
//!
//! The resolution (validate, then `resolve_scene` per scene) is kept apart from the writing (`flatten`): the planned
//! build into Rust code (the `init_scene.rs` of a template) is a second writer over the same resolved trees.
//!
//! Who uses it: the `gfscene` tool (`build`, `bundle`), and so the generation of games (the MCP, the CLI).

use crate::expand::SCENE_FILES;
use crate::issue::{has_errors, Issue};
use crate::project::validate_project;
use crate::registry::Registry;
use crate::tree::{resolve, resolve_doc, Files, Node, Tree};
use serde_json::{json, Map, Value as Json};
use std::collections::BTreeMap;

/// The keys of a scene file, which a scene of the project may also hold inline.
pub const SCENE_FILE_KEYS: &[&str] = &["requires", "nodes", "kits", "on", "meta"];
/// The name of the published manifest, next to the project file.
pub const MANIFEST: &str = "build.json";

/// The scene file written inline in a scene of the project (it has `nodes` and no `file`), or `None`.
pub fn inline_doc(scene: &Json) -> Option<Json> {
    let o = scene.as_object()?;
    if o.contains_key("file") || !o.contains_key("nodes") {
        return None;
    }
    let mut doc = Map::new();
    for k in SCENE_FILE_KEYS {
        if let Some(v) = o.get(*k) {
            doc.insert((*k).to_string(), v.clone());
        }
    }
    Some(Json::Object(doc))
}

/// The name an inline scene carries in the issues, in place of a file path (`#annex`).
pub fn inline_label(id: &str) -> String {
    format!("#{id}")
}

/// The scene files carried in the project itself (`data.scene_files`, by project path).
pub fn carried(project: &Json) -> Map<String, Json> {
    project
        .pointer(&format!("/data/{SCENE_FILES}"))
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default()
}

/// A scene of the project resolved, from its file or inline, with the name its issues carry (`None`: neither).
pub fn resolve_scene(scene: &Json, reg: &Registry, files: Files) -> Option<(String, Tree)> {
    if let Some(f) = scene.get("file").and_then(Json::as_str) {
        return Some((f.to_string(), resolve(f, reg, files)));
    }
    let id = scene.get("id").and_then(Json::as_str).unwrap_or("");
    let doc = inline_doc(scene)?;
    let label = inline_label(id);
    Some((label.clone(), resolve_doc(&label, &doc, reg, files)))
}

/// What `build` produces: the project of the published form, its files (project paths → content) and the issues.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Built {
    /// The published project: every scene names its resolved file, nothing inline, nothing carried.
    pub project: Json,
    /// The resolved scene files and the manifest (`build.json`), by project path.
    pub files: Vec<(String, Json)>,
    /// The warnings found (an error refuses the build).
    pub issues: Vec<Issue>,
}

/// A resolved tree written back as a scene file without sub-scenes: instance nodes become groups holding the nodes of
/// their scene (overrides applied), names are the resolved ones, every valid `id` the resolved one.
fn flatten(tree: &Tree, assets: &mut dyn FnMut(&Node, &str, &str) -> Json) -> Json {
    let mut kids: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, n) in tree.nodes.iter().enumerate() {
        kids.entry(n.parent.as_str()).or_default().push(i);
    }
    fn under(
        parent: &str,
        tree: &Tree,
        kids: &BTreeMap<&str, Vec<usize>>,
        assets: &mut dyn FnMut(&Node, &str, &str) -> Json,
    ) -> Vec<Json> {
        let mut out = Vec::new();
        for &i in kids.get(parent).into_iter().flatten() {
            let n = &tree.nodes[i];
            let mut raw = n.raw.clone();
            let name = n.path.rsplit('/').next().unwrap_or(&n.path);
            raw.insert("name".into(), json!(name));
            match &n.id {
                Some(id) => raw.insert("id".into(), json!(id)),
                None => raw.remove("id"),
            };
            if n.ty == "scene" {
                raw.remove("scene");
                raw.remove("overrides");
                raw.insert("type".into(), json!("group"));
            }
            if let Some(Json::Object(props)) = raw.get_mut(&n.ty) {
                for (k, v) in props.iter_mut() {
                    if let Some(s) = v.as_str().map(str::to_string) {
                        *v = assets(n, k, &s);
                    }
                }
            }
            let children = under(&n.path, tree, kids, assets);
            if !children.is_empty() {
                raw.insert("children".into(), Json::Array(children));
            }
            out.push(Json::Object(raw));
        }
        out
    }
    let mut doc = Map::new();
    let reqs: Vec<Json> = tree
        .requires
        .iter()
        .map(|r| {
            if r.version.is_empty() {
                json!(r.id)
            } else {
                json!({ "id": r.id, "version": r.version })
            }
        })
        .collect();
    doc.insert("requires".into(), Json::Array(reqs));
    doc.insert("nodes".into(), Json::Array(under("", tree, &kids, assets)));
    if !tree.kits.is_empty() {
        doc.insert("kits".into(), Json::Object(tree.kits.clone()));
    }
    if !tree.on.is_empty() {
        doc.insert("on".into(), Json::Array(tree.on.clone()));
    }
    Json::Object(doc)
}

/// Build the published form of a project (either form), its scenes under `dir` (`scenes`). `asset` turns an asset
/// id of the library into its project path (`crate` → `library/crate/crate.glb`), `None` when there is no such asset.
/// Refused with the issues only on an error (a required kit missing): warnings never stop it.
pub fn build(
    project: &Json,
    files: Files,
    reg: &Registry,
    asset: &dyn Fn(&str) -> Option<String>,
    dir: &str,
) -> Result<Built, Vec<Issue>> {
    let carried_files = carried(project);
    let all = |p: &str| carried_files.get(p).cloned().or_else(|| files(p));
    let files: Files = &all;
    let issues = validate_project(project, files, reg);
    if has_errors(&issues) {
        return Err(issues);
    }
    let mut out = Built {
        issues,
        ..Built::default()
    };
    let mut p = project.clone();
    let mut manifest = Vec::new();
    if let Some(d) = p.get_mut("data").and_then(Json::as_object_mut) {
        d.remove(SCENE_FILES);
    }
    let scenes = p
        .pointer_mut("/data/scenes")
        .and_then(Json::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default();
    let mut published = Vec::new();
    for mut scene in scenes {
        let id = scene
            .get("id")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        let Some((_, tree)) = resolve_scene(&scene, reg, files) else {
            published.push(scene);
            continue;
        };
        let mut used: Vec<String> = Vec::new();
        let mut warn: Vec<Issue> = Vec::new();
        let mut assets = |n: &Node, k: &str, v: &str| -> Json {
            let is_path = reg
                .node_types
                .get(&n.ty)
                .and_then(|t| t.props.get(k))
                .is_some_and(|f| f.kind == "path");
            if !is_path || v.is_empty() {
                return json!(v);
            }
            // A value with a / is a path of the project; without, an id of the library, else a file at the root.
            let resolved = if v.contains('/') {
                Some(v.to_string())
            } else {
                asset(v)
            };
            let path = resolved.unwrap_or_else(|| {
                if !v.contains('.') {
                    warn.push(Issue::warning("asset_unresolved", format!("{v} is neither a path of the project nor an asset of the library"), format!("write the path of the file (library/{v}/{v}.glb), or put the asset in library/{v}/ with its passport")).in_file(&n.file).at(&n.path, &format!("{}.{k}", n.ty)));
                }
                v.to_string()
            });
            if !used.contains(&path) {
                used.push(path.clone());
            }
            json!(path)
        };
        let doc = flatten(&tree, &mut assets);
        out.issues.extend(warn);
        let file = format!("{dir}/{id}.gfscene");
        if let Some(o) = scene.as_object_mut() {
            for k in SCENE_FILE_KEYS {
                o.remove(*k);
            }
            o.insert("file".into(), json!(file));
        }
        let reqs: Vec<&str> = tree.requires.iter().map(|r| r.id.as_str()).collect();
        manifest.push(json!({ "id": id, "file": file, "requires": reqs, "assets": used }));
        out.files.push((file, doc));
        published.push(scene);
    }
    if let Some(d) = p.get_mut("data").and_then(Json::as_object_mut) {
        d.insert("scenes".into(), Json::Array(published));
    }
    out.files.push((
        MANIFEST.to_string(),
        json!({ "schema": "gripforge.build/1", "scenes": manifest }),
    ));
    out.project = p;
    Ok(out)
}

/// Gather a project into one file: every scene that names a file written inline in its entry, and every sub-scene
/// file it instances carried in `data.scene_files` by its path. Missing files are reported and left named.
pub fn bundle(project: &Json, files: Files) -> (Json, Vec<Issue>) {
    let mut p = project.clone();
    let mut issues = Vec::new();
    let mut carried_files = carried(project);
    let mut todo: Vec<String> = Vec::new();
    let mut scenes = p
        .pointer_mut("/data/scenes")
        .and_then(Json::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default();
    for scene in scenes.iter_mut() {
        let Some(file) = scene.get("file").and_then(Json::as_str).map(str::to_string) else {
            if let Some(doc) = inline_doc(scene) {
                todo.extend(sub_scenes(&doc));
            }
            continue;
        };
        let Some(doc) = files(&file) else {
            issues.push(
                Issue::warning(
                    "scene_file_missing",
                    format!("the scene file {file} is not in the project: it stays named"),
                    "check the path, or create the file",
                )
                .in_file(&file),
            );
            continue;
        };
        todo.extend(sub_scenes(&doc));
        if let (Some(o), Some(d)) = (scene.as_object_mut(), doc.as_object()) {
            o.remove("file");
            for k in SCENE_FILE_KEYS {
                if let Some(v) = d.get(*k) {
                    o.insert((*k).to_string(), v.clone());
                }
            }
            if !o.contains_key("nodes") {
                o.insert("nodes".into(), json!([]));
            }
        }
    }
    while let Some(f) = todo.pop() {
        if carried_files.contains_key(&f) {
            continue;
        }
        match files(&f) {
            Some(doc) => {
                todo.extend(sub_scenes(&doc));
                carried_files.insert(f, doc);
            }
            None => issues.push(
                Issue::warning(
                    "scene_file_missing",
                    format!("the instanced scene {f} is not in the project"),
                    "check the path, or create the file",
                )
                .in_file(&f),
            ),
        }
    }
    if p.get("data").is_none() {
        p["data"] = json!({});
    }
    p["data"]["scenes"] = Json::Array(scenes);
    if !carried_files.is_empty() {
        p["data"][SCENE_FILES] = Json::Object(carried_files);
    }
    (p, issues)
}

/// The scene files a scene file instances (`scene` of its `scene` nodes), at any depth of its nodes.
fn sub_scenes(doc: &Json) -> Vec<String> {
    fn walk(list: Option<&Json>, out: &mut Vec<String>) {
        for n in list.and_then(Json::as_array).into_iter().flatten() {
            if n.get("type").and_then(Json::as_str) == Some("scene") {
                if let Some(s) = n.get("scene").and_then(Json::as_str) {
                    out.push(s.to_string());
                }
            }
            walk(n.get("children"), out);
        }
    }
    let mut out = Vec::new();
    walk(doc.get("nodes"), &mut out);
    out
}
