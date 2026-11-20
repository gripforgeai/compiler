//! The bounded JSON Schema subset kits declare their archetype components with
//! (packages/gamekits/src/schema/validate.ts `validateSchema`): same checks, same issue paths and messages.
//! `apply_defaults` is its `applyDefaults`.

use crate::js::{number_to_string, stringify, to_js_string};
use serde_json::{Map, Value as Json};

const MAX_DEPTH: usize = 32;

pub struct SchemaIssue {
    pub path: String,
    pub message: String,
}

struct Ctx<'a> {
    defs: Option<&'a Map<String, Json>>,
    issues: Vec<SchemaIssue>,
}

impl Ctx<'_> {
    fn issue(&mut self, path: &str, message: impl Into<String>) {
        self.issues.push(SchemaIssue {
            path: path.to_string(),
            message: message.into(),
        });
    }
}

fn type_name(v: &Json) -> &'static str {
    match v {
        Json::Null => "null",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
        Json::Bool(_) => "boolean",
        Json::Number(_) => "number",
        Json::String(_) => "string",
    }
}

fn limit(schema: &Map<String, Json>, key: &str) -> Option<f64> {
    schema
        .get(key)
        .filter(|v| v.is_number())
        .and_then(Json::as_f64)
}

fn enum_text(list: &[Json]) -> String {
    list.iter()
        .map(|v| to_js_string(Some(v)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `#/$defs/<name>`.
fn ref_name(r: Option<&Json>) -> Option<&str> {
    let name = r?.as_str()?.strip_prefix("#/$defs/")?;
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    ok.then_some(name)
}

/// One node of a parsed pattern. Units are UTF-16 code units, as a JavaScript `RegExp` without the `u` flag
/// reads a string.
enum Node {
    /// One unit inside (or, negated, outside) these ranges.
    Set(Vec<(u16, u16)>, bool),
    Start,
    End,
    /// Alternatives, each a sequence.
    Group(Vec<Vec<Node>>),
    Repeat(Box<Node>, usize, usize),
}

const DIGIT: &[(u16, u16)] = &[(0x30, 0x39)];
const WORD: &[(u16, u16)] = &[(0x30, 0x39), (0x41, 0x5a), (0x5f, 0x5f), (0x61, 0x7a)];
const SPACE: &[(u16, u16)] = &[
    (0x09, 0x0d),
    (0x20, 0x20),
    (0xa0, 0xa0),
    (0x1680, 0x1680),
    (0x2000, 0x200a),
    (0x2028, 0x2029),
    (0x202f, 0x202f),
    (0x205f, 0x205f),
    (0x3000, 0x3000),
    (0xfeff, 0xfeff),
];
/// What `.` leaves out.
const LINE_ENDS: &[(u16, u16)] = &[(0x0a, 0x0a), (0x0d, 0x0d), (0x2028, 0x2029)];
/// A match that needs more steps than this is refused (a pattern that backtracks without end on a hostile value).
const MAX_STEPS: usize = 2_000_000;
/// Nested repetitions of a group: beyond, the match is refused the same way.
const MAX_REPEAT_DEPTH: usize = 4_096;

/// The ASCII character of a unit, `\0` for any other.
fn ascii(c: u16) -> char {
    u8::try_from(c)
        .ok()
        .filter(u8::is_ascii)
        .map_or('\0', char::from)
}

/// The unit of a set that holds exactly one.
fn single(units: &[(u16, u16)]) -> Option<u16> {
    match units {
        [(lo, hi)] if lo == hi => Some(*lo),
        _ => None,
    }
}

struct Parser<'a> {
    p: &'a [u16],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u16> {
        self.p.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let hit = self.peek().map(ascii) == Some(c);
        if hit {
            self.i += 1;
        }
        hit
    }

    /// `a|b|c` up to the end or to the `)` that closes the group.
    fn alternatives(&mut self) -> Option<Vec<Vec<Node>>> {
        let mut alts = vec![Vec::new()];
        while let Some(c) = self.peek() {
            match ascii(c) {
                ')' => break,
                '|' => {
                    self.i += 1;
                    alts.push(Vec::new());
                }
                _ => {
                    let atom = self.atom()?;
                    let node = self.quantified(atom)?;
                    alts.last_mut()?.push(node);
                }
            }
        }
        Some(alts)
    }

    fn atom(&mut self) -> Option<Node> {
        let c = self.peek()?;
        self.i += 1;
        Some(match ascii(c) {
            '^' => Node::Start,
            '$' => Node::End,
            '.' => Node::Set(LINE_ENDS.to_vec(), true),
            '(' => {
                // `(?:…)` groups like `(…)`; a look-around or a named group is outside the subset.
                if self.eat('?') && !self.eat(':') {
                    return None;
                }
                let alts = self.alternatives()?;
                if !self.eat(')') {
                    return None;
                }
                Node::Group(alts)
            }
            '[' => self.class()?,
            '\\' => {
                let (set, negated) = self.escape()?;
                Node::Set(set, negated)
            }
            '*' | '+' | '?' | '{' | '}' | ']' | ')' | '|' => return None,
            _ => Node::Set(vec![(c, c)], false),
        })
    }

    /// After a `\`: the units it stands for, negated or not. `None`: `\b`, a back reference, `\p`, `\k`, `\c`…
    fn escape(&mut self) -> Option<(Vec<(u16, u16)>, bool)> {
        let c = self.peek()?;
        self.i += 1;
        let one = |c: u16| Some((vec![(c, c)], false));
        match ascii(c) {
            'd' => Some((DIGIT.to_vec(), false)),
            'D' => Some((DIGIT.to_vec(), true)),
            'w' => Some((WORD.to_vec(), false)),
            'W' => Some((WORD.to_vec(), true)),
            's' => Some((SPACE.to_vec(), false)),
            'S' => Some((SPACE.to_vec(), true)),
            'n' => one(0x0a),
            't' => one(0x09),
            'r' => one(0x0d),
            'f' => one(0x0c),
            'v' => one(0x0b),
            '0' if !self.peek().is_some_and(|n| ascii(n).is_ascii_digit()) => one(0),
            'x' => one(self.hex(2)?),
            'u' => one(self.hex(4)?),
            a if a.is_ascii_alphanumeric() => None,
            _ => one(c),
        }
    }

    fn hex(&mut self, digits: usize) -> Option<u16> {
        let mut n: u32 = 0;
        for _ in 0..digits {
            n = n * 16 + ascii(self.peek()?).to_digit(16)?;
            self.i += 1;
        }
        u16::try_from(n).ok()
    }

    /// After a `[`.
    fn class(&mut self) -> Option<Node> {
        let negated = self.eat('^');
        let mut set = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            let lo = match ascii(c) {
                ']' => break,
                '\\' => {
                    let (units, not) = self.escape()?;
                    if not {
                        return None;
                    }
                    match single(&units) {
                        Some(unit) => unit,
                        None => {
                            set.extend(units);
                            continue;
                        }
                    }
                }
                _ => c,
            };
            // `lo-hi`; a `-` right before the `]` is a literal.
            let range = self.peek().map(ascii) == Some('-')
                && self.p.get(self.i + 1).is_some_and(|n| ascii(*n) != ']');
            if !range {
                set.push((lo, lo));
                continue;
            }
            self.i += 1;
            let h = self.peek()?;
            self.i += 1;
            let hi = if ascii(h) == '\\' {
                match self.escape()? {
                    (units, false) => single(&units)?,
                    _ => return None,
                }
            } else {
                h
            };
            if hi < lo {
                return None;
            }
            set.push((lo, hi));
        }
        Some(Node::Set(set, negated))
    }

    /// `atom` with the quantifier that follows it, if any.
    fn quantified(&mut self, atom: Node) -> Option<Node> {
        let (min, max) = match self.peek().map(ascii) {
            Some('*') => {
                self.i += 1;
                (0, usize::MAX)
            }
            Some('+') => {
                self.i += 1;
                (1, usize::MAX)
            }
            Some('?') => {
                self.i += 1;
                (0, 1)
            }
            Some('{') => {
                let end = (self.i..self.p.len()).find(|&j| ascii(self.p[j]) == '}')?;
                let inner = String::from_utf16(&self.p[self.i + 1..end]).ok()?;
                self.i = end + 1;
                let count = |t: &str| {
                    let digits = !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
                    digits.then(|| t.parse::<usize>().ok()).flatten()
                };
                match inner.split_once(',') {
                    None => {
                        let n = count(&inner)?;
                        (n, n)
                    }
                    Some((a, "")) => (count(a)?, usize::MAX),
                    Some((a, b)) => (count(a)?, count(b)?),
                }
            }
            _ => return Some(atom),
        };
        if matches!(atom, Node::Start | Node::End) || min > max {
            return None;
        }
        // A lazy quantifier accepts the same strings as the greedy one.
        self.eat('?');
        Some(Node::Repeat(Box::new(atom), min, max))
    }
}

