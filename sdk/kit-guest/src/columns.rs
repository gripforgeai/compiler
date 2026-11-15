//! The entity rows of the zone as a kit module sees them (layout: gripforge-kitcall src/rows.rs; the core side:
//! gripforge-kithost src/kitcall/columns.rs; docs/kit-modules.md §7.9): one row per entity in the `WORLD` buffer,
//! laid out by the game schema — the fixed base (id, position, rotation), then the columns the modules and the game
//! declared.
//!
//! A kit module of the **common memory** loads and stores in the rows where they are: reading a position is a few
//! loads, no call and nothing copied ([`pose`]); a column is read and written by its name ([`read`], [`write`]),
//! at the place and with the access the core told this module once ([`col`]: its view of the schema). A write the
//! view does not allow is an error that names the module and the column, never a silent write.
//!
//! Elsewhere (a kit in its own memory, on another thread, a native build) there is no table here: the transform
//! reads take the direct links and the calls, as before.

use crate::direct::FastMap;
use gripforge_abi::rows::{self, row, Table};
use std::cell::RefCell;

/// The table as it is now (never kept: the zone may move when it grows).
#[cfg(feature = "common-memory")]
#[inline]
pub(crate) fn table() -> Option<Table> {
    let w = crate::zone::world();
    if w.size == 0 {
        return None;
    }
    // SAFETY: the `WORLD` buffer of the zone, which lives in the memory this module shares with the core.
    unsafe {
        Table::at(
            (crate::mem::zone_base() + w.offset as usize) as *mut u8,
            w.size,
        )
    }
}

#[cfg(not(feature = "common-memory"))]
#[inline]
pub(crate) fn table() -> Option<Table> {
    None
}

/// Whether the entity `id` has a row in the table: then it is an entity of this world. False says nothing (an id
/// that is no `e<number>`, a table that was full, no table here): the caller asks the core.
#[inline]
pub(crate) fn has_row(id: &str) -> bool {
    match (table(), rows::id_number(id)) {
        (Some(t), Some(n)) => t.find(n).is_some(),
        _ => false,
    }
}

/// x, y, z, yaw of the entity `id` from its row; `None` when the row cannot say (no table here, no row, no
/// transform object, or the core handed the entity out for any change): the caller asks the core.
#[inline]
pub(crate) fn pose(id: &str) -> Option<[f64; 4]> {
    let row = table()?.readable(id)?;
    // SAFETY: a row of the table; the base is within every row.
    unsafe {
        if rows::get::<u32>(row, row::FLAGS) & row::F_POSE == 0 {
            return None;
        }
        Some([
            rows::get(row, row::POSITION),
            rows::get(row, row::POSITION + 8),
            rows::get(row, row::POSITION + 16),
            rows::get(row, row::ROTATION),
        ])
    }
}

/// The revision of the entity `id` and its last change that is more than new x, y, z, yaw (the core's columns
/// `core.revision@1`, `core.shape@1`), then its base when a copy of the transform can be written again exactly
/// from it (the four numbers, which of them are floats). `None`: the row cannot say (as [`pose`], or a game whose
/// schema has no such columns): the caller asks the core.
#[inline]
#[allow(
    clippy::type_complexity,
    reason = "lint of a newer clippy than the source; copied as is (docs/sources.md)"
)]
pub(crate) fn revision(id: &str) -> Option<(u64, u64, Option<([f64; 4], u32)>)> {
    thread_local! {
        static AT: std::cell::Cell<(u32, Option<(u32, u32)>)> = const { std::cell::Cell::new((0, None)) };
    }
    let t = table()?;
    let generation = crate::zone::generation();
    let (known, mut at) = AT.with(std::cell::Cell::get);
    if known != generation {
        let u64_at = |name: &str| {
            col(name)
                .filter(|c| c.read && c.code == 8)
                .map(|c| c.offset)
        };
        at = u64_at(rows::COL_REVISION).zip(u64_at(rows::COL_SHAPE));
        AT.with(|c| c.set((generation, at)));
    }
    let (rev_at, shape_at) = at?;
    let row = t.readable(id)?;
    // SAFETY: a row of the table; the base and the columns of the schema are within it.
    unsafe {
        let flags = rows::get::<u32>(row, row::FLAGS);
        let exact =
            (flags & (row::F_POSE | row::F_EXACT) == row::F_POSE | row::F_EXACT).then(|| {
                (
                    [
                        rows::get(row, row::POSITION),
                        rows::get(row, row::POSITION + 8),
                        rows::get(row, row::POSITION + 16),
                        rows::get(row, row::ROTATION),
                    ],
                    (flags & row::F_FLOATS) >> row::FLOATS_SHIFT,
                )
            });
        Some((rows::get(row, rev_at), rows::get(row, shape_at), exact))
    }
}

