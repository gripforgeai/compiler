//! The fixed-layout value (docs/kit-modules.md §9.7): what crosses between modules of the common memory when a
//! value of free shape — a list, a dictionary — is passed. Its layout is defined here, by GripForge: `repr(C)`,
//! versioned, the same whatever compiler (or language) built the module. No Rust type crosses.
//!
//! - [`FixedValue`] (16 bytes): a tag, a length, 8 bytes of data — the number itself, or the address of the bytes
//!   of a text, of the items of a list ([`FixedValue`] × `len`), of the pairs of a dictionary ([`FixedPair`] × `len`,
//!   in insertion order).
//! - [`FixedBlock`]: the header of a value that travels — one allocation of the one allocator that holds the root
//!   and everything it reaches.
//!
//! The meaning is exactly that of `serde_json::Value` with `preserve_order` and `float_roundtrip`: an integer is
//! not a float, the keys keep their order, a float keeps its bits.
//!
//! Ownership: the writer allocates the block ([`Owned::from_json`]) and gives its address to the message
//! ([`Owned::into_address`]); the reader of the message takes it ([`Owned::take`]), reads it **in place**
//! ([`View`]), copies what it keeps ([`View::to_json`]) and frees it (drop). An address is 64 bits wide whatever
//! the address space.

use serde_json::{Map, Number, Value as Json};
use std::alloc::{alloc, dealloc, Layout};
use std::mem::{align_of, offset_of, size_of};

/// The version of the structures of this file (`major << 16 | minor`): written in every block, compared at load.
pub const VERSION: u32 = 1 << 16;
/// `GFV1`, the first four bytes of a block.
pub const MAGIC: u32 = 0x3156_4647;

/// The tags of a [`FixedValue`].
pub mod tag {
    pub const NULL: u8 = 0;
    /// `data` 0 or 1.
    pub const BOOL: u8 = 1;
    /// `data` an i64: every integer in the i64 range.
    pub const INT: u8 = 2;
    /// `data` a u64: the unsigned integers above the i64 range.
    pub const UINT: u8 = 3;
    /// `data` the bits of an f64 (never NaN nor infinite).
    pub const FLOAT: u8 = 4;
    /// `data` the address of `len` bytes of UTF-8.
    pub const TEXT: u8 = 5;
    /// `data` the address of `len` [`super::FixedValue`].
    pub const LIST: u8 = 6;
    /// `data` the address of `len` [`super::FixedPair`], in insertion order.
    pub const DICT: u8 = 7;
}

/// One value. Reserved bytes are 0.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedValue {
    pub tag: u8,
    pub reserved: [u8; 3],
    /// Bytes of a text, items of a list, pairs of a dictionary; 0 otherwise.
    pub len: u32,
    /// The number, or an address.
    pub data: u64,
}

/// One pair of a dictionary: the key (UTF-8 bytes by address) and its value.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedPair {
    pub key: u64,
    pub key_len: u32,
    pub reserved: u32,
    pub value: FixedValue,
}

/// The start of a value that travels: `size` bytes in one allocation (alignment 8), the root right after the
/// header, then what the root reaches.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedBlock {
    pub magic: u32,
    pub version: u32,
    pub size: u32,
    pub reserved: u32,
    pub root: FixedValue,
}

/// What this module did with the blocks it received (the QA reads it: which reads still copy).
pub mod stats {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static COPIED: AtomicU64 = AtomicU64::new(0);
    static UPDATED: AtomicU64 = AtomicU64::new(0);
    static VIEWED: AtomicU64 = AtomicU64::new(0);
    /// A block copied whole into a `serde_json::Value` the module keeps (`Reader::value`).
    pub fn copied() {
        COPIED.fetch_add(1, Relaxed);
    }
    /// A block written over a copy the module already had (`Reader::value_into`: no allocation when the shape is the same).
    pub fn updated() {
        UPDATED.fetch_add(1, Relaxed);
    }
    /// A block read in place (`Reader::fixed`): only what the reader took out of it was copied.
    pub fn viewed() {
        VIEWED.fetch_add(1, Relaxed);
    }
    /// `[copied, updated, viewed]` so far.
    pub fn get() -> [u64; 3] {
        [
            COPIED.load(Relaxed),
            UPDATED.load(Relaxed),
            VIEWED.load(Relaxed),
        ]
    }
}

/// The alignment a block is allocated (and freed) with.
pub const ALIGN: usize = 8;

// The layouts, pinned: a change here is a new major of the structures (docs/kit-modules.md §9.7).
const _: () = {
    assert!(size_of::<FixedValue>() == 16 && align_of::<FixedValue>() == 8);
    assert!(offset_of!(FixedValue, tag) == 0 && offset_of!(FixedValue, reserved) == 1);
    assert!(offset_of!(FixedValue, len) == 4 && offset_of!(FixedValue, data) == 8);
    assert!(size_of::<FixedPair>() == 32 && align_of::<FixedPair>() == 8);
    assert!(offset_of!(FixedPair, key) == 0 && offset_of!(FixedPair, key_len) == 8);
    assert!(offset_of!(FixedPair, reserved) == 12 && offset_of!(FixedPair, value) == 16);
    assert!(size_of::<FixedBlock>() == 32 && align_of::<FixedBlock>() == 8);
    assert!(offset_of!(FixedBlock, magic) == 0 && offset_of!(FixedBlock, version) == 4);
    assert!(
        offset_of!(FixedBlock, size) == 8
            && offset_of!(FixedBlock, reserved) == 12
            && offset_of!(FixedBlock, root) == 16
    );
};

impl FixedValue {
    pub const NULL: FixedValue = FixedValue {
        tag: tag::NULL,
        reserved: [0; 3],
        len: 0,
        data: 0,
    };
    const fn of(tag: u8, len: u32, data: u64) -> FixedValue {
        FixedValue {
            tag,
            reserved: [0; 3],
            len,
            data,
        }
    }
}

/// Bytes of items and pairs, and bytes of texts and keys, that `v` needs below its own 16 bytes.
fn measure(v: &Json, nodes: &mut u64, text: &mut u64) {
    match v {
        Json::String(s) => *text += s.len() as u64,
        Json::Array(a) => {
            *nodes += (a.len() * size_of::<FixedValue>()) as u64;
            for x in a {
                measure(x, nodes, text);
            }
        }
        Json::Object(o) => {
            *nodes += (o.len() * size_of::<FixedPair>()) as u64;
            for (k, x) in o {
                *text += k.len() as u64;
                measure(x, nodes, text);
            }
        }
        _ => {}
    }
}

/// Where the next items and the next text bytes of a block being filled go.
struct Fill {
    nodes: *mut u8,
    text: *mut u8,
    /// Subtracted from every address written: 0 for a block of this memory (absolute addresses), the address of
    /// the block for one that travels to another memory ([`write_crossing`]: offsets from its start).
    base: usize,
}

