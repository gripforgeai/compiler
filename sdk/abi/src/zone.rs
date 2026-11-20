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
//! | 1 | `INPUT`: the pointer total, the ring of raw samples and the directory of the regions published by kits | `0x000100` | `0x002f00` | below |
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
//! Neutral (#1766): the host's devices add the pointer total in place; the core lays the ring of raw samples; the
//! kits publish their regions by name (core.input its actions `core.input.actions` and the look of a player
//! `core.input.look.0`, a [`pointer_follow`] block: their layouts are the kit's, in its kit.api.json `regions`).
//!
//! At 0 a block of the contract `abi.zone.input` (core/abi/contracts/abi.zone.input.json, reference
//! docs/contrats-abi.md), laid by the core with the zone: `TableHead`, then one row: `pointer` (f64 × 2, the total relative
//! motion of the pointer added by the devices, never reset), `ring` (the block of the ring of raw input samples,
//! `crate::ring`, contract `abi.input.ring`), `bindings` (the block of [`action_bindings`], laid by the kit that owns the
//! inputs), `published` (the block of [`published`]); an offset 0 is « none ». No position is written here: a reader
//! binds the head once and reads each field where the generated contract puts it ([`input::Fields`]).

//! ## `STATE` (id 2)
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | `rev`: the revision of the game state (vars and phase): changes at every write |
//! | 4 | u32 | `mailboxes`: offset of the directory of the mailboxes (`ports::mailbox::Table`, docs/decisions/2026-11-19-canaux-directs.md), laid by the core in `REGIONS` with the zone; 0: none |
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
///
/// History since, the value unchanged (docs/decisions/versions-dev.md: the number is given at the tag):
/// - #1766 (entries + audio, docs/design/entrees-hors-abi.md): `input::PUBLISHED` (92, a word that was 0) points to
///   the directory of the regions published by kits ([`published`]) and the neutral block [`pointer_follow`] is
///   added. core.input publishes `core.input.actions` and `core.input.look.0`; its readers find them by name. Nothing
///   moved yet (the retraits of `input::*` / `look::*` follow, grouped). Rebuild core.input, core.camera,
///   locomotion.first_person, core.control.
/// - #1766 (agent final): `input::{REV, PLAYERS, COUNT, CAPACITY, TAKEN, GAIN, LOOK, LOOK_ACTION, LOOK_AT, RECORDS,
///   RECORD, ACTIONS, NO_ACTION, R_*}`, `look::*`, `Layout::actions` (the schema's `input.actions` is no longer read)
///   and the structs `ZoneInputHeader` / `ZoneInputRecord` retired: core.input lays its actions in a region of its own
///   (`core.input.actions`, sized by its table) and the look in a [`pointer_follow`] block. Offsets of `POINTER`,
///   `RING`, `PUBLISHED` unchanged. Rebuild the player, core.input, core.camera, core.control,
///   locomotion.first_person and every gateway that read `ZoneInputHeader`.
/// - feat/entrees-hors-tick (docs/design/etat-des-actions-hors-tick.md, decision of 20/11 « l'état des actions est
///   écrit hors tick »): `input::BINDINGS` (88, a word that was 0) points to the bindings the host applies AT EACH EVENT
///   ([`action_bindings`], laid by core.input, published as data), and the neutral block [`action_state`] (one per
///   action, written by the host's input port, read in place by the physics, the render and the tick) is added; the
///   ring gets the axis `ring::axis::ACTION`. Nothing moved. Rebuild the logic module (kithost, web and native), every
///   gateway that feeds the input port, core.input, core.locomotion, locomotion.third_person,
///   locomotion.first_person and the physics module.
/// - lot 3 of the contracts (docs/decisions/2026-11-20-abi-par-contrat.md §9, feat/abi-contrat-lot3): the words of
///   `INPUT` (pointer 16, ring 84, bindings 88, published 92) become the block `abi.zone.input` at the start of the
///   buffer; the directory of the published regions (`abi.zone.published`), `PointerFollow` (`abi.input.pointer`),
///   `ActionState` (`abi.input.actions`, one block of rows), `ActionBindings` (`abi.input.bindings`) and the ring
///   (`abi.input.ring`) begin with a `TableHead`; every position comes from the generated contracts, the ring is read
///   at the stride it stores. Rebuild kithost.core, the native player, the physics module, every kit module and the
///   page's gateway (core/gateway-web).
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