/// A column of the rows as this module may use it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Col {
    /// Its offset in a row, the code of its type (`rows::TYPES`).
    pub offset: u32,
    pub code: u8,
    pub read: bool,
    pub write: bool,
    /// The phases of the step at which this module writes it (bit `1 << phase`, `rows::phase`): the game schema's
    /// `entity.phases`, as the core answered them. Every phase when the core does not say.
    pub phases: u8,
}

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "gf:kithost.world@^1.2")]
extern "C" {
    fn col_info(name: u32) -> u64;
}
#[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
use crate::native::link::col_info;

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
fn ask(name: &str) -> u64 {
    // SAFETY: an import wired by the calls host to the export `gfx_col_info` of the core.
    unsafe { col_info(crate::direct::atom(name)) }
}

#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
fn ask(_: &str) -> u64 {
    0
}

/// The column `name` (`combat.health@1`) in this game: where it is and what this module may do with it — asked of
/// the core once per name and mount. `None`: the game has no such column.
pub fn col(name: &str) -> Option<Col> {
    thread_local! {
        static COLS: RefCell<(u32, FastMap<String, Option<Col>>)> = RefCell::new((0, FastMap::default()));
    }
    let generation = crate::zone::generation();
    if let Some(known) = COLS.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != generation {
            c.0 = generation;
            c.1.clear();
        }
        c.1.get(name).copied()
    }) {
        return known;
    }
    let info = ask(name);
    let col = (info & rows::INFO_FOUND != 0).then_some(Col {
        offset: info as u32,
        code: (info >> 32) as u8,
        read: info & rows::INFO_READ != 0,
        write: info & rows::INFO_WRITE != 0,
        phases: if info & rows::INFO_PHASED != 0 {
            (info >> rows::INFO_PHASES_SHIFT) as u8 & rows::phase::ALL
        } else {
            rows::phase::ALL
        },
    });
    COLS.with(|c| c.borrow_mut().1.insert(name.to_string(), col));
    col
}

/// Adds a force to the column `physics.force@1` of the entity (what several kits do to one body in a tick, each
/// in its turn; the physics takes the sum at its step). `single`: the port adds in single precision. `Ok(false)`:
/// not possible here (no table, no such column in this game, no row, no room in the inbox): the caller makes the
/// call. An error: this game does not let this kit write the column.
pub(crate) fn add_force(kit: &str, id: &str, f: [f64; 3], single: bool) -> Result<bool, String> {
    let Some(t) = table() else { return Ok(false) };
    let Some(c) = col(rows::COL_FORCE).filter(|c| c.code == rows::T_F64X3 && c.offset >= row::BASE)
    else {
        return Ok(false);
    };
    if !c.write {
        return Err(format!(
            "{kit} writes column {}: not allowed (not in its view of the game schema)",
            rows::COL_FORCE
        ));
    }
    // Its phase: a force is taken by the physics at its own phase; a kit writes it at the phases the schema gives it
    // (docs/game-schema.md `entity.phases`) — never while the physics takes it.
    let now = t.head(rows::header::PHASE);
    if c.phases & (1 << now.min(7)) == 0 {
        let mine: Vec<&str> = rows::phase::NAMES
            .iter()
            .enumerate()
            .filter(|(p, _)| c.phases & (1 << p) != 0)
            .map(|(_, n)| *n)
            .collect();
        return Err(format!(
            "{kit} writes column {} during {}: not its phase (it writes during: {})",
            rows::COL_FORCE,
            rows::phase::name(now),
            if mine.is_empty() {
                "none".into()
            } else {
                mine.join(", ")
            }
        ));
    }
    let Some(n) = rows::id_number(id) else {
        return Ok(false);
    };
    // SAFETY: the column is within a row (the core checked the schema); the inbox is the header's.
    Ok(unsafe { t.add_force(n, c.offset, f, single) })
}

