//! The structures of the contract and their layout as THIS compiler made it (docs/kit-modules.md §9.7,
//! docs/kit-structures.md). What crosses between modules of the common memory is declared here field by field —
//! `repr(C)` types and the records of the shared zone — never a type whose layout a compiler chooses.
//!
//! - [`STRUCTS`]: each structure with its number, its version, its size and alignment and the offset and size of
//!   every field, computed by the compiler from the types themselves (`offset_of!`, `size_of`).
//! - [`TABLE`]: the same as bytes, the custom section `gripforge.structs` of the core and of every kit module
//!   ([`structs_section!`]). The build tool checks it against the declarations
//!   (`kits/kitcall/structs.json`, tools/kit-modules/structs.mjs) and the host compares a kit of the
//!   common memory with the owner at load: the **version of the contract**, then each structure.
//! - [`PROBE`]: one number folded from the table, exported as `gf_layout()`; the host compares it once the module
//!   is instantiated.
//!
//! Nothing here names a compiler: two modules built from this contract by any compiler carry the same table.
//!
//! ## The section `gripforge.structs` (every integer little-endian)
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | magic `GFST` (`0x54534647`) |
//! | 4 | u32 | version of the structures contract, `major << 16 \| minor` |
//! | 8 | u32 | number of structures |
//! | 12 | u32 | reserved, 0 |
//!
//! then for each structure: its name (24 bytes, padded with 0), `id` u32, `version` u32, `size` u32, `align` u32,
//! the number of fields u32, and for each field its name (16 bytes, padded with 0), `offset` u32, `size` u32.

use crate::fixed::{FixedBlock, FixedPair, FixedValue};
use crate::mem::Bytes;
use crate::{events, zone, Ctx};
use std::mem::{align_of, offset_of, size_of};

/// `GFST`.
pub const MAGIC: u32 = 0x5453_4647;
/// The version of the structures contract (`major << 16 | minor`). A structure added, or a field added in reserved
/// bytes: a minor. A size, an offset or a meaning changed: a major — modules of two majors do not share a memory.
pub const VERSION: u32 = 2 << 16;
/// The name of the custom section.
pub const SECTION: &str = "gripforge.structs";

/// One field of a structure: where it is and how many bytes.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub name: &'static str,
    pub offset: u32,
    pub size: u32,
}

/// One structure of the contract.
#[derive(Clone, Copy, Debug)]
pub struct Struct {
    pub name: &'static str,
    /// Stable number (never reused).
    pub id: u32,
    /// `major << 16 | minor` of this structure.
    pub version: u32,
    pub size: u32,
    pub align: u32,
    pub fields: &'static [Field],
}

macro_rules! field {
    ($t:ty, $f:ident : $ft:ty) => {
        Field {
            name: stringify!($f),
            offset: offset_of!($t, $f) as u32,
            size: size_of::<$ft>() as u32,
        }
    };
}
const fn at(name: &'static str, offset: u32, size: u32) -> Field {
    Field { name, offset, size }
}

const V1: u32 = 1 << 16;

