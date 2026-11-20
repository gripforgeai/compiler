//! gripforge-wire — the binary format shared by the kit logic (gripforge-kithost, compiled to wasm), the player, its
//! modules and the network: no JSON per frame, only what changed. Specification: docs/wire-format.md.
//!
//! - [`value`]: the tagged value codec (1-byte tags, LEB128 varints, zigzag integers, f64, length-prefixed UTF-8),
//!   the same byte for byte in JavaScript (player/web/wire.mjs) and in the network messages;
//! - [`frame`]: the delta frame of a shared memory region (a fixed header, then sections: new strings, spawned and
//!   despawned entities, transforms as fixed f32 records, changed components, vars, events, views, page ports):
//!   [`frame::FrameWriter`] writes it from the logic's state, [`frame::Mirror`] applies it on the reader's side.

pub mod frame;
pub mod frame_records;
pub mod value;

/// Appends `v` as an unsigned LEB128 varint.
pub fn put_uvar(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends `v` zigzag-encoded as a LEB128 varint.
pub fn put_ivar(out: &mut Vec<u8>, v: i64) {
    put_uvar(out, ((v << 1) ^ (v >> 63)) as u64);
}

/// Appends a length-prefixed UTF-8 string.
pub fn put_str(out: &mut Vec<u8>, s: &str) {
    put_uvar(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// A reading position in a byte slice; every read fails (None) past the end instead of panicking.
pub struct Cursor<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Cursor<'a> {
        Cursor { bytes, at: 0 }
    }
    pub fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }
    pub fn u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(b)
    }
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    pub fn uvar(&mut self) -> Option<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            if shift >= 64 {
                return None;
            }
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
        }
    }
    pub fn ivar(&mut self) -> Option<i64> {
        let z = self.uvar()?;
        Some(((z >> 1) as i64) ^ -((z & 1) as i64))
    }
    pub fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn str(&mut self) -> Option<&'a str> {
        let n = self.uvar()? as usize;
        std::str::from_utf8(self.take(n)?).ok()
    }
}
