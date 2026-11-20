//! Change a JSON text in place, keeping everything else as written: the order of the keys, the layout, the unknown
//! fields. Used to rewrite a project file (`<name>.gripforge`) or a sub-scene it names after an edit with the smallest
//! diff (crate::ops): only the bytes of the changed value move. A small parser that keeps the byte span of every value; no other dependency.

use serde_json::Value as Json;

/// A value of the text and where it is.
#[derive(Clone, Debug)]
pub struct Span {
    /// The first byte of the value.
    pub start: usize,
    /// The byte after the value.
    pub end: usize,
    /// What it holds.
    pub kind: Kind,
}

/// The shape of a value.
#[derive(Clone, Debug)]
pub enum Kind {
    /// An object: its members in the order of the text.
    Object(Vec<(String, Span)>),
    /// An array: its items in order.
    Array(Vec<Span>),
    /// A string, a number, `true`, `false` or `null`.
    Scalar,
}

/// One step down a JSON value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    /// A member of an object.
    Key(String),
    /// An item of an array.
    Index(usize),
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn string(&mut self) -> Result<(), String> {
        self.i += 1;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return Ok(());
                }
                _ => self.i += 1,
            }
        }
        Err("unterminated string".into())
    }
    fn value(&mut self) -> Result<Span, String> {
        self.ws();
        let start = self.i;
        let kind = match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut members = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b'}') {
                        self.i += 1;
                        break;
                    }
                    if self.b.get(self.i) != Some(&b'"') {
                        return Err(format!("a key expected at byte {}", self.i));
                    }
                    let k0 = self.i;
                    self.string()?;
                    let key: String =
                        serde_json::from_slice(&self.b[k0..self.i]).map_err(|e| e.to_string())?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err(format!(": expected at byte {}", self.i));
                    }
                    self.i += 1;
                    members.push((key, self.value()?));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(format!(", or }} expected at byte {}", self.i)),
                    }
                }
                Kind::Object(members)
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b']') {
                        self.i += 1;
                        break;
                    }
                    items.push(self.value()?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(format!(", or ] expected at byte {}", self.i)),
                    }
                }
                Kind::Array(items)
            }
            Some(b'"') => {
                self.string()?;
                Kind::Scalar
            }
            Some(_) => {
                while self.i < self.b.len()
                    && !matches!(
                        self.b[self.i],
                        b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'
                    )
                {
                    self.i += 1;
                }
                if self.i == start {
                    return Err(format!("a value expected at byte {start}"));
                }
                Kind::Scalar
            }
            None => return Err("unexpected end of the text".into()),
        };
        Ok(Span {
            start,
            end: self.i,
            kind,
        })
    }
}

/// Parse a JSON text, keeping the span of every value (the text must also be valid for serde_json).
pub fn parse(text: &str) -> Result<Span, String> {
    serde_json::from_str::<Json>(text).map_err(|e| e.to_string())?;
    let mut p = P {
        b: text.as_bytes(),
        i: 0,
    };
    p.value()
}

/// The value at `path`, or `None`.
pub fn find<'a>(root: &'a Span, path: &[Seg]) -> Option<&'a Span> {
    let mut v = root;
    for s in path {
        v = match (&v.kind, s) {
            (Kind::Object(m), Seg::Key(k)) => &m.iter().find(|(x, _)| x == k)?.1,
            (Kind::Array(a), Seg::Index(i)) => a.get(*i)?,
            _ => return None,
        };
    }
    Some(v)
}

fn line_indent(text: &str, at: usize) -> &str {
    let line = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let lead = &text[line..at];
    let n = lead.len() - lead.trim_start_matches([' ', '\t']).len();
    &lead[..n]
}

/// The JSON of a value, on one line when short, else laid out under `indent`.
pub fn render(v: &Json, indent: &str) -> String {
    layout_value(v, indent, 100)
}

/// A whole file as GripForge writes it: two spaces, a value on one line when it holds in 80 characters (a position,
/// a short node), else one member per line. Read well by a person, an AI and a diff.
pub fn to_text(v: &Json) -> String {
    format!("{}\n", layout_value(v, "", 80))
}

fn layout_value(v: &Json, indent: &str, width: usize) -> String {
    let compact = one_line(v);
    if compact.len() + indent.len() <= width || !(v.is_object() || v.is_array()) {
        return compact;
    }
    let inner = format!("{indent}  ");
    let parts: Vec<String> = match v {
        Json::Object(o) => o
            .iter()
            .map(|(k, x)| {
                format!(
                    "{inner}{}: {}",
                    Json::String(k.clone()),
                    layout_value(x, &inner, width)
                )
            })
            .collect(),
        Json::Array(a) => a
            .iter()
            .map(|x| format!("{inner}{}", layout_value(x, &inner, width)))
            .collect(),
        _ => unreachable!("scalars are compact"),
    };
    let (open, close) = if v.is_object() {
        ("{", "}")
    } else {
        ("[", "]")
    };
    format!("{open}\n{}\n{indent}{close}", parts.join(",\n"))
}

