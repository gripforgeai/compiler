//! The entity rows of the `WORLD` buffer of the zone (docs/kit-modules.md §7.9, docs/game-schema.md `entity`): one
//! row per entity, laid out by the game schema — the fixed base (`core.id@1`, `core.position@1`,
//! `core.rotation@1`), then every column the modules and the game declared. The core keeps the base of each row
//! equal to the entity's `transform`; the other columns hold only what their writers put there.
//!
//! Every integer is little-endian; offsets are from the start of the `WORLD` buffer.
//!
//! ## Header (64 bytes by default: `entity.headerBytes` of the schema)
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | magic `GFW1` |
//! | 4 | u32 | `capacity`: rows of the table (`entity.capacity`) |
//! | 8 | u32 | `rowBytes` |
//! | 12 | u32 | where the rows start (`headerBytes`) |
//! | 16 | u32 | the tag of the world the rows speak of (0: none yet) |
//! | 20 | u32 | `lent`: 1 + the row of the entity the core handed out for any change (`get_mut`): its base may be stale, read that entity by a call |
//! | 24 | u32 | rows in use |
//! | 28 | u32 | rows left by a destroyed entity (`GRAVE`) |
//! | 32 | u32 | `pending`: writes of the physics columns and records of the physics inbox the physics has not taken yet |
//! | 36 | u32 | the physics inbox: its offset **from the start of the `WORLD` buffer** (a block of `REGIONS`, which follows; 0: none) |
//! | 40 | u32 | records the inbox holds at most |
//! | 44 | u32 | records in the inbox now |
//! | 48 | u64 | the epoch of that world: the clock value of its last change (0: not written) |
//! | 56 | u32 | the phase of the step the owner thread is in ([`phase`]: 0 outside the step, then update, physics, late_update, events): what a write of a column is checked against, on any thread |
//! | 60 | u32 | the mirror table: its offset **from the start of the `WORLD` buffer** (a block of `REGIONS`; 0: none) |
//!
//! ## A row
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | `core.id@1`: the number of the id (`e12` → 12); 0 an empty row, `0xffff_ffff` a row left by a destroyed entity |
//! | 4 | u32 | flags: 1 the base is readable (the entity has a transform object), 2 a force waits in `physics.force@1`, 4 the four numbers of the base are plain numbers of the transform, bits 4–7 which of them are floats |
//! | 8 | f64 × 3 | `core.position@1`: x, y, z |
//! | 32 | f64 | `core.rotation@1`: yaw |
//! | 40 … | | the columns of the schema, at their offsets |
//!
//! **Finding a row**: the table is open-addressed by the id's number — start at `number % capacity`, walk forward
//! (wrapping) until the number is found or an empty row ends the search ([`Table::find`]). Ids are `e<number>`,
//! given in sequence: without collisions until the numbers wrap the capacity. An entity whose id is not of that
//! form, or that found no room, has no row: it is read and written by the calls.
//!
//! ## The physics inbox (a block of 32-byte records)
//!
//! `kind u32` (1 impulse, 2 torque impulse, 3 "the force of this row"), `id u32` (the entity's number), `x, y, z
//! f64`: what the kits ask of the physics that is an event and not a state — taken in order, before anything reads
//! the physics again and at the latest by the physics step. Forces are a state: they add up in the column
//! `physics.force@1` ([`Table::add_force`]); the first one written on a row since the physics last took it also
//! leaves a record of kind 3, so the physics finds the rows without a scan ([`Table::push`]).
//!
//! ## The mirror table (a count, then 64-byte records, [`mirror`])
//!
//! A column a module declared as the mirror of **a scalar field of a component** (`of: { component, field }` in
//! the schema: `vehicle.speed@1` is `vehicle.state.speed`) is kept equal to that field by the core at every change
//! of the component; the component stays the truth (saves, the frame the player reads). The table says, for every
//! such column of the game, where it is in a row and which field it mirrors. The block starts with the number of
//! records (`u32`, then 4 bytes left at 0); each record: `offset u32`, `type u8` (`f64`, `bool`,
//! `atom` or `f64x3`), the lengths of the two names (`u8` each), then the component's name (40 bytes) and the
//! field's (16 bytes; empty: the component itself). In a row: an `f64` that is not a number in the component is
//! NaN, a `bool` that is no boolean is 2, an `atom` that is no string is `0xffff_ffff`, an `f64x3` that is not an
//! array of three numbers is three NaN.
//!
//! ## A contact in a result buffer ([`contact`])
//!
//! What the physics answers stays a call, by scalars, its result written in a buffer the caller names: 6 words of
//! 64 bits per contact — `a` and `b` (each an entity name packed in two words: its bytes, at most 16, zero
//! padded), a value (`f64` bits: the impulse of an event, the speed of a pair), flags (1: the contact began).