struct Run<'a> {
    s: &'a [u16],
    steps: usize,
    depth: usize,
}

type Next<'a> = &'a mut dyn FnMut(usize, &mut Run) -> bool;

fn accepts(set: &[(u16, u16)], negated: bool, c: u16) -> bool {
    set.iter().any(|(lo, hi)| *lo <= c && c <= *hi) != negated
}

/// `nodes` from unit `i`, then `next` from where they end: backtracking, greedy first.
fn seq(nodes: &[Node], i: usize, run: &mut Run, next: Next) -> bool {
    run.steps += 1;
    if run.steps > MAX_STEPS {
        return false;
    }
    let Some((first, rest)) = nodes.split_first() else {
        return next(i, run);
    };
    match first {
        Node::Set(set, negated) => {
            i < run.s.len() && accepts(set, *negated, run.s[i]) && seq(rest, i + 1, run, next)
        }
        Node::Start => i == 0 && seq(rest, i, run, next),
        Node::End => i == run.s.len() && seq(rest, i, run, next),
        Node::Group(alts) => alts
            .iter()
            .any(|alt| seq(alt, i, run, &mut |j, run| seq(rest, j, run, next))),
        Node::Repeat(inner, min, max) => match &**inner {
            // One unit at a time: as many as it takes, then giving back one at a time (no recursion per unit).
            Node::Set(set, negated) => {
                let mut n = 0;
                while n < *max && i + n < run.s.len() && accepts(set, *negated, run.s[i + n]) {
                    n += 1;
                }
                loop {
                    if n < *min {
                        return false;
                    }
                    if seq(rest, i + n, run, next) {
                        return true;
                    }
                    if n == 0 {
                        return false;
                    }
                    n -= 1;
                }
            }
            inner => repeat(inner, (*min, *max), 0, i, run, &mut |j, run| {
                seq(rest, j, run, next)
            }),
        },
    }
}