/// Every structure of the contract, in number order.
pub const STRUCTS: &[Struct] = &[
    Struct {
        name: "FixedValue",
        id: 1,
        version: V1,
        size: size_of::<FixedValue>() as u32,
        align: align_of::<FixedValue>() as u32,
        fields: &[
            field!(FixedValue, tag: u8),
            field!(FixedValue, reserved: [u8; 3]),
            field!(FixedValue, len: u32),
            field!(FixedValue, data: u64),
        ],
    },
    Struct {
        name: "FixedPair",
        id: 2,
        version: V1,
        size: size_of::<FixedPair>() as u32,
        align: align_of::<FixedPair>() as u32,
        fields: &[
            field!(FixedPair, key: u64),
            field!(FixedPair, key_len: u32),
            field!(FixedPair, reserved: u32),
            field!(FixedPair, value: FixedValue),
        ],
    },
    Struct {
        name: "FixedBlock",
        id: 3,
        version: V1,
        size: size_of::<FixedBlock>() as u32,
        align: align_of::<FixedBlock>() as u32,
        fields: &[
            field!(FixedBlock, magic: u32),
            field!(FixedBlock, version: u32),
            field!(FixedBlock, size: u32),
            field!(FixedBlock, reserved: u32),
            field!(FixedBlock, root: FixedValue),
        ],
    },
    Struct {
        name: "Bytes",
        id: 4,
        version: V1,
        size: size_of::<Bytes>() as u32,
        align: align_of::<Bytes>() as u32,
        fields: &[
            field!(Bytes, ptr: u32),
            field!(Bytes, len: u32),
            field!(Bytes, cap: u32),
        ],
    },
    Struct {
        name: "Ctx",
        id: 5,
        version: V1,
        size: size_of::<Ctx>() as u32,
        align: align_of::<Ctx>() as u32,
        fields: &[
            field!(Ctx, inst: u32),
            field!(Ctx, kit: u32),
            field!(Ctx, tick: u64),
            field!(Ctx, frame: u64),
            field!(Ctx, elapsed: f64),
            field!(Ctx, fixed_dt: f64),
            field!(Ctx, services_gen: u32),
            field!(Ctx, config_gen: u32),
        ],
    },
    // The records of the shared zone (src/zone.rs, src/events.rs): bytes at fixed places, not Rust types — the
    // numbers are the constants every accessor uses.
    Struct {
        name: "ZoneBuffer",
        id: 16,
        version: zone::VERSION,
        size: zone::DIR_ENTRY,
        align: 4,
        fields: &[
            at("id", 0, 4),
            at("offset", 4, 4),
            at("size", 8, 4),
            at("reserved", 12, 4),
        ],
    },
    Struct {
        name: "ZoneDirectory",
        id: 17,
        version: zone::VERSION,
        size: zone::DIR_SIZE,
        align: 8,
        fields: &[
            at("magic", 0, 4),
            at("version", 4, 4),
            at("size", 8, 4),
            at("buffers", 12, 4),
            at("entries", zone::DIR_ENTRIES, zone::DIR_ENTRY * 15),
        ],
    },
    Struct {
        name: "ZoneInputHeader",
        id: 18,
        version: zone::VERSION,
        size: zone::input::RECORDS,
        align: 8,
        fields: &[
            at("rev", zone::input::REV, 4),
            at("players", zone::input::PLAYERS, 4),
            at("actions", zone::input::COUNT, 4),
            at("capacity", zone::input::CAPACITY, 4),
            at("pointer", zone::input::POINTER, 16),
            at("taken", zone::input::TAKEN, 16),
            at("gain", zone::input::GAIN, 16),
            at("look", zone::input::LOOK, 16),
            at("look_action", zone::input::LOOK_ACTION, 4),
            at("reserved", 84, 12),
        ],
    },
    Struct {
        name: "ZoneInputRecord",
        id: 19,
        version: zone::VERSION,
        size: zone::input::RECORD,
        align: 4,
        fields: &[
            at("x", zone::input::R_X, 4),
            at("y", zone::input::R_Y, 4),
            at("held", zone::input::R_HELD, 4),
            at("since_end", zone::input::R_SINCE_END, 4),
            at("phase", zone::input::R_PHASE, 1),
            at("flags", zone::input::R_FLAGS, 1),
            at("presses", zone::input::R_PRESSES, 1),
            at("taps", zone::input::R_TAPS, 1),
            at("tick", zone::input::R_TICK, 4),
        ],
    },
    Struct {
        name: "ZoneStateHeader",
        id: 21,
        version: zone::VERSION,
        size: 4,
        align: 4,
        fields: &[at("rev", zone::state::REV, 4)],
    },
    Struct {
        name: "ZoneEventsHeader",
        id: 22,
        version: zone::VERSION,
        size: events::HEADER,
        align: 8,
        fields: &[
            at("head", events::HEAD, 4),
            at("records", events::RECORDS, 4),
            at("wraps", events::WRAPS, 4),
            at("reserved", 12, 20),
        ],
    },
    Struct {
        name: "ZoneEventRecord",
        id: 23,
        version: zone::VERSION,
        size: events::REC_HEADER,
        align: 8,
        fields: &[at("len", 0, 4), at("kind", 4, 4)],
    },
    Struct {
        name: "ZoneRegionsHeader",
        id: 24,
        version: zone::VERSION,
        size: zone::regions::FIRST,
        align: 8,
        fields: &[at("used", zone::regions::NEXT, 4), at("reserved", 4, 4)],
    },
];

const NAME: usize = 24;
const FIELD_NAME: usize = 16;

const fn table_len(structs: &[Struct]) -> usize {
    let (mut n, mut i) = (16, 0);
    while i < structs.len() {
        n += NAME + 20 + structs[i].fields.len() * (FIELD_NAME + 8);
        i += 1;
    }
    n
}

/// Bytes of [`TABLE`].
pub const TABLE_LEN: usize = table_len(STRUCTS);

const fn put_u32(mut out: [u8; TABLE_LEN], at: usize, v: u32) -> [u8; TABLE_LEN] {
    let b = v.to_le_bytes();
    out[at] = b[0];
    out[at + 1] = b[1];
    out[at + 2] = b[2];
    out[at + 3] = b[3];
    out
}

const fn put_name(
    mut out: [u8; TABLE_LEN],
    at: usize,
    name: &str,
    width: usize,
) -> [u8; TABLE_LEN] {
    let b = name.as_bytes();
    assert!(
        b.len() < width,
        "a name of the structures table is too long"
    );
    let mut i = 0;
    while i < b.len() {
        out[at + i] = b[i];
        i += 1;
    }
    out
}

