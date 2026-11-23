//! The kits a game is started with, checked against what they need: the kits are fixed at the start, like the
//! libraries of a program (decision of Damien, module-kits `docs/dependances.md`, « Au démarrage »). The project lists
//! EVERY kit of the game (`installed`), the ones its kits need included: a kit that needs a kit absent from that list
//! stops the start (or the build) with ONE error naming every lack, never a warning, never a kit added on the way.
//!
//! ONE copy, called by every reader of a game: the web gateway (module-kits `core/gateway-web/kits-path.mjs`, through
//! module-kits `core/scene-format-wasm` `gf_kits_check`), the native gateway (module-kits `core/gateway`
//! `modules::check_kits`), dev.builder and gripforge-build. No host, no engine, no kit name: JSON in, issues out.
//!
//! What a kit needs is read in its `kit.api.json` (module-kits `docs/dependances.md`):
//! - `requires`: `"<id>"` or `"<id>@<range>"`, a kit or a capability. Met by a kit of that id, by a kit that
//!   `provides` it (`provides: [{ "id": … }]`), or by the host itself (`runtime.core`, the host's API).
//! - `dependencies`: `{ kit, range, targets?, recommended?, optional?, reason }` or `{ anyOf: [{ kit, … }], reason }`.
//!   A dependency is needed when it has no `recommended` (`true`: a suggestion; `false`: facultative, the kit takes its
//!   default without it, e.g. world.zones of the locomotions) and is not `optional` and, with `targets`, when the target
//!   is one of them. `anyOf` is met by one of its kits present. A recommendation is never needed and never added.
//!
//! The ranges are not read here: in development every range is `"*"` (module-kits `docs/decisions/versions-dev.md`);
//! the published ranges are checked by the resolver of the CLI. A kit whose `kit.api.json` is not given (the module of
//! the game's code, a test module) declares nothing.

use crate::issue::Issue;
use serde_json::Value as Json;

/// The stable code of a kit needed and not listed in the project.
pub const KIT_REQUIRED: &str = "kit_required";

/// What the host itself provides to every kit: never a kit to list.
pub const HOST_CAPABILITIES: &[&str] = &["runtime.core"];

/// The kits of `installed` that need a kit (or capability) the list does not have, for `target` (`bevy-web`, `bevy`,
/// `godot`…, the names of the gateways). `apis`: the `kit.api.json` of each kit, by id (`{ "<id>": { … } }`); a kit
/// absent from it declares nothing. One [`KIT_REQUIRED`] error per lack, in the order of `installed` then of each
/// kit's declarations; empty when nothing lacks.
pub fn check_required(installed: &[&str], apis: &Json, target: &str) -> Vec<Issue> {
    let provided: Vec<&str> = installed
        .iter()
        .filter_map(|id| apis.get(*id))
        .flat_map(|api| {
            api.get("provides")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|p| p.get("id").and_then(Json::as_str).or_else(|| p.as_str()))
        .collect();
    let present = |id: &str| {
        installed.contains(&id) || provided.contains(&id) || HOST_CAPABILITIES.contains(&id)
    };
    let mut out = Vec::new();
    for kit in installed {
        let Some(api) = apis.get(*kit) else { continue };
        let mut lacks: Vec<String> = Vec::new();
        for r in api
            .get("requires")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = r.as_str().or_else(|| r.get("id").and_then(Json::as_str)) else {
                continue;
            };
            let id = id.split('@').next().unwrap_or(id);
            if !present(id) && !lacks.iter().any(|l| l == id) {
                lacks.push(id.to_string());
            }
        }
        for d in api
            .get("dependencies")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            if !needed(d, target) {
                continue;
            }
            if let Some(choices) = d.get("anyOf").and_then(Json::as_array) {
                let ids: Vec<&str> = choices
                    .iter()
                    .filter_map(|c| c.get("kit").and_then(Json::as_str))
                    .collect();
                if !ids.is_empty() && !ids.iter().any(|id| present(id)) {
                    lacks.push(format!("l'un de {}", ids.join(", ")));
                }
                continue;
            }
            let Some(id) = d.get("kit").and_then(Json::as_str) else {
                continue;
            };
            if !present(id) && !lacks.iter().any(|l| l == id) {
                lacks.push(id.to_string());
            }
        }
        for lack in lacks {
            out.push(Issue::error(
                KIT_REQUIRED,
                format!("kit_required: {kit} a besoin de {lack} (cible {target}) — ajoute-le dans \"installed\" du project.gripforge"),
                format!("ajoute {lack} dans \"installed\" du project.gripforge (docs/dependances.md)"),
            ));
        }
    }
    out
}

/// Whether a dependency is needed for `target`: no `recommended` at all (true or false), not optional, and its `targets` (when written)
/// name the target.
fn needed(d: &Json, target: &str) -> bool {
    if d.get("recommended").and_then(Json::as_bool).is_some()
        || d.get("optional").and_then(Json::as_bool) == Some(true)
    {
        return false;
    }
    match d.get("targets").and_then(Json::as_array) {
        Some(t) => t.iter().any(|x| x.as_str() == Some(target)),
        None => true,
    }
}

/// The one error of a refused start or build: the message of every [`KIT_REQUIRED`] issue, one per line. `None` when
/// there is none.
pub fn required_message(issues: &[Issue]) -> Option<String> {
    let lines: Vec<&str> = issues
        .iter()
        .filter(|i| i.code == KIT_REQUIRED)
        .map(|i| i.message.as_str())
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The ids of a project's `installed` list (`[{ "id": … }]`), in order.
pub fn installed_ids(project: &Json) -> Vec<&str> {
    project
        .get("installed")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|k| k.get("id").and_then(Json::as_str))
        .collect()
}

