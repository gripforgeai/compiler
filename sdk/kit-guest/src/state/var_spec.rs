//! The state types (runtime/state.ts) without the store — shared word for word by the core and the kit logic
//! modules (the file is the kit façade's, the core includes it).

use serde_json::{json, Value as Json};

/// A state variable shown by HUDs (`HudVarSpec` of types.ts).
#[derive(Clone, Debug, PartialEq)]
pub struct HudVarSpec {
    pub path: String,
    /// `number` | `string` | `boolean`.
    pub kind: String,
    pub label: String,
    /// `int` | `meter` | `time` | `money`.
    pub format: Option<String>,
    pub owner: String,
}

impl HudVarSpec {
    pub fn new(
        path: &str,
        kind: &str,
        label: &str,
        format: Option<&str>,
        owner: &str,
    ) -> HudVarSpec {
        HudVarSpec {
            path: path.into(),
            kind: kind.into(),
            label: label.into(),
            format: format.map(str::to_string),
            owner: owner.into(),
        }
    }
    pub fn to_json(&self) -> Json {
        let mut o = json!({ "path": self.path, "type": self.kind, "label": self.label, "owner": self.owner });
        if let Some(f) = &self.format {
            o["format"] = Json::String(f.clone());
        }
        o
    }
}

/// A bound path changed: what `KitModule::on_var` receives (`None` is `undefined`).
#[derive(Clone, Debug, PartialEq)]
pub struct VarChange {
    pub kit: usize,
    pub tag: String,
    pub path: String,
    pub value: Option<Json>,
    pub prev: Option<Json>,
}