/// `inner` once more when it may (greedy), else `next`, once it was met `bounds.0` times.
fn repeat(
    inner: &Node,
    bounds: (usize, usize),
    count: usize,
    i: usize,
    run: &mut Run,
    next: Next,
) -> bool {
    let (min, max) = bounds;
    if count < max {
        run.depth += 1;
        if run.depth > MAX_REPEAT_DEPTH {
            run.steps = MAX_STEPS + 1;
        }
        let more = run.steps <= MAX_STEPS
            && seq(std::slice::from_ref(inner), i, run, &mut |j, run| {
                // An iteration that took nothing only counts towards the minimum: no endless loop.
                (j > i || count < min) && repeat(inner, bounds, count + 1, j, run, next)
            });
        run.depth -= 1;
        if more {
            return true;
        }
    }
    count >= min && next(i, run)
}

/// `new RegExp(pattern).test(value)` for the patterns kit schemas use: literals, classes, groups, alternatives,
/// `^`, `$`, `.`, the escapes `\d \w \s` (and `\D \W \S`), `\n \t \r \f \v \0 \xHH \uHHHH`, escaped punctuation,
/// and the quantifiers `* + ? {n} {n,} {n,m}`, lazy or not. Like `test`, the match may start anywhere: a pattern
/// anchors itself. `None`: a construct outside that subset (look-around, back reference, `\b`, named group,
/// Unicode class) — validate.ts refuses such a pattern when the schema is declared, and the value is refused here.
/// tests/patterns.json holds every pattern of the catalogue with the verdicts of V8.
pub(crate) fn pattern_matches(pattern: &str, value: &str) -> Option<bool> {
    let p: Vec<u16> = pattern.encode_utf16().collect();
    let mut parser = Parser { p: &p, i: 0 };
    let alts = parser.alternatives()?;
    if parser.i != p.len() {
        return None;
    }
    let root = [Node::Group(alts)];
    let s: Vec<u16> = value.encode_utf16().collect();
    let mut run = Run {
        s: &s,
        steps: 0,
        depth: 0,
    };
    Some((0..=s.len()).any(|start| seq(&root, start, &mut run, &mut |_, _| true)))
}