/// [`check_required`] as JSON, for a caller across a boundary (the page's wasm, a tool): `{ installed: [ids],
/// apis: { id: kit.api.json }, target }` → `{ issues: [Issue::to_json…], message: string | null }`.
pub fn check_json(req: &Json) -> Json {
    let installed: Vec<&str> = req
        .get("installed")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .collect();
    let target = req.get("target").and_then(Json::as_str).unwrap_or("");
    let issues = check_required(&installed, &req["apis"], target);
    serde_json::json!({
        "issues": issues.iter().map(Issue::to_json).collect::<Vec<_>>(),
        "message": required_message(&issues),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn apis() -> Json {
        json!({
            "game.engine": {
                "requires": ["runtime.core"],
                "dependencies": [
                    { "kit": "core.scene", "range": "*", "targets": ["bevy", "bevy-web"], "reason": "r" },
                    { "kit": "core.render", "range": "*", "targets": ["bevy", "bevy-web"], "reason": "r" }
                ]
            },
            "core.render": { "requires": ["game.engine"], "dependencies": [] },
            "core.scene": { "requires": ["game.engine"] },
            "ai.combat": {
                "requires": ["core.ai@*"],
                "dependencies": [
                    { "kit": "core.input.pointer_lock", "range": "*", "recommended": true, "reason": "r" },
                    { "kit": "net.state", "range": "*", "optional": true, "reason": "r" },
                    { "kit": "world.zones", "range": "*", "recommended": false, "reason": "r" }
                ]
            },
            "core.ai": { "provides": [{ "id": "core.ai", "version": "1.0.0" }, { "id": "ai.brain" }] },
            "ai.squad": { "requires": ["ai.brain"] },
            "world.props": { "dependencies": [{ "anyOf": [{ "kit": "world.terrain" }, { "kit": "world.arena" }], "reason": "r" }] }
        })
    }

    /// Criterion 1 of the decision: game.engine without core.render on bevy-web names both kits and the target.
    #[test]
    fn a_kit_needed_on_the_target_and_missing_is_one_error_naming_both_kits_and_the_target() {
        let issues = check_required(&["game.engine", "core.scene"], &apis(), "bevy-web");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].is_error());
        assert_eq!(issues[0].code, "kit_required");
        assert_eq!(
            issues[0].message,
            "kit_required: game.engine a besoin de core.render (cible bevy-web) — ajoute-le dans \"installed\" du project.gripforge"
        );
    }

    /// Criterion 2: two lacks are both said, at once, in one message.
    #[test]
    fn every_lack_is_said_at_once() {
        let issues = check_required(&["game.engine"], &apis(), "bevy");
        assert_eq!(issues.len(), 2);
        let m = required_message(&issues).unwrap();
        assert!(
            m.contains("core.scene (cible bevy)") && m.contains("core.render (cible bevy)"),
            "{m}"
        );
        assert_eq!(m.lines().count(), 2);
    }

    /// `recommended: false` is facultative like `recommended: true` (module-kits tools/kit-api isRequiredDependency, the
    /// same rule): the third-person template without world.zones starts.
    #[test]
    fn recommended_false_is_facultative_never_needed() {
        assert!(check_required(&["ai.combat", "core.ai"], &apis(), "bevy").is_empty());
    }

    /// A dependency of another target is not needed here.
    #[test]
    fn a_dependency_of_another_target_is_not_needed() {
        assert!(check_required(&["game.engine"], &apis(), "godot").is_empty());
    }

    /// Criterion 3: a recommendation (or an optional kit) absent blocks nothing and is never added.
    #[test]
    fn a_recommendation_absent_blocks_nothing() {
        assert!(check_required(&["ai.combat", "core.ai"], &apis(), "bevy-web").is_empty());
    }

    /// A requirement is met by the kit of that id, a kit that provides it, or the host.
    #[test]
    fn a_requirement_is_met_by_a_kit_a_provider_or_the_host() {
        assert!(check_required(&["ai.squad", "core.ai"], &apis(), "bevy").is_empty());
        let issues = check_required(&["ai.squad"], &apis(), "bevy");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("ai.squad a besoin de ai.brain"));
        // runtime.core is the host's: never a lack.
        assert!(check_required(
            &["core.render", "game.engine", "core.scene"],
            &apis(),
            "bevy"
        )
        .is_empty());
    }

    /// `anyOf` is met by one of its kits; none present is a lack naming the choices.
    #[test]
    fn one_of_is_met_by_one_and_said_with_its_choices() {
        assert!(check_required(&["world.props", "world.arena"], &apis(), "bevy").is_empty());
        let issues = check_required(&["world.props"], &apis(), "bevy");
        assert!(
            issues[0]
                .message
                .contains("a besoin de l'un de world.terrain, world.arena"),
            "{}",
            issues[0].message
        );
    }

    /// A kit without its kit.api.json declares nothing.
    #[test]
    fn a_kit_without_its_api_declares_nothing() {
        assert!(check_required(&["game.main"], &apis(), "bevy").is_empty());
    }

    /// The JSON form a page's wasm calls.
    #[test]
    fn the_json_form_gives_the_issues_and_the_message() {
        let out = check_json(
            &json!({ "installed": ["game.engine", "core.scene"], "apis": apis(), "target": "bevy-web" }),
        );
        assert_eq!(out["issues"][0]["code"], "kit_required");
        assert!(out["message"].as_str().unwrap().contains("core.render"));
        let ok =
            check_json(&json!({ "installed": ["core.ai"], "apis": apis(), "target": "bevy-web" }));
        assert_eq!(ok["message"], Json::Null);
        assert_eq!(
            installed_ids(&json!({ "installed": [{ "id": "a" }, { "x": 1 }, { "id": "b" }] })),
            vec!["a", "b"]
        );
    }
}