/// Leaves an impulse (`rows::inbox::IMPULSE`) or a torque impulse for the physics in its inbox; false: not
/// possible here (no table, an id that is no number, no room): the caller makes the call.
pub(crate) fn push_impulse(kind: u32, id: &str, v: [f64; 3]) -> bool {
    let (Some(t), Some(n)) = (table(), rows::id_number(id)) else {
        return false;
    };
    // An entity without a row may still have a body: only the inbox is needed.
    // SAFETY: the inbox is the header's.
    unsafe { t.push(kind, n, v) }
}

fn row_of(id: &str) -> Option<*mut u8> {
    let t = table()?;
    Some(t.row(t.find(rows::id_number(id)?)?))
}

/// The column `name` if this game has it and lets `kit` read it; else an error that names both.
fn readable(kit: &str, name: &str) -> Result<Col, String> {
    let c = col(name)
        .ok_or_else(|| format!("{kit} reads column {name}, which this game does not have"))?;
    if !c.read {
        return Err(format!(
            "{kit} reads column {name}: not allowed (not in its view of the game schema)"
        ));
    }
    Ok(c)
}

/// The column `name` if this game lets `kit` write it now (its access, not the base, its phase); else an error that
/// names both.
fn writable(kit: &str, name: &str) -> Result<Col, String> {
    let c = col(name)
        .ok_or_else(|| format!("{kit} writes column {name}, which this game does not have"))?;
    if !c.write {
        return Err(format!(
            "{kit} writes column {name}: not allowed (not in its view of the game schema)"
        ));
    }
    if c.offset < row::BASE {
        return Err(format!("{kit} writes column {name}: the base follows the entity's transform (write it with set_pose)"));
    }
    // Its phase: the header of the rows says which phase of the step the owner thread is in; the schema says at
    // which phases this module writes this column. Checked here, where the write is made — on any thread.
    let now = table().map_or(rows::phase::IDLE, |t| t.head(rows::header::PHASE));
    if c.phases & (1 << now.min(7)) == 0 {
        let mine: Vec<&str> = rows::phase::NAMES
            .iter()
            .enumerate()
            .filter(|(p, _)| c.phases & (1 << p) != 0)
            .map(|(_, n)| *n)
            .collect();
        return Err(format!(
            "{kit} writes column {name} during {}: not its phase (it writes during: {})",
            rows::phase::name(now),
            if mine.is_empty() {
                "none".into()
            } else {
                mine.join(", ")
            }
        ));
    }
    Ok(c)
}

/// Reads the `u64` column `name` of the entity `id` for the kit `kit`, whole (a bit mask: no rounding through an
/// `f64`). `Ok(None)`: the entity has no row here. An error when the column is not a `u64`.
pub fn read_bits(kit: &str, id: &str, name: &str) -> Result<Option<u64>, String> {
    let c = readable(kit, name)?;
    if c.code != rows::type_code("u64") {
        return Err(format!(
            "{kit} reads column {name} as bits: it is not a u64"
        ));
    }
    let Some(row) = row_of(id) else {
        return Ok(None);
    };
    // SAFETY: a column of the schema is within the row.
    Ok(Some(unsafe { rows::get::<u64>(row, c.offset) }))
}

/// Writes the `u64` column `name` of the entity `id` for the kit `kit`, whole. `Ok(false)`: the entity has no row here.
pub fn write_bits(kit: &str, id: &str, name: &str, v: u64) -> Result<bool, String> {
    let c = writable(kit, name)?;
    if c.code != rows::type_code("u64") {
        return Err(format!(
            "{kit} writes column {name} as bits: it is not a u64"
        ));
    }
    let Some(row) = row_of(id) else {
        return Ok(false);
    };
    // SAFETY: a column of the schema is within the row.
    unsafe { rows::put(row, c.offset, v) };
    Ok(true)
}

