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

/// The root aliases a path of a project may start with, instead of climbing (`../../../tests/…`): `#<name>/<rest>` is
/// `<rest>` under the folder `<name>` of the root. The same table as the `imports` of the kits repository's
/// package.json (`"#kits/*": "./kits/*"`, and the import map of its pages); the root is the nearest folder above the
/// project file whose package.json declares the alias in `imports` (its target), else the project's folder
/// ([`on_disk`]). A page maps it from the root of its site (`/<name>/<rest>`, as its import map).
pub const ROOT_ALIASES: &[&str] = &["kits", "tools", "core", "tests"];

/// The alias and the rest of a path that starts with `#` and a `/` (`#tests/a/b` → `("tests", "a/b")`), or `None`.
/// Whether the alias is known and the rest valid: [`root_alias_error`].
pub fn root_alias(p: &str) -> Option<(&str, &str)> {
    p.strip_prefix('#')?.split_once('/')
}

/// Why a path that starts with `#` is not a valid root alias (`#kits/`, `#tools/`, `#core/` or `#tests/` and a path
/// that stays under it), or `None` (also for a path without `#`).
pub fn root_alias_error(p: &str) -> Option<&'static str> {
    if !p.starts_with('#') {
        return None;
    }
    let Some((_, rest)) = root_alias(p).filter(|(n, _)| ROOT_ALIASES.contains(n)) else {
        return Some("starts with # but not with a root alias: #kits/, #tools/, #core/ or #tests/");
    };
    if rest.is_empty() || rest.contains('\\') || rest.split('/').any(|s| s.is_empty() || s == "..")
    {
        return Some(
            "climbs above its root alias or is empty after it: write #<alias>/<path under it>",
        );
    }
    None
}

/// Where a path the format asks for lies on a disk, from the folder of the project file: a root alias from the root
/// ([`ROOT_ALIASES`]: the nearest folder at or above `project_dir` whose package.json maps `#<name>/*` in `imports`,
/// to its target; else `project_dir/<name>/<rest>`), any other path from `project_dir`. The one rule of every reader
/// of a disk (gripforge-build, the reference reader of the kits repository).
pub fn on_disk(project_dir: &std::path::Path, p: &str) -> std::path::PathBuf {
    let Some((name, rest)) = root_alias(p).filter(|_| root_alias_error(p).is_none()) else {
        return project_dir.join(p);
    };
    let key = format!("#{name}/*");
    for dir in project_dir.ancestors() {
        let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
            continue;
        };
        let target = serde_json::from_str::<Json>(&text)
            .ok()
            .and_then(|v| v.get("imports")?.get(&key)?.as_str().map(str::to_string));
        if let Some(t) = target.and_then(|t| t.strip_suffix('*').map(str::to_string)) {
            return dir.join(t.trim_start_matches("./")).join(rest);
        }
    }
    project_dir.join(name).join(rest)
}

/// Why a project path is not valid, or `None`: relative to the project file, with `/`, never leaving the project; or
/// from the root by a root alias ([`ROOT_ALIASES`]).
pub fn project_path_error(p: &str) -> Option<&'static str> {
    if p.is_empty() {
        return Some("is empty");
    }
    if p.starts_with('#') {
        return root_alias_error(p);
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

/// The directory part of a project path (`scenes/start.json` → `scenes`).
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
