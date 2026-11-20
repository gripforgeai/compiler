//! The contracts a module reads or writes, and their check at load (docs/decisions/2026-11-20-abi-par-contrat.md §8).
//!
//! Each module carries the list of the shared contracts it reads or writes — every one with its major and its fields
//! (type, position, size, whether it has a default) as its build laid them out — in the custom section
//! [`SECTION`] of its wasm, or behind the symbol [`SYMBOL`] of its native library ([`contracts_section!`]). The host
//! (`core/kithost` natively, `core/gateway-web/contracts.mjs` on the web: the same rules, the same words) checks only
//! those contracts, each against the modules already loaded that declare it ([`Registry::admit`]):
//!
//! 1. the same major, or refused;
//! 2. the common prefix of the fields (head and row) has the same type, position and size field by field, or refused
//!    by the name of the field (a break without a new major);
//! 3. a reader that knows more fields than the writer: every field beyond has a default, or refused (rebuild the
//!    writer);
//! 4. nothing more: two readers, or a contract no other module declares, load.
//!
//! So changing a contract refuses only the modules that declare it, and adding a field at the end refuses nobody.
//! The check compares fields, never a number of the ABI: it works in development, where every ABI is `ABI_DEV`.
//!
//! The entries are what the generated crate `gripforge-contracts` writes per contract (`DECL`, its role byte 0); the
//! module chooses the role ([`entry`]). Byte format (`interface-B-C`, little-endian, no alignment):
//!
//! - section: magic `GFCS` (u32), format u16 = [`FORMAT`], count u16, then the entries;
//! - entry: id u32 (FNV-1a 32 of the name), major u16, kind u8, role u8, head_fields u16, fields u16, name (u8 length
//!   + UTF-8), then `head_fields` then `fields` fields;
//! - field: offset u16, size u16, type u8, flags u8 (bit 0: has a default), name (u8 length + UTF-8).
//!
//! The format of the section is not an ABI number: a change of it is a new magic. It never takes `ABI_DEV`.

/// The custom section of a wasm.
pub const SECTION: &str = "gripforge.contracts";
/// The symbol of a native library: `extern "C" fn gf_contracts(len: *mut usize) -> *const u8`.
pub const SYMBOL: &[u8] = b"gf_contracts\0";
/// `GFCS`.
pub const MAGIC: u32 = 0x5343_4647;
/// The format of the section.
pub const FORMAT: u16 = 1;
/// The module reads the contract.
pub const READ: u8 = 1;
/// The module writes the contract.
pub const WRITE: u8 = 2;
/// The module reads and writes it.
pub const BOTH: u8 = 3;
/// The position of the role byte in an entry.
const ROLE_AT: usize = 7;

/// The signature of [`SYMBOL`].
pub type ContractsFn = unsafe extern "C" fn(len: *mut usize) -> *const u8;

/// The bytes of a section from its entries (`(DECL, role)`), at compile time.
pub const fn section_len(entries: &[(&[u8], u8)]) -> usize {
    let (mut n, mut i) = (8, 0);
    while i < entries.len() {
        n += entries[i].0.len();
        i += 1;
    }
    n
}

/// The section of `entries`, `N` = [`section_len`]: the role of each written in its entry.
pub const fn section<const N: usize>(entries: &[(&[u8], u8)]) -> [u8; N] {
    let mut out = [0u8; N];
    let m = MAGIC.to_le_bytes();
    let f = FORMAT.to_le_bytes();
    let c = (entries.len() as u16).to_le_bytes();
    let head = [m[0], m[1], m[2], m[3], f[0], f[1], c[0], c[1]];
    let mut at = 0;
    while at < 8 {
        out[at] = head[at];
        at += 1;
    }
    let mut i = 0;
    while i < entries.len() {
        let (decl, role) = entries[i];
        let mut j = 0;
        while j < decl.len() {
            out[at] = if j == ROLE_AT { role } else { decl[j] };
            at += 1;
            j += 1;
        }
        i += 1;
    }
    out
}

