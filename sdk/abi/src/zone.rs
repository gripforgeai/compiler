//! The shared zone of the kit logic modules (docs/kit-modules.md §7): ONE block of bytes with a fixed layout, cut
//! into buffers (the published inputs and state versions, the event queue, the arrays of the world, regions given
//! on demand). It is not an allocator: every module keeps its own memory and its own allocator; the zone only
//! holds what several modules read or write at fixed places.
//!
//! Where it lives: in the core's memory (the core writes it in place). A kit never sees an address, only
//! **offsets in the zone**, through the accessors of `kithost.zone@1` (`zone_read_*`, `zone_write_*`, `zone_copy_*`:
//! kits/kit_guest/src/zone.rs). A kit module of the common memory loads and stores in the zone where it
//! is (the same memory); an isolated one reaches the accessors by direct links (a wasm-to-wasm call to the core's
//! `gfx_zone_*`, the two copies by the calls host, which refuses a write outside what the kit may write).
//!
//! Every integer is little-endian. An offset is from the start of the zone; `0` is never a buffer.
//!
//! ## Directory (at 0, 256 bytes)
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | magic `GFZ1` (`0x315A4647`) |
//! | 4 | u32 | layout version, `major << 16 \| minor` (this is 2.0) |
//! | 8 | u32 | size of the zone in bytes |
//! | 12 | u32 | number of buffers `n` |
//! | 16 + 16 × i | u32 × 4 | buffer `i`: id, offset, size, 0 (reserved) |
//!
//! A reader finds a buffer **by its id** in the directory once at load ([`Buffers::read`]); the offsets below are
//! those of layout 1.0, a later layout may move or grow a buffer (same ids, new offsets).
//!
//! | id | buffer | offset | size | owner of the layout |
//! |---|---|---|---|---|
//! | 1 | `INPUT`: the input actions as read now (core.input) and the pointer total | `0x000100` | `0x002f00` | below |
//! | 2 | `STATE`: versions of the game state | `0x003000` | `0x001000` | below |
//! | 3 | `EVENTS`: the event queue (a ring) | `0x004000` | `0x03c000` | step 5 (its own doc) |
//! | 4 | `WORLD`: the hot arrays of the world | `0x040000` | `0x0c0000` | step 4 (its own doc) |
//! | 5 | `REGIONS`: blocks given on demand | `0x100000` | the rest | below |
//!
//! 4 MiB at start (`0x400000`). **These are the defaults**: the sizes, the offsets and the number of input actions
//! of a game come from its schema (`gripforge.game-schema/1`, docs/game-schema.md; [`Layout::from_schema`]), given to
//! the core before the game starts. The zone grows when `REGIONS` is full (it doubles, up to the schema's ceiling):
//! `REGIONS` is the last buffer, so no offset moves; only the size in the directory changes.
//!
//! ## `INPUT` (id 1)
//!
//! Written by the kit **core.input** (the only source of the actions: no table in the core), save the pointer
//! total, added in place by the host's devices; read in place by every engine, gateway and kit.
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 × 4 | `rev` (moved at each tick that changed a record; 0: no core.input), `players`, `actions` per player, `capacity` (records the buffer holds, `core.input` of the game schema) |
//! | 16 | f64 × 2 | `pointer`: the total relative motion of the pointer (x right, y down), added by the devices, never reset |
//! | 32 | f64 × 2 × 3 | `taken` (the pointer total the last tick read), `gain` (look of player 0 per unit of motion now), `look` (the sum of its tick values) |
//! | 80 | u32, 12 bytes | `look_action` (`0xffffffff`: none), reserved |
//! | 96 + 24 × (player × actions + action) | record | core.input' `ActionState` (`x`, `y`, `held`, `since_end` f32; `phase`, `flags`, `presses`, `taps` u8; `tick` u32) |
//!
//! At the render, `look + (pointer - taken) × gain` is the look of player 0 now (no frame late); the names of the
//! actions are the order of the service `core.input.actions`.
//!
//! ## `STATE` (id 2)
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | `rev`: the revision of the game state (vars and phase): changes at every write |
//!
//! ## `REGIONS` (id 5)
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | bytes in use (information: the allocator's own records are in the core, not in the zone) |
//! | 8 | … | the blocks: `region(len)` gives one (8-byte aligned, zeroed), `region_free(offset)` takes it back; `tick_region(len)` gives one that lives until the next tick (an arena emptied when the tick changes) |

use serde_json::Value as Json;

/// `GFZ1`.
pub const MAGIC: u32 = 0x315A_4647;
/// Layout version (`major << 16 | minor`): 2.0 since the `INPUT` buffer is core.input' (no table in the core).
pub const VERSION: u32 = 2 << 16;
/// Bytes of the zone at start when no game schema says otherwise (it grows in `REGIONS`, the last buffer).
pub const SIZE: u32 = 0x40_0000;
/// The ceiling of the zone when no game schema says otherwise.
pub const MAX_SIZE: u32 = 256 << 20;
/// Bytes of the directory.
pub const DIR_SIZE: u32 = 0x100;
/// Directory: where the entries start, and the bytes of one.
pub const DIR_ENTRIES: u32 = 16;
pub const DIR_ENTRY: u32 = 16;

/// One buffer of the zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buffer {
    pub id: u32,
    pub offset: u32,
    pub size: u32,
}