fn one_line(v: &Json) -> String {
    match v {
        Json::Object(o) if o.is_empty() => "{}".into(),
        Json::Object(o) => format!(
            "{{ {} }}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", Json::String(k.clone()), one_line(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Json::Array(a) => format!(
            "[{}]",
            a.iter().map(one_line).collect::<Vec<_>>().join(", ")
        ),
        other => other.to_string(),
    }
}

/// Replace the value at `path` (which exists).
pub fn replace(text: &str, root: &Span, path: &[Seg], v: &Json) -> Option<String> {
    let s = find(root, path)?;
    let indent = line_indent(text, s.start).to_string();
    // A value laid out on several lines keeps that layout, so only its changed lines differ.
    if text[s.start..s.end].contains('\n') && (v.is_array() || v.is_object()) {
        let out = layout_value(v, &indent, 0);
        return Some(format!("{}{out}{}", &text[..s.start], &text[s.end..]));
    }
    Some(format!(
        "{}{}{}",
        &text[..s.start],
        render(v, &indent),
        &text[s.end..]
    ))
}

/// How the members of a container are laid out: on their own lines (and with which indent) or on one line.
fn layout(text: &str, c: &Span, first: Option<&Span>) -> Option<String> {
    if !text[c.start..c.end].contains('\n') {
        return None;
    }
    Some(match first {
        Some(f) => line_indent(text, f.start).to_string(),
        None => format!("{}  ", line_indent(text, c.start)),
    })
}

/// Set `key` in the object at `path`: replaced when it exists, else added after the last member.
pub fn set_key(text: &str, root: &Span, path: &[Seg], key: &str, v: &Json) -> Option<String> {
    let obj = find(root, path)?;
    let Kind::Object(m) = &obj.kind else {
        return None;
    };
    if m.iter().any(|(k, _)| k == key) {
        let mut p = path.to_vec();
        p.push(Seg::Key(key.into()));
        return replace(text, root, &p, v);
    }
    let k = Json::String(key.into());
    match (layout(text, obj, m.first().map(|x| &x.1)), m.last()) {
        (Some(ind), Some((_, last))) => Some(format!(
            "{},\n{ind}{k}: {}{}",
            &text[..last.end],
            render(v, &ind),
            &text[last.end..]
        )),
        (None, Some((_, last))) => Some(format!(
            "{}, {k}: {}{}",
            &text[..last.end],
            render(v, ""),
            &text[last.end..]
        )),
        (_, None) => Some(format!(
            "{}{{ {k}: {} }}{}",
            &text[..obj.start],
            render(v, line_indent(text, obj.start)),
            &text[obj.end..]
        )),
    }
}

/// Insert an item in the array at `path`, before `index` (at the end when `index` is its length or more).
pub fn insert_item(
    text: &str,
    root: &Span,
    path: &[Seg],
    index: usize,
    v: &Json,
) -> Option<String> {
    let arr = find(root, path)?;
    let Kind::Array(items) = &arr.kind else {
        return None;
    };
    let lay = layout(text, arr, items.first());
    if items.is_empty() {
        let ind = line_indent(text, arr.start).to_string();
        let body = render(v, &format!("{ind}  "));
        let out = if body.contains('\n') {
            format!("[\n{ind}  {body}\n{ind}]")
        } else {
            format!("[{body}]")
        };
        return Some(format!("{}{out}{}", &text[..arr.start], &text[arr.end..]));
    }
    let ind = lay.clone().unwrap_or_default();
    let body = render(v, &ind);
    if index < items.len() {
        let at = items[index].start;
        let sep = if lay.is_some() {
            format!(",\n{ind}")
        } else {
            ", ".into()
        };
        Some(format!("{}{body}{sep}{}", &text[..at], &text[at..]))
    } else {
        let at = items[items.len() - 1].end;
        let sep = if lay.is_some() {
            format!(",\n{ind}")
        } else {
            ", ".into()
        };
        Some(format!("{}{sep}{body}{}", &text[..at], &text[at..]))
    }
}

/// Remove the item `index` of the array at `path`, with its separator.
pub fn remove_item(text: &str, root: &Span, path: &[Seg], index: usize) -> Option<String> {
    let arr = find(root, path)?;
    let Kind::Array(items) = &arr.kind else {
        return None;
    };
    let it = items.get(index)?;
    let (from, to) = if items.len() == 1 {
        (arr.start + 1, arr.end - 1)
    } else if index + 1 < items.len() {
        (it.start, items[index + 1].start)
    } else {
        (items[index - 1].end, it.end)
    };
    Some(format!("{}{}", &text[..from], &text[to..]))
}