/// Reads the column `name` of the entity `id` for the kit `kit`: a scalar as one number, an `f64x3` as three.
/// `Ok(None)`: the entity has no row here (the caller takes its other way). An error names the kit and the column.
pub fn read(kit: &str, id: &str, name: &str) -> Result<Option<[f64; 3]>, String> {
    let c = readable(kit, name)?;
    let Some(row) = row_of(id) else {
        return Ok(None);
    };
    // SAFETY: a column of the schema is within the row.
    Ok(Some(unsafe {
        if c.code == rows::T_F64X3 {
            [
                rows::get(row, c.offset),
                rows::get(row, c.offset + 8),
                rows::get(row, c.offset + 16),
            ]
        } else {
            [rows::get_number(row, c.offset, c.code), 0.0, 0.0]
        }
    }))
}

/// Writes the column `name` of the entity `id` for the kit `kit`, in place (`add`: adds to what is there — what
/// several kits do to one column in a tick, each in its turn). `Ok(false)`: the entity has no row here. The base
/// (position, rotation) is not written this way: it follows the transform (`entities.set_pose`).
pub fn write(kit: &str, id: &str, name: &str, v: [f64; 3], add: bool) -> Result<bool, String> {
    let c = writable(kit, name)?;
    let Some(row) = row_of(id) else {
        return Ok(false);
    };
    // SAFETY: as `read`.
    unsafe {
        if c.code == rows::T_F64X3 {
            for (k, x) in v.iter().enumerate() {
                let at = c.offset + 8 * k as u32;
                rows::put(
                    row,
                    at,
                    if add {
                        rows::get::<f64>(row, at) + x
                    } else {
                        *x
                    },
                );
            }
        } else {
            let x = if add {
                rows::get_number(row, c.offset, c.code) + v[0]
            } else {
                v[0]
            };
            if !rows::put_number(row, c.offset, c.code, x) {
                return Err(format!("{kit} writes column {name}: not a number"));
            }
        }
    }
    Ok(true)
}

// ---- the columns that mirror a field of a component (docs/kit-modules.md §7.12) ------------------------------------

/// A record of the mirror table of the zone, kept for one generation of the zone.
pub(crate) struct Mirror {
    pub offset: u32,
    pub code: u8,
    pub component: String,
    pub field: String,
}

thread_local! {
    /// The mirror table as it was read at this generation of the zone (leaked: one small table per mount of a game,
    /// read at every typed access without a count touched).
    static MIRRORS: std::cell::Cell<(u32, &'static [Mirror])> = const { std::cell::Cell::new((u32::MAX, &[])) };
}

/// The mirror table of this game (empty: none, or no rows here).
#[inline]
pub(crate) fn mirrors() -> &'static [Mirror] {
    let generation = crate::zone::generation();
    let (known, list) = MIRRORS.with(std::cell::Cell::get);
    // (An empty table is read again: the core writes it when the first kit module runs.)
    if known == generation && !list.is_empty() {
        return list;
    }
    read_mirrors(generation)
}

#[cold]
fn read_mirrors(generation: u32) -> &'static [Mirror] {
    let mut list = Vec::new();
    if let Some((at, len)) = table().and_then(|t| t.mirrors()) {
        for i in 0..len {
            // SAFETY: the mirror table of the zone, `len` records the core wrote.
            match unsafe { rows::mirror::read(at, i) } {
                Some((offset, code, component, field)) => list.push(Mirror {
                    offset,
                    code,
                    component: component.to_string(),
                    field: field.to_string(),
                }),
                None => {
                    list.clear();
                    break;
                }
            }
        }
    }
    // (Nothing is allocated while the game has no such column.)
    let list: &'static [Mirror] = if list.is_empty() {
        &[]
    } else {
        Vec::leak(list)
    };
    MIRRORS.with(|m| m.set((generation, list)));
    list
}

