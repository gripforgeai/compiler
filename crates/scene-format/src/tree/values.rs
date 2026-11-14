//! The values of a node checked against the registries (split from tree.rs): the properties of its type and the
//! sections of the kits. Tolerant: a value that does not fit is a warning and is dropped, so the default applies and
//! the scene still loads. Used by tree.rs's walk only.

use super::*;

impl Walk<'_> {
    /// The properties of the type checked; one that does not fit its type is a warning and is dropped, so the default
    /// of the registry applies and the scene still loads (only a missing required kit refuses a scene).
    pub(super) fn props(&mut self, file: &str, node: &mut Map<String, Json>, path: &str, ty: &str) {
        let Some(t) = self.reg.node_types.get(ty) else {
            return;
        };
        if ty == "scene" {
            return;
        }
        let Some(v) = node.get(ty) else { return };
        let Some(props) = v.as_object() else {
            node.remove(ty);
            self.issue(
                Issue::warning(
                    "prop_invalid",
                    format!("the key {ty} of a {ty} node is not an object: the defaults of the type are used"),
                    format!("write its properties as an object: \"{ty}\": {{ ... }}"),
                )
                .at(path, ty),
                file,
            );
            return;
        };
        let mut found = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        for (k, v) in props {
            let field = format!("{ty}.{k}");
            match t.props.get(k) {
                None => found.push(
                    Issue::warning(
                        "field_unknown",
                        format!("a {ty} has no property {k}: it is kept and not read"),
                        format!(
                            "use one of {}",
                            t.props.keys().cloned().collect::<Vec<_>>().join(", ")
                        ),
                    )
                    .at(path, &field),
                ),
                Some(f) if !fits(f, v) => found.push({
                    dropped.push(k.clone());
                    Issue::warning(
                        "prop_invalid",
                        format!("{field} is not {}: the default is used", expected(f)),
                        format!(
                            "write {field} as {}{}",
                            expected(f),
                            if f.default.is_null() {
                                String::new()
                            } else {
                                format!(" (default {})", f.default)
                            }
                        ),
                    )
                    .at(path, &field)
                }),
                Some(f) if f.kind == "path" => {
                    if let Some(why) = v
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .and_then(project_path_error)
                    {
                        found.push(Issue::warning("path_invalid", format!("{field} {why}"), "write the path relative to the project file, with /, like models/crate.glb").at(path, &field));
                    }
                }
                _ => {}
            }
        }
        if let Some(Json::Object(props)) = node.get_mut(ty) {
            for k in dropped {
                props.remove(&k);
            }
        }
        for i in found {
            self.issue(i, file);
        }
    }

    /// The sections of the kits checked (`at`: on a node of type `ty`, or on the scene itself); one that is not an
    /// object, or a property that does not fit the element its kit exposes, is a warning and is dropped (the kit's
    /// defaults apply): the scene still loads. A section of a kit that exposes nothing is kept as written.
    pub(super) fn sections(
        &mut self,
        file: &str,
        node: &mut Map<String, Json>,
        path: &str,
        ty: &str,
        at: At,
    ) {
        let Some(kits) = node.get("kits") else { return };
        let Some(kits) = kits.as_object() else {
            node.remove("kits");
            self.issue(
                Issue::warning(
                    "section_invalid",
                    "kits is not an object: no section is read",
                    "write kits as { \"<kit id>\": { ... } }",
                )
                .at(path, "kits"),
                file,
            );
            return;
        };
        let mut found = Vec::new();
        let (mut gone, mut gone_fields): (Vec<String>, Vec<(String, String)>) =
            (Vec::new(), Vec::new());
        for (kit, section) in kits {
            let (issues, drop, fields) = check_section(self.reg, kit, section, ty, at);
            found.extend(issues.into_iter().map(|i| {
                let field = i.field.clone();
                i.at(path, &field)
            }));
            if drop {
                gone.push(kit.clone());
            }
            gone_fields.extend(fields.into_iter().map(|f| (kit.clone(), f)));
        }
        if let Some(Json::Object(kits)) = node.get_mut("kits") {
            for k in gone {
                kits.remove(&k);
            }
            for (k, f) in gone_fields {
                if let Some(Json::Object(sec)) = kits.get_mut(&k) {
                    sec.remove(&f);
                }
            }
        }
        for i in found {
            self.issue(i, file);
        }
    }
}

