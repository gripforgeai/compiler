//! The kits a file needs (`requires`), for the whole file, never per node: `"core.physics"` or
//! `{ "id": "core.physics", "version": "^1" }`. Not an installation: a missing kit refuses the scene with what to install;
//! the build, the CLI and the MCP read these lists to propose or install the kits (docs/dev-builder.md).

use crate::issue::Issue;
use serde_json::Value as Json;

/// One required kit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Require {
    /// The kit id (`core.physics`).
    pub id: String,
    /// The range of versions (`^1`, `~1.2`, `>=1.4.0`, `1.2.3`, `*`), empty for any.
    pub version: String,
}

/// Whether a kit id is well formed: `domain.name` in lowercase (`core.physics`, `asset.meta`).
pub fn is_kit_id(s: &str) -> bool {
    let mut parts = s.split('.');
    let first = parts.next().unwrap_or("");
    let ok = |p: &str| {
        !p.is_empty()
            && p.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    ok(first)
        && first.starts_with(|c: char| c.is_ascii_lowercase())
        && s.contains('.')
        && parts.all(ok)
}

/// Read a `requires` list; what cannot be read is an issue (`requires_invalid`), the rest is kept.
pub fn read(raw: Option<&Json>, issues: &mut Vec<Issue>) -> Vec<Require> {
    let mut out: Vec<Require> = Vec::new();
    let Some(raw) = raw else { return out };
    let Some(list) = raw.as_array() else {
        issues.push(Issue::warning("requires_invalid", "requires is not a list", "write requires as a list: [\"game.engine\", { \"id\": \"<family>.<name>\", \"version\": \"^1\" }]").at("", "requires"));
        return out;
    };
    for (i, r) in list.iter().enumerate() {
        let (id, version) = match r {
            Json::String(s) => (s.clone(), String::new()),
            Json::Object(o) => (
                o.get("id").and_then(Json::as_str).unwrap_or("").to_string(),
                o.get("version")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => (String::new(), String::new()),
        };
        if !is_kit_id(&id) {
            issues.push(Issue::warning("requires_invalid", format!("requires[{i}] is not a kit id"), "write a kit id, <family>.<name>, or { \"id\": \"<family>.<name>\", \"version\": \"^1\" }").at("", &format!("requires[{i}]")));
            continue;
        }
        if !version.is_empty() && parse_range(&version).is_none() {
            issues.push(
                Issue::warning(
                    "requires_invalid",
                    format!("the version range \"{version}\" of {id} cannot be read"),
                    "write a range like \"^1\", \"~1.2\", \">=1.4.0\", \"1.2.3\" or \"*\"",
                )
                .at("", &format!("requires[{i}].version")),
            );
            continue;
        }
        if !out.iter().any(|x| x.id == id) {
            out.push(Require { id, version });
        }
    }
    out
}

/// The issues of the required kits that are not installed, or whose installed version is outside the range.
/// `installed` gives each kit id and its version when known.
pub fn check(requires: &[Require], installed: &[(String, Option<String>)]) -> Vec<Issue> {
    let mut out = Vec::new();
    for r in requires {
        let Some((_, v)) = installed.iter().find(|(id, _)| *id == r.id) else {
            let want = if r.version.is_empty() {
                r.id.clone()
            } else {
                format!("{} {}", r.id, r.version)
            };
            out.push(Issue::error(
                "kit_required_missing",
                format!("the kit {} is required by this file but is not installed in the project", r.id),
                format!("install {want} in the project (installed list), or remove it from requires if the file does not need it"),
            ).at("", "requires"));
            continue;
        };
        if let (Some(v), false) = (v, r.version.is_empty()) {
            if !satisfies(v, &r.version) {
                out.push(Issue::warning(
                    "kit_version_mismatch",
                    format!("the kit {} is installed in version {v}, outside the range {} this file requires", r.id, r.version),
                    format!("install a version of {} that matches {}, or widen the range", r.id, r.version),
                ).at("", "requires"));
            }
        }
    }
    out
}

fn nums(s: &str) -> Option<Vec<u64>> {
    let core = s.split(['-', '+']).next()?;
    let v: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    v.filter(|v| !v.is_empty() && v.len() <= 3)
}

fn padded(v: &[u64]) -> [u64; 3] {
    [
        v.first().copied().unwrap_or(0),
        v.get(1).copied().unwrap_or(0),
        v.get(2).copied().unwrap_or(0),
    ]
}

/// `(operator, numbers)` of a range.
fn parse_range(r: &str) -> Option<(&str, Vec<u64>)> {
    let r = r.trim();
    if r == "*" || r.is_empty() {
        return Some(("*", Vec::new()));
    }
    for op in [">=", "^", "~", "="] {
        if let Some(rest) = r.strip_prefix(op) {
            return nums(rest.trim()).map(|n| (op, n));
        }
    }
    nums(r).map(|n| ("=", n))
}

/// Whether a version (`1.22.0`) is inside a range (`^1`, `~1.2`, `>=1.4.0`, `1.2.3`, `*`).
pub fn satisfies(version: &str, range: &str) -> bool {
    let (Some(v), Some((op, want))) = (nums(version), parse_range(range)) else {
        return false;
    };
    let (v, w) = (padded(&v), padded(&want));
    match op {
        "*" => true,
        ">=" => v >= w,
        "=" => (0..want.len()).all(|i| v[i] == w[i]),
        "~" => v >= w && v[0] == w[0] && (want.len() < 2 || v[1] == w[1]),
        // ^: the first non-zero number of the range is kept.
        _ => {
            let lead = want
                .iter()
                .position(|n| *n != 0)
                .unwrap_or(want.len().saturating_sub(1));
            v >= w && (0..=lead.min(2)).all(|i| v[i] == w[i])
        }
    }
}