/// Blocks of the zone that follow a shared contract, reached by their offset (bind, lay, rows at the stride stored).
pub mod table;

/// `INPUT`: its head, a block of the contract `abi.zone.input` (core/abi/contracts/abi.zone.input.json) at the start of
/// the buffer, laid by the core with the zone: the pointer total the host's devices add in place, and where the ring,
/// the bindings and the directory of the published regions are. Neutral since #1766: nothing of a kit (the kit that
/// owns the inputs lays and publishes its own regions, found by name: [`published`]). Every position comes from the
/// generated contract; a reader binds the head once ([`input::Fields::bind`]) and reads at the row it found.
pub mod input {
    pub use gripforge_contracts::abi_zone_input::{at, SPEC};
    /// Bytes of the head as the core lays it (TableHead and one row).
    pub const BYTES: u32 = super::table::bytes(&SPEC, 1);

    /// Where each field of the head is in the zone, its `TableHead` read once.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Fields {
        /// f64 × 2: the total relative motion of the pointer, added in place by the devices, never reset.
        pub pointer: u32,
        /// u32: where the ring of the raw input samples is (0: none), laid by the core.
        pub ring: u32,
        /// u32: where the bindings the host applies at each event are (0: none), laid by the kit that owns the inputs.
        pub bindings: u32,
        /// u32: where the directory of the regions published by kits is (0: none yet).
        pub published: u32,
    }

    impl Fields {
        /// Binds the head of the `INPUT` buffer at `input` (read with `read_u32(offset)`); `None` without a zone or
        /// when the head is not one this build reads.
        pub fn bind(input: u32, read_u32: impl Fn(u32) -> u32) -> Option<Fields> {
            let r = super::table::bind(&SPEC, input, read_u32)?.row(0);
            Some(Self::at_row(r))
        }
        /// The fields of the row at `row` (the writer of this build, which laid it: [`lay`]).
        pub const fn at_row(row: u32) -> Fields {
            Fields {
                pointer: row + at::POINTER as u32,
                ring: row + at::RING as u32,
                bindings: row + at::BINDINGS as u32,
                published: row + at::PUBLISHED as u32,
            }
        }
        /// The fields of the head the core lays at `input` (its own build: what it writes, never what a reader reads).
        pub const fn laid(input: u32) -> Fields {
            Self::at_row(super::table::row_of(&SPEC, input, 0))
        }
    }

    /// Lays the head at `input` of the bytes of the zone (the core, with the zone; the row is zeroed).
    pub fn lay(zone: &mut [u8], input: usize) {
        super::table::lay(zone, input, &SPEC, 1, 1);
    }
}

