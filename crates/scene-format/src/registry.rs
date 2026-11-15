//! The two registries the format reads, both declared in data, never in code: the node types (`nodeTypes`: the base
//! ones in `node_types.json`, more added by a kit or a mod) and the kits' sections (`scene_sections`: the elements each
//! kit exposes to be driven, `expose` entries `for: ["scene"]` of its `kit.api.json`, docs/design/dev-builder-sections.md).

use serde_json::{Map, Value as Json};
use std::collections::BTreeMap;

/// The base node types, as data (the same file the dev.builder kit serves as its `nodeTypes` registry).
pub const BASE_NODE_TYPES: &str = include_str!("node_types.json");

/// One property of a node type, or one field of a kit's section: its type, default and description.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// `number`, `bool`, `string`, `enum`, `path`, `vec3`, `json`, or a kit-api parameter type (`f64`, `u32`...).
    pub kind: String,
    /// What a reader takes when the field is not written.
    pub default: Json,
    /// The allowed values of an `enum`.
    pub choices: Vec<String>,
    /// What it does, for the editor and for an AI.
    pub description: String,
}

/// A node type: its properties (in the node's key named like the type) and the bindings it accepts.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeType {
    /// What the type is.
    pub description: String,
    /// Its properties, by name.
    pub props: BTreeMap<String, Field>,
    /// The `when` of the bindings it accepts.
    pub when: Vec<String>,
    /// What game.engine receives from such a node: `instance`, `entry`, `zone`, or `none`.
    pub lowers: String,
    /// The kit and the element that read a node of this type (`"section": { "kit", "element" }` of its declaration):
    /// its properties (`"<type>": { ... }`) are that element's section, as if written in `kits.<kit>` with
    /// `"element": "<element>"`, and are checked against what the kit exposes, never here. A collision shape, an
    /// animation: a node of the tree like any other, read by the kit that knows it.
    pub section: Option<(String, String)>,
}

/// Where an element lives: on a node of a scene (`kits.<kit>` of the node) or on the scene itself (`kits.<kit>` of the
/// file).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum At {
    /// On a node: the kit receives the node's path, its resolved transform and the engine node to create under.
    #[default]
    Node,
    /// On the scene: one per entered scene (a gravity, an ambience, a weather).
    Scene,
}

/// An element a kit exposes to be driven (docs/design/dev-builder-sections.md): its properties, where it lives, and
/// the four methods of the kit's service that make it, change it, let it go and read it back. The same methods are
/// what the game calls in code, what dev.builder calls for a scene file, and what the editor and the debugger call.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    /// Its name in the kit (the last segment of the entry's id: `core.physics.collider` → `collider`).
    pub name: String,
    /// On a node or on the scene.
    pub at: At,
    /// Its properties, by key.
    pub props: BTreeMap<String, Field>,
    /// The node types where it means something (empty: all; a node element only).
    pub node_types: Vec<String>,
    /// The kit places the node that carries it; dev.builder does not.
    pub places: bool,
    /// The kits whose elements of the same scene are made before this one's (a scenery after its terrain).
    pub after: Vec<String>,
    /// `<service>.<method>`: `create(spec) -> handle`.
    pub create: String,
    /// `<service>.<method>`: `set(handle, props)`; empty: changed by remove then create.
    pub set: String,
    /// `<service>.<method>`: `remove(handle)`.
    pub remove: String,
    /// `<service>.<method>`: `get(handle) -> props`; empty: the last props given are shown.
    pub get: String,
    /// What it is, for the editor and for an AI.
    pub description: String,
}

/// A kit's section: the elements it exposes (`expose` entries `for: ["scene"]` of its `kit.api.json`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    /// Its elements, in the order of its `kit.api.json`.
    pub elements: Vec<Element>,
}

impl Section {
    /// Whether one of its node elements places the node.
    pub fn places(&self) -> bool {
        self.elements.iter().any(|e| e.at == At::Node && e.places)
    }

    /// The element a written section stands for: the one its `element` key names, else the kit's first one of that
    /// place. `Err` with the names when the key names none.
    pub fn pick(&self, at: At, written: &Map<String, Json>) -> Result<&Element, Vec<String>> {
        let here: Vec<&Element> = self.elements.iter().filter(|e| e.at == at).collect();
        match written.get(ELEMENT_KEY).and_then(Json::as_str) {
            Some(name) => here.iter().find(|e| e.name == name).copied(),
            None => here.first().copied(),
        }
        .ok_or_else(|| here.iter().map(|e| e.name.clone()).collect())
    }
}