impl Fill {
    /// # Safety
    /// The block has room for what `measure` counted.
    unsafe fn text(&mut self, s: &str) -> u64 {
        let at = self.text;
        std::ptr::copy_nonoverlapping(s.as_ptr(), at, s.len());
        self.text = at.add(s.len());
        (at as usize - self.base) as u64
    }
    /// # Safety
    /// As [`Fill::text`]; `slot` is inside the block.
    unsafe fn value(&mut self, v: &Json, slot: *mut FixedValue) {
        let out = match v {
            Json::Null => FixedValue::NULL,
            Json::Bool(b) => FixedValue::of(tag::BOOL, 0, *b as u64),
            Json::Number(n) => {
                if let Some(i) = n.as_i64() {
                    FixedValue::of(tag::INT, 0, i as u64)
                } else if let Some(u) = n.as_u64() {
                    FixedValue::of(tag::UINT, 0, u)
                } else {
                    FixedValue::of(tag::FLOAT, 0, n.as_f64().unwrap_or(0.0).to_bits())
                }
            }
            Json::String(s) => FixedValue::of(tag::TEXT, s.len() as u32, self.text(s)),
            Json::Array(a) => {
                let items = self.nodes as *mut FixedValue;
                self.nodes = self.nodes.add(a.len() * size_of::<FixedValue>());
                for (i, x) in a.iter().enumerate() {
                    self.value(x, items.add(i));
                }
                FixedValue::of(
                    tag::LIST,
                    a.len() as u32,
                    (items as usize - self.base) as u64,
                )
            }
            Json::Object(o) => {
                let pairs = self.nodes as *mut FixedPair;
                self.nodes = self.nodes.add(o.len() * size_of::<FixedPair>());
                for (i, (k, x)) in o.iter().enumerate() {
                    let pair = pairs.add(i);
                    let key = self.text(k);
                    std::ptr::addr_of_mut!((*pair).key).write_unaligned(key);
                    std::ptr::addr_of_mut!((*pair).key_len).write_unaligned(k.len() as u32);
                    std::ptr::addr_of_mut!((*pair).reserved).write_unaligned(0);
                    self.value(x, std::ptr::addr_of_mut!((*pair).value));
                }
                FixedValue::of(
                    tag::DICT,
                    o.len() as u32,
                    (pairs as usize - self.base) as u64,
                )
            }
        };
        // Unaligned: a block that travels is filled inside a message, wherever its buffer lies.
        slot.write_unaligned(out);
    }
}

/// A block this module owns: freed with the one allocator when dropped.
#[derive(Debug)]
pub struct Owned {
    block: *mut FixedBlock,
}

impl Owned {
    /// A new block that holds `v`. `None` when the value does not fit a block (4 GiB) or the allocator has no room:
    /// the writer then writes it out.
    pub fn from_json(v: &Json) -> Option<Owned> {
        let (mut nodes, mut text) = (0u64, 0u64);
        measure(v, &mut nodes, &mut text);
        let size = size_of::<FixedBlock>() as u64 + nodes + text;
        if size > u32::MAX as u64 {
            return None;
        }
        // SAFETY: a layout of non-zero size; the block is filled within what was measured (items and pairs right
        // after the header, 8-byte aligned as they are multiples of 16; the text bytes after them).
        unsafe {
            let base = alloc(Layout::from_size_align(size as usize, ALIGN).ok()?);
            if base.is_null() {
                return None;
            }
            let block = base as *mut FixedBlock;
            std::ptr::addr_of_mut!((*block).magic).write(MAGIC);
            std::ptr::addr_of_mut!((*block).version).write(VERSION);
            std::ptr::addr_of_mut!((*block).size).write(size as u32);
            std::ptr::addr_of_mut!((*block).reserved).write(0);
            let nodes_at = base.add(size_of::<FixedBlock>());
            let mut fill = Fill {
                nodes: nodes_at,
                text: nodes_at.add(nodes as usize),
                base: 0,
            };
            fill.value(v, std::ptr::addr_of_mut!((*block).root));
            debug_assert_eq!(fill.text as usize, base as usize + size as usize);
            Some(Owned { block })
        }
    }

    /// Gives the block to a message: whoever [`Owned::take`]s the address owns it.
    pub fn into_address(self) -> u64 {
        let at = self.block as usize as u64;
        std::mem::forget(self);
        at
    }

    /// Takes the block a message carries. Refused (and not freed) when it is not a block of this major.
    ///
    /// # Safety
    /// `address` was written in a message by a module of the same memory (the same allocator), and is taken once.
    pub unsafe fn take(address: u64) -> Result<Owned, String> {
        let at = usize::try_from(address).unwrap_or(0);
        if at == 0 || at % ALIGN != 0 {
            return Err(format!(
                "fixed value: {address:#x} is not the address of a block"
            ));
        }
        let block = at as *mut FixedBlock;
        let (magic, version, size) = ((*block).magic, (*block).version, (*block).size);
        if magic != MAGIC {
            return Err(format!(
                "fixed value: no block at {address:#x} (magic {magic:#x}, expected {MAGIC:#x})"
            ));
        }
        if version >> 16 != VERSION >> 16 {
            return Err(format!(
                "fixed value: block of structures version {}.{}, this module reads {}.{}",
                version >> 16,
                version & 0xffff,
                VERSION >> 16,
                VERSION & 0xffff
            ));
        }
        if (size as usize) < size_of::<FixedBlock>() {
            return Err(format!(
                "fixed value: block of {size} bytes, shorter than its header"
            ));
        }
        Ok(Owned { block })
    }

    pub fn address(&self) -> u64 {
        self.block as usize as u64
    }

    /// Bytes of the whole block.
    pub fn size(&self) -> u32 {
        // SAFETY: a live block.
        unsafe { (*self.block).size }
    }

    /// The value, read where it is.
    pub fn view(&self) -> View<'_> {
        let lo = self.block as usize;
        // SAFETY: a live block, borrowed for as long as the view.
        View {
            v: unsafe { &(*self.block).root },
            lo,
            hi: lo + self.size() as usize,
            off: 0,
        }
    }

    /// A copy this module keeps (the block is still to be dropped).
    pub fn to_json(&self) -> Result<Json, String> {
        self.view().to_json()
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: allocated by `from_json` (here or in another module of the same memory) with this layout.
        unsafe {
            dealloc(
                self.block as *mut u8,
                Layout::from_size_align_unchecked(self.size() as usize, ALIGN),
            )
        }
    }
}

/// A value read in place: nothing is parsed or copied. Every address it follows is checked against the bounds of
/// the block it comes from (a text, a list or a dictionary that would lie outside reads as absent).
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    v: &'a FixedValue,
    lo: usize,
    hi: usize,
    /// Added to every address the value holds: 0 in a block of this memory, the address of the block for one that
    /// crossed from another memory (its addresses are offsets from its start: [`crossed`]).
    off: usize,
}