pub const INPUT: Buffer = Buffer {
    id: 1,
    offset: 0x100,
    size: 0x2f00,
};
pub const STATE: Buffer = Buffer {
    id: 2,
    offset: 0x3000,
    size: 0x1000,
};
pub const EVENTS: Buffer = Buffer {
    id: 3,
    offset: 0x4000,
    size: 0x3_c000,
};
pub const WORLD: Buffer = Buffer {
    id: 4,
    offset: 0x4_0000,
    size: 0xc_0000,
};
pub const REGIONS: Buffer = Buffer {
    id: 5,
    offset: 0x10_0000,
    size: 0x30_0000,
};

/// The buffers of layout 1.0, in directory order.
pub const BUFFERS: &[Buffer] = &[INPUT, STATE, EVENTS, WORLD, REGIONS];

/// `INPUT`: offsets in the buffer.
pub mod input {
    pub const REV: u32 = 0;
    pub const PLAYERS: u32 = 4;
    pub const COUNT: u32 = 8;
    pub const CAPACITY: u32 = 12;
    pub const POINTER: u32 = 16;
    pub const TAKEN: u32 = 32;
    pub const GAIN: u32 = 48;
    pub const LOOK: u32 = 64;
    pub const LOOK_ACTION: u32 = 80;
    pub const RECORDS: u32 = 96;
    pub const RECORD: u32 = 24;
    /// Records of the default layout.
    pub const ACTIONS: u32 = 256;
    /// No action `look`.
    pub const NO_ACTION: u32 = u32::MAX;
    /// In a record.
    pub const R_X: u32 = 0;
    pub const R_Y: u32 = 4;
    pub const R_HELD: u32 = 8;
    pub const R_SINCE_END: u32 = 12;
    pub const R_PHASE: u32 = 16;
    pub const R_FLAGS: u32 = 17;
    pub const R_PRESSES: u32 = 18;
    pub const R_TAPS: u32 = 19;
    pub const R_TICK: u32 = 20;
    /// Bytes of the buffer for `capacity` records.
    pub const fn bytes(capacity: u32) -> u32 {
        RECORDS + RECORD * capacity
    }
}

/// `STATE`: offsets in the buffer.
pub mod state {
    pub const REV: u32 = 0;
}

/// `REGIONS`: offsets in the buffer.
pub mod regions {
    pub const NEXT: u32 = 0;
    pub const FIRST: u32 = 8;
}

/// Writes the directory of layout 1.0 at the start of `zone` (the defaults).
pub fn write_directory(zone: &mut [u8]) {
    Layout::v1().write_directory(zone, SIZE);
}