/// `GFW1`.
pub const MAGIC: u32 = 0x3157_4647;
/// Bytes of the header when no schema says otherwise.
pub const HEADER: u32 = 64;
/// Rows when no schema says otherwise.
pub const CAPACITY: u32 = 1024;

/// The header: offsets in the `WORLD` buffer.
pub mod header {
    pub const MAGIC: u32 = 0;
    pub const CAPACITY: u32 = 4;
    pub const ROW_BYTES: u32 = 8;
    pub const HEADER_BYTES: u32 = 12;
    pub const OWNER: u32 = 16;
    pub const LENT: u32 = 20;
    pub const ROWS: u32 = 24;
    pub const GRAVES: u32 = 28;
    pub const PENDING: u32 = 32;
    pub const INBOX: u32 = 36;
    pub const INBOX_CAPACITY: u32 = 40;
    pub const INBOX_LEN: u32 = 44;
    /// u64: the epoch of the world the rows speak of (the clock value of its last change), written by the core at
    /// every change: a module reads it here instead of asking (`hot_epoch`).
    pub const EPOCH: u32 = 48;
    /// u32: the phase of the step the owner of the rows is in ([`super::phase`]), written by the core at each
    /// change of phase. A column says who writes it at which phase (the game schema, `entity.phases`): a write is
    /// checked against this word where it is made — the owner's thread or another.
    pub const PHASE: u32 = 56;
    /// The mirror table (`mirror`): its offset from the start of the `WORLD` buffer (0: none). The block starts
    /// with its number of records (`mirror::PREFIX` bytes), the records follow.
    pub const MIRRORS: u32 = 60;
}

/// The phases of a step, as the header names them (`header::PHASE`) and as `col_info` masks them (bit `1 << phase`).
pub mod phase {
    /// Outside the step: the setup, timers, hooks of a paused game (`idle` in the schema).
    pub const IDLE: u32 = 0;
    pub const UPDATE: u32 = 1;
    pub const PHYSICS: u32 = 2;
    pub const LATE_UPDATE: u32 = 3;
    pub const EVENTS: u32 = 4;
    /// Their names in the game schema, by number.
    pub const NAMES: [&str; 5] = ["idle", "update", "physics", "late_update", "events"];
    /// Every phase.
    pub const ALL: u8 = 0x1f;
    /// The name of phase `p` (a number the header does not know reads as outside the step).
    pub fn name(p: u32) -> &'static str {
        NAMES.get(p as usize).copied().unwrap_or(NAMES[0])
    }
}

/// A row: offsets of the fixed base, the flags.
pub mod row {
    pub const ID: u32 = 0;
    pub const FLAGS: u32 = 4;
    pub const POSITION: u32 = 8;
    pub const ROTATION: u32 = 32;
    /// Bytes of the base (a row with no other column).
    pub const BASE: u32 = 40;
    /// The base is readable.
    pub const F_POSE: u32 = 1;
    /// A force waits in `physics.force@1`.
    pub const F_FORCE: u32 = 2;
    /// x, y, z, yaw are each a plain number of the transform: a reader can write them again exactly from the base
    /// (`FLOATS`: per field, bit `FLOATS_SHIFT + i`, whether it is a float or an integer).
    pub const F_EXACT: u32 = 4;
    pub const FLOATS_SHIFT: u32 = 4;
    pub const F_FLOATS: u32 = 0xf << FLOATS_SHIFT;
    /// The engine holds the pose (a node of the game the game moves, tag `game.engine.held`): the base is the pose it had
    /// when it was seen; the pose now is read from the engine at the moment it is needed (`game.engine.nodes.pose`, or the
    /// core's `transform`, which does it).
    pub const F_HELD: u32 = 1 << 8;
}