fn outside(what: &str) -> String {
    format!("fixed value: {what} lies outside its block")
}

impl<'a> View<'a> {
    /// A value lent outside a block (its owner says until when it is valid): no bounds to check against.
    ///
    /// # Safety
    /// `v` and everything it reaches are valid and unchanged for `'a`.
    pub unsafe fn lent(v: &'a FixedValue) -> View<'a> {
        View {
            v,
            lo: 0,
            hi: usize::MAX,
            off: 0,
        }
    }
    fn child(&self, v: &'a FixedValue) -> View<'a> {
        View {
            v,
            lo: self.lo,
            hi: self.hi,
            off: self.off,
        }
    }
    /// `count` things of `size` bytes at the address `data`, when they lie in the block.
    fn span(&self, data: u64, count: u32, size: usize, align: usize) -> Option<usize> {
        if count == 0 {
            return Some(self.lo.max(align));
        }
        let at = usize::try_from(data).ok()?.checked_add(self.off)?;
        let end = at.checked_add(count as usize * size)?;
        (at >= self.lo && end <= self.hi && at % align == 0 && at != 0).then_some(at)
    }
    pub fn raw(&self) -> &'a FixedValue {
        self.v
    }
    pub fn tag(&self) -> u8 {
        self.v.tag
    }
    pub fn is_null(&self) -> bool {
        self.v.tag == tag::NULL
    }
    pub fn as_bool(&self) -> Option<bool> {
        (self.v.tag == tag::BOOL).then_some(self.v.data != 0)
    }
    /// As `serde_json`: an integer in the i64 range.
    pub fn as_i64(&self) -> Option<i64> {
        match self.v.tag {
            tag::INT => Some(self.v.data as i64),
            tag::UINT => i64::try_from(self.v.data).ok(),
            _ => None,
        }
    }
    /// As `serde_json`: a non-negative integer.
    pub fn as_u64(&self) -> Option<u64> {
        match self.v.tag {
            tag::INT => u64::try_from(self.v.data as i64).ok(),
            tag::UINT => Some(self.v.data),
            _ => None,
        }
    }
    /// As `serde_json`: any number.
    pub fn as_f64(&self) -> Option<f64> {
        match self.v.tag {
            tag::INT => Some(self.v.data as i64 as f64),
            tag::UINT => Some(self.v.data as f64),
            tag::FLOAT => Some(f64::from_bits(self.v.data)),
            _ => None,
        }
    }
    /// `len` bytes at `data`, when they lie in the block.
    fn bytes(&self, data: u64, len: u32) -> Option<&'a [u8]> {
        if len == 0 {
            return Some(&[]);
        }
        let at = self.span(data, len, 1, 1)?;
        // SAFETY: `len` bytes inside the block (or lent), unchanged for `'a`.
        Some(unsafe { std::slice::from_raw_parts(at as *const u8, len as usize) })
    }
    fn text(&self, data: u64, len: u32) -> Option<&'a str> {
        std::str::from_utf8(self.bytes(data, len)?).ok()
    }
    /// The text, borrowed from the block (`None`: not a text, or not UTF-8).
    pub fn as_str(&self) -> Option<&'a str> {
        (self.v.tag == tag::TEXT)
            .then(|| self.text(self.v.data, self.v.len))
            .flatten()
    }
    /// The items of a list, where they are.
    pub fn items(&self) -> Option<&'a [FixedValue]> {
        if self.v.tag != tag::LIST {
            return None;
        }
        let at = self.span(
            self.v.data,
            self.v.len,
            size_of::<FixedValue>(),
            align_of::<FixedValue>(),
        )?;
        // SAFETY: `len` values inside the block, aligned, unchanged for `'a` (a dangling aligned address for 0).
        Some(unsafe { std::slice::from_raw_parts(at as *const FixedValue, self.v.len as usize) })
    }
    /// The pairs of a dictionary, where they are, in insertion order.
    pub fn pairs(&self) -> Option<&'a [FixedPair]> {
        if self.v.tag != tag::DICT {
            return None;
        }
        let at = self.span(
            self.v.data,
            self.v.len,
            size_of::<FixedPair>(),
            align_of::<FixedPair>(),
        )?;
        // SAFETY: as `items`.
        Some(unsafe { std::slice::from_raw_parts(at as *const FixedPair, self.v.len as usize) })
    }
    /// Items of a list or pairs of a dictionary (0 for the others).
    pub fn len(&self) -> usize {
        if matches!(self.v.tag, tag::LIST | tag::DICT) {
            self.v.len as usize
        } else {
            0
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Item `i` of a list.
    pub fn index(&self, i: usize) -> Option<View<'a>> {
        self.items()?.get(i).map(|v| self.child(v))
    }
    /// The value of `key` in a dictionary (the last pair with that key: what a map built in order holds).
    pub fn get(&self, key: &str) -> Option<View<'a>> {
        // Bytes against bytes: a key equal to a `&str` is UTF-8, nothing to validate.
        let want = key.as_bytes();
        self.pairs()?
            .iter()
            .rev()
            .find(|p| {
                p.key_len as usize == want.len() && self.bytes(p.key, p.key_len) == Some(want)
            })
            .map(|p| self.child(&p.value))
    }
    /// The items of a list as views (empty for anything else).
    pub fn iter(&self) -> impl Iterator<Item = View<'a>> + 'a {
        let me = *self;
        self.items().unwrap_or(&[]).iter().map(move |v| me.child(v))
    }
    /// The pairs of a dictionary as `(key, value)` in insertion order (empty for anything else; a key that is not
    /// UTF-8 is skipped).
    pub fn entries(&self) -> impl Iterator<Item = (&'a str, View<'a>)> + 'a {
        let me = *self;
        self.pairs()
            .unwrap_or(&[])
            .iter()
            .filter_map(move |p| Some((me.text(p.key, p.key_len)?, me.child(&p.value))))
    }

    /// The copy a module keeps: exactly the value the writer had (`serde_json` with `preserve_order`).
    pub fn to_json(&self) -> Result<Json, String> {
        Ok(match self.v.tag {
            tag::NULL => Json::Null,
            tag::BOOL => Json::Bool(self.v.data != 0),
            tag::INT => Json::Number(Number::from(self.v.data as i64)),
            tag::UINT => Json::Number(Number::from(self.v.data)),
            tag::FLOAT => Number::from_f64(f64::from_bits(self.v.data))
                .map(Json::Number)
                .unwrap_or(Json::Null),
            tag::TEXT => Json::String(
                self.as_str()
                    .ok_or_else(|| outside("a text (or it is not UTF-8)"))?
                    .to_string(),
            ),
            tag::LIST => {
                let items = self.items().ok_or_else(|| outside("a list"))?;
                let mut a = Vec::with_capacity(items.len());
                for v in items {
                    a.push(self.child(v).to_json()?);
                }
                Json::Array(a)
            }
            tag::DICT => {
                let pairs = self.pairs().ok_or_else(|| outside("a dictionary"))?;
                let mut o = Map::with_capacity(pairs.len());
                for p in pairs {
                    let key = self
                        .text(p.key, p.key_len)
                        .ok_or_else(|| outside("a key (or it is not UTF-8)"))?;
                    o.insert(key.to_string(), self.child(&p.value).to_json()?);
                }
                Json::Object(o)
            }
            t => return Err(format!("fixed value: unknown tag {t}")),
        })
    }

    /// Makes `slot` exactly what [`View::to_json`] would give, reusing what `slot` already holds when it has the
    /// same shape (a dictionary with the same keys in the same order, a list of the same length, a text): no map,
    /// key or buffer is allocated again. For a copy brought up to date.
    pub fn write_into(&self, slot: &mut Json) -> Result<(), String> {
        match (self.v.tag, &mut *slot) {
            (tag::DICT, Json::Object(o)) if o.len() == self.v.len as usize => {
                let pairs = self.pairs().ok_or_else(|| outside("a dictionary"))?;
                if pairs
                    .iter()
                    .zip(o.keys())
                    .all(|(p, k)| self.bytes(p.key, p.key_len) == Some(k.as_bytes()))
                {
                    for (p, v) in pairs.iter().zip(o.values_mut()) {
                        self.child(&p.value).write_into(v)?;
                    }
                    return Ok(());
                }
            }
            (tag::LIST, Json::Array(a)) if a.len() == self.v.len as usize => {
                let items = self.items().ok_or_else(|| outside("a list"))?;
                for (v, s) in items.iter().zip(a.iter_mut()) {
                    self.child(v).write_into(s)?;
                }
                return Ok(());
            }
            (tag::TEXT, Json::String(text)) => {
                let read = self
                    .as_str()
                    .ok_or_else(|| outside("a text (or it is not UTF-8)"))?;
                text.clear();
                text.push_str(read);
                return Ok(());
            }
            _ => {}
        }
        *slot = self.to_json()?;
        Ok(())
    }
}