fn walk(schema: &Json, value: &Json, path: &str, ctx: &mut Ctx, depth: usize) {
    if depth > MAX_DEPTH {
        return ctx.issue(path, "schema nesting too deep");
    }
    let empty = Map::new();
    let s = schema.as_object().unwrap_or(&empty);
    // `nullable: true` on any schema: `null` is valid too.
    if value.is_null() && s.get("nullable") == Some(&Json::Bool(true)) {
        return;
    }
    if s.contains_key("$ref") {
        let target = ref_name(s.get("$ref")).and_then(|n| ctx.defs.and_then(|d| d.get(n)));
        match target {
            Some(t) => walk(&t.clone(), value, path, ctx, depth + 1),
            None => ctx.issue(
                path,
                format!("unresolvable $ref {}", to_js_string(s.get("$ref"))),
            ),
        }
        return;
    }
    if s.contains_key("oneOf") {
        let list = s.get("oneOf").and_then(Json::as_array);
        let matches = list
            .into_iter()
            .flatten()
            .filter(|alt| validate_with(alt, value, path, ctx.defs).is_empty())
            .count();
        if matches != 1 {
            ctx.issue(
                path,
                if matches == 0 {
                    "matches none of the alternatives".to_string()
                } else {
                    format!("matches {matches} alternatives, expected exactly one")
                },
            );
        }
        return;
    }
    if let Some(c) = s.get("const") {
        if !crate::js::strict_eq(Some(value), Some(c)) {
            ctx.issue(path, format!("expected {}", stringify(c)));
        }
        return;
    }
    // A list of types (`["string", "null"]`): the value is checked against the listed type it is (`null` passes when
    // listed); a value of none of them says the types allowed. A correct value is never refused.
    if let Some(list) = s.get("type").and_then(Json::as_array) {
        let names: Vec<&str> = list.iter().filter_map(Json::as_str).collect();
        if value.is_null() && names.contains(&"null") {
            return;
        }
        let fits = |t: &str| match t {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "number" => value.is_number(),
            "integer" => value.as_f64().is_some_and(|n| n == n.trunc()),
            _ => false,
        };
        let Some(t) = names.iter().find(|t| fits(t)) else {
            return ctx.issue(
                path,
                format!("expected {}, got {}", names.join(" or "), type_name(value)),
            );
        };
        let mut one = s.clone();
        one.insert("type".into(), Json::String((*t).to_string()));
        return walk(&Json::Object(one), value, path, ctx, depth + 1);
    }
    // No `type`: any value; only the keywords that need none are read (`enum`, `anyOf`, `allOf`, and `properties` on an
    // object).
    if !s.contains_key("type") {
        if let Some(list) = s.get("enum").and_then(Json::as_array) {
            if !list.contains(value) {
                ctx.issue(path, format!("expected one of {}", enum_text(list)));
            }
        }
        if let Some(alts) = s.get("anyOf").and_then(Json::as_array) {
            if !alts
                .iter()
                .any(|alt| validate_with(alt, value, path, ctx.defs).is_empty())
            {
                ctx.issue(path, "matches none of the alternatives");
            }
        }
        for alt in s
            .get("allOf")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            walk(alt, value, path, ctx, depth + 1);
        }
        if value.is_object() && s.contains_key("properties") {
            let mut one = s.clone();
            one.insert("type".into(), Json::String("object".into()));
            walk(&Json::Object(one), value, path, ctx, depth + 1);
        }
        return;
    }
    let kind = to_js_string(s.get("type"));
    match kind.as_str() {
        "object" => {
            let Some(o) = value.as_object() else {
                return ctx.issue(path, format!("expected object, got {}", type_name(value)));
            };
            let props = s.get("properties").and_then(Json::as_object);
            for key in s
                .get("required")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_str)
            {
                if !o.contains_key(key) {
                    ctx.issue(&format!("{path}.{key}"), "required");
                }
            }
            for (key, v) in o {
                match props.and_then(|p| p.get(key)) {
                    Some(p) => walk(p, v, &format!("{path}.{key}"), ctx, depth + 1),
                    None if s.get("additionalProperties") == Some(&Json::Bool(false)) => {
                        ctx.issue(&format!("{path}.{key}"), "unknown property")
                    }
                    None => {}
                }
            }
        }
        "array" => {
            let Some(a) = value.as_array() else {
                return ctx.issue(path, format!("expected array, got {}", type_name(value)));
            };
            if let Some(n) = limit(s, "minItems").filter(|n| (a.len() as f64) < *n) {
                ctx.issue(
                    path,
                    format!("expected at least {} items", number_to_string(n)),
                );
            }
            if let Some(n) = limit(s, "maxItems").filter(|n| (a.len() as f64) > *n) {
                ctx.issue(
                    path,
                    format!("expected at most {} items", number_to_string(n)),
                );
            }
            if let Some(items) = s.get("items").filter(|i| crate::js::truthy(Some(i))) {
                for (i, v) in a.iter().enumerate() {
                    walk(items, v, &format!("{path}[{i}]"), ctx, depth + 1);
                }
            }
        }
        "string" => {
            let Some(text) = value.as_str() else {
                return ctx.issue(path, format!("expected string, got {}", type_name(value)));
            };
            if let Some(list) = s.get("enum").and_then(Json::as_array) {
                if !list.contains(value) {
                    ctx.issue(path, format!("expected one of {}", enum_text(list)));
                }
            }
            let len = text.encode_utf16().count() as f64;
            if let Some(n) = limit(s, "minLength").filter(|n| len < *n) {
                ctx.issue(
                    path,
                    format!("expected at least {} characters", number_to_string(n)),
                );
            }
            if let Some(n) = limit(s, "maxLength").filter(|n| len > *n) {
                ctx.issue(
                    path,
                    format!("expected at most {} characters", number_to_string(n)),
                );
            }
            // A pattern outside the subset refuses the value: a mod is never applied on a check that was skipped.
            if let Some(p) = s.get("pattern").and_then(Json::as_str) {
                if pattern_matches(p, text) != Some(true) {
                    ctx.issue(path, format!("does not match {p}"));
                }
            }
        }
        "number" | "integer" => {
            let Some(n) = value.as_f64().filter(|_| value.is_number()) else {
                return ctx.issue(path, format!("expected {kind}, got {}", type_name(value)));
            };
            if kind == "integer" && n != n.trunc() {
                ctx.issue(path, "expected integer");
            }
            if let Some(m) = limit(s, "minimum").filter(|m| n < *m) {
                ctx.issue(path, format!("expected >= {}", number_to_string(m)));
            }
            if let Some(m) = limit(s, "maximum").filter(|m| n > *m) {
                ctx.issue(path, format!("expected <= {}", number_to_string(m)));
            }
            if let Some(list) = s.get("enum").and_then(Json::as_array) {
                if !list.iter().any(|x| x.is_number() && x.as_f64() == Some(n)) {
                    ctx.issue(path, format!("expected one of {}", enum_text(list)));
                }
            }
        }
        "boolean" => {
            if !value.is_boolean() {
                ctx.issue(path, format!("expected boolean, got {}", type_name(value)));
            }
        }
        other => ctx.issue(path, format!("unsupported schema type {other}")),
    }
}

