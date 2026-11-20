//! The game's own code: the Rust files `project.gripforge` declares in `sources` (decision of gripforgeai/gripforge#420,
//! announced on gripforgeai/compiler#10: the project is the only entry, no file is taken by convention; it replaces the
//! `index.rs` that 0.1.2 took from beside the project).
//!
//! ```json
//! "sources": ["rules.rs", "code/weapons.rs"]
//! ```
//!
//! Each file is a module of the game's module `game.main` (`.build/game`), taken by path (`#[path]`), never copied nor
//! written: `rules.rs` is `crate::rules`, `code/weapons.rs` is `crate::weapons`. One of them declares **the module of
//! the game**: `pub struct X;` with `impl KitModule for X` (the trait of the kits' modules, `gripforge_kit_guest`). The
//! generated `module.rs` hands every hook of `game.main` to it (`setup`, `ready`, `update` at the tick, `on_event`,
//! `scene_enter` once the scene is posed, …): the same code native and wasm, in the official web player too.
//!
//! The format of the key (a list of relative `.rs` paths, each once) is checked by the common core
//! (`gripforge_scene_format::sources`); this module checks the disk: the file is there, it stays inside the project
//! (links resolved), its module name is free, it declares no `mod x;` of its own (every file of the game is declared),
//! and there is one module of the game.

use gripforge_scene_format::sources::{declared, normal, KEY};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

/// One declared file.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// As `project.gripforge` writes it.
    pub declared: String,
    /// The file (absolute, links resolved).
    pub file: PathBuf,
    /// Its module in the crate of game.main (`crate::<module>`): the name of the file.
    pub module: String,
}

/// The game's code: its files, and the module of the game (`<module>::<name>`).
#[derive(Debug, Clone, PartialEq)]
pub struct GameCode {
    pub sources: Vec<Source>,
    /// The module (file) that declares it.
    pub entry_module: String,
    /// The unit struct that implements `KitModule`.
    pub entry: String,
}

impl GameCode {
    /// `crate::<module>::<name>`, for the report.
    pub fn entry_path(&self) -> String {
        format!("crate::{}::{}", self.entry_module, self.entry)
    }
}

/// Names a source file cannot take: Rust keywords, and the modules and crates the generated root already names.
const RESERVED: &[&str] = &[
    "lib",
    "main",
    "module",
    "init_scene",
    "host",
    "gripforge_kit_guest",
    "gripforge_kit_access",
    "serde_json",
    "std",
    "core",
    "alloc",
];
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "gen", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
];

fn issue(code: &str, file: &Path, field: &str, message: String, fix: &str) -> Json {
    json!({ "level": "error", "code": code, "file": file.to_string_lossy(), "field": field, "message": message, "fix": fix })
}