/// The key of a section that names which of the kit's elements it is, when the kit exposes several at one place.
pub const ELEMENT_KEY: &str = "element";

/// Both registries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registry {
    /// The node types, by name.
    pub node_types: BTreeMap<String, NodeType>,
    /// The kits' sections, by kit id.
    pub sections: BTreeMap<String, Section>,
    /// The project level of each kit: the `configSchema` of its kit.api.json, what `installed[].config` of the
    /// project file holds for it (the kit reads it itself, `kx.config()`), by kit id.
    pub configs: BTreeMap<String, Json>,
}

fn field_of(raw: &Json) -> Field {
    let s = |k: &str| raw.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    Field {
        kind: s("type"),
        default: raw.get("default").cloned().unwrap_or(Json::Null),
        choices: raw
            .get("choices")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect(),
        description: s("description"),
    }
}

impl Registry {
    /// The base node types and no section.
    pub fn base() -> Self {
        let mut r = Registry::default();
        let doc: Json =
            serde_json::from_str(BASE_NODE_TYPES).expect("node_types.json is valid JSON");
        r.add_node_types(&doc);
        r
    }

    /// Add node types from a `{ <name>: { description, props, when, lowers, section? } }` document (a kit, a mod).
    pub fn add_node_types(&mut self, doc: &Json) {
        for (name, t) in doc.as_object().into_iter().flatten() {
            let props = t
                .get("props")
                .and_then(Json::as_object)
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), field_of(v)))
                .collect();
            let when = t
                .get("when")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect();
            let description = t
                .get("description")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string();
            let lowers = t
                .get("lowers")
                .and_then(Json::as_str)
                .unwrap_or("none")
                .to_string();
            let section = t.get("section").and_then(|v| {
                let k = v.get("kit")?.as_str()?.to_string();
                let e = v.get("element")?.as_str()?.to_string();
                Some((k, e))
            });
            self.node_types.insert(
                name.clone(),
                NodeType {
                    description,
                    props,
                    when,
                    lowers,
                    section,
                },
            );
        }
    }

    /// Add the elements a kit exposes: the `expose` entries of its `kit.api.json` whose `for` holds `scene`, and the
    /// node types it declares with them (`nodeType`).
    pub fn add_kit_api(&mut self, api: &Json) {
        let Some(kit) = api.get("id").and_then(Json::as_str) else {
            return;
        };
        if let Some(c) = api.get("configSchema").filter(|c| c.is_object()) {
            self.configs.insert(kit.to_string(), c.clone());
        }
        let entries = api
            .get("expose")
            .and_then(Json::as_array)
            .into_iter()
            .flatten();
        for e in entries.filter(|e| for_scene(e)) {
            // The registry of node types: a kit declares its own (`"nodeType": "collision"` on a node element) beside
            // the base ones; the node then carries the element's properties under that key, read by this kit.
            if let (Some(ty), Some(el)) =
                (e.get("nodeType").and_then(Json::as_str), element_of(kit, e))
            {
                self.node_types.insert(
                    ty.to_string(),
                    NodeType {
                        description: el.description.clone(),
                        props: BTreeMap::new(),
                        when: vec!["event".to_string()],
                        lowers: "none".to_string(),
                        section: Some((kit.to_string(), el.name.clone())),
                    },
                );
            }
            if let Some(el) = element_of(kit, e) {
                self.sections
                    .entry(kit.to_string())
                    .or_default()
                    .elements
                    .push(el);
            }
        }
    }

    /// Add the elements a kit hands at run time (`dev.builder.expose(kit, entries)`): the same entries.
    pub fn add_entries(&mut self, kit: &str, entries: &Json) {
        self.add_kit_api(&serde_json::json!({ "id": kit, "expose": entries }));
    }
}

/// Whether an `expose` entry is meant for scenes.
pub fn for_scene(e: &Json) -> bool {
    e.get("for")
        .and_then(Json::as_array)
        .is_some_and(|f| f.iter().any(|r| r == "scene"))
}

fn strings(v: Option<&Json>) -> Vec<String> {
    v.and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .map(str::to_string)
        .collect()
}