fn validate_with(
    schema: &Json,
    value: &Json,
    path: &str,
    defs: Option<&Map<String, Json>>,
) -> Vec<SchemaIssue> {
    let mut ctx = Ctx {
        defs,
        issues: Vec::new(),
    };
    walk(schema, value, path, &mut ctx, 0);
    ctx.issues
}

/// Issues found in `value` against `schema`; empty when valid.
pub fn validate_schema(schema: &Json, value: &Json, path: &str) -> Vec<SchemaIssue> {
    let defs = schema.get("$defs").and_then(Json::as_object);
    validate_with(schema, value, path, defs)
}

/// `deref` of validate.ts: one level, and a `$ref` that does not resolve leaves the schema as it is.
fn deref<'a>(schema: &'a Json, defs: Option<&'a Map<String, Json>>) -> &'a Json {
    ref_name(schema.get("$ref"))
        .and_then(|name| defs?.get(name))
        .unwrap_or(schema)
}

fn fill(
    schema: &Json,
    value: Option<&Json>,
    defs: Option<&Map<String, Json>>,
    depth: usize,
) -> Option<Json> {
    if depth > MAX_DEPTH {
        return value.cloned();
    }
    let s = deref(schema, defs);
    let Some(value) = value else {
        if let Some(default) = s.get("default") {
            return Some(default.clone());
        }
        // A missing object is filled from an empty one, and kept only when it ends up with content.
        let props = s.get("properties").and_then(Json::as_object)?;
        if s.get("type").and_then(Json::as_str) != Some("object") {
            return None;
        }
        let out = fill_props(props, Map::new(), defs, depth);
        // Kept only with content and every key its schema requires (`applyDefaults`): the defaults of its inner
        // properties alone never make an object the schema would refuse.
        let complete = s
            .get("required")
            .and_then(Json::as_array)
            .is_none_or(|keys| {
                keys.iter()
                    .filter_map(Json::as_str)
                    .all(|k| out.contains_key(k))
            });
        return (!out.is_empty() && complete).then_some(Json::Object(out));
    };
    match (s.get("type").and_then(Json::as_str), value) {
        (Some("object"), Json::Object(base)) => match s.get("properties").and_then(Json::as_object)
        {
            Some(props) => Some(Json::Object(fill_props(props, base.clone(), defs, depth))),
            None => Some(value.clone()),
        },
        (Some("array"), Json::Array(list)) => {
            match s.get("items").filter(|i| crate::js::truthy(Some(i))) {
                Some(items) => Some(Json::Array(
                    list.iter()
                        .map(|v| fill(items, Some(v), defs, depth + 1).unwrap_or(Json::Null))
                        .collect(),
                )),
                None => Some(value.clone()),
            }
        }
        _ => Some(value.clone()),
    }
}