const fn table(structs: &[Struct]) -> [u8; TABLE_LEN] {
    let mut out = [0u8; TABLE_LEN];
    out = put_u32(out, 0, MAGIC);
    out = put_u32(out, 4, VERSION);
    out = put_u32(out, 8, structs.len() as u32);
    let (mut at, mut i) = (16, 0);
    while i < structs.len() {
        let s = &structs[i];
        out = put_name(out, at, s.name, NAME);
        out = put_u32(out, at + NAME, s.id);
        out = put_u32(out, at + NAME + 4, s.version);
        out = put_u32(out, at + NAME + 8, s.size);
        out = put_u32(out, at + NAME + 12, s.align);
        out = put_u32(out, at + NAME + 16, s.fields.len() as u32);
        at += NAME + 20;
        let mut f = 0;
        while f < s.fields.len() {
            out = put_name(out, at, s.fields[f].name, FIELD_NAME);
            out = put_u32(out, at + FIELD_NAME, s.fields[f].offset);
            out = put_u32(out, at + FIELD_NAME + 4, s.fields[f].size);
            at += FIELD_NAME + 8;
            f += 1;
        }
        i += 1;
    }
    out
}

/// The structures as bytes: the custom section `gripforge.structs` of every module.
pub const TABLE: [u8; TABLE_LEN] = table(STRUCTS);

/// One number folded from [`TABLE`] at compile time (FNV-1a, 64 bits): what `gf_layout()` answers.
pub const PROBE: u64 = {
    let (mut h, mut i) = (0xcbf2_9ce4_8422_2325u64, 0);
    while i < TABLE_LEN {
        h = (h ^ TABLE[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
};

/// What `gf_layout()` answers.
pub fn probe() -> u64 {
    PROBE
}

/// Writes the structures table of this build in the custom section `gripforge.structs` of the module (once per
/// wasm: the core, and `export_kit!` for a kit).
#[macro_export]
macro_rules! structs_section {
    () => {
        #[cfg(target_arch = "wasm32")]
        #[link_section = "gripforge.structs"]
        #[used]
        static GF_STRUCTS: [u8; $crate::layout::TABLE_LEN] = $crate::layout::TABLE;
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_what_the_contract_pins() {
        assert_eq!(&TABLE[..4], b"GFST");
        assert_eq!(u32::from_le_bytes(TABLE[4..8].try_into().unwrap()), VERSION);
        assert_eq!(
            u32::from_le_bytes(TABLE[8..12].try_into().unwrap()) as usize,
            STRUCTS.len()
        );
        assert_eq!(&TABLE[16..26], b"FixedValue");
        // Version 1.0 of the structures, as any compiler must lay them out: this number changes only with them.
        assert_eq!(PROBE, probe());
        let by = |name: &str| STRUCTS.iter().find(|s| s.name == name).unwrap();
        let layout = |name: &str| {
            (
                by(name).size,
                by(name).align,
                by(name)
                    .fields
                    .iter()
                    .map(|f| (f.offset, f.size))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            layout("FixedValue"),
            (16, 8, vec![(0, 1), (1, 3), (4, 4), (8, 8)])
        );
        assert_eq!(
            layout("FixedPair"),
            (32, 8, vec![(0, 8), (8, 4), (12, 4), (16, 16)])
        );
        assert_eq!(
            layout("FixedBlock"),
            (32, 8, vec![(0, 4), (4, 4), (8, 4), (12, 4), (16, 16)])
        );
        assert_eq!(layout("Bytes"), (12, 4, vec![(0, 4), (4, 4), (8, 4)]));
        assert_eq!(
            layout("Ctx"),
            (
                48,
                8,
                vec![
                    (0, 4),
                    (4, 4),
                    (8, 8),
                    (16, 8),
                    (24, 8),
                    (32, 8),
                    (40, 4),
                    (44, 4)
                ]
            )
        );
        // Numbers are unique, fields lie inside their structure and do not overlap.
        let mut ids = std::collections::HashSet::new();
        for s in STRUCTS {
            assert!(ids.insert(s.id), "{}: number {} used twice", s.name, s.id);
            let mut end = 0;
            for f in s.fields {
                assert!(
                    f.offset >= end && f.offset + f.size <= s.size,
                    "{}.{}",
                    s.name,
                    f.name
                );
                end = f.offset + f.size;
            }
        }
    }

    #[test]
    fn the_context_of_a_call_is_its_structure() {
        // `Ctx` is written field by field in a message; its bytes there are the `repr(C)` structure.
        let c = Ctx {
            inst: 1,
            kit: 2,
            tick: 3,
            frame: 4,
            elapsed: 5.5,
            fixed_dt: 6.5,
            services_gen: 7,
            config_gen: 8,
        };
        let mut w = crate::Writer::new();
        c.write(&mut w);
        let raw: [u8; 48] = unsafe { std::mem::transmute(c) };
        assert_eq!(w.bytes(), &raw[..]);
    }
}