/// Rust source without its comments and the text of its strings and characters: what is looked for in.
pub fn code_only(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1)) {
            (b'/', Some(b'/')) => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            (b'/', Some(b'*')) => {
                let mut depth = 0;
                while i < b.len() {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                out.push(' ');
            }
            (b'"', _) => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
                out.push_str("\"\"");
            }
            _ => {
                let c = text[i..].chars().next().unwrap_or(' ');
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// The tokens of Rust source: identifiers, and every other character but spaces alone.
pub fn tokens(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in code.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        if !c.is_whitespace() {
            out.push(c.to_string());
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn is_ident(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The modules of the game a source declares: each `pub struct X;` with `impl KitModule for X` (a path ending in
/// `KitModule` too: `gripforge_kit_guest::KitModule`).
pub fn find_modules(text: &str) -> Vec<String> {
    let t = tokens(&code_only(text));
    let mut impls = Vec::new();
    let mut units = Vec::new();
    for k in 0..t.len() {
        if t[k] == "impl" {
            let end = t[k..]
                .iter()
                .position(|x| x == "for" || x == "{" || x == ";")
                .map(|e| k + e);
            if let Some(e) = end.filter(|e| t[*e] == "for" && *e > k && t[*e - 1] == "KitModule") {
                if let Some(name) = t.get(e + 1) {
                    impls.push(name.clone());
                }
            }
        }
        if t[k] == "pub" && t.get(k + 1).map(String::as_str) == Some("struct") {
            if let (Some(name), Some(";")) = (t.get(k + 2), t.get(k + 3).map(String::as_str)) {
                units.push(name.clone());
            }
        }
    }
    let mut found: Vec<String> = impls.into_iter().filter(|n| units.contains(n)).collect();
    found.dedup();
    found
}

/// The `mod x;` a source declares (a module in a file of its own, which the project does not list).
pub fn file_modules(text: &str) -> Vec<String> {
    let t = tokens(&code_only(text));
    (0..t.len())
        .filter(|&k| {
            t[k] == "mod"
                && t.get(k + 1).is_some_and(|n| is_ident(n))
                && t.get(k + 2).map(String::as_str) == Some(";")
        })
        .map(|k| t[k + 1].clone())
        .collect()
}

/// The game's code of a project (`dir`: the folder of `file`, the project file). `Ok(None)`: no `sources` (the module
/// of the game only poses the scenes). `warnings`: the `.rs` files beside the project that it does not declare.
pub fn game_code(
    project: &Json,
    file: &Path,
    dir: &Path,
    warnings: &mut Vec<Json>,
) -> Result<Option<GameCode>, Vec<Json>> {
    let list = declared(project);
    let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut issues = Vec::new();
    let mut sources: Vec<Source> = Vec::new();
    let mut found: Vec<(String, String)> = Vec::new();
    for (i, path) in list.iter().enumerate() {
        let field = format!("{KEY}[{i}]");
        let Some(n) = normal(path) else {
            continue; // said by the common core (source_outside)
        };
        let at = dir.join(&n);
        if !at.is_file() {
            issues.push(issue("source_missing", file, &field, format!("{path}, declared in sources, is not there ({})", at.display()),
                "write the file, or remove it from sources (paths are relative to the folder of the project file)"));
            continue;
        }
        let real = at.canonicalize().unwrap_or_else(|_| at.clone());
        if !real.starts_with(&root) {
            issues.push(issue("source_outside", file, &field, format!("{path} leads outside the project ({})", real.display()),
                "keep the game's code inside the folder of the project file (a link may not lead out of it)"));
            continue;
        }
        let module = real
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(other) = sources.iter().find(|s| s.file == real) {
            issues.push(issue(
                "source_duplicate",
                file,
                &field,
                format!(
                    "{path} is the same file as {}, already in sources",
                    other.declared
                ),
                "list each file of the game's code once",
            ));
            continue;
        }
        if !is_ident(&module)
            || KEYWORDS.contains(&module.as_str())
            || RESERVED.contains(&module.as_str())
        {
            issues.push(issue("source_name_invalid", file, &field,
                format!("{path}: its module would be `{module}`, which is not a free Rust module name (a keyword, a name the generated crate takes: {}, or not letters, digits and _)", RESERVED.join(", ")),
                "rename the file: its name is its module (crate::<name>), letters, digits and _"));
            continue;
        }
        if let Some(other) = sources.iter().find(|s| s.module == module) {
            issues.push(issue(
                "source_duplicate",
                file,
                &field,
                format!(
                    "{path} and {} would both be the module crate::{module}",
                    other.declared
                ),
                "give the two files different names: the name of a file is its module",
            ));
            continue;
        }
        let text = match std::fs::read_to_string(&real) {
            Ok(t) => t,
            Err(e) => {
                issues.push(issue(
                    "source_unreadable",
                    file,
                    &field,
                    format!("{path} cannot be read: {e}"),
                    "write the file as UTF-8 Rust",
                ));
                continue;
            }
        };
        for m in file_modules(&text) {
            issues.push(issue("source_mod_undeclared", &real, &field,
                format!("{path} declares `mod {m};`, a file that project.gripforge does not list: every file of the game's code is declared"),
                &format!("add the file of {m} to sources (it is then crate::{m}: `use crate::{m};`), or write the module inline (`mod {m} {{ … }}`)")));
        }
        for name in find_modules(&text) {
            found.push((module.clone(), name));
        }
        sources.push(Source {
            declared: path.clone(),
            file: real,
            module,
        });
    }
    // The .rs files beside the project that it does not declare: never compiled, said.
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut beside: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "rs"))
            .collect();
        beside.sort();
        for p in beside {
            let real = p.canonicalize().unwrap_or_else(|_| p.clone());
            if !sources.iter().any(|s| s.file == real) {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                warnings.push(json!({ "level": "warning", "code": "source_undeclared", "file": p.to_string_lossy(), "field": KEY,
                    "message": format!("{name} is beside the project but not in its sources: it is not compiled (no file is taken by convention)"),
                    "fix": format!("add \"{name}\" to \"sources\" in project.gripforge to compile it into the game, or move it away") }));
            }
        }
    }
    if list.is_empty() && issues.is_empty() {
        return Ok(None);
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    match found.len() {
        1 => {
            let (entry_module, entry) = found.remove(0);
            Ok(Some(GameCode {
                sources,
                entry_module,
                entry,
            }))
        }
        0 => Err(vec![issue("sources_module_missing", file, KEY,
            "no file of sources declares the module of the game: no `pub struct X;` with `impl KitModule for X`".into(),
            "write the module of the game in one of the sources: `pub struct MyGame;` and `impl KitModule for MyGame { fn id(&self) -> &'static str { \"game.main\" } fn setup(&self, kx: &mut Kx) { … } }`")]),
        _ => Err(vec![issue("sources_module_ambiguous", file, KEY,
            format!("the sources declare several modules of the game ({}): the game has one", found.iter().map(|(m, n)| format!("crate::{m}::{n}")).collect::<Vec<_>>().join(", ")),
            "keep one `impl KitModule` in the sources, the game's, and call the others from it")]),
    }
}