/// The names of the base columns and of the physics column.
pub const COL_ID: &str = "core.id@1";
pub const COL_POSITION: &str = "core.position@1";
pub const COL_ROTATION: &str = "core.rotation@1";
pub const COL_FORCE: &str = "physics.force@1";
/// Declared by the core (u64 each, clock values): the entity's revision, and its last change that is more than new
/// values for x, y, z, yaw. A module that keeps a copy of an entity knows from them, without a call, that its
/// copy is current, or that only the base moved since.
pub const COL_REVISION: &str = "core.revision@1";
pub const COL_SHAPE: &str = "core.shape@1";

/// `core.id@1` of an empty row, and of a row left by a destroyed entity.
pub const EMPTY: u32 = 0;
pub const GRAVE: u32 = u32::MAX;

/// The inbox: bytes of a record, its fields, the kinds.
pub mod inbox {
    pub const RECORD: u32 = 32;
    pub const KIND: u32 = 0;
    pub const ID: u32 = 4;
    pub const X: u32 = 8;
    pub const IMPULSE: u32 = 1;
    pub const TORQUE_IMPULSE: u32 = 2;
    /// The force that waits in `physics.force@1` of the row of that entity.
    pub const FORCE: u32 = 3;
    /// Records when no schema says otherwise.
    pub const CAPACITY: u32 = 256;
}

/// The mirror table: one 64-byte record per column that mirrors a field of a component.
pub mod mirror {
    pub const RECORD: u32 = 64;
    /// Bytes before the first record: the number of records (u32), 4 bytes left at 0.
    pub const PREFIX: u32 = 8;
    pub const OFFSET: u32 = 0;
    pub const TYPE: u32 = 4;
    pub const COMPONENT_LEN: u32 = 5;
    pub const FIELD_LEN: u32 = 6;
    pub const COMPONENT: u32 = 8;
    pub const FIELD: u32 = 48;
    /// Bytes a component name and a field name have at most.
    pub const COMPONENT_MAX: usize = 40;
    pub const FIELD_MAX: usize = 16;
    /// What a row holds for a field that is not of the column's type. A number column holds NaN — `F64_ABSENT`
    /// (the bits of `f64::NAN`) when the component or the field is not there, `F64_OTHER` when the field holds
    /// something else than a number — so a reader that only asks "a number?" tests `is_nan`, and one that asks
    /// "is the field there?" compares the bits.
    pub const F64_ABSENT: u64 = 0x7ff8_0000_0000_0000;
    pub const F64_OTHER: u64 = 0x7ff8_0000_0000_0001;
    pub const NO_FLAG: u8 = 2;
    pub const NO_ATOM: u32 = u32::MAX;
    /// The types a mirror may have (`TYPES`): `bool`, `atom`, `f64`, `f64x3`.
    pub const T_BOOL: u8 = 2;
    pub const T_ATOM: u8 = 7;
    pub const T_F64: u8 = 9;

    /// Writes the record `i` of the table at `table`; false when a name does not fit.
    ///
    /// # Safety
    /// `table` is valid for `(i + 1) * RECORD` bytes.
    pub unsafe fn write(
        table: *mut u8,
        i: u32,
        offset: u32,
        code: u8,
        component: &str,
        field: &str,
    ) -> bool {
        if component.len() > COMPONENT_MAX || field.len() > FIELD_MAX || component.is_empty() {
            return false;
        }
        let rec = table.add((i * RECORD) as usize);
        std::ptr::write_bytes(rec, 0, RECORD as usize);
        super::put(rec, OFFSET, offset);
        super::put(rec, TYPE, code);
        super::put(rec, COMPONENT_LEN, component.len() as u8);
        super::put(rec, FIELD_LEN, field.len() as u8);
        std::ptr::copy_nonoverlapping(
            component.as_ptr(),
            rec.add(COMPONENT as usize),
            component.len(),
        );
        std::ptr::copy_nonoverlapping(field.as_ptr(), rec.add(FIELD as usize), field.len());
        true
    }