/// The properties of an object schema over `out` (a copy of the value): present keys keep their place, filled
/// ones are appended in the order of the schema, like `{ ...base }` then `out[key] = filled`.
fn fill_props(
    props: &Map<String, Json>,
    mut out: Map<String, Json>,
    defs: Option<&Map<String, Json>>,
    depth: usize,
) -> Map<String, Json> {
    for (key, prop) in props {
        let missing = !out.contains_key(key);
        let Some(filled) = fill(prop, out.get(key), defs, depth + 1) else {
            continue;
        };
        if missing && filled.as_object().is_some_and(Map::is_empty) {
            continue;
        }
        out.insert(key.clone(), filled);
    }
    out
}

/// `applyDefaults(schema, value)`: deep-fills the `default`s of the missing object properties and returns a new
/// value. `None` is JavaScript's `undefined` (a missing value, in and out); a JSON `null` is a value and is kept.
/// `$ref`s resolve in the `$defs` of `schema`.
pub fn apply_defaults(schema: &Json, value: Option<&Json>) -> Option<Json> {
    apply_defaults_in(schema, value, schema.get("$defs").and_then(Json::as_object))
}

/// `applyDefaults(schema, value, root)`: the same with the `$defs` of another root schema.
pub fn apply_defaults_in(
    schema: &Json,
    value: Option<&Json>,
    defs: Option<&Map<String, Json>>,
) -> Option<Json> {
    fill(schema, value, defs, 0)
}
