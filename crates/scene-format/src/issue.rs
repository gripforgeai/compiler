//! What the validator, the lowering and the conversion say: one [`Issue`] per problem, with a stable code, the file,
//! the node's path, the field, what is wrong and the fix. Written for a human and for an AI that corrects the file
//! alone (docs/ecrire-un-projet.md lists every code).

use serde_json::{json, Value as Json};

/// How bad an issue is. The validator is tolerant (docs/dev-builder.md, « Un validateur tolérant »): a file loads
/// with what is readable and the schema's defaults fill the rest, so nearly everything is a warning. An error is kept
/// for what no default can mend: a required kit missing (the scene refused whole), a command that cannot apply, a
/// pack that cannot write a kit's version, a verb game.engine would not compile (its whole rule left out).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Stops what it touches: the scene, the command, the pack.
    Error,
    /// Read with the default, or kept as written and not read; never stops the loading.
    Warning,
}

/// One problem found in a project or a scene file.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// Error or warning.
    pub level: Level,
    /// A stable code (`kit_required_missing`, `section_invalid`...): tools match on it, never on the message.
    pub code: &'static str,
    /// The project path of the file (`scenes/start.json`), empty for the project file itself.
    pub file: String,
    /// The node's path in its scene (`Hall/Door`), empty for the file itself.
    pub node: String,
    /// The field inside the node or the file (`kits.core.physics.body`), empty when it is the whole node.
    pub field: String,
    /// What is wrong, in one sentence.
    pub message: String,
    /// What to write instead, in one sentence.
    pub fix: String,
}

impl Issue {
    /// An error.
    pub fn error(code: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::new(Level::Error, code, message, fix)
    }

    /// A warning.
    pub fn warning(code: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::new(Level::Warning, code, message, fix)
    }

    fn new(
        level: Level,
        code: &'static str,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            level,
            code,
            file: String::new(),
            node: String::new(),
            field: String::new(),
            message: message.into(),
            fix: fix.into(),
        }
    }

    /// The same issue, located in a file.
    pub fn in_file(mut self, file: &str) -> Self {
        self.file = file.to_string();
        self
    }

    /// The same issue, located on a node.
    pub fn at(mut self, node: &str, field: &str) -> Self {
        self.node = node.to_string();
        self.field = field.to_string();
        self
    }

    /// Whether it is an error.
    pub fn is_error(&self) -> bool {
        self.level == Level::Error
    }

    /// The JSON form: `{ level, code, file, node, field, message, fix }` (empty locations left out).
    pub fn to_json(&self) -> Json {
        let mut o = json!({
            "level": if self.is_error() { "error" } else { "warning" },
            "code": self.code,
        });
        for (k, v) in [
            ("file", &self.file),
            ("node", &self.node),
            ("field", &self.field),
        ] {
            if !v.is_empty() {
                o[k] = json!(v);
            }
        }
        o["message"] = json!(self.message);
        o["fix"] = json!(self.fix);
        o
    }

    /// One line for a terminal: `error kit_required_missing #start Hall/Door kits.x: message → fix`.
    pub fn line(&self) -> String {
        let level = if self.is_error() { "error" } else { "warning" };
        let mut at: Vec<&str> = Vec::new();
        for v in [&self.file, &self.node, &self.field] {
            if !v.is_empty() {
                at.push(v);
            }
        }
        format!(
            "{level} {} {}: {} → {}",
            self.code,
            at.join(" "),
            self.message,
            self.fix
        )
    }
}

/// Whether a list holds an error.
pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(Issue::is_error)
}