/// The layout of the zone of one game: what its schema fixes (docs/game-schema.md), or the defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The fingerprint of the game schema it comes from (empty: the defaults).
    pub version: String,
    /// Bytes at start, and the ceiling the zone never passes.
    pub initial: u32,
    pub max: u32,
    pub buffers: Buffers,
    /// Records of the `INPUT` buffer.
    pub actions: u32,
}

impl Layout {
    /// The defaults (no game schema given).
    pub fn v1() -> Layout {
        Layout {
            version: String::new(),
            initial: SIZE,
            max: MAX_SIZE,
            buffers: Buffers::V1,
            actions: input::ACTIONS,
        }
    }

    /// The buffers in directory order.
    pub fn list(&self) -> [Buffer; 5] {
        let b = &self.buffers;
        [b.input, b.state, b.events, b.world, b.regions]
    }

    /// From a game schema (`gripforge.game-schema/1`: its `version`, `zone` and `input` sections). An error names
    /// what does not hold; nothing is half applied.
    pub fn from_schema(schema: &Json) -> Result<Layout, String> {
        let int = |v: &Json, what: &str| {
            v.as_u64()
                .filter(|n| *n <= u32::MAX as u64)
                .map(|n| n as u32)
                .ok_or(format!("game schema: {what} is not a size"))
        };
        let zone = &schema["zone"];
        let buffer = |name: &str, id: u32| -> Result<Buffer, String> {
            let b = &zone["buffers"][name];
            Ok(Buffer {
                id,
                offset: int(&b["offset"], &format!("zone.buffers.{name}.offset"))?,
                size: int(&b["bytes"], &format!("zone.buffers.{name}.bytes"))?,
            })
        };
        let layout = Layout {
            version: schema["version"].as_str().unwrap_or_default().to_string(),
            initial: int(&zone["initialBytes"], "zone.initialBytes")?,
            max: int(&zone["maxBytes"], "zone.maxBytes")?,
            buffers: Buffers {
                input: buffer("input", 1)?,
                state: buffer("state", 2)?,
                events: buffer("events", 3)?,
                world: buffer("world", 4)?,
                regions: buffer("regions", 5)?,
            },
            actions: int(&schema["input"]["actions"], "core.input")?,
        };
        let mut end = DIR_SIZE;
        for b in layout.list() {
            if b.offset < end || b.offset % 8 != 0 {
                return Err(format!("game schema: buffer {} of the zone starts at {:#x}, before the end of the previous one or unaligned", b.id, b.offset));
            }
            end = b
                .offset
                .checked_add(b.size)
                .ok_or("game schema: the zone passes 4 GiB")?;
        }
        if end != layout.initial || layout.initial > layout.max {
            return Err(format!("game schema: the buffers of the zone end at {end:#x}, its start size is {:#x} and its ceiling {:#x}", layout.initial, layout.max));
        }
        if layout.actions == 0 || input::bytes(layout.actions) > layout.buffers.input.size {
            return Err(format!(
                "game schema: {} input actions do not fit the input buffer ({} bytes)",
                layout.actions, layout.buffers.input.size
            ));
        }
        if layout.buffers.regions.size < 64 || layout.buffers.state.size < 8 {
            return Err("game schema: the state or the regions buffer is too small".into());
        }
        Ok(layout)
    }

    /// Writes the directory at the start of `zone`, for a zone of `size` bytes now (the last buffer, `REGIONS`,
    /// reaches the end of the zone).
    pub fn write_directory(&self, zone: &mut [u8], size: u32) {
        let mut put =
            |at: u32, v: u32| zone[at as usize..at as usize + 4].copy_from_slice(&v.to_le_bytes());
        put(0, MAGIC);
        put(4, VERSION);
        put(8, size);
        let list = self.list();
        put(12, list.len() as u32);
        for (i, b) in list.iter().enumerate() {
            let at = DIR_ENTRIES + DIR_ENTRY * i as u32;
            put(at, b.id);
            put(at + 4, b.offset);
            put(at + 8, if b.id == 5 { size - b.offset } else { b.size });
            put(at + 12, 0);
        }
    }
}

