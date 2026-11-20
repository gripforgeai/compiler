//! `sources` of a project: the Rust files of the game's own code, declared by the project itself (decision of
//! gripforgeai/gripforge#420 and gripforgeai/compiler#10: `project.gripforge` is the only entry, no file is taken by
//! convention). Each is a path relative to the folder of the project file, with `/`, ending in `.rs`:
//!
//! ```json
//! "sources": ["rules.rs", "code/weapons.rs"]
//! ```
//!
//! The compiler (`gripforge-build`) adds them to the game's module `game.main`; the development mode, which compiles
//! nothing, only reads them. This is the check of the format, the same for both (files are not opened here: whether
//! they exist and stay inside the project is the compiler's check, it reads the disk).

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