/// The bytes of a section of two lists of entries (the façade's and a module's own), at compile time.
pub const fn section_len2(a: &[(&[u8], u8)], b: &[(&[u8], u8)]) -> usize {
    section_len(a) + section_len(b) - 8
}

/// The section of the entries of `a` then `b`, `N` = [`section_len2`]: one section, the role of each in its entry.
pub const fn section2<const N: usize>(a: &[(&[u8], u8)], b: &[(&[u8], u8)]) -> [u8; N] {
    let mut out = [0u8; N];
    let m = MAGIC.to_le_bytes();
    let f = FORMAT.to_le_bytes();
    let c = ((a.len() + b.len()) as u16).to_le_bytes();
    let head = [m[0], m[1], m[2], m[3], f[0], f[1], c[0], c[1]];
    let mut at = 0;
    while at < 8 {
        out[at] = head[at];
        at += 1;
    }
    let mut i = 0;
    while i < a.len() + b.len() {
        let (decl, role) = if i < a.len() { a[i] } else { b[i - a.len()] };
        let mut j = 0;
        while j < decl.len() {
            out[at] = if j == ROLE_AT { role } else { decl[j] };
            at += 1;
            j += 1;
        }
        i += 1;
    }
    out
}

/// The section of `entries` at run time (a host's own declarations, read by the same [`read`]).
pub fn section_vec(entries: &[(&[u8], u8)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(section_len(entries));
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (decl, role) in entries {
        let at = out.len();
        out.extend_from_slice(decl);
        if let Some(b) = out.get_mut(at + ROLE_AT) {
            *b = *role;
        }
    }
    out
}

/// Declares the contracts of this binary: `contracts_section!(&[(physics_bodies::DECL, contract::READ), …]);`, once per
/// module (wasm: the custom section; native: the symbol `gf_contracts`). An empty list declares that the module reads
/// no converted contract.
#[macro_export]
macro_rules! contracts_section {
    // Two lists in one section: the façade's (every kit module) and this module's own (a contract only it touches).
    ($entries:expr, $own:expr) => {
        const GF_CONTRACTS_ENTRIES: &[(&[u8], u8)] = $entries;
        const GF_CONTRACTS_OWN: &[(&[u8], u8)] = $own;
        const GF_CONTRACTS_LEN: usize =
            $crate::contract::section_len2(GF_CONTRACTS_ENTRIES, GF_CONTRACTS_OWN);
        #[cfg_attr(target_arch = "wasm32", link_section = "gripforge.contracts")]
        #[used]
        static GF_CONTRACTS_SECTION: [u8; GF_CONTRACTS_LEN] =
            $crate::contract::section2::<GF_CONTRACTS_LEN>(GF_CONTRACTS_ENTRIES, GF_CONTRACTS_OWN);
        /// The contracts this library reads or writes (gripforge_abi::contract).
        #[cfg(not(target_arch = "wasm32"))]
        #[no_mangle]
        pub unsafe extern "C" fn gf_contracts(len: *mut usize) -> *const u8 {
            if !len.is_null() {
                *len = GF_CONTRACTS_LEN;
            }
            GF_CONTRACTS_SECTION.as_ptr()
        }
    };
    ($entries:expr) => {
        const GF_CONTRACTS_ENTRIES: &[(&[u8], u8)] = $entries;
        const GF_CONTRACTS_LEN: usize = $crate::contract::section_len(GF_CONTRACTS_ENTRIES);
        #[cfg_attr(target_arch = "wasm32", link_section = "gripforge.contracts")]
        #[used]
        static GF_CONTRACTS_SECTION: [u8; GF_CONTRACTS_LEN] =
            $crate::contract::section::<GF_CONTRACTS_LEN>(GF_CONTRACTS_ENTRIES);
        /// The contracts this library reads or writes (gripforge_abi::contract).
        #[cfg(not(target_arch = "wasm32"))]
        #[no_mangle]
        pub unsafe extern "C" fn gf_contracts(len: *mut usize) -> *const u8 {
            if !len.is_null() {
                *len = GF_CONTRACTS_LEN;
            }
            GF_CONTRACTS_SECTION.as_ptr()
        }
    };
}

/// One field as a module laid it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub offset: u16,
    pub size: u16,
    pub ty: u8,
    pub default: bool,
    pub name: String,
}