/// Where the buffers are, as the directory says (a reader asks once at load, with its way to read a u32).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buffers {
    pub input: Buffer,
    pub state: Buffer,
    pub events: Buffer,
    pub world: Buffer,
    pub regions: Buffer,
}

impl Buffers {
    /// Layout 1.0 as compiled (what the core writes).
    pub const V1: Buffers = Buffers {
        input: INPUT,
        state: STATE,
        events: EVENTS,
        world: WORLD,
        regions: REGIONS,
    };

    /// Reads the directory with `read_u32(offset)`. `None`: not a zone of this major version. A buffer the
    /// directory does not list has size 0 (the reader then keeps to the calls).
    pub fn read(read_u32: impl Fn(u32) -> u32) -> Option<Buffers> {
        if read_u32(0) != MAGIC || read_u32(4) >> 16 != VERSION >> 16 {
            return None;
        }
        let n = read_u32(12).min((DIR_SIZE - DIR_ENTRIES) / DIR_ENTRY);
        let find = |id: u32| {
            (0..n)
                .map(|i| DIR_ENTRIES + DIR_ENTRY * i)
                .find(|at| read_u32(*at) == id)
                .map_or(
                    Buffer {
                        id,
                        offset: 0,
                        size: 0,
                    },
                    |at| Buffer {
                        id,
                        offset: read_u32(at + 4),
                        size: read_u32(at + 8),
                    },
                )
        };
        Some(Buffers {
            input: find(1),
            state: find(2),
            events: find(3),
            world: find(4),
            regions: find(5),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_round_trip_and_no_overlap() {
        let mut z = vec![0u8; SIZE as usize];
        write_directory(&mut z);
        let b = Buffers::read(|at| {
            u32::from_le_bytes(z[at as usize..at as usize + 4].try_into().unwrap())
        })
        .unwrap();
        assert_eq!(b.input, INPUT);
        assert_eq!(b.regions, REGIONS);
        let mut end = DIR_SIZE;
        for b in BUFFERS {
            assert!(b.offset >= end && b.offset % 8 == 0, "{b:?}");
            end = b.offset + b.size;
        }
        assert_eq!(end, SIZE);
        assert!(input::bytes(input::ACTIONS) <= INPUT.size);
        // A game's schema moves and grows the buffers; what does not hold is refused.
        let schema = serde_json::json!({ "version": "abc", "input": { "actions": 600 }, "zone": { "initialBytes": 0x50_0000, "maxBytes": 0x100_0000, "buffers": {
            "input": { "offset": 0x100, "bytes": 0x5000 }, "state": { "offset": 0x5100, "bytes": 0x1000 }, "events": { "offset": 0x6100, "bytes": 0x10_0000 },
            "world": { "offset": 0x10_6100, "bytes": 0x20_0000 }, "regions": { "offset": 0x30_6100, "bytes": 0x1f_9f00 } } } });
        let l = Layout::from_schema(&schema).unwrap();
        assert_eq!(
            (l.actions, l.buffers.world.offset, l.max),
            (600, 0x10_6100, 0x100_0000)
        );
        let mut z = vec![0u8; l.initial as usize];
        l.write_directory(&mut z, l.initial);
        let b = Buffers::read(|at| {
            u32::from_le_bytes(z[at as usize..at as usize + 4].try_into().unwrap())
        })
        .unwrap();
        assert_eq!(b, l.buffers);
        let mut bad = schema.clone();
        bad["input"]["actions"] = serde_json::json!(900);
        assert!(Layout::from_schema(&bad)
            .unwrap_err()
            .contains("900 input actions do not fit"));
        bad = schema.clone();
        bad["zone"]["buffers"]["state"]["offset"] = serde_json::json!(0x4000);
        assert!(Layout::from_schema(&bad).is_err());
    }
}