    /// Reads the record `i`: `(offset, type, component, field)`; `None`: not a record this contract wrote.
    ///
    /// # Safety
    /// `table` is valid for `(i + 1) * RECORD` bytes, which stay as they are while the names are used.
    pub unsafe fn read<'a>(table: *const u8, i: u32) -> Option<(u32, u8, &'a str, &'a str)> {
        let rec = table.add((i * RECORD) as usize);
        let (cl, fl) = (
            super::get::<u8>(rec, COMPONENT_LEN) as usize,
            super::get::<u8>(rec, FIELD_LEN) as usize,
        );
        if cl == 0 || cl > COMPONENT_MAX || fl > FIELD_MAX {
            return None;
        }
        let component =
            std::str::from_utf8(std::slice::from_raw_parts(rec.add(COMPONENT as usize), cl))
                .ok()?;
        let field =
            std::str::from_utf8(std::slice::from_raw_parts(rec.add(FIELD as usize), fl)).ok()?;
        Some((
            super::get(rec, OFFSET),
            super::get(rec, TYPE),
            component,
            field,
        ))
    }
}

/// A short string packed in two words: its bytes (at most 16, none of them zero), little-endian, zero padded.
pub fn pack16(s: &str) -> Option<(u64, u64)> {
    let b = s.as_bytes();
    if b.len() > 16 || b.contains(&0) {
        return None;
    }
    let mut buf = [0u8; 16];
    buf[..b.len()].copy_from_slice(b);
    Some((
        u64::from_le_bytes(buf[..8].try_into().unwrap()),
        u64::from_le_bytes(buf[8..].try_into().unwrap()),
    ))
}

/// The string of [`pack16`] (`None`: not UTF-8).
pub fn unpack16(lo: u64, hi: u64) -> Option<String> {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&lo.to_le_bytes());
    buf[8..].copy_from_slice(&hi.to_le_bytes());
    let len = buf.iter().position(|b| *b == 0).unwrap_or(16);
    std::str::from_utf8(&buf[..len]).ok().map(str::to_string)
}

/// A contact in a result buffer: `a` (2 words), `b` (2 words), a value, flags.
pub mod contact {
    pub const WORDS: usize = 6;
    pub const F_BEGAN: u64 = 1;

    /// Writes one contact; false when a name does not pack (the caller answers by the call instead).
    pub fn write(out: &mut [u64], a: &str, b: &str, value: f64, began: bool) -> bool {
        let (Some(a), Some(b)) = (super::pack16(a), super::pack16(b)) else {
            return false;
        };
        out[..WORDS].copy_from_slice(&[a.0, a.1, b.0, b.1, value.to_bits(), began as u64]);
        true
    }

    /// Reads one contact: `(a, b, value, began)`.
    pub fn read(w: &[u64]) -> Option<(String, String, f64, bool)> {
        Some((
            super::unpack16(w[0], w[1])?,
            super::unpack16(w[2], w[3])?,
            f64::from_bits(w[4]),
            w[5] & F_BEGAN != 0,
        ))
    }
}

/// The types of a column: the code a view carries, bytes, alignment (docs/game-schema.md).
pub const TYPES: &[(&str, u8, u32, u32)] = &[
    ("u8", 1, 1, 1),
    ("bool", 2, 1, 1),
    ("u16", 3, 2, 2),
    ("u32", 4, 4, 4),
    ("i32", 5, 4, 4),
    ("f32", 6, 4, 4),
    ("atom", 7, 4, 4),
    ("u64", 8, 8, 8),
    ("f64", 9, 8, 8),
    ("f64x3", 10, 24, 8),
    ("entity", 11, 16, 8),
    ("bits128", 12, 16, 8),
];
pub const T_F64X3: u8 = 10;

/// The code of a column type (0: no such type).
pub fn type_code(name: &str) -> u8 {
    TYPES.iter().find(|t| t.0 == name).map_or(0, |t| t.1)
}