/// One contract a module declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    pub id: u32,
    pub major: u16,
    pub kind: u8,
    pub role: u8,
    pub name: String,
    pub head: Vec<Field>,
    pub row: Vec<Field>,
}

/// The name of a type code (for the messages).
pub fn type_name(ty: u8) -> &'static str {
    match ty {
        1 => "u8",
        2 => "u16",
        3 => "u32",
        4 => "u64",
        5 => "i32",
        6 => "i64",
        7 => "f32",
        8 => "f64",
        9 => "i8",
        10 => "i16",
        11 => "v3",
        12 => "quat",
        13 => "addr",
        14 => "str",
        15 => "record",
        _ => "?",
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self
            .b
            .get(self.at..self.at + n)
            .ok_or_else(|| format!("contracts section cut short at byte {}", self.at))?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn name(&mut self) -> Result<String, String> {
        let n = self.u8()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn field(&mut self) -> Result<Field, String> {
        let (offset, size, ty, flags) = (self.u16()?, self.u16()?, self.u8()?, self.u8()?);
        Ok(Field {
            offset,
            size,
            ty,
            default: flags & 1 != 0,
            name: self.name()?,
        })
    }
}

/// Reads a section (or the bytes of `gf_contracts`).
pub fn read(bytes: &[u8]) -> Result<Vec<Decl>, String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.u32()? != MAGIC {
        return Err("not a contracts section (magic GFCS missing)".into());
    }
    let format = r.u16()?;
    if format != FORMAT {
        return Err(format!(
            "contracts section of format {format}, this host reads {FORMAT}"
        ));
    }
    let count = r.u16()?;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (id, major, kind, role) = (r.u32()?, r.u16()?, r.u8()?, r.u8()?);
        let (heads, rows) = (r.u16()?, r.u16()?);
        let name = r.name()?;
        let head = (0..heads).map(|_| r.field()).collect::<Result<_, _>>()?;
        let row = (0..rows).map(|_| r.field()).collect::<Result<_, _>>()?;
        out.push(Decl {
            id,
            major,
            kind,
            role,
            name,
            head,
            row,
        });
    }
    if r.at != bytes.len() {
        return Err(format!(
            "contracts section: {} bytes after the last entry",
            bytes.len() - r.at
        ));
    }
    Ok(out)
}

fn verb(role: u8) -> &'static str {
    match role {
        READ => "reads",
        WRITE => "writes",
        _ => "reads and writes",
    }
}

fn field_text(f: &Field) -> String {
    format!(
        "`{}` {} ({} bytes) at {}",
        f.name,
        type_name(f.ty),
        f.size,
        f.offset
    )
}

/// The first field of the common prefix laid out otherwise: `(part, index, mine, theirs)`.
fn prefix_differs<'a>(
    a: &'a Decl,
    b: &'a Decl,
) -> Option<(&'static str, usize, &'a Field, &'a Field)> {
    for (part, x, y) in [("head", &a.head, &b.head), ("row", &a.row, &b.row)] {
        for (i, (f, g)) in x.iter().zip(y.iter()).enumerate() {
            if (f.ty, f.offset, f.size) != (g.ty, g.offset, g.size) {
                return Some((part, i, f, g));
            }
        }
    }
    None
}