/// One written section checked against the element its kit exposes (shared by the file validator and dev.builder,
/// which checks again with the elements the kits handed it at run time): the issues (their `field` set, the node left
/// to the caller), whether the whole section is dropped, and the properties dropped (their default applies).
pub fn check_section(
    reg: &Registry,
    kit: &str,
    section: &Json,
    ty: &str,
    at: At,
) -> (Vec<Issue>, bool, Vec<String>) {
    let here = format!("kits.{kit}");
    let mut found = Vec::new();
    if !is_kit_id(kit) {
        found.push(
            Issue::warning(
                "section_invalid",
                format!("{kit} is not a kit id"),
                "name a section by the id of its kit, like core.physics",
            )
            .at("", &here),
        );
        return (found, true, Vec::new());
    }
    let Some(fields) = section.as_object() else {
        found.push(
            Issue::warning(
                "section_invalid",
                format!(
                    "the section {kit} is not an object: it is not read, the kit's defaults apply"
                ),
                format!("write it as \"{kit}\": {{ ... }}"),
            )
            .at("", &here),
        );
        return (found, true, Vec::new());
    };
    let Some(decl) = reg.sections.get(kit) else {
        return (found, false, Vec::new());
    };
    let where_ = if at == At::Scene {
        "the scene"
    } else {
        "a node"
    };
    let el = match decl.pick(at, fields) {
        Ok(el) => el,
        Err(names) if names.is_empty() => {
            found.push(
                Issue::warning(
                    "section_type_mismatch",
                    format!(
                        "{kit} exposes no element on {where_}: the section is kept and not read"
                    ),
                    format!(
                        "move it to {}",
                        if at == At::Scene {
                            "a node"
                        } else {
                            "the kits of the scene file"
                        }
                    ),
                )
                .at("", &here),
            );
            return (found, false, Vec::new());
        }
        Err(names) => {
            found.push(
                Issue::warning(
                    "section_invalid",
                    format!(
                        "{kit} exposes no element {} on {where_}",
                        fields[ELEMENT_KEY]
                    ),
                    format!("write \"element\" as one of {}", names.join(", ")),
                )
                .at("", &format!("{here}.element")),
            );
            return (found, false, Vec::new());
        }
    };
    if at == At::Node && !el.node_types.is_empty() && !el.node_types.iter().any(|t| t == ty) {
        found.push(
            Issue::warning(
                "section_type_mismatch",
                format!("the element {kit}.{} is not meant for a {ty} node", el.name),
                format!("put it on a node of type {}", el.node_types.join(" or ")),
            )
            .at("", &here),
        );
    }
    let mut dropped = Vec::new();
    for (k, v) in fields.iter().filter(|(k, _)| k.as_str() != ELEMENT_KEY) {
        match el.props.get(k) {
            None => found.push(
                Issue::warning(
                    "section_field_unknown",
                    format!(
                        "{kit}.{} has no property {k}: it is kept and not read",
                        el.name
                    ),
                    format!(
                        "use one of {}",
                        el.props.keys().cloned().collect::<Vec<_>>().join(", ")
                    ),
                )
                .at("", &format!("{here}.{k}")),
            ),
            Some(f) if !fits(f, v) => {
                dropped.push(k.clone());
                found.push(
                    Issue::warning(
                        "section_invalid",
                        format!("{here}.{k} is not {}: the default is used", expected(f)),
                        format!(
                            "write {here}.{k} as {} (default {})",
                            expected(f),
                            f.default
                        ),
                    )
                    .at("", &format!("{here}.{k}")),
                );
            }
            _ => {}
        }
    }
    (found, false, dropped)
}