// ---- a block that crosses to another memory ------------------------------------------------------------------------

/// `FixedBlock::reserved` of a block that **crosses** from one memory to another (an isolated kit module,
/// docs/kit-third-party.md): every address it holds is an **offset from the start of the block**, so the block is
/// the same bytes wherever it is copied. 0: the addresses are addresses of the memory the block lies in.
pub const CROSSING: u32 = 1;

/// Blocks this module wrote for another memory so far (the QA reads it).
pub fn crossed_out() -> u64 {
    CROSSED.load(std::sync::atomic::Ordering::Relaxed)
}
static CROSSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Appends to `out` the block of `v` for a reader of **another memory**: the same structures as [`Owned`] (header,
/// root, items and pairs, text bytes), with offsets from the start of the block where a block of this memory holds
/// addresses ([`CROSSING`]). The bytes are copied once, by whoever sees both memories; the reader reads them where
/// they land ([`crossed`]). `false` (nothing appended) when the value does not fit a block.
pub fn write_crossing(v: &Json, out: &mut Vec<u8>) -> bool {
    let (mut nodes, mut text) = (0u64, 0u64);
    measure(v, &mut nodes, &mut text);
    let size = size_of::<FixedBlock>() as u64 + nodes + text;
    if size > u32::MAX as u64 {
        return false;
    }
    let start = out.len();
    out.resize(start + size as usize, 0);
    // SAFETY: `size` bytes were just reserved at `start`; the block is filled within what was measured, by
    // unaligned writes (the buffer of a message has no alignment of its own).
    unsafe {
        let base = out.as_mut_ptr().add(start);
        let block = base as *mut FixedBlock;
        std::ptr::addr_of_mut!((*block).magic).write_unaligned(MAGIC);
        std::ptr::addr_of_mut!((*block).version).write_unaligned(VERSION);
        std::ptr::addr_of_mut!((*block).size).write_unaligned(size as u32);
        std::ptr::addr_of_mut!((*block).reserved).write_unaligned(CROSSING);
        let nodes_at = base.add(size_of::<FixedBlock>());
        let mut fill = Fill {
            nodes: nodes_at,
            text: nodes_at.add(nodes as usize),
            base: base as usize,
        };
        fill.value(v, std::ptr::addr_of_mut!((*block).root));
    }
    CROSSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    true
}

/// Bytes of the block that starts `bytes` (a block that crossed: [`write_crossing`]), once its header is checked —
/// the magic, the major of the structures, the [`CROSSING`] mark, a size that holds the header and fits `bytes`.
pub fn crossed_size(bytes: &[u8]) -> Result<usize, String> {
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let (Some(magic), Some(version), Some(size), Some(mark)) =
        (word(0), word(4), word(8), word(12))
    else {
        return Err("fixed value: a block cut short (no header)".into());
    };
    if magic != MAGIC {
        return Err(format!(
            "fixed value: no block in the message (magic {magic:#x}, expected {MAGIC:#x})"
        ));
    }
    if version >> 16 != VERSION >> 16 {
        return Err(format!(
            "fixed value: block of structures version {}.{}, this module reads {}.{}",
            version >> 16,
            version & 0xffff,
            VERSION >> 16,
            VERSION & 0xffff
        ));
    }
    if mark != CROSSING {
        return Err("fixed value: a block of another memory whose addresses are not offsets (it cannot be read here)".into());
    }
    if (size as usize) < size_of::<FixedBlock>() || size as usize > bytes.len() {
        return Err(format!(
            "fixed value: block of {size} bytes in {} bytes of message",
            bytes.len()
        ));
    }
    Ok(size as usize)
}

/// Reads a block that crossed from another memory (`block`: exactly its bytes, [`crossed_size`]) **where it
/// landed**: `read` gets the view of its root. Every address is an offset checked against the block, so a block
/// written by a module that is not trusted reads as absent where it lies — never outside. A buffer that is not
/// aligned on 8 is moved once to one that is.
pub fn crossed<R>(
    block: &[u8],
    read: impl FnOnce(View<'_>) -> Result<R, String>,
) -> Result<R, String> {
    if crossed_size(block)? != block.len() {
        return Err("fixed value: a block and its size disagree".into());
    }
    let view_at = |at: *const u8| -> View<'_> {
        // SAFETY: `at` is aligned on 8 and holds `block.len()` bytes that start with a checked header; the root is
        // 16 bytes in. Everything else is reached through `span`, bounded by `lo..hi`.
        View {
            v: unsafe { &*(at.add(offset_of!(FixedBlock, root)) as *const FixedValue) },
            lo: at as usize,
            hi: at as usize + block.len(),
            off: at as usize,
        }
    };
    if (block.as_ptr() as usize).is_multiple_of(ALIGN) {
        return read(view_at(block.as_ptr()));
    }
    let mut moved = vec![0u64; block.len().div_ceil(8)];
    // SAFETY: `moved` holds at least `block.len()` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(block.as_ptr(), moved.as_mut_ptr() as *mut u8, block.len())
    };
    read(view_at(moved.as_ptr() as *const u8))
}