/// What a module may do with a column, as the core answers it (`col_info`): `FOUND | PHASED | write phases << 48 |
/// access << 40 | type << 32 | offset in the row`. 0: no such column in this game.
pub const INFO_FOUND: u64 = 1 << 63;
pub const INFO_READ: u64 = 1 << 40;
pub const INFO_WRITE: u64 = 2 << 40;
/// The answer carries the phases at which the module may write the column: bit `INFO_PHASES_SHIFT + phase` each
/// (`phase::*`). Without this flag (a core before the phases) every phase is allowed.
pub const INFO_PHASED: u64 = 1 << 62;
pub const INFO_PHASES_SHIFT: u32 = 48;

/// The number of an entity id of the form `e<decimal>` (no sign, no leading zero, 1 ..= `GRAVE` - 1): its row key.
#[inline]
pub fn id_number(id: &str) -> Option<u32> {
    let b = id.as_bytes();
    if b.len() < 2 || b.len() > 11 || b[0] != b'e' || b[1] == b'0' {
        return None;
    }
    let mut n: u64 = 0;
    for d in &b[1..] {
        if !d.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (d - b'0') as u64;
    }
    (n < GRAVE as u64).then_some(n as u32)
}

/// Writes `e<n>` in `buf`: the id of a row (no allocation).
pub fn id_text(n: u32, buf: &mut [u8; 11]) -> &str {
    let mut digits = [0u8; 10];
    let (mut i, mut n) = (0, n);
    loop {
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
        if n == 0 {
            break;
        }
    }
    buf[0] = b'e';
    for k in 0..i {
        buf[1 + k] = digits[i - 1 - k];
    }
    // SAFETY: ASCII only.
    unsafe { std::str::from_utf8_unchecked(&buf[..1 + i]) }
}

/// The rows as they are in memory now: the address of the `WORLD` buffer and what its header says.
///
/// Made for one access and dropped (the zone may move when it grows): never kept across a call.
#[derive(Clone, Copy)]
pub struct Table {
    /// The `WORLD` buffer.
    world: *mut u8,
    /// The first row.
    rows: *mut u8,
    pub capacity: u32,
    pub row_bytes: u32,
}

impl Table {
    /// The table of the `WORLD` buffer at `world` (`bytes` long): `None` when no header was written there or it
    /// does not fit the buffer.
    ///
    /// # Safety
    /// `world .. world + bytes` is readable and writable for as long as the table is used.
    #[inline]
    pub unsafe fn at(world: *mut u8, bytes: u32) -> Option<Table> {
        if bytes < HEADER || (world as *const u32).read_unaligned() != MAGIC {
            return None;
        }
        let word = |at: u32| (world.add(at as usize) as *const u32).read_unaligned();
        let (capacity, row_bytes, head) = (
            word(header::CAPACITY),
            word(header::ROW_BYTES),
            word(header::HEADER_BYTES),
        );
        if capacity == 0
            || row_bytes < row::BASE
            || (head as u64) + capacity as u64 * row_bytes as u64 > bytes as u64
        {
            return None;
        }
        Some(Table {
            world,
            rows: world.add(head as usize),
            capacity,
            row_bytes,
        })
    }

    /// A word of the header.
    #[inline]
    pub fn head(&self, at: u32) -> u32 {
        // SAFETY: in the header (`at` is one of `header::*`).
        unsafe { (self.world.add(at as usize) as *const u32).read_unaligned() }
    }
    #[inline]
    pub fn set_head(&self, at: u32, v: u32) {
        // SAFETY: as `head`.
        unsafe { (self.world.add(at as usize) as *mut u32).write_unaligned(v) }
    }

    /// The epoch of the world the rows speak of (0: not written).
    #[inline]
    pub fn epoch(&self) -> u64 {
        // SAFETY: in the header.
        unsafe { (self.world.add(header::EPOCH as usize) as *const u64).read_unaligned() }
    }
    #[inline]
    pub fn set_epoch(&self, e: u64) {
        // SAFETY: in the header.
        unsafe { (self.world.add(header::EPOCH as usize) as *mut u64).write_unaligned(e) }
    }

    /// The mirror table of the header: its address and its records (`None`: none).
    #[inline]
    pub fn mirrors(&self) -> Option<(*mut u8, u32)> {
        let at = self.head(header::MIRRORS);
        if at == 0 {
            return None;
        }
        // SAFETY: a block of the same memory, after the `WORLD` buffer (the core placed it): its count, its records.
        let (len, records) = unsafe {
            let block = self.world.add(at as usize);
            (
                (block as *const u32).read_unaligned(),
                block.add(mirror::PREFIX as usize),
            )
        };
        (len != 0).then_some((records, len))
    }

