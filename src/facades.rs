//! The Rust façades the generated crate depends on: each kit ships its own, in its folder (`<kit>/facade/`, a crate
//! named `gripforge-kit-facade-<kit>` at the version of the kit), beside its `kit.api.json`. The compiler never writes
//! a façade: it finds the one of each kit of the built game in the same folder of kits it read the `kit.api.json`
//! from — the kits downloaded into the project by default, the kits of the sources with `--kits-path` (the demos of
//! the kits repository) — and checks its name and version against the kit's.
//!
//! Who uses it: [`crate::build`] (before writing the crate), [`crate::crate_gen`] (the dependency lines).

use crate::project::INSTALL_KITS;
use serde_json::{json, Value as Json};
use std::path::{Component, Path, PathBuf};

/// The folder of a kit that holds its Rust façade.
pub const FACADE_DIR: &str = "facade";

/// The façade of one kit, found in its folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Facade {
    /// The kit's id (`core.light`).
    pub kit: String,
    /// The kit's version, which is the façade's.
    pub version: String,
    /// The crate's name (`gripforge-kit-facade-core-light`).
    pub krate: String,
    /// The crate's folder (`<kit>/facade`), absolute.
    pub dir: PathBuf,
}

/// The crate name of a kit's façade: `gripforge-kit-facade-` and the kit's module name (`.`, `_` and `-` as `-`), so
/// that its Rust name is `gripforge_kit_facade_<module>` (what the generator imports).
pub fn crate_name(kit: &str) -> String {
    format!("gripforge-kit-facade-{}", kit.replace(['.', '_'], "-"))
}

/// The `name` and `version` of the `[package]` table of a Cargo.toml (the only keys read: no TOML dependency).
pub fn package_of(text: &str) -> (Option<String>, Option<String>) {
    let (mut name, mut version, mut in_package) = (None, None, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let value = v
            .strip_prefix('"')
            .and_then(|v| v.split_once('"'))
            .map(|(v, _)| v.to_string());
        match k.trim() {
            "name" => name = value,
            "version" => version = value,
            _ => {}
        }
    }
    (name, version)
}

fn issue(code: &str, kit: &str, file: &Path, message: String, fix: String) -> Json {
    json!({ "level": "error", "code": code, "file": file.to_string_lossy(), "field": format!("kits.{kit}.facade"),
        "message": message, "fix": fix })
}

/// The façade of each kit of `wanted`, from the kits read (`(folder, kit.api.json)`), or the issues: a kit without a
/// façade (`facade_missing`), a façade of another name or version than its kit (`facade_mismatch`).
pub fn resolve(kits: &[(PathBuf, Json)], wanted: &[String]) -> Result<Vec<Facade>, Vec<Json>> {
    let (mut found, mut issues) = (Vec::new(), Vec::new());
    for id in wanted {
        let Some((folder, api)) = kits.iter().find(|(_, a)| a["id"] == id.as_str()) else {
            continue; // a kit without kit.api.json is already kit_unknown
        };
        let version = api["version"].as_str().unwrap_or("").to_string();
        let dir = folder.join(FACADE_DIR);
        let manifest = dir.join("Cargo.toml");
        let krate = crate_name(id);
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            issues.push(issue("facade_missing", id, &manifest, format!(
                "the kit {id} version {version} has no Rust façade: {} does not exist, so the generated game cannot call it", manifest.display()),
                format!("install the kits of the project again with a version that ships its façade ({INSTALL_KITS}: each kit brings {FACADE_DIR}/ beside its kit.api.json), or for a demo of the kits repository give --kits-path <its kits/ folder>")));
            continue;
        };
        match package_of(&text) {
            (Some(n), Some(v)) if n == krate && v == version => found.push(Facade {
                kit: id.clone(),
                version,
                krate,
                dir,
            }),
            (n, v) => issues.push(issue("facade_mismatch", id, &manifest, format!(
                "the façade of the kit {id} version {version} is the crate {} version {}, not {krate} version {version}",
                n.as_deref().unwrap_or("(no name)"), v.as_deref().unwrap_or("(no version)")),
                format!("take the kit and its façade from the same package ({INSTALL_KITS}), or regenerate the façades of the sources (node tools/kit-api/emit.mjs)"))),
        }
    }
    if issues.is_empty() {
        Ok(found)
    } else {
        Err(issues)
    }
}

/// `to` relative to `from` (both absolute), with `..`; `to` itself when they share no root (another drive).
pub fn relative(from: &Path, to: &Path) -> PathBuf {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let (from, to) = (canon(from), canon(to));
    let (a, b): (Vec<Component>, Vec<Component>) =
        (from.components().collect(), to.components().collect());
    if a.first() != b.first() {
        return to;
    }
    let same = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in same..a.len() {
        out.push("..");
    }
    for c in &b[same..] {
        out.push(c);
    }
    out
}
