//! What the compiler reads from the disk: the project file (`project.gripforge`, or the only `*.gripforge` of a
//! folder), the scene files it still names (an old project not yet in one file; sub-scenes carried in
//! `data.scene_files` need none), and the catalog of the kits (every `kits/<kit>/kit.api.json`). The core
//! ([`crate::core`]) never touches the disk: it gets all of it in one request.

use serde_json::{json, Map, Value as Json};
use std::path::{Path, PathBuf};

/// Where a project keeps the kits it uses (their `kit.api.json`), relative to its folder, looked at in this order.
pub const PROJECT_KITS: &[&str] = &["kits", ".gripforge/kits"];

/// The command a user runs to put the kits of a project in it.
pub const INSTALL_KITS: &str = "gripforge game-attach <project folder>";

/// A problem of the build said as an issue (code, message, fix) rather than a panic.
#[derive(Debug, Clone)]
pub struct BuildError(pub Json);

impl BuildError {
    /// An error issue.
    pub fn new(code: &str, file: Option<&Path>, message: String, fix: &str) -> Self {
        let mut i = json!({ "level": "error", "code": code });
        if let Some(f) = file {
            i["file"] = json!(f.to_string_lossy());
        }
        i["message"] = json!(message);
        i["fix"] = json!(fix);
        BuildError(i)
    }
}

/// The project file a path names: the file, or in a folder `project.gripforge`, else its only `*.gripforge`.
pub fn project_file(path: Option<&Path>) -> Result<PathBuf, BuildError> {
    let p = absolute(path.unwrap_or(Path::new(".")));
    if !p.exists() {
        return Err(BuildError::new(
            "project_missing",
            Some(&p),
            format!("{} does not exist", p.display()),
            "give the path of project.gripforge or of its folder",
        ));
    }
    if !p.is_dir() {
        return Ok(p);
    }
    if p.join("project.gripforge").exists() {
        return Ok(p.join("project.gripforge"));
    }
    let mut all: Vec<String> = std::fs::read_dir(&p)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|f| f.ends_with(".gripforge"))
                .collect()
        })
        .unwrap_or_default();
    all.sort();
    if all.len() == 1 {
        return Ok(p.join(&all[0]));
    }
    let message = if all.is_empty() {
        format!("{} holds no project.gripforge", p.display())
    } else {
        format!(
            "{} holds several .gripforge files ({})",
            p.display(),
            all.join(", ")
        )
    };
    Err(BuildError::new(
        "project_missing",
        Some(&p),
        message,
        "name the project file itself, or write project.gripforge in the folder",
    ))
}

/// A path made absolute against the current folder (not resolved through links).
pub fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

/// A JSON file, or a build error that says where it is not JSON (line and column).
pub fn read_json(file: &Path, code: &str) -> Result<Json, BuildError> {
    let text = std::fs::read_to_string(file).map_err(|e| {
        BuildError::new(
            code,
            Some(file),
            format!("{} cannot be read: {e}", file.display()),
            "check the path and its rights",
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        BuildError::new(
            code,
            Some(file),
            format!(
                "{} is not JSON (line {}, column {}): {e}",
                file.display(),
                e.line(),
                e.column()
            ),
            "fix the JSON syntax (commas, quotes, brackets) at that place",
        )
    })
}

/// The scene files a project still names (`data.scenes[].file`, scene nodes' `scene`), read from beside it.
pub fn scene_files(project: &Json, dir: &Path) -> Result<Map<String, Json>, BuildError> {
    let mut files = Map::new();
    let carried = project
        .pointer("/data/scene_files")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let scenes = project
        .pointer("/data/scenes")
        .cloned()
        .unwrap_or(Json::Null);
    let mut todo: Vec<String> = Vec::new();
    for s in scenes.as_array().into_iter().flatten() {
        if let Some(f) = s.get("file").and_then(Json::as_str) {
            todo.push(f.to_string());
        }
    }
    walk(&scenes, &mut todo);
    for v in carried.values() {
        walk(v, &mut todo);
    }
    while let Some(path) = todo.pop() {
        if files.contains_key(&path) || carried.contains_key(&path) {
            continue;
        }
        let file = dir.join(&path);
        if !file.exists() {
            continue; // the core says scene_file_missing, with the node that names it
        }
        let v = read_json(&file, "file_not_json")?;
        walk(&v, &mut todo);
        files.insert(path, v);
    }
    Ok(files)
}

/// Every `scene` of a node of type `scene` under `v`, in the order met (pushed so that the first is read first).
fn walk(v: &Json, todo: &mut Vec<String>) {
    let mut found = Vec::new();
    fn visit(v: &Json, found: &mut Vec<String>) {
        match v {
            Json::Array(a) => a.iter().for_each(|x| visit(x, found)),
            Json::Object(o) => {
                if o.get("type").and_then(Json::as_str) == Some("scene") {
                    if let Some(s) = o.get("scene").and_then(Json::as_str) {
                        found.push(s.to_string());
                    }
                }
                o.values().for_each(|x| visit(x, found));
            }
            _ => {}
        }
    }
    visit(v, &mut found);
    todo.extend(found.into_iter().rev());
}

/// The folder of the kits of a project: `<dir>/kits`, else `<dir>/.gripforge/kits`, else a `kits/` folder above it
/// (a workspace holding several games); None when there is none.
pub fn find_kits_dir(from: &Path) -> Option<PathBuf> {
    let has_kits = |k: &Path| {
        k.is_dir()
            && std::fs::read_dir(k)
                .map(|r| r.flatten().any(|e| e.path().join("kit.api.json").exists()))
                .unwrap_or(false)
    };
    let d = absolute(from);
    if let Some(k) = PROJECT_KITS.iter().map(|p| d.join(p)).find(|k| has_kits(k)) {
        return Some(k);
    }
    let mut up = d;
    while up.pop() {
        if has_kits(&up.join("kits")) {
            return Some(up.join("kits"));
        }
    }
    None
}

/// The kit.api.json of every kit of a folder (`<dir>/<any folder>/kit.api.json`), sorted by folder.
pub fn kit_catalog(dir: Option<&Path>) -> Result<Vec<Json>, BuildError> {
    let Some(dir) = dir else {
        return Err(BuildError::new("kits_missing", None, "the project has no kits: no kits/ or .gripforge/kits/ folder holding <kit>/kit.api.json beside it".into(), &format!("install the kits of the project ({INSTALL_KITS}), or give --kits <folder holding <kit>/kit.api.json>")));
    };
    if !dir.exists() {
        return Err(BuildError::new(
            "kits_missing",
            Some(dir),
            format!("the folder of the kits {} does not exist", dir.display()),
            "give --kits <folder holding <kit>/kit.api.json>",
        ));
    }
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    names.sort();
    names
        .iter()
        .filter(|p| p.join("kit.api.json").exists())
        .map(|p| read_json(&p.join("kit.api.json"), "file_not_json"))
        .collect()
}