/// A block that crossed, **kept**: its bytes on 8 (shared: a clone is a count, not a copy), read in place as often
/// as asked — what a module holds of a typed value it may never turn into JSON (the payload of an event: the bus
/// keeps it, delivers it, records it; a reader that wants the JSON value makes it once).
#[derive(Clone)]
pub struct Kept {
    words: std::sync::Arc<[u64]>,
    len: usize,
}

impl Kept {
    /// The bytes of a block that crossed ([`crossed_size`] checked), copied once onto 8.
    pub fn from_bytes(block: &[u8]) -> Result<Kept, String> {
        if crossed_size(block)? != block.len() {
            return Err("fixed value: a block and its size disagree".into());
        }
        let mut words = vec![0u64; block.len().div_ceil(8)];
        // SAFETY: `words` holds at least `block.len()` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                block.as_ptr(),
                words.as_mut_ptr() as *mut u8,
                block.len(),
            )
        };
        Ok(Kept {
            words: words.into(),
            len: block.len(),
        })
    }
    /// The block of `v` ([`write_crossing`]); `None` when it does not fit one.
    pub fn from_json(v: &Json) -> Option<Kept> {
        let mut out = Vec::new();
        write_crossing(v, &mut out)
            .then(|| Kept::from_bytes(&out).ok())
            .flatten()
    }
    /// The bytes of the block, as a message carries it after its tag and padding.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `len` bytes of `words` were written by `from_bytes`.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr() as *const u8, self.len) }
    }
    /// The root, read in place.
    pub fn view(&self) -> View<'_> {
        let at = self.words.as_ptr() as *const u8;
        // SAFETY: aligned on 8, `len` bytes whose header `from_bytes` checked; everything else is reached through
        // `span`, bounded by the block.
        View {
            v: unsafe { &*(at.add(offset_of!(FixedBlock, root)) as *const FixedValue) },
            lo: at as usize,
            hi: at as usize + self.len,
            off: at as usize,
        }
    }
}

impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Kept({} bytes)", self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Through a block and back; the text of both (which keeps the order of keys and the kind of numbers) too.
    fn round(v: &Json) -> Json {
        let block = Owned::from_json(v).expect("fits a block");
        let back = block.to_json().unwrap();
        assert_eq!(&back, v);
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            serde_json::to_string(v).unwrap()
        );
        // Through an address, as a message carries it.
        let at = Owned::from_json(v).unwrap().into_address();
        let taken = unsafe { Owned::take(at) }.unwrap();
        assert_eq!(&taken.to_json().unwrap(), v);
        back
    }

    #[test]
    fn layouts_are_what_the_contract_says() {
        // Also pinned at compile time (the `const` block above); here against the bytes.
        let v = FixedValue::of(tag::LIST, 0x0403_0201, 0x0807_0605_0403_0201);
        let bytes: [u8; 16] = unsafe { std::mem::transmute(v) };
        assert_eq!(bytes, [6, 0, 0, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8]);
        let p = FixedPair {
            key: 0x11,
            key_len: 0x22,
            reserved: 0,
            value: FixedValue::NULL,
        };
        let bytes: [u8; 32] = unsafe { std::mem::transmute(p) };
        assert_eq!((bytes[0], bytes[8], bytes[16]), (0x11, 0x22, 0));
        let block = Owned::from_json(&json!([1])).unwrap();
        let head = unsafe { std::slice::from_raw_parts(block.address() as usize as *const u8, 16) };
        assert_eq!(&head[..4], b"GFV1");
        assert_eq!(u32::from_le_bytes(head[4..8].try_into().unwrap()), VERSION);
        assert_eq!(u32::from_le_bytes(head[8..12].try_into().unwrap()), 32 + 16);
        assert_eq!(block.size(), 48);
    }

    #[test]
    fn numbers_keep_their_kind_at_the_limits() {
        for v in [
            json!(0),
            json!(-0),
            json!(1),
            json!(-1),
            json!(i64::MAX),
            json!(i64::MIN),
            json!(u64::MAX),
            json!(i64::MAX as u64 + 1),
            json!(u32::MAX),
            json!(0.0),
            json!(-0.0),
            json!(3.0),
            json!(f64::MAX),
            json!(f64::MIN),
            json!(f64::MIN_POSITIVE),
            json!(5e-324),
            json!(1.5e300),
            json!(0.1),
            json!(1e21),
            json!(9007199254740993u64),
            json!(-9007199254740993i64),
            json!(true),
            json!(false),
            json!(null),
        ] {
            let back = round(&json!([v]));
            let (a, b) = (&back[0], &v);
            assert_eq!(
                (a.is_i64(), a.is_u64(), a.is_f64()),
                (b.is_i64(), b.is_u64(), b.is_f64()),
                "{v}"
            );
            assert_eq!(
                a.as_f64().map(f64::to_bits),
                b.as_f64().map(f64::to_bits),
                "{v}: the bits of the float"
            );
        }
        // 3 is not 3.0, and -0.0 keeps its sign.
        let back = round(&json!({ "i": 3, "f": 3.0, "z": -0.0 }));
        assert_eq!((back["i"].as_u64(), back["f"].as_u64()), (Some(3), None));
        assert!(back["z"].as_f64().unwrap().is_sign_negative());
        // In place, as serde_json answers.
        let block = Owned::from_json(&json!([u64::MAX, -7, 2.5, 7])).unwrap();
        let v = block.view();
        assert_eq!(
            (v.index(0).unwrap().as_u64(), v.index(0).unwrap().as_i64()),
            (Some(u64::MAX), None)
        );
        assert_eq!(
            (v.index(1).unwrap().as_i64(), v.index(1).unwrap().as_u64()),
            (Some(-7), None)
        );
        assert_eq!(
            (v.index(2).unwrap().as_f64(), v.index(2).unwrap().as_i64()),
            (Some(2.5), None)
        );
        assert_eq!(
            (v.index(3).unwrap().as_u64(), v.index(3).unwrap().as_f64()),
            (Some(7), Some(7.0))
        );
    }

    #[test]
    fn texts_empty_long_and_not_ascii() {
        let long = "é€𝄞 long — ".repeat(40_000);
        let v = json!({ "": "", "empty": "", "long": long, "é": "ü", "nul": "a\0b", "list": ["", "x", ""] });
        let back = round(&v);
        assert_eq!(back["long"].as_str().unwrap().len(), long.len());
        let block = Owned::from_json(&v).unwrap();
        let view = block.view();
        assert_eq!(view.get("").unwrap().as_str(), Some(""));
        assert_eq!(view.get("long").unwrap().as_str(), Some(long.as_str()));
        assert_eq!(view.get("nul").unwrap().as_str(), Some("a\0b"));
        assert_eq!(view.get("é").unwrap().as_str(), Some("ü"));
        // The text is read where the block holds it: no copy.
        let at = view.get("long").unwrap().as_str().unwrap().as_ptr() as usize;
        assert!(
            at > block.address() as usize && at < block.address() as usize + block.size() as usize
        );
        // A root that is a scalar or a text is a block too (the header and the root, plus the bytes).
        for v in [
            json!("text"),
            json!(""),
            json!(7),
            json!(null),
            json!([]),
            json!({}),
        ] {
            round(&v);
        }
        assert_eq!(Owned::from_json(&json!("abc")).unwrap().size(), 35);
    }

    #[test]
    fn deep_nesting_and_the_order_of_keys() {
        let mut deep = json!("leaf");
        for i in 0..1000 {
            deep = if i % 2 == 0 {
                json!([deep])
            } else {
                json!({ "k": deep })
            };
        }
        round(&deep);
        // Insertion order, not sorted, through nested dictionaries.
        let v = json!({ "zeta": 1, "alpha": { "y": [], "x": {}, "b": [{ "2": 2, "10": 10, "1": 1 }] }, "Beta": null, "a": 0 });
        let back = round(&v);
        assert_eq!(
            back.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["zeta", "alpha", "Beta", "a"]
        );
        assert_eq!(
            back["alpha"]["b"][0]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["2", "10", "1"]
        );
        let block = Owned::from_json(&v).unwrap();
        assert_eq!(
            block.view().entries().map(|(k, _)| k).collect::<Vec<_>>(),
            ["zeta", "alpha", "Beta", "a"]
        );
        assert_eq!(
            block
                .view()
                .get("alpha")
                .unwrap()
                .get("b")
                .unwrap()
                .index(0)
                .unwrap()
                .get("10")
                .unwrap()
                .as_i64(),
            Some(10)
        );
        assert!(block.view().get("missing").is_none() && block.view().index(0).is_none());
        // A wide one.
        let wide: Map<String, Json> = (0..5000)
            .rev()
            .map(|i| (format!("k{i}"), json!(i)))
            .collect();
        round(&Json::Object(wide));
    }

    /// A block made by hand, as a module in another language would fill it: the keys `a`, `b`, `a`.
    #[test]
    fn duplicate_keys_keep_the_first_place_and_the_last_value() {
        #[repr(C, align(8))]
        struct Hand {
            head: FixedBlock,
            pairs: [FixedPair; 3],
            text: [u8; 2],
        }
        let mut hand = Box::new(Hand {
            head: FixedBlock {
                magic: MAGIC,
                version: VERSION,
                size: size_of::<Hand>() as u32,
                reserved: 0,
                root: FixedValue::NULL,
            },
            pairs: [FixedPair {
                key: 0,
                key_len: 1,
                reserved: 0,
                value: FixedValue::NULL,
            }; 3],
            text: *b"ab",
        });
        let (a, b) = (
            hand.text.as_ptr() as usize as u64,
            hand.text.as_ptr() as usize as u64 + 1,
        );
        hand.pairs[0] = FixedPair {
            key: a,
            key_len: 1,
            reserved: 0,
            value: FixedValue::of(tag::INT, 0, 1),
        };
        hand.pairs[1] = FixedPair {
            key: b,
            key_len: 1,
            reserved: 0,
            value: FixedValue::of(tag::INT, 0, 2),
        };
        hand.pairs[2] = FixedPair {
            key: a,
            key_len: 1,
            reserved: 0,
            value: FixedValue::of(tag::FLOAT, 0, 3.5f64.to_bits()),
        };
        hand.head.root = FixedValue::of(tag::DICT, 3, hand.pairs.as_ptr() as usize as u64);
        let lo = &*hand as *const Hand as usize;
        let view = View {
            v: &hand.head.root,
            lo,
            hi: lo + size_of::<Hand>(),
            off: 0,
        };
        let back = view.to_json().unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), r#"{"a":3.5,"b":2}"#);
        assert_eq!(
            view.get("a").unwrap().as_f64(),
            Some(3.5),
            "in place: the last pair of the key"
        );
        assert_eq!(
            view.entries().count(),
            3,
            "in place the pairs are all there, in order"
        );
        // What serde_json itself does with the same insertions.
        let mut m = Map::new();
        m.insert("a".into(), json!(1));
        m.insert("b".into(), json!(2));
        m.insert("a".into(), json!(3.5));
        assert_eq!(back, Json::Object(m));
    }

    #[test]
    fn a_block_that_is_not_one_is_refused_and_bounds_are_checked() {
        assert!(unsafe { Owned::take(0) }
            .unwrap_err()
            .contains("not the address of a block"));
        #[repr(C, align(8))]
        struct Raw([u8; 48]);
        let mut raw = Raw([0; 48]);
        let at = raw.0.as_mut_ptr() as usize as u64;
        assert!(unsafe { Owned::take(at + 1) }
            .unwrap_err()
            .contains("not the address"));
        assert!(unsafe { Owned::take(at) }
            .unwrap_err()
            .contains("no block at"));
        raw.0[..4].copy_from_slice(&MAGIC.to_le_bytes());
        raw.0[4..8].copy_from_slice(&(2u32 << 16).to_le_bytes());
        assert!(unsafe { Owned::take(at) }
            .unwrap_err()
            .contains("structures version 2.0, this module reads 1.0"));
        raw.0[4..8].copy_from_slice(&(VERSION | 7).to_le_bytes()); // a later minor of the same major is read
        raw.0[8..12].copy_from_slice(&8u32.to_le_bytes());
        assert!(unsafe { Owned::take(at) }
            .unwrap_err()
            .contains("shorter than its header"));
        // A list, a text, a key that point outside the block: read as an error, never followed.
        let root = FixedValue::of(tag::LIST, 4, at + 32);
        let view = View {
            v: &root,
            lo: at as usize,
            hi: at as usize + 48,
            off: 0,
        };
        assert!(
            view.items().is_none() && view.to_json().unwrap_err().contains("outside its block")
        );
        let root = FixedValue::of(tag::TEXT, 1, at - 8);
        assert!(View {
            v: &root,
            lo: at as usize,
            hi: at as usize + 48,
            off: 0
        }
        .to_json()
        .is_err());
        let root = FixedValue::of(tag::TEXT, 2, at + 40);
        raw.0[40..42].copy_from_slice(&[0xff, 0xfe]);
        assert!(View {
            v: &root,
            lo: at as usize,
            hi: at as usize + 48,
            off: 0
        }
        .to_json()
        .unwrap_err()
        .contains("UTF-8"));
        let root = FixedValue::of(99, 0, 0);
        assert!(View {
            v: &root,
            lo: 0,
            hi: usize::MAX,
            off: 0
        }
        .to_json()
        .unwrap_err()
        .contains("unknown tag 99"));
        // A float that is not a number reads null (what the JSON text would hold).
        let root = FixedValue::of(tag::FLOAT, 0, f64::NAN.to_bits());
        assert_eq!(unsafe { View::lent(&root) }.to_json().unwrap(), Json::Null);
    }

    #[test]
    fn a_copy_brought_up_to_date_is_the_value() {
        let cases = [
            (
                json!({"x": 1, "y": 2.5, "tags": ["a", "b"], "o": {"k": "v"}}),
                json!({"x": 1.0, "y": -0.0, "tags": ["c", "dd"], "o": {"k": "w"}}),
            ),
            (json!({"x": 1, "y": 2}), json!({"y": 2, "x": 1})),
            (json!({"x": 1, "y": 2}), json!({"x": 1})),
            (json!({"x": 1}), json!({"x": 1, "y": {"deep": [1, 2, 3]}})),
            (json!([1, 2, 3]), json!([1, 2])),
            (
                json!([{"a": 1}, "s"]),
                json!([{"a": null}, "longer string"]),
            ),
            (json!("text"), json!(3)),
            (json!(null), json!({"a": [true, false]})),
        ];
        for (was, now) in cases {
            let mut slot = was.clone();
            Owned::from_json(&now)
                .unwrap()
                .view()
                .write_into(&mut slot)
                .unwrap();
            assert_eq!(
                serde_json::to_string(&slot).unwrap(),
                serde_json::to_string(&now).unwrap(),
                "{was} -> {now}"
            );
        }
        // The same shape keeps its allocations.
        let mut slot = json!({ "name": "old name", "list": [1, 2] });
        let before = slot["name"].as_str().unwrap().as_ptr();
        Owned::from_json(&json!({ "name": "new", "list": [3, 4] }))
            .unwrap()
            .view()
            .write_into(&mut slot)
            .unwrap();
        assert_eq!(slot["name"].as_str().unwrap().as_ptr(), before);
        assert_eq!(slot, json!({ "name": "new", "list": [3, 4] }));
    }
    #[test]
    fn a_block_crosses_to_another_memory_as_the_same_bytes() {
        let v = json!({ "name": "torch", "tags": ["a", "é", ""], "n": -3, "f": 3.0, "u": u64::MAX, "deep": [{ "k": [null, true] }], "empty": {} });
        // Written at any place of a message (here: an odd offset), copied elsewhere, read where it landed.
        for lead in [0usize, 1, 3, 8] {
            let mut message = vec![0xAAu8; lead];
            assert!(write_crossing(&v, &mut message));
            let size = crossed_size(&message[lead..]).unwrap();
            assert_eq!(size, message.len() - lead);
            let landed: Vec<u8> = message[lead..].to_vec(); // "the other memory": other addresses, the same bytes
            drop(message);
            let back = crossed(&landed, |view| {
                assert_eq!(view.get("name").unwrap().as_str(), Some("torch"));
                assert_eq!(
                    view.get("tags").unwrap().index(1).unwrap().as_str(),
                    Some("é")
                );
                view.to_json()
            })
            .unwrap();
            assert_eq!(
                serde_json::to_string(&back).unwrap(),
                serde_json::to_string(&v).unwrap()
            );
            // Not aligned: moved once, the same value.
            let mut odd = vec![0u8; 1];
            odd.extend_from_slice(&landed);
            assert_eq!(crossed(&odd[1..], |view| view.to_json()).unwrap(), v);
        }
    }

    #[test]
    fn a_crossing_block_that_lies_is_refused_or_reads_absent_never_outside() {
        let mut bytes = Vec::new();
        assert!(write_crossing(
            &json!({ "k": "value", "l": [1, 2, 3] }),
            &mut bytes
        ));
        // Cut short, another magic, another major, the mark of a block of one memory, a size past the bytes.
        assert!(crossed_size(&bytes[..10])
            .unwrap_err()
            .contains("cut short"));
        let with = |at: usize, v: u32| {
            let mut b = bytes.clone();
            b[at..at + 4].copy_from_slice(&v.to_le_bytes());
            b
        };
        assert!(crossed_size(&with(0, 7)).unwrap_err().contains("no block"));
        assert!(crossed_size(&with(4, 2 << 16))
            .unwrap_err()
            .contains("structures version 2.0"));
        assert!(crossed_size(&with(12, 0))
            .unwrap_err()
            .contains("not offsets"));
        assert!(crossed_size(&with(8, bytes.len() as u32 + 1))
            .unwrap_err()
            .contains("bytes of message"));
        // An offset that points outside the block: the dictionary reads as absent (an error for a copy), nothing is read outside.
        let mut out = bytes.clone();
        out[24..32].copy_from_slice(&(bytes.len() as u64 + 64).to_le_bytes());
        assert!(crossed(&out, |view| view.to_json())
            .unwrap_err()
            .contains("outside its block"));
        // A huge offset (an address of the other memory, or an overflow) the same.
        out[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(crossed(&out, |view| view.to_json()).is_err());
    }
}