/// One element from its `expose` entry; `None` when its id is not `<kit>.<name>` or it has no `create`.
fn element_of(kit: &str, e: &Json) -> Option<Element> {
    let s = |k: &str| e.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let name = s("id").strip_prefix(&format!("{kit}."))?.to_string();
    if s("create").is_empty() {
        return None;
    }
    let props = e
        .get("props")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), field_of(v)))
        .collect();
    Some(Element {
        name,
        at: if s("at") == "scene" {
            At::Scene
        } else {
            At::Node
        },
        props,
        node_types: strings(e.get("nodeTypes")),
        places: e.get("places") == Some(&Json::Bool(true)),
        after: strings(e.get("after")),
        create: s("create"),
        set: s("set"),
        remove: s("remove"),
        get: s("get"),
        description: s("description"),
    })
}

/// Whether a value fits a field's type (`json` and unknown types accept anything; `null` is "not written").
pub fn fits(field: &Field, v: &Json) -> bool {
    if v.is_null() {
        return true;
    }
    match field.kind.as_str() {
        "number" | "f32" | "f64" => v.is_number(),
        "u8" | "u16" | "u32" | "u64" => v.as_u64().is_some(),
        "i32" => v.as_i64().is_some(),
        "bool" => v.is_boolean(),
        "string" | "path" => v.is_string(),
        "enum" => v
            .as_str()
            .is_some_and(|s| field.choices.iter().any(|c| c == s)),
        "vec3" | "f64x3" => v
            .as_array()
            .is_some_and(|a| a.len() == 3 && a.iter().all(Json::is_number)),
        _ => true,
    }
}

/// A short form of a field's type, for a message (`a number`, `one of box, sphere`).
pub fn expected(field: &Field) -> String {
    match field.kind.as_str() {
        "number" | "f32" | "f64" => "a number".into(),
        "u8" | "u16" | "u32" | "u64" => "a whole number from 0".into(),
        "i32" => "a whole number".into(),
        "bool" => "true or false".into(),
        "string" => "a text".into(),
        "path" => "a path in the project, like models/crate.glb".into(),
        "enum" => format!("one of {}", field.choices.join(", ")),
        "vec3" | "f64x3" => "three numbers [x, y, z]".into(),
        k => format!("a {k}"),
    }
}

/// The object of a key, or an empty one.
pub fn obj<'a>(v: Option<&'a Json>, empty: &'a Map<String, Json>) -> &'a Map<String, Json> {
    v.and_then(Json::as_object).unwrap_or(empty)
}

/// The project level of a kit checked against its `configSchema` (the `config` of its `installed` entry in the
/// project file): a key the schema closes out (`additionalProperties: false`) or a value of another JSON type is a
/// warning; the kit keeps its default for it. Only the top-level keys: the rest is the kit's own reading.
pub fn check_config(reg: &Registry, kit: &str, config: &Json) -> Vec<crate::Issue> {
    use crate::Issue;
    let (Some(schema), Some(given)) = (reg.configs.get(kit), config.as_object()) else {
        return Vec::new();
    };
    let props = schema.get("properties").and_then(Json::as_object);
    let closed = schema.get("additionalProperties") == Some(&Json::Bool(false));
    let at = |k: &str| format!("installed.{kit}.config.{k}");
    let mut out = Vec::new();
    for (k, v) in given {
        match props.and_then(|p| p.get(k)) {
            None if closed => out.push(
                Issue::warning(
                    "config_field_unknown",
                    format!("{kit} declares no config {k}: it is kept and not read"),
                    format!(
                        "use one of {}",
                        props
                            .map(|p| p.keys().cloned().collect::<Vec<_>>().join(", "))
                            .unwrap_or_default()
                    ),
                )
                .at("", &at(k)),
            ),
            Some(p) => {
                let ty = p.get("type").and_then(Json::as_str).unwrap_or("");
                let ok = match ty {
                    "boolean" => v.is_boolean(),
                    "number" => v.is_number(),
                    "integer" => v.is_i64() || v.is_u64(),
                    "string" => v.is_string(),
                    "array" => v.is_array(),
                    "object" => v.is_object(),
                    _ => true,
                };
                if !ok && !v.is_null() {
                    out.push(
                        Issue::warning(
                            "config_invalid",
                            format!("{} is not of type {ty}: {kit} keeps its default", at(k)),
                            format!(
                                "write {} as a {ty} (default {})",
                                at(k),
                                p.get("default").unwrap_or(&Json::Null)
                            ),
                        )
                        .at("", &at(k)),
                    );
                }
            }
            None => {}
        }
    }
    out
}