    /// The row of the entity of number `n` when it may be read: it has one and the core did not hand that entity
    /// out for any change.
    #[inline]
    pub fn readable_n(&self, n: u32) -> Option<*mut u8> {
        let i = self.find(n)?;
        (self.head(header::LENT) != i + 1).then(|| self.row(i))
    }

    /// The address of row `i` (`i < capacity`).
    #[inline]
    pub fn row(&self, i: u32) -> *mut u8 {
        debug_assert!(i < self.capacity);
        // SAFETY: in the table.
        unsafe { self.rows.add(i as usize * self.row_bytes as usize) }
    }

    #[inline]
    fn id_at(&self, i: u32) -> u32 {
        // SAFETY: in the table.
        unsafe { (self.row(i) as *const u32).read_unaligned() }
    }

    /// The row of the entity of number `n`.
    #[inline]
    pub fn find(&self, n: u32) -> Option<u32> {
        let mut i = n % self.capacity;
        for _ in 0..self.capacity {
            let id = self.id_at(i);
            if id == n {
                return Some(i);
            }
            if id == EMPTY {
                return None;
            }
            i += 1;
            if i == self.capacity {
                i = 0;
            }
        }
        None
    }

    /// The row of the entity of that id when its base may be read: it has one, and the core did not hand that
    /// entity out for any change.
    #[inline]
    pub fn readable(&self, id: &str) -> Option<*mut u8> {
        let i = self.find(id_number(id)?)?;
        (self.head(header::LENT) != i + 1).then(|| self.row(i))
    }

    /// Takes a row for the entity of number `n` (zeroed, its id set): `None` when the table is full.
    pub fn take(&self, n: u32) -> Option<u32> {
        let mut i = n % self.capacity;
        for _ in 0..self.capacity {
            let id = self.id_at(i);
            if id == EMPTY || id == GRAVE {
                if id == GRAVE {
                    self.set_head(header::GRAVES, self.head(header::GRAVES).saturating_sub(1));
                }
                // SAFETY: row `i` of the table.
                unsafe {
                    std::ptr::write_bytes(self.row(i), 0, self.row_bytes as usize);
                    (self.row(i) as *mut u32).write_unaligned(n);
                }
                self.set_head(header::ROWS, self.head(header::ROWS) + 1);
                return Some(i);
            }
            i += 1;
            if i == self.capacity {
                i = 0;
            }
        }
        None
    }

    /// Gives the row `i` back: empty when the next one is (no search goes through it), else a grave.
    pub fn leave(&self, i: u32) {
        let next = if i + 1 == self.capacity { 0 } else { i + 1 };
        let id = if self.id_at(next) == EMPTY {
            EMPTY
        } else {
            GRAVE
        };
        // SAFETY: row `i` of the table.
        unsafe { (self.row(i) as *mut u32).write_unaligned(id) };
        self.set_head(header::ROWS, self.head(header::ROWS).saturating_sub(1));
        if id == GRAVE {
            self.set_head(header::GRAVES, self.head(header::GRAVES) + 1);
        }
    }

    /// Adds `f` to the force column (at `force_at` in a row) of the entity of number `n`, in place. `single`: the
    /// port that will take it adds in single precision (Rapier), so the sum is made in single precision, as its
    /// own would be. The first force on a row since the physics last took it leaves a record in the inbox. False
    /// (nothing written): no row, or no room for that record — the caller makes the call.
    ///
    /// # Safety
    /// `force_at .. force_at + 24` is within a row of this table.
    pub unsafe fn add_force(&self, n: u32, force_at: u32, f: [f64; 3], single: bool) -> bool {
        let Some(i) = self.find(n) else { return false };
        let row = self.row(i);
        let flags = get::<u32>(row, row::FLAGS);
        if flags & row::F_FORCE == 0 {
            if !self.push(inbox::FORCE, n, [0.0; 3]) {
                return false;
            }
            put(row, row::FLAGS, flags | row::F_FORCE);
        }
        for (k, x) in f.iter().enumerate() {
            let at = force_at + 8 * k as u32;
            let had = get::<f64>(row, at);
            put(
                row,
                at,
                if single {
                    (had as f32 + *x as f32) as f64
                } else {
                    had + x
                },
            );
        }
        true
    }