// ---- blocks filled from typed values (the kit façade, docs/kit-modules.md §7.13) --------------------------------

/// One node of a value a block is filled from **without a JSON value first**: the typed arguments and payloads of
/// the kit façade (`crate::typed::Arg` of gripforge-kit-guest implements [`Source`]). The block is the one
/// [`Owned::from_json`] / [`write_crossing`] would make of the same value.
pub enum Node<'a, T> {
    Null,
    Bool(bool),
    /// A number as JavaScript has it (`num()`): an integral value is an integer, a value not finite is null.
    Num(f64),
    /// A float written as a float (not finite: null).
    Float(f64),
    Int(i64),
    Text(&'a str),
    /// `[num(x), num(y), num(z)]`.
    Vec3([f64; 3]),
    List(&'a [T]),
    /// In order; keys are distinct (a later duplicate is not merged as a JSON object would).
    Dict(&'a [(&'a str, T)]),
    Json(&'a Json),
}

/// A value whose nodes a block is filled from.
pub trait Source: Sized {
    fn node(&self) -> Node<'_, Self>;
}

fn num_value(n: f64) -> FixedValue {
    if n.is_finite() && n == n.trunc() && n.abs() < 9.007_199_254_740_992e15 {
        FixedValue::of(tag::INT, 0, (if n == 0.0 { 0 } else { n as i64 }) as u64)
    } else if n.is_finite() {
        FixedValue::of(tag::FLOAT, 0, n.to_bits())
    } else {
        FixedValue::NULL
    }
}

fn measure_src<T: Source>(v: &T, nodes: &mut u64, text: &mut u64) {
    match v.node() {
        Node::Text(s) => *text += s.len() as u64,
        Node::Vec3(_) => *nodes += (3 * size_of::<FixedValue>()) as u64,
        Node::List(a) => {
            *nodes += (a.len() * size_of::<FixedValue>()) as u64;
            for x in a {
                measure_src(x, nodes, text);
            }
        }
        Node::Dict(d) => {
            *nodes += (d.len() * size_of::<FixedPair>()) as u64;
            for (k, x) in d {
                *text += k.len() as u64;
                measure_src(x, nodes, text);
            }
        }
        Node::Json(j) => measure(j, nodes, text),
        _ => {}
    }
}

