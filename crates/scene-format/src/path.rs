//! Names and paths, as in a Godot project: a node is named among its siblings and designated by its path in the tree
//! (`Hall/Door`, relative `../Door` from a node); a file is designated by its path in the project (`models/crate.glb`).
//! Also the identifier game.engine receives for a node (`hall_door`).

use serde_json::Value as Json;

/// Whether a node name is valid: ASCII letters, digits, `_` and `-`, not empty (it is one link of a path).
pub fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A valid name made from any text: other signs become `_` (used by the conversion).
pub fn to_name(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "node".into()
    } else {
        out
    }
}

/// Whether a game.engine identifier is valid: a lowercase letter, then lowercase letters, digits and `_`.
pub fn is_engine_id(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|c| c.is_ascii_lowercase())
        && c.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The game.engine identifier of a node path: lowercase, every other sign `_`, `n` in front of a leading digit
/// (`Hall/Door-2` → `hall_door_2`).
pub fn engine_id(path: &str) -> String {
    let mut out: String = path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out.insert(0, 'n');
    }
    out
}

/// Why a project path is not valid, or `None`: relative to the project file, with `/`, never leaving the project.
pub fn project_path_error(p: &str) -> Option<&'static str> {
    if p.is_empty() {
        return Some("is empty");
    }
    if p.starts_with('/') || p.contains('\\') || p.as_bytes().get(1) == Some(&b':') {
        return Some(
            "is an absolute or Windows path: write it relative to the project file, with /",
        );
    }
    if p.split('/').any(|s| s == "..") {
        return Some("leaves the project (..)");
    }
    None
}

/// Join a node path and a child name.
pub fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Resolve a node path written in a binding of the node at `from`: `.` and `..` start relative to `from`, anything
/// else starts at the root of the scene. `None` when it climbs above the root.
pub fn resolve(from: &str, target: &str) -> Option<String> {
    let relative =
        target == "." || target == ".." || target.starts_with("./") || target.starts_with("../");
    let mut parts: Vec<&str> = if relative {
        from.split('/').filter(|s| !s.is_empty()).collect()
    } else {
        Vec::new()
    };
    for seg in target.trim_start_matches('/').split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// The directory part of a project path (`scenes/start.gfscene` → `scenes`).
pub fn dir_of(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

/// The path, in a scene file, of the node whose `id` is `id`, when no node has `id` as its path (sub-scenes not
/// entered: their nodes are reached through their own instance).
pub fn id_path(doc: &Json, id: &str) -> Option<String> {
    fn walk(list: Option<&Json>, rel: &str, f: &dyn Fn(&Json, &str) -> bool) -> Option<String> {
        for n in list?.as_array()? {
            let Some(name) = n.get("name").and_then(Json::as_str) else {
                continue;
            };
            let path = join(rel, name);
            if f(n, &path) {
                return Some(path);
            }
            if let Some(found) = walk(n.get("children"), &path, f) {
                return Some(found);
            }
        }
        None
    }
    if walk(doc.get("nodes"), "", &|_, p| p == id).is_some() {
        return None;
    }
    walk(doc.get("nodes"), "", &|n, _| {
        n.get("id").and_then(Json::as_str) == Some(id)
    })
}