/// Whether this game has a column that mirrors a field (two loads once the table was read at this generation).
#[inline]
pub(crate) fn any_mirrors() -> bool {
    thread_local! {
        static ANY: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((u32::MAX, false)) };
    }
    let generation = crate::zone::generation();
    let (known, any) = ANY.with(std::cell::Cell::get);
    if known == generation {
        return any;
    }
    // Once the rows exist (the core writes the table with them, before a kit module runs) the answer holds for
    // this generation of the zone; until then it is asked again.
    let any = !mirrors().is_empty();
    if any || table().is_some() {
        ANY.with(|c| c.set((generation, any)));
    }
    any
}

/// Where the column that mirrors `f` is in the mirror table of this game (`None`: no column mirrors it). Found
/// once per field and per generation of the zone, then kept in the field itself: no name is compared after that.
#[inline]
pub(crate) fn mirror_of(f: &crate::entities::Field) -> Option<u32> {
    use std::sync::atomic::Ordering::Relaxed;
    let generation = crate::zone::generation() as u64;
    let memo = f.memo.load(Relaxed);
    if memo >> 32 == generation && memo as u32 != 0 {
        return (memo as u32).checked_sub(2);
    }
    let list = mirrors();
    let found = list
        .iter()
        .position(|m| m.component == f.component && m.field == f.name);
    if !list.is_empty() {
        f.memo.store(
            generation << 32 | found.map_or(1, |i| i as u64 + 2),
            Relaxed,
        );
    }
    found.map(|i| i as u32)
}

/// A copy of an entity whose row says only numbers the rows carry changed since it was made: the fields the
/// columns mirror, written in the copy from the row — a number or a flag that differs becomes the row's (stored
/// as the core stored it: `js::num`). False: the copy cannot take it (it is then read again whole).
///
/// # Safety
/// `row` is a row of the live table the mirrors were read from.
pub(crate) unsafe fn mirrors_into(e: &mut crate::entities::Entity, row: *const u8) -> bool {
    let list = mirrors();
    let mut at = 0;
    while at < list.len() {
        let name = &list[at].component;
        let end = at
            + list[at..]
                .iter()
                .take_while(|m| &m.component == name)
                .count();
        // The component is looked up only for a field the row may write: a text (an atom) that changed is more
        // than a number (the core says so: `core.shape@1`), and a number or a flag the row says is not there
        // leaves the copy as it is.
        let mut found: Option<Option<usize>> = None;
        for m in &list[at..end] {
            let writes = match m.code {
                rows::mirror::T_F64 => !rows::get::<f64>(row, m.offset).is_nan(),
                rows::mirror::T_BOOL => rows::get::<u8>(row, m.offset) < rows::mirror::NO_FLAG,
                rows::T_F64X3 => true,
                _ => false,
            };
            if !writes {
                continue;
            }
            let Some(i) =
                *found.get_or_insert_with(|| e.components.iter().position(|(k, _)| k == name))
            else {
                break;
            };
            let c = &mut e.components[i].1;
            let slot = if m.field.is_empty() {
                Some(&mut *c)
            } else {
                c.get_mut(&m.field)
            };
            let Some(slot) = slot else { continue };
            match m.code {
                rows::mirror::T_F64 => num_from(slot, rows::get(row, m.offset)),
                rows::mirror::T_BOOL => {
                    let b = rows::get::<u8>(row, m.offset);
                    if *slot != serde_json::Value::Bool(b != 0) {
                        *slot = serde_json::Value::Bool(b != 0);
                    }
                }
                _ => {
                    let serde_json::Value::Array(a) = slot else {
                        continue;
                    };
                    if a.len() != 3 {
                        return false;
                    }
                    for (k, slot) in a.iter_mut().enumerate() {
                        num_from(slot, rows::get(row, m.offset + 8 * k as u32));
                    }
                }
            }
        }
        at = end;
    }
    true
}

/// `slot` becomes the number `v` of a row unless it is that number already (NaN: the field is no number there).
#[inline]
fn num_from(slot: &mut serde_json::Value, v: f64) {
    if v.is_nan()
        || matches!(&*slot, serde_json::Value::Number(n) if n.as_f64().map(f64::to_bits) == Some(v.to_bits()))
    {
        return;
    }
    *slot = crate::js::num(v);
}