impl Fill {
    /// [`Fill::value`] of a [`Source`].
    ///
    /// # Safety
    /// As [`Fill::value`], the block measured by `measure_src`.
    unsafe fn value_src<T: Source>(&mut self, v: &T, slot: *mut FixedValue) {
        let out = match v.node() {
            Node::Null => FixedValue::NULL,
            Node::Bool(b) => FixedValue::of(tag::BOOL, 0, b as u64),
            Node::Num(n) => num_value(n),
            Node::Float(n) if n.is_finite() => FixedValue::of(tag::FLOAT, 0, n.to_bits()),
            Node::Float(_) => FixedValue::NULL,
            Node::Int(i) => FixedValue::of(tag::INT, 0, i as u64),
            Node::Text(s) => FixedValue::of(tag::TEXT, s.len() as u32, self.text(s)),
            Node::Vec3(p) => {
                let items = self.nodes as *mut FixedValue;
                self.nodes = self.nodes.add(3 * size_of::<FixedValue>());
                for (i, x) in p.into_iter().enumerate() {
                    items.add(i).write_unaligned(num_value(x));
                }
                FixedValue::of(tag::LIST, 3, (items as usize - self.base) as u64)
            }
            Node::List(a) => {
                let items = self.nodes as *mut FixedValue;
                self.nodes = self.nodes.add(a.len() * size_of::<FixedValue>());
                for (i, x) in a.iter().enumerate() {
                    self.value_src(x, items.add(i));
                }
                FixedValue::of(
                    tag::LIST,
                    a.len() as u32,
                    (items as usize - self.base) as u64,
                )
            }
            Node::Dict(d) => {
                let pairs = self.nodes as *mut FixedPair;
                self.nodes = self.nodes.add(d.len() * size_of::<FixedPair>());
                for (i, (k, x)) in d.iter().enumerate() {
                    let pair = pairs.add(i);
                    let key = self.text(k);
                    std::ptr::addr_of_mut!((*pair).key).write_unaligned(key);
                    std::ptr::addr_of_mut!((*pair).key_len).write_unaligned(k.len() as u32);
                    std::ptr::addr_of_mut!((*pair).reserved).write_unaligned(0);
                    self.value_src(x, std::ptr::addr_of_mut!((*pair).value));
                }
                FixedValue::of(
                    tag::DICT,
                    d.len() as u32,
                    (pairs as usize - self.base) as u64,
                )
            }
            Node::Json(j) => return self.value(j, slot),
        };
        slot.write_unaligned(out);
    }
}

impl Owned {
    /// [`Owned::from_json`] of a [`Source`]: the same block, no JSON value made.
    pub fn from_src<T: Source>(v: &T) -> Option<Owned> {
        let (mut nodes, mut text) = (0u64, 0u64);
        measure_src(v, &mut nodes, &mut text);
        let size = size_of::<FixedBlock>() as u64 + nodes + text;
        if size > u32::MAX as u64 {
            return None;
        }
        // SAFETY: as `from_json`.
        unsafe {
            let base = alloc(Layout::from_size_align(size as usize, ALIGN).ok()?);
            if base.is_null() {
                return None;
            }
            let block = base as *mut FixedBlock;
            std::ptr::addr_of_mut!((*block).magic).write(MAGIC);
            std::ptr::addr_of_mut!((*block).version).write(VERSION);
            std::ptr::addr_of_mut!((*block).size).write(size as u32);
            std::ptr::addr_of_mut!((*block).reserved).write(0);
            let nodes_at = base.add(size_of::<FixedBlock>());
            let mut fill = Fill {
                nodes: nodes_at,
                text: nodes_at.add(nodes as usize),
                base: 0,
            };
            fill.value_src(v, std::ptr::addr_of_mut!((*block).root));
            debug_assert_eq!(fill.text as usize, base as usize + size as usize);
            Some(Owned { block })
        }
    }
}

/// [`write_crossing`] of a [`Source`]: the same bytes, no JSON value made.
pub fn write_crossing_src<T: Source>(v: &T, out: &mut Vec<u8>) -> bool {
    let (mut nodes, mut text) = (0u64, 0u64);
    measure_src(v, &mut nodes, &mut text);
    let size = size_of::<FixedBlock>() as u64 + nodes + text;
    if size > u32::MAX as u64 {
        return false;
    }
    let start = out.len();
    out.resize(start + size as usize, 0);
    // SAFETY: as `write_crossing`.
    unsafe {
        let base = out.as_mut_ptr().add(start);
        let block = base as *mut FixedBlock;
        std::ptr::addr_of_mut!((*block).magic).write_unaligned(MAGIC);
        std::ptr::addr_of_mut!((*block).version).write_unaligned(VERSION);
        std::ptr::addr_of_mut!((*block).size).write_unaligned(size as u32);
        std::ptr::addr_of_mut!((*block).reserved).write_unaligned(CROSSING);
        let nodes_at = base.add(size_of::<FixedBlock>());
        let mut fill = Fill {
            nodes: nodes_at,
            text: nodes_at.add(nodes as usize),
            base: base as usize,
        };
        fill.value_src(v, std::ptr::addr_of_mut!((*block).root));
    }
    CROSSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    true
}

#[cfg(test)]
mod src_tests {
    use super::*;
    use serde_json::json;

    enum A<'a> {
        N(f64),
        T(&'a str),
        D(&'a [(&'a str, A<'a>)]),
        V([f64; 3]),
    }
    impl Source for A<'_> {
        fn node(&self) -> Node<'_, Self> {
            match self {
                A::N(n) => Node::Num(*n),
                A::T(s) => Node::Text(s),
                A::D(d) => Node::Dict(d),
                A::V(v) => Node::Vec3(*v),
            }
        }
    }

    #[test]
    fn a_source_makes_the_block_of_its_json() {
        let d = [
            ("entity", A::T("e1")),
            ("damage", A::N(2.0)),
            ("slip", A::N(0.25)),
            ("at", A::V([1.0, 0.5, 0.0])),
        ];
        let v = A::D(&d);
        let j = json!({ "entity": "e1", "damage": 2, "slip": 0.25, "at": [1, 0.5, 0] });
        assert_eq!(
            Owned::from_src(&v).unwrap().to_json().unwrap(),
            Owned::from_json(&j).unwrap().to_json().unwrap()
        );
        let (mut a, mut b) = (Vec::new(), Vec::new());
        assert!(write_crossing_src(&v, &mut a) && write_crossing(&j, &mut b));
        assert_eq!(a, b);
    }
}
