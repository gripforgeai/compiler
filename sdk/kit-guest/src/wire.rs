//! The GripForge wire value codec, v1 (docs/wire-format.md § Values): a JSON value as compact bytes, byte for byte
//! `packages/gamekits/src/net/wire.ts` (and `kits/wire`, the shared crate of the player's memory
//! frames, which this module will defer to once it is on master).
//!
//! One tag byte, then its body: 0 null, 1 false, 2 true, 3 integer (zigzag then unsigned LEB128: a finite number
//! without a fraction, |v| ≤ 2^53−1, −0 is 0 — decided by the value, not by the serde type), 4 f64 (8 bytes
//! little-endian, every other finite number), 5 string (byte length LEB128 + UTF-8), 6 array (count + values),
//! 7 object (count + pairs: key as a string body, value), keys in insertion order. Tags 8 and 9 (by reference to an
//! interned table) never appear in a network message.

use serde_json::{Map, Number, Value as Json};

pub const NULL: u8 = 0;
pub const FALSE: u8 = 1;
pub const TRUE: u8 = 2;
pub const INT: u8 = 3;
pub const F64: u8 = 4;
pub const STR: u8 = 5;
pub const ARR: u8 = 6;
pub const OBJ: u8 = 7;
/// Nesting bound of a decoded value.
pub const MAX_DEPTH: usize = 64;
const SAFE: f64 = 9_007_199_254_740_991.0;

pub fn put_uvar(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push((n & 127) as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}

pub fn put_str(out: &mut Vec<u8>, s: &str) {
    put_uvar(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn put_int(out: &mut Vec<u8>, v: i64) {
    out.push(INT);
    put_uvar(out, ((v << 1) ^ (v >> 63)) as u64);
}

/// Appends a value.
pub fn put_value(out: &mut Vec<u8>, v: &Json) {
    match v {
        Json::Null => out.push(NULL),
        Json::Bool(false) => out.push(FALSE),
        Json::Bool(true) => out.push(TRUE),
        Json::Number(n) => {
            if let Some(i) = n.as_i64().filter(|i| (*i as f64).abs() <= SAFE) {
                return put_int(out, i);
            }
            let f = n.as_f64().unwrap_or(f64::NAN);
            if !f.is_finite() {
                out.push(NULL);
            } else if f == f.trunc() && f.abs() <= SAFE {
                put_int(out, f as i64);
            } else {
                out.push(F64);
                out.extend_from_slice(&f.to_le_bytes());
            }
        }
        Json::String(s) => {
            out.push(STR);
            put_str(out, s);
        }
        Json::Array(a) => {
            out.push(ARR);
            put_uvar(out, a.len() as u64);
            for x in a {
                put_value(out, x);
            }
        }
        Json::Object(o) => {
            out.push(OBJ);
            put_uvar(out, o.len() as u64);
            for (k, x) in o {
                put_str(out, k);
                put_value(out, x);
            }
        }
    }
}

/// A value as wire bytes.
pub fn encode(v: &Json) -> Vec<u8> {
    let mut out = Vec::new();
    put_value(&mut out, v);
    out
}

/// Reads values from bytes; every read answers None on a truncated or malformed input.
pub struct Cursor<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a [u8], pos: usize) -> Self {
        Cursor { buf, pos }
    }
    pub fn byte(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }
    pub fn uvar(&mut self) -> Option<u64> {
        let mut n = 0u64;
        for i in 0..10 {
            let b = self.byte()?;
            n |= u64::from(b & 127).checked_shl(7 * i)?;
            if b & 128 == 0 {
                return Some(n);
            }
        }
        None
    }
    pub fn str(&mut self) -> Option<String> {
        let n = usize::try_from(self.uvar()?).ok()?;
        let end = self.pos.checked_add(n)?;
        let s = std::str::from_utf8(self.buf.get(self.pos..end)?)
            .ok()?
            .to_string();
        self.pos = end;
        Some(s)
    }
    pub fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        Some(match self.byte()? {
            NULL => Json::Null,
            FALSE => Json::Bool(false),
            TRUE => Json::Bool(true),
            INT => {
                let z = self.uvar()?;
                let v = (z >> 1) as i64 ^ -((z & 1) as i64);
                Json::Number(Number::from(v))
            }
            F64 => {
                let bytes: [u8; 8] = self.buf.get(self.pos..self.pos + 8)?.try_into().ok()?;
                self.pos += 8;
                let f = f64::from_le_bytes(bytes);
                Number::from_f64(f).map_or(Json::Null, Json::Number)
            }
            STR => Json::String(self.str()?),
            ARR => {
                let n = usize::try_from(self.uvar()?).ok()?;
                if n > self.buf.len() - self.pos {
                    return None;
                }
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(self.value(depth + 1)?);
                }
                Json::Array(a)
            }
            OBJ => {
                let n = usize::try_from(self.uvar()?).ok()?;
                if n > self.buf.len() - self.pos {
                    return None;
                }
                let mut o = Map::new();
                for _ in 0..n {
                    let k = self.str()?;
                    let v = self.value(depth + 1)?;
                    o.insert(k, v);
                }
                Json::Object(o)
            }
            _ => return None,
        })
    }
}

/// Wire bytes back to a value; None on bytes left over or a malformed input.
pub fn decode(bytes: &[u8]) -> Option<Json> {
    let mut c = Cursor::new(bytes, 0);
    let v = c.value(0)?;
    (c.pos == bytes.len()).then_some(v)
}
