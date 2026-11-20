//! `sources` of a project: the Rust files of the game's own code, declared by the project itself (decision of
//! gripforgeai/gripforge#420 and gripforgeai/compiler#10: `project.gripforge` is the only entry, no file is taken by
//! convention). Each is a path relative to the folder of the project file, with `/`, ending in `.rs`:
//!
//! ```json
//! "sources": ["rules.rs", "code/weapons.rs"]
//! ```
//!
//! The compiler (`gripforge-build`) adds them to the game's module `game.main`; the development mode, which compiles
//! nothing, loads that module compiled apart (`gripforge-build --dev`, then the CLI) and refuses the project without it
//! or with a module of other files ([`dev_module_issue`], the digest of crate::sources_digest). This is the check of
//! the format, the same for both (whether the files exist and stay inside the project is the compiler's check, it reads
//! the disk).

use crate::issue::Issue;
use serde_json::Value as Json;

/// The key.
pub const KEY: &str = "sources";

/// The declared paths, as written, in order (only the strings; [`check`] says what is wrong with the rest).
pub fn declared(project: &Json) -> Vec<String> {
    project
        .get(KEY)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(String::from))
        .collect()
}

/// A declared path written in the normal form: `./` segments dropped. `None`: not a relative path inside the project
/// (absolute, a drive, a `\`, a `..`, empty).
pub fn normal(path: &str) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.as_bytes().get(1) == Some(&b':')
    {
        return None;
    }
    let mut parts = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => continue,
            ".." => return None,
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The issues of `sources` (warnings of the tolerant validator: the strict build makes them errors).
pub fn check(project: &Json) -> Vec<Issue> {
    let mut out = Vec::new();
    let Some(v) = project.get(KEY) else {
        return out;
    };
    let Some(list) = v.as_array() else {
        out.push(
            Issue::warning(
                "sources_invalid",
                "sources is not a list: it lists the Rust files of the game's code",
                "write \"sources\": [\"rules.rs\"], paths relative to the project file",
            )
            .at("", KEY),
        );
        return out;
    };
    let mut seen: Vec<String> = Vec::new();
    for (i, s) in list.iter().enumerate() {
        let field = format!("{KEY}[{i}]");
        let Some(path) = s.as_str() else {
            out.push(
                Issue::warning(
                    "sources_invalid",
                    format!("{field} is not a path"),
                    "write each source as a string, a path relative to the project file (\"rules.rs\")",
                )
                .at("", &field),
            );
            continue;
        };
        let Some(n) = normal(path) else {
            out.push(Issue::warning("source_outside", format!("{field} ({path}) is not a path inside the project"),
                "write the path relative to the folder of the project file, with /, without .. nor a drive (\"code/rules.rs\")").at("", &field));
            continue;
        };
        if !n.ends_with(".rs") {
            out.push(
                Issue::warning(
                    "source_not_rust",
                    format!("{field} ({path}) is not a Rust file"),
                    "list only the .rs files of the game's code",
                )
                .at("", &field),
            );
            continue;
        }
        if seen.contains(&n) {
            out.push(
                Issue::warning(
                    "source_duplicate",
                    format!("{field} ({path}) is listed twice"),
                    "list each file of the game's code once",
                )
                .at("", &field),
            );
            continue;
        }
        seen.push(n);
    }
    out
}

/// The digest of the declared files as they are now ([`crate::sources_digest::digest`]): `read(path)` gives the bytes
/// of a file relative to the project (`None`: not there). Only the paths [`check`] accepts count, in the declared
/// order. A declared file that cannot be read is the issue `source_missing`.
pub fn digest_of(project: &Json, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<String, Issue> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, path) in accepted(project).into_iter().enumerate() {
        let Some(bytes) = read(&path) else {
            return Err(Issue::error(
                "source_missing",
                format!("{KEY}[{i}] ({path}) is not there"),
                "write the file beside the project, or take it out of sources",
            )
            .at("", &format!("{KEY}[{i}]")));
        };
        files.push((path, bytes));
    }
    let pairs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    Ok(String::from_utf8_lossy(&crate::sources_digest::digest(&pairs)).into_owned())
}

