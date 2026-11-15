//! The tagged value codec (docs/wire-format.md « Values »), v1. One byte of tag, then:
//!
//! | tag | value | body |
//! |---|---|---|
//! | 0 | null | — |
//! | 1 | false | — |
//! | 2 | true | — |
//! | 3 | integer | zigzag LEB128 (i64); a finite number without fraction and |v| <= 2^53-1, whatever its type (as `Number.isSafeInteger`; -0 gives 0) |
//! | 4 | float | f64, 8 bytes little-endian; every other number |
//! | 5 | string | LEB128 byte length + UTF-8 |
//! | 6 | array | LEB128 count + values |
//! | 7 | object | LEB128 count + (key as tag-5 body, value), in insertion order |
//! | 8 | string ref | LEB128 id in the stream's string table (streams with a table only: the frame) |
//! | 9 | object, keys by ref | LEB128 count + (LEB128 key id, value) (streams with a table only) |
//!
//! Non-finite floats are written as null, as `JSON.stringify` and serde_json do.

pub const NULL: u8 = 0;
pub const FALSE: u8 = 1;
pub const TRUE: u8 = 2;
pub const INT: u8 = 3;
pub const FLOAT: u8 = 4;
pub const STRING: u8 = 5;
pub const ARRAY: u8 = 6;
pub const OBJECT: u8 = 7;
pub const STRING_REF: u8 = 8;
pub const OBJECT_REFS: u8 = 9;

#[cfg(feature = "json")]
mod json {
    use super::*;
    use crate::wire::{put_ivar, put_str, put_uvar, Cursor};
    use serde_json::{Map, Number, Value as Json};

    /// Appends `v` (tags 0-7: no string table).
    pub fn encode(out: &mut Vec<u8>, v: &Json) {
        encode_with(out, v, &mut |_| None)
    }

    /// Appends `v`; `intern(s)` answers the id of a string in the stream's table (then object keys are written by
    /// ref, tag 9, and strings found in it as tag 8), or None to write them in full.
    pub fn encode_with(out: &mut Vec<u8>, v: &Json, intern: &mut dyn FnMut(&str) -> Option<u32>) {
        match v {
            Json::Null => out.push(NULL),
            Json::Bool(false) => out.push(FALSE),
            Json::Bool(true) => out.push(TRUE),
            Json::Number(n) => {
                // By value, not by type: the kit host makes many integers as f64 (`num`), TypeScript has one type.
                const SAFE: i64 = (1 << 53) - 1;
                if let Some(i) = n.as_i64().filter(|i| i.abs() <= SAFE) {
                    out.push(INT);
                    put_ivar(out, i);
                } else if let Some(f) = n.as_f64().filter(|f| f.is_finite()) {
                    if f.fract() == 0.0 && f.abs() <= SAFE as f64 {
                        out.push(INT);
                        put_ivar(out, f as i64);
                    } else {
                        out.push(FLOAT);
                        out.extend_from_slice(&f.to_le_bytes());
                    }
                } else {
                    out.push(NULL);
                }
            }
            Json::String(s) => string(out, s, intern),
            Json::Array(a) => {
                out.push(ARRAY);
                put_uvar(out, a.len() as u64);
                for x in a {
                    encode_with(out, x, intern);
                }
            }
            Json::Object(o) => {
                let ids: Option<Vec<u32>> = o.keys().map(|k| intern(k)).collect();
                match ids {
                    Some(ids) => {
                        out.push(OBJECT_REFS);
                        put_uvar(out, o.len() as u64);
                        for (id, x) in ids.into_iter().zip(o.values()) {
                            put_uvar(out, id as u64);
                            encode_with(out, x, intern);
                        }
                    }
                    None => {
                        out.push(OBJECT);
                        put_uvar(out, o.len() as u64);
                        for (k, x) in o {
                            put_str(out, k);
                            encode_with(out, x, intern);
                        }
                    }
                }
            }
        }
    }

    fn string(out: &mut Vec<u8>, s: &str, intern: &mut dyn FnMut(&str) -> Option<u32>) {
        // Short strings only by ref: an id is worth it when the text is longer than its varint.
        match (s.len() > 1).then(|| intern(s)).flatten() {
            Some(id) => {
                out.push(STRING_REF);
                put_uvar(out, id as u64);
            }
            None => {
                out.push(STRING);
                put_str(out, s);
            }
        }
    }

    /// Reads one value (tags 8 and 9 need `table`).
    pub fn decode(c: &mut Cursor, table: &[String]) -> Option<Json> {
        Some(match c.u8()? {
            NULL => Json::Null,
            FALSE => Json::Bool(false),
            TRUE => Json::Bool(true),
            INT => Json::Number(Number::from(c.ivar()?)),
            FLOAT => Number::from_f64(c.f64()?)
                .map(Json::Number)
                .unwrap_or(Json::Null),
            STRING => Json::String(c.str()?.to_string()),
            ARRAY => {
                let n = c.uvar()? as usize;
                let mut a = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    a.push(decode(c, table)?);
                }
                Json::Array(a)
            }
            OBJECT => {
                let n = c.uvar()? as usize;
                let mut o = Map::new();
                for _ in 0..n {
                    let k = c.str()?.to_string();
                    o.insert(k, decode(c, table)?);
                }
                Json::Object(o)
            }
            STRING_REF => Json::String(table.get(c.uvar()? as usize)?.clone()),
            OBJECT_REFS => {
                let n = c.uvar()? as usize;
                let mut o = Map::new();
                for _ in 0..n {
                    let k = table.get(c.uvar()? as usize)?.clone();
                    o.insert(k, decode(c, table)?);
                }
                Json::Object(o)
            }
            _ => return None,
        })
    }
}

#[cfg(feature = "json")]
pub use json::{decode, encode, encode_with};