/// The row of the entity `id` when it may be read now (it has one, the core did not hand the entity out).
#[inline]
pub(crate) fn readable_row(id: &str) -> Option<*mut u8> {
    table()?.readable(id)
}

/// The field `f` of the entity `id` read in its row: `None` when no column mirrors it or the row cannot say (the
/// caller reads the component); `Some((offset, type, row))` to load from.
#[inline]
pub(crate) fn mirror_row(id: &str, f: &crate::entities::Field) -> Option<(u32, u8, *const u8)> {
    let i = mirror_of(f)? as usize;
    let row = readable_row(id)?;
    let m = mirrors().get(i)?;
    Some((m.offset, m.code, row as *const u8))
}

/// The direct link `w_fields` (`kithost.world@1.4`): fields of one component written in place by scalars.
/// `None`: not this way here (no link, an id that is no `e<number>`, a field no column mirrors, too many fields, or
/// refused — the caller sets the component as a value, which answers the reason). `Some(Ok(answer))`: written, as
/// `hot::pose`. `Some(Err(()))`: not written — no such entity, or the component does not hold those fields.
#[cfg(feature = "common-memory")]
pub(crate) fn write_fields(
    id: &str,
    fields: &[(&crate::entities::Field, crate::entities::FieldVal)],
    whole: bool,
    epoch: u64,
) -> Option<Result<[u64; 4], ()>> {
    use crate::entities::FieldVal;
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.world@^1.4")]
    extern "C" {
        fn w_fields(n: u32, count: u32, recs: *const u64, epoch: u64, out: *mut u64) -> u32;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    use crate::native::link::w_fields;
    const MAX: usize = 8;
    if fields.is_empty() || fields.len() > MAX {
        return None;
    }
    let n = rows::id_number(id)?;
    let mut recs = [0u64; 2 * MAX];
    for (i, (f, v)) in fields.iter().enumerate() {
        let at = mirror_of(f)? as u64;
        let (kind, bits) = match v {
            FieldVal::Num(x) => (0u64, x.to_bits()),
            FieldVal::Flag(b) => (1, *b as u64),
            FieldVal::Text(s) => (2, crate::direct::atom(s) as u64),
            FieldVal::Vec3(a) => (3, a.as_ptr() as usize as u64),
        };
        recs[2 * i] = at | kind << 32;
        recs[2 * i + 1] = bits;
    }
    let mut out = [0u64; 4];
    // SAFETY: an import wired by the calls host to the export `gfx_w_fields` of the core, which reads the records
    // (and the three numbers a record points to) and writes 4 `u64` on this module's stack — in the memory both share.
    match unsafe {
        w_fields(
            n,
            fields.len() as u32 | (whole as u32) << 16,
            recs.as_ptr(),
            epoch,
            out.as_mut_ptr(),
        )
    } {
        0 => Some(Ok(out)),
        2 => None,
        _ => Some(Err(())),
    }
}

/// One entry of `write_fields_many`: the arguments of one `write_fields` (id, fields, flag).
#[cfg(feature = "common-memory")]
pub(crate) type FieldsOf<'a> = (
    &'a str,
    &'a [(&'a crate::entities::Field, crate::entities::FieldVal<'a>)],
    bool,
);