    /// Appends a record to the physics inbox; false when it is full or there is none.
    ///
    /// # Safety
    /// The inbox the header names is `INBOX_CAPACITY` records of the same memory, after the `WORLD` buffer.
    pub unsafe fn push(&self, kind: u32, n: u32, v: [f64; 3]) -> bool {
        let (at, len) = (self.head(header::INBOX), self.head(header::INBOX_LEN));
        if at == 0 || len >= self.head(header::INBOX_CAPACITY) {
            return false;
        }
        let rec = self.world.add(at as usize + (len * inbox::RECORD) as usize);
        put(rec, inbox::KIND, kind);
        put(rec, inbox::ID, n);
        for (k, x) in v.iter().enumerate() {
            put(rec, inbox::X + 8 * k as u32, *x);
        }
        self.set_head(header::INBOX_LEN, len + 1);
        self.set_head(header::PENDING, self.head(header::PENDING) + 1);
        true
    }

    /// Empties every row.
    pub fn clear(&self) {
        // SAFETY: the whole table.
        unsafe {
            std::ptr::write_bytes(
                self.rows,
                0,
                self.capacity as usize * self.row_bytes as usize,
            )
        };
        self.set_head(header::ROWS, 0);
        self.set_head(header::GRAVES, 0);
        self.set_head(header::LENT, 0);
    }
}

/// Reads and writes in a row (`at`: an offset of the row, in its bounds; any alignment).
///
/// # Safety
/// `row` is a row of a live [`Table`] and `at` + the size of the type is within `row_bytes`.
#[inline]
pub unsafe fn get<T: Copy>(row: *const u8, at: u32) -> T {
    (row.add(at as usize) as *const T).read_unaligned()
}
/// # Safety
/// As [`get`].
#[inline]
pub unsafe fn put<T: Copy>(row: *mut u8, at: u32, v: T) {
    (row.add(at as usize) as *mut T).write_unaligned(v)
}

/// The value of a scalar column as an `f64` (`code`: its type; 0.0 for a type that is not a number).
///
/// # Safety
/// As [`get`].
pub unsafe fn get_number(row: *const u8, at: u32, code: u8) -> f64 {
    match code {
        1 | 2 => get::<u8>(row, at) as f64,
        3 => get::<u16>(row, at) as f64,
        4 | 7 => get::<u32>(row, at) as f64,
        5 => get::<i32>(row, at) as f64,
        6 => get::<f32>(row, at) as f64,
        8 => get::<u64>(row, at) as f64,
        9 => get::<f64>(row, at),
        _ => 0.0,
    }
}