/// The directory of the regions published by kits (#1766, docs/design/entrees-hors-abi.md): a kit lays a block of
/// its own in `REGIONS` and publishes it under a name; a reader finds it ONCE by name and reads it in place. The core
/// knows no name and no layout: a published region is a block of a contract of its kit (`kits/<kit>/contracts/`).
/// A block of the contract `abi.zone.published`, its place in [`input::Fields::published`], laid by the first kit that
/// publishes: one row per name, the rows in use are its `count`, those it holds its `slots`.
pub mod published {
    pub use gripforge_contracts::abi_zone_published::{at, SPEC};
    /// Entries of the directory a first publisher lays.
    pub const DEFAULT_CAPACITY: u32 = 32;
    /// Bytes of a directory of `capacity` entries.
    pub const fn bytes(capacity: u32) -> u32 {
        super::table::bytes(&SPEC, capacity)
    }
    /// The name's key: FNV-1a 64 of its UTF-8 bytes (fixed: the same in every module and every run).
    pub const fn hash(name: &str) -> u64 {
        let b = name.as_bytes();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
    /// Positions in a row (from the generated contract).
    pub const E_KEY: u32 = at::KEY as u32;
    pub const E_OFFSET: u32 = at::OFFSET as u32;
    pub const E_BYTES: u32 = at::BYTES as u32;
}

/// `PointerFollow`: a neutral primitive (#1766), the contract `abi.input.pointer` (a block of one row): a 2D value a
/// kit writes at its tick and any reader extrapolates at the instant it draws, `sum + (pointer - taken) × gain +
/// velocity × (time since)`. No meaning is attached to it. `seq` odd while written. The positions below are those of
/// the generated contract, from the start of the ROW (a reader binds the block once: [`table::bind`]).
pub mod pointer_follow {
    pub use gripforge_contracts::abi_input_pointer::{at, SPEC};
    pub const SEQ: u32 = at::SEQ as u32;
    pub const TICK: u32 = at::TICK as u32;
    pub const DT: u32 = at::DT as u32;
    pub const GAIN: u32 = at::GAIN as u32;
    pub const VELOCITY: u32 = at::VELOCITY as u32;
    pub const SUM: u32 = at::SUM as u32;
    /// f64 × 2: the pointer total the writer's tick read.
    pub const TAKEN: u32 = at::TAKEN as u32;
    /// i32: the stick of this value, a row of an [`super::action_state`] block, as its distance in bytes from THIS ROW
    /// (0: none; decision of 20/11).
    pub const STICK: u32 = at::STICK as u32;
    /// f64 × 2: what one unit of the stick's value adds per second now (look per second: rate × ramp).
    pub const STICK_GAIN: u32 = at::STICK_GAIN as u32;
    /// Bytes of a block (TableHead and its row) as this build writes it.
    pub const BYTES: u32 = super::table::bytes(&SPEC, 1);
    /// Bytes of the row as this build writes it.
    pub const ROW: u32 = gripforge_contracts::abi_input_pointer::STRIDE;
}

/// `ActionState` (decision of 20/11, docs/design/etat-des-actions-hors-tick.md), the contract `abi.input.actions`: one
/// row per (action × players + player), the cumulated value of ONE action, written by the host's input port AT EACH
/// EVENT that changes it ([`action_bindings`], `crate::actions::evaluate`), dated on the host's clock (`crate::ring`),
/// read in place by every loop at its own instant. Laid by the kit that owns the inputs, its rows written by the host
/// only. `seq` odd while a row is written (a reader keeps its last value then). Positions from the start of a row.
pub mod action_state {
    pub use gripforge_contracts::abi_input_actions::{at, SPEC};
    /// u32: odd while written.
    pub const SEQ: u32 = at::SEQ as u32;
    /// u32: the changes written since the row was laid.
    pub const CHANGES: u32 = at::CHANGES as u32;
    /// f64: the host's clock at the last change (s).
    pub const T: u32 = at::T as u32;
    /// f64 × 2: the value now (x, y).
    pub const VALUE: u32 = at::VALUE as u32;
    /// f64 × 2: the integral of the value up to `T` (value × seconds).
    pub const INTEGRAL: u32 = at::INTEGRAL as u32;
    /// The head (from the start of the block): actions laid, players per action.
    pub const ACTIONS: u32 = at::head::ACTIONS as u32;
    pub const PLAYERS: u32 = at::head::PLAYERS as u32;
    /// Bytes of a row as this build writes it (a reader reads at the stride of the writer).
    pub const ROW: u32 = gripforge_contracts::abi_input_actions::STRIDE;
    /// Bytes of a block of `rows` rows as this build writes it.
    pub const fn bytes(rows: u32) -> u32 {
        super::table::bytes(&SPEC, rows)
    }
}

/// `ActionBindings`: the bindings the host applies at each event (decision of 20/11), the contract
/// `abi.input.bindings`: a table laid by the kit that owns the inputs (from ITS bindings: contexts, rebinds, players
/// resolved), its place in [`input::Fields::bindings`]. The host decodes it again when `seq` moves (odd while
/// written), then at each record of a device evaluates the actions whose entries name the control (`crate::actions`)
/// and writes their [`action_state`] rows. One row: a control (its key) of a device, what it gives its group, the
/// response, the row it drives. Positions from the start of a row (from the generated contract).
pub mod action_bindings {
    pub use gripforge_contracts::abi_input_bindings::{at, SPEC};
    /// u32 (in the head, from the start of the block): odd while written; moves at each new table.
    pub const SEQ: u32 = at::head::SEQ as u32;
    pub const E_CONTROL: u32 = at::CONTROL as u32;
    pub const E_KIND: u32 = at::KIND as u32;
    pub const E_INDEX: u32 = at::INDEX as u32;
    pub const E_ROLE: u32 = at::ROLE as u32;
    pub const E_FLAGS: u32 = at::FLAGS as u32;
    pub const E_STATE: u32 = at::STATE as u32;
    pub const E_GROUP: u32 = at::GROUP as u32;
    pub const E_DEADZONE: u32 = at::DEADZONE as u32;
    pub const E_SATURATION: u32 = at::SATURATION as u32;
    pub const E_CURVE: u32 = at::CURVE as u32;
    pub const E_GAIN: u32 = at::GAIN as u32;
    pub const E_POST: u32 = at::POST as u32;
    /// Bytes of a row as this build writes it (a reader reads at the stride of the writer).
    pub const ENTRY: u32 = gripforge_contracts::abi_input_bindings::STRIDE;
    /// Any device kind or index.
    pub const ANY: u8 = 0xff;
    /// What a control gives its group.
    pub mod role {
        /// A stick: (x, y), the dead zone on its length (or on each axis: `flag::AXIAL`), the curve.
        pub const VECTOR: u8 = 0;
        /// An analog axis: x, its dead zone and curve.
        pub const AXIS: u8 = 1;
        /// A button read as an axis, as it is (x).
        pub const RAW: u8 = 2;
        /// A key of a composite: +x, −x, +y, −y (the group is 1 long at most).
        pub const POS_X: u8 = 3;
        pub const NEG_X: u8 = 4;
        pub const POS_Y: u8 = 5;
        pub const NEG_Y: u8 = 6;
        /// A modifier of a combination: the group counts only while it is held over the threshold.
        pub const WITH: u8 = 7;
    }
    /// Flags of an entry.
    pub mod flag {
        /// The dead zone on each axis apart (a stick snaps to the axes).
        pub const AXIAL: u8 = 1;
        /// The action is an axis: its sum is clamped to -1..1 (x only) before the action's factor.
        pub const CLAMP_X: u8 = 2;
    }
    /// Bytes of a table of `capacity` entries as this build writes it.
    pub const fn bytes(capacity: u32) -> u32 {
        super::table::bytes(&SPEC, capacity)
    }
}

/// `STATE`: offsets in the buffer.
pub mod state {
    pub const REV: u32 = 0;
    /// Offset of the directory of the mailboxes (`ports::mailbox::Table`, laid by the core in `REGIONS`; 0: none).
    pub const MAILBOXES: u32 = 4;
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
    /// Samples of the ring of raw input samples (crate::ring): the duration it covers (`input.ringSeconds`, 0.5 s
    /// by default) times the highest rate of the pointer (`input.pointerHz`, 1 000 Hz), rounded up.
    pub ring_slots: u32,
}

impl Layout {
    /// The defaults (no game schema given).
    pub fn v1() -> Layout {
        Layout {
            version: String::new(),
            initial: SIZE,
            max: MAX_SIZE,
            buffers: Buffers::V1,
            ring_slots: crate::ring::slots_for(crate::ring::SECONDS, crate::ring::POINTER_HZ),
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
            ring_slots: crate::ring::slots_for(
                schema["input"]["ringSeconds"]
                    .as_f64()
                    .unwrap_or(crate::ring::SECONDS),
                schema["input"]["pointerHz"]
                    .as_f64()
                    .unwrap_or(crate::ring::POINTER_HZ),
            ),
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
        if layout.buffers.input.size < 96 {
            return Err("game schema: the input buffer is too small".into());
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
        // A game's schema moves and grows the buffers; what does not hold is refused.
        let schema = serde_json::json!({ "version": "abc", "input": { "actions": 600 }, "zone": { "initialBytes": 0x50_0000, "maxBytes": 0x100_0000, "buffers": {
            "input": { "offset": 0x100, "bytes": 0x5000 }, "state": { "offset": 0x5100, "bytes": 0x1000 }, "events": { "offset": 0x6100, "bytes": 0x10_0000 },
            "world": { "offset": 0x10_6100, "bytes": 0x20_0000 }, "regions": { "offset": 0x30_6100, "bytes": 0x1f_9f00 } } } });
        let l = Layout::from_schema(&schema).unwrap();
        assert_eq!((l.buffers.world.offset, l.max), (0x10_6100, 0x100_0000));
        let mut z = vec![0u8; l.initial as usize];
        l.write_directory(&mut z, l.initial);
        let b = Buffers::read(|at| {
            u32::from_le_bytes(z[at as usize..at as usize + 4].try_into().unwrap())
        })
        .unwrap();
        assert_eq!(b, l.buffers);
        let mut bad = schema.clone();
        bad["zone"]["buffers"]["input"]["bytes"] = serde_json::json!(16);
        assert!(Layout::from_schema(&bad).is_err());
        bad = schema.clone();
        bad["zone"]["buffers"]["state"]["offset"] = serde_json::json!(0x4000);
        assert!(Layout::from_schema(&bad).is_err());
    }
}