/// The declared paths [`check`] accepts, in the normal form, in order.
pub fn accepted(project: &Json) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for s in declared(project) {
        if let Some(n) = normal(&s).filter(|n| n.ends_with(".rs")) {
            if !seen.contains(&n) {
                seen.push(n);
            }
        }
    }
    seen
}

/// What keeps the development mode from playing a project with its code: the project declares `sources` and
/// `built` (the digest the compiled module of the game's code carries; `None`: no compiled module found) is missing
/// (`game_main_missing`) or is not the digest of the files now (`game_main_stale`). `None`: nothing to say (no
/// sources, or the module is the one of these files). The same rule in the page (core/gateway-web/dev-game-main.mjs
/// `gameMainIssue`) and in the compiler's docs (docs/sources.md): never a game run without its code.
pub fn dev_module_issue(
    project: &Json,
    built: Option<&str>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Option<Issue> {
    if accepted(project).is_empty() {
        return None;
    }
    let Some(built) = built else {
        return Some(Issue::error("game_main_missing",
            format!("the project declares its code in {KEY} but the compiled module of the game's code was not found"),
            "build the game's module: gripforge-build --dev, then compile .build/dev/game (the CLI writes .build/dev/web/kit-modules.json and .build/dev/native)").at("", KEY));
    };
    let now = match digest_of(project, read) {
        Ok(d) => d,
        Err(i) => return Some(i),
    };
    (now != built).then(|| Issue::error("game_main_stale",
        format!("the compiled module of the game's code ({built}) is not the code the project declares now ({now})"),
        "compile the game's module again (gripforge-build --dev, then the CLI): a file of sources changed since").at("", KEY))
}

/// The game of the development mode (the envelope of crate::project::pack) with the compiled module of the game's
/// code: `id` and `version` as the module says them (its kit.api.json: the format names no module here), added last
/// to `project.installed` and to the lock, as in a built game (gripforge-build), so its `setup` runs after every kit's
/// and before the first scene. Nothing changes when the lock already has it.
pub fn with_game_module(game: &mut Json, id: &str, version: &str) {
    let has = game["lock"]["order"]
        .as_array()
        .is_some_and(|o| o.iter().any(|k| k.as_str() == Some(id)));
    if has {
        return;
    }
    let entry = serde_json::json!({ "id": id, "version": version, "enabled": true });
    match game["project"]["installed"].as_array_mut() {
        Some(list) => list.push(entry),
        None => game["project"]["installed"] = serde_json::json!([entry]),
    }
    match game["lock"]["order"].as_array_mut() {
        Some(o) => o.push(Json::String(id.into())),
        None => game["lock"]["order"] = serde_json::json!([id]),
    }
    game["lock"]["kits"][id] =
        serde_json::json!({ "version": version, "enabled": true, "dependencies": {} });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn codes(p: Json) -> Vec<&'static str> {
        check(&p).iter().map(|i| i.code).collect()
    }

    #[test]
    fn declared_sources_are_checked_as_paths_inside_the_project() {
        assert!(codes(json!({ "id": "g" })).is_empty());
        assert!(codes(json!({ "sources": ["rules.rs", "./code/weapons.rs"] })).is_empty());
        assert_eq!(codes(json!({ "sources": "rules.rs" })), ["sources_invalid"]);
        assert_eq!(codes(json!({ "sources": [3] })), ["sources_invalid"]);
        for bad in [
            "/abs/rules.rs",
            "../rules.rs",
            "code/../../x.rs",
            "C:/x.rs",
            "code\\x.rs",
            "",
        ] {
            assert_eq!(
                codes(json!({ "sources": [bad] })),
                ["source_outside"],
                "{bad}"
            );
        }
        assert_eq!(
            codes(json!({ "sources": ["rules.txt"] })),
            ["source_not_rust"]
        );
        assert_eq!(
            codes(json!({ "sources": ["rules.rs", "./rules.rs"] })),
            ["source_duplicate"]
        );
    }
}