/// The fields a reader knows beyond its writer that have no default: `(part, field)`.
fn missing<'a>(reader: &'a Decl, writer: &Decl) -> Option<(&'static str, &'a Field)> {
    let extra =
        |mine: &'a [Field], theirs: &[Field]| mine.iter().skip(theirs.len()).find(|f| !f.default);
    extra(&reader.head, &writer.head)
        .map(|f| ("head", f))
        .or_else(|| extra(&reader.row, &writer.row).map(|f| ("row", f)))
}

/// Whether `module`'s declaration `mine` may share its contract with `other`'s `theirs`; the refusal, named.
pub fn check_pair(module: &str, mine: &Decl, other: &str, theirs: &Decl) -> Result<(), String> {
    let says = |why: &str, who: &str| {
        format!(
            "contract_refused: {module} {} {} {}.{}, {other} {} it {}.{} — {why}: rebuild {who}",
            verb(mine.role),
            mine.name,
            mine.major,
            mine.row.len(),
            verb(theirs.role),
            theirs.major,
            theirs.row.len()
        )
    };
    if mine.major != theirs.major {
        return Err(says(
            "another major",
            &format!("the one built against the older major ({module} or {other})"),
        ));
    }
    if let Some((part, i, f, g)) = prefix_differs(mine, theirs) {
        return Err(says(
            &format!(
                "{part} field #{i} is {} here, {} in {other} (a break without a new major)",
                field_text(f),
                field_text(g)
            ),
            &format!("{module} and {other} from the same contract"),
        ));
    }
    // A reader newer than its writer reads the defaults of the fields the writer does not know, or cannot load.
    let pairs = [(mine, theirs, other), (theirs, mine, module)];
    for (reader, writer, writer_name) in pairs {
        if reader.role & READ != 0 && writer.role & WRITE != 0 {
            if let Some((part, f)) = missing(reader, writer) {
                return Err(says(
                    &format!(
                        "the reader needs {part} field {} that the writer does not know and that has no default",
                        field_text(f)
                    ),
                    writer_name,
                ));
            }
        }
    }
    Ok(())
}

/// The contracts the modules loaded so far declare: the host first (its own), then each module as it loads.
#[derive(Default, Debug)]
pub struct Registry {
    loaded: Vec<(String, Vec<Decl>)>,
}

impl Registry {
    /// Checks every contract of `module` against the modules already admitted that declare it, then admits it: all
    /// or nothing. The same module admitted again with the same declarations is accepted (a library loaded on a
    /// second thread).
    pub fn admit(&mut self, module: &str, decls: Vec<Decl>) -> Result<(), String> {
        if let Some((_, known)) = self.loaded.iter().find(|(n, _)| n == module) {
            if *known == decls {
                return Ok(());
            }
        }
        for mine in &decls {
            for (other, theirs) in &self.loaded {
                if other == module {
                    continue;
                }
                for t in theirs.iter().filter(|t| t.id == mine.id) {
                    check_pair(module, mine, other, t)?;
                }
            }
        }
        self.loaded.retain(|(n, _)| n != module);
        self.loaded.push((module.to_string(), decls));
        Ok(())
    }

    /// Reads the section `bytes` of `module` ([`read`]) and admits it ([`Registry::admit`]): an unreadable section is
    /// refused as `contract_refused: <module>: <why>`, the same words as the web host (`admitSection`, contracts.mjs).
    pub fn admit_section(&mut self, module: &str, bytes: &[u8]) -> Result<(), String> {
        let decls = read(bytes).map_err(|e| format!("contract_refused: {module}: {e}"))?;
        self.admit(module, decls)
    }

    /// The modules admitted and what they declare.
    pub fn loaded(&self) -> &[(String, Vec<Decl>)] {
        &self.loaded
    }
}

/// FNV-1a 32 of a contract's name: its id.
pub const fn id_of(name: &str) -> u32 {
    let b = name.as_bytes();
    let (mut h, mut i) = (0x811c_9dc5u32, 0);
    while i < b.len() {
        h = (h ^ b[i] as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