/// Writes a scalar column from an `f64` (converted as `as` does); false for a type that is not a number.
///
/// # Safety
/// As [`get`].
pub unsafe fn put_number(row: *mut u8, at: u32, code: u8, v: f64) -> bool {
    match code {
        1 => put(row, at, v as u8),
        2 => put(row, at, (v != 0.0) as u8),
        3 => put(row, at, v as u16),
        4 | 7 => put(row, at, v as u32),
        5 => put(row, at, v as i32),
        6 => put(row, at, v as f32),
        8 => put(row, at, v as u64),
        9 => put(row, at, v),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(buf: &mut Vec<u64>, capacity: u32, row_bytes: u32) -> Table {
        let world = buf.as_mut_ptr() as *mut u8;
        unsafe {
            put(world, header::MAGIC, MAGIC);
            put(world, header::CAPACITY, capacity);
            put(world, header::ROW_BYTES, row_bytes);
            put(world, header::HEADER_BYTES, HEADER);
            Table::at(world, (buf.len() * 8) as u32).unwrap()
        }
    }

    #[test]
    fn ids_are_numbers_only_in_their_canonical_form() {
        assert_eq!(id_number("e1"), Some(1));
        assert_eq!(id_number("e4294967294"), Some(u32::MAX - 1));
        for bad in [
            "",
            "e",
            "e0",
            "e01",
            "e+1",
            "e-1",
            "x1",
            "e1a",
            "e4294967295",
            "e99999999999",
        ] {
            assert_eq!(id_number(bad), None, "{bad}");
        }
        let mut buf = [0u8; 11];
        for n in [1u32, 9, 10, 160, 4294967294] {
            assert_eq!(id_number(id_text(n, &mut buf)), Some(n));
        }
    }

    #[test]
    fn rows_are_found_by_number_through_collisions_and_graves() {
        let mut buf = vec![0u64; (HEADER as usize + 8 * 48) / 8];
        let t = table(&mut buf, 8, 48);
        // 3, 11 and 19 start at the same row.
        let (a, b, c) = (t.take(3).unwrap(), t.take(11).unwrap(), t.take(19).unwrap());
        assert_eq!((a, b, c), (3, 4, 5));
        assert_eq!(
            (t.find(3), t.find(11), t.find(19), t.find(27)),
            (Some(3), Some(4), Some(5), None)
        );
        // The middle one leaves a grave: the search goes through it; the last one leaves an empty row.
        t.leave(b);
        assert_eq!(
            (t.find(11), t.find(19), t.head(header::GRAVES)),
            (None, Some(5), 1)
        );
        t.leave(c);
        assert_eq!(
            (t.find(19), t.head(header::GRAVES), t.head(header::ROWS)),
            (None, 1, 1)
        );
        // A grave is taken again, zeroed.
        unsafe { put(t.row(4), row::POSITION, 7.5f64) };
        assert_eq!(t.take(27), Some(4));
        assert_eq!(unsafe { get::<f64>(t.row(4), row::POSITION) }, 0.0);
        assert_eq!(t.head(header::GRAVES), 0);
        // Full: no row.
        for n in [100, 101, 102, 103, 104, 105] {
            assert!(t.take(n).is_some());
        }
        assert_eq!(t.take(200), None);
        assert_eq!(t.find(200), None);
        // The entity handed out is not readable by its row.
        t.set_head(header::LENT, 3 + 1);
        assert!(
            t.readable("e3").is_none()
                && t.readable("e27").is_some()
                && t.readable("boss").is_none()
        );
        t.clear();
        assert_eq!(
            (t.find(3), t.head(header::ROWS), t.head(header::LENT)),
            (None, 0, 0)
        );
    }

    #[test]
    fn a_mirror_record_reads_back() {
        let mut buf = vec![0u64; 16];
        let t = buf.as_mut_ptr() as *mut u8;
        unsafe {
            assert!(mirror::write(
                t,
                0,
                72,
                mirror::T_F64,
                "vehicle.state",
                "speed"
            ));
            assert!(mirror::write(t, 1, 80, mirror::T_ATOM, "team", ""));
            assert!(!mirror::write(
                t,
                1,
                80,
                mirror::T_F64,
                "a",
                "a field name that is too long"
            ));
            assert_eq!(
                mirror::read(t, 0),
                Some((72, mirror::T_F64, "vehicle.state", "speed"))
            );
            assert_eq!(mirror::read(t, 1), Some((80, mirror::T_ATOM, "team", "")));
        }
        assert_eq!(
            (type_code("f64"), type_code("bool"), type_code("atom")),
            (mirror::T_F64, mirror::T_BOOL, mirror::T_ATOM)
        );
    }

    #[test]
    fn numbers_keep_their_type() {
        let mut row = [0u64; 6];
        let p = row.as_mut_ptr() as *mut u8;
        unsafe {
            assert!(
                put_number(p, 0, type_code("f32"), 1.5)
                    && get_number(p, 0, type_code("f32")) == 1.5
            );
            assert!(
                put_number(p, 8, type_code("u8"), 300.0)
                    && get_number(p, 8, type_code("u8")) == 255.0
            );
            assert!(
                put_number(p, 9, type_code("bool"), 3.0)
                    && get_number(p, 9, type_code("bool")) == 1.0
            );
            assert!(
                put_number(p, 12, type_code("i32"), -4.0)
                    && get_number(p, 12, type_code("i32")) == -4.0
            );
            assert!(!put_number(p, 16, T_F64X3, 1.0));
        }
    }
}