/// `write_fields` for several entities in one call (the direct link `w_fields_many`, `kithost.world@1.5`). An entry
/// that cannot go this way (an id that is no `e<number>`, a field no column mirrors, too many fields) is answered
/// `None` without a call, as `write_fields`; the others as `write_fields` answers them.
#[cfg(feature = "common-memory")]
pub(crate) fn write_fields_many(
    entries: &[FieldsOf<'_>],
    epoch: u64,
) -> Vec<Option<Result<[u64; 4], ()>>> {
    use crate::entities::FieldVal;
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.world@^1.5")]
    extern "C" {
        fn w_fields_many(entities: u32, block: *const u64, epoch: u64, out: *mut u64) -> u32;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    use crate::native::link::w_fields_many;
    const MAX: usize = 8;
    let mut answers: Vec<Option<Result<[u64; 4], ()>>> = vec![None; entries.len()];
    let mut block: Vec<u64> = Vec::with_capacity(entries.len() * 9);
    let mut sent: Vec<usize> = Vec::with_capacity(entries.len());
    'entry: for (k, (id, fields, whole)) in entries.iter().enumerate() {
        if fields.is_empty() || fields.len() > MAX {
            continue;
        }
        let Some(n) = rows::id_number(id) else {
            continue;
        };
        let start = block.len();
        block.push(n as u64 | ((fields.len() as u64 | (*whole as u64) << 16) << 32));
        for (f, v) in fields.iter() {
            let Some(at) = mirror_of(f) else {
                block.truncate(start);
                continue 'entry;
            };
            let (kind, bits) = match v {
                FieldVal::Num(x) => (0u64, x.to_bits()),
                FieldVal::Flag(b) => (1, *b as u64),
                FieldVal::Text(s) => (2, crate::direct::atom(s) as u64),
                FieldVal::Vec3(a) => (3, a.as_ptr() as usize as u64),
            };
            block.push(at as u64 | kind << 32);
            block.push(bits);
        }
        sent.push(k);
    }
    if sent.is_empty() {
        return answers;
    }
    let mut out = vec![0u64; 5 * sent.len()];
    // SAFETY: an import wired by the calls host to the export `gfx_w_fields_many` of the core, which reads the block
    // (and the three numbers a record points to) and writes 5 `u64` per entity — all in the memory both share.
    unsafe { w_fields_many(sent.len() as u32, block.as_ptr(), epoch, out.as_mut_ptr()) };
    for (j, k) in sent.into_iter().enumerate() {
        let o = &out[5 * j..5 * j + 5];
        answers[k] = match o[0] {
            0 => Some(Ok([o[1], o[2], o[3], o[4]])),
            2 => None,
            _ => Some(Err(())),
        };
    }
    answers
}

#[cfg(not(feature = "common-memory"))]
#[allow(
    clippy::type_complexity,
    reason = "lint of a newer clippy than the source; copied as is (docs/sources.md)"
)]
pub(crate) fn write_fields_many(
    entries: &[(
        &str,
        &[(&crate::entities::Field, crate::entities::FieldVal)],
        bool,
    )],
    _: u64,
) -> Vec<Option<Result<[u64; 4], ()>>> {
    vec![None; entries.len()]
}

#[cfg(not(feature = "common-memory"))]
pub(crate) fn write_fields(
    _: &str,
    _: &[(&crate::entities::Field, crate::entities::FieldVal)],
    _: bool,
    _: u64,
) -> Option<Result<[u64; 4], ()>> {
    None
}

impl crate::host::Kx<'_> {
    /// The column `name` of the entity `id` (a scalar: the first number): see [`read`].
    pub fn column(&self, id: &str, name: &str) -> Result<Option<[f64; 3]>, String> {
        read(&self.host.kits.slot.id, id, name)
    }
    /// Writes the column `name` of the entity `id`: see [`write`].
    pub fn set_column(&mut self, id: &str, name: &str, v: [f64; 3]) -> Result<bool, String> {
        write(&self.host.kits.slot.id, id, name, v, false)
    }
    /// The `u64` column `name` of the entity `id`, whole (a bit mask): see [`read_bits`].
    pub fn column_bits(&self, id: &str, name: &str) -> Result<Option<u64>, String> {
        read_bits(&self.host.kits.slot.id, id, name)
    }
    /// Writes the `u64` column `name` of the entity `id`, whole: see [`write_bits`].
    pub fn set_column_bits(&mut self, id: &str, name: &str, v: u64) -> Result<bool, String> {
        write_bits(&self.host.kits.slot.id, id, name, v)
    }
    /// Adds to the column `name` of the entity `id`: see [`write`].
    pub fn add_column(&mut self, id: &str, name: &str, v: [f64; 3]) -> Result<bool, String> {
        write(&self.host.kits.slot.id, id, name, v, true)
    }
}
