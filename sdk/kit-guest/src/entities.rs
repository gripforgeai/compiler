//! Façade of src/entities.rs: the entity types are shared with the core (src/entities/entity.rs, which the core compiles too); the
//! registry lives in the core (`kithost.world@1`).
//!
//! What a kit reads by reference (`get`, `component`, `all`, `query`…) is a copy read from the core. The copies of
//! whole entities are kept from one hook to the next with the revision they are (src/hot.rs): the world's epoch,
//! read by a direct link, tells when they must be checked again, and the core then sends only what changed. The
//! other reads (a component alone, the whole list) are kept for the epoch they were read at.
//!
//! Nothing a reference points to is freed or changed from a `&self` method: a copy that must change while it may be
//! referenced is replaced (the old one waits in `grave`), and all of it is freed from a `&mut self` method (a
//! write) or a new entry of the core (`enter`). `get_mut` hands a guard that writes back what changed in the entity
//! when it is dropped.

mod entity;
pub use entity::{
    default_transform, field_flag, field_num, field_vec3, fields_into, fields_merge, merge,
    near_one, pose_exact, pose_exact_into, pose_into, pose_patch_into, Cond, Entity, Field,
    FieldVal, Pos, Query,
};

use crate::direct::FastMap;
use crate::host::{call_core_ok, Host};
use crate::hot::{self, Kept};
use gripforge_abi::{hot as fh, world as f, Reader, Writer};
use serde_json::{Map, Value as Json};
use std::cell::RefCell;

/// The entity a value holds (`{ id, kind, tags, components, owner, refs }`, see src/kitcall/world.rs of the core).
pub(crate) fn entity_of(v: Json) -> Option<Entity> {
    let Json::Object(mut o) = v else { return None };
    let mut text = |k: &str| match o.remove(k) {
        Some(Json::String(s)) => s,
        _ => String::new(),
    };
    let (id, kind, owner) = (text("id"), text("kind"), text("owner"));
    let tags = match o.remove("tags") {
        Some(Json::Array(a)) => a
            .into_iter()
            .filter_map(|t| {
                if let Json::String(s) = t {
                    Some(s)
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    let components = match o.remove("components") {
        Some(Json::Object(c)) => c.into_iter().collect(),
        _ => Vec::new(),
    };
    let refs = match o.remove("refs") {
        Some(Json::Object(r)) => r,
        _ => Map::new(),
    };
    Some(Entity {
        id,
        kind,
        tags,
        components,
        owner,
        refs,
    })
}

/// The value of an entity (the inverse of [`entity_of`]).
pub(crate) fn entity_value(e: &Entity) -> Json {
    let mut components = Map::with_capacity(e.components.len());
    for (k, v) in &e.components {
        components.insert(k.clone(), v.clone());
    }
    let mut o = Map::with_capacity(6);
    o.insert("id".into(), Json::String(e.id.clone()));
    o.insert("kind".into(), Json::String(e.kind.clone()));
    o.insert(
        "tags".into(),
        Json::Array(e.tags.iter().map(|t| Json::String(t.clone())).collect()),
    );
    o.insert("components".into(), Json::Object(components));
    o.insert("owner".into(), Json::String(e.owner.clone()));
    o.insert("refs".into(), Json::Object(e.refs.clone()));
    Json::Object(o)
}

pub(crate) fn write_query(w: &mut Writer, q: &Query) {
    w.opt_str(q.kind.as_deref())
        .strs(&q.all)
        .strs(&q.any)
        .strs(&q.none);
}

fn read_entities(bytes: &[u8]) -> Vec<Entity> {
    let mut r = Reader::new(bytes);
    let n = r.u32().unwrap_or(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        // In place when the core passed a fixed-layout block (crate::fixed_read): no value built in between.
        match crate::fixed_read::entity(&mut r) {
            Some(e) => out.push(e),
            None => break,
        }
    }
    out
}

fn call(func: crate::uses::Func, args: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = crate::host::writer();
    args(&mut w);
    call_core_ok(func, &w)
}

/// Values handed out by reference: each allocated once and never moved (raw, so no `Box` is moved while a
/// reference to its content lives), freed when the arena drops.
pub(crate) struct Arena<T: ?Sized> {
    ptrs: Vec<*mut T>,
}

impl<T: ?Sized> Default for Arena<T> {
    fn default() -> Self {
        Arena { ptrs: Vec::new() }
    }
}

impl<T: ?Sized> Arena<T> {
    /// Keeps `b`; the pointer stays valid until the arena drops.
    pub(crate) fn keep(&mut self, b: Box<T>) -> *const T {
        let p = Box::into_raw(b);
        self.ptrs.push(p);
        p
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.ptrs.is_empty()
    }
}

impl<T: ?Sized> Drop for Arena<T> {
    fn drop(&mut self) {
        for p in self.ptrs.drain(..) {
            // SAFETY: made by Box::into_raw in `keep`, freed once.
            drop(unsafe { Box::from_raw(p) });
        }
    }
}

/// What was read at one epoch of the world: valid as long as the world has that epoch.
#[derive(Default)]
struct AtEpoch {
    /// Every copy handed out that is not a kept entity: entity lists, values.
    lists: Arena<[Entity]>,
    values: Arena<Json>,
    /// Ids that name no entity.
    absent: FastMap<String, ()>,
    /// id → component → its copy in `values` (None: absent). Looked up without building a key.
    components: FastMap<String, Vec<(String, Option<*const Json>)>>,
    /// The whole list (`all()`).
    all: Option<*const [Entity]>,
    /// Query (its bytes) → the answer.
    firsts: FastMap<Vec<u8>, Option<*const Kept>>,
    queries: FastMap<Vec<u8>, Vec<*const Kept>>,
}

impl AtEpoch {
    /// What was read of a component alone: `None` not read, `Some(None)` absent.
    fn component(&self, id: &str, name: &str) -> Option<Option<*const Json>> {
        self.components
            .get(id)?
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| *v)
    }
}

/// Copies read from the core (see the module's doc and src/hot.rs).
struct Reads {
    /// id → the copy of the entity, with the revision it is. Kept from one hook to the next.
    kept: FastMap<String, *mut Kept>,
    /// Copies replaced while a reference to them may live: freed at the next `&mut self`.
    grave: Vec<*mut Kept>,
    /// Counts the `&mut self` points (a write, an entry): no reference handed out before one lives after it.
    era: u64,
    /// The epoch of the world `now` and the copies with `seen == check` stand for.
    epoch: u64,
    check: u64,
    now: AtEpoch,
    /// What was read at earlier epochs since the last `&mut self` (references to it may live).
    old: Vec<AtEpoch>,
    /// The structure of the world (the list, kinds, tags: `hot::structure`) at `epoch`.
    structure: u64,
    /// Query (its bytes) → the structure it was answered at and the ids of the answer. While the world has that
    /// structure the answer is those entities (no query asked again); after, they are the hints of the next one.
    first_hint: FastMap<Vec<u8>, (u64, Option<String>)>,
    query_hint: FastMap<Vec<u8>, (u64, Vec<String>)>,
    /// The whole list (`all()`), kept from one read to the next and brought up to date in place.
    all_kept: Option<AllKept>,
    /// Tag → the structure it was asked at and the id of the first entity that has it (`None`: none has): what
    /// `first` of one tag answers while the world has that structure.
    tag_first: Vec<(String, u64, Option<std::rc::Rc<str>>)>,
}

/// The list `all()` hands out, kept: while the structure of the world is the one it was read at, the next `all()`
/// asks the core only what changed in each entity since the revision held (`GET_MANY_IF`) and applies it in place —
/// where a whole-list read copied every entity again (in the native host `all()` copies nothing; a game that walks
/// every entity each tick paid thousands of allocations a frame here).
struct AllKept {
    /// Owned (`Box::into_raw`): a slice, so that `all()` keeps answering `&[Entity]`.
    list: *mut [Entity],
    /// The revision of each entity of `list` (0: unknown, the core sends it whole once).
    revs: Vec<u64>,
    /// Their `core.shape@1` when each was read (0: not known), as `Kept::other`.
    others: Vec<u64>,
    /// The structure of the world the list (its ids, their order) stands for.
    structure: u64,
    /// The era it was last handed out in: changed in place only in a later one (no reference to it lives then).
    lent: u64,
}

impl Drop for AllKept {
    fn drop(&mut self) {
        // SAFETY: made by Box::into_raw, freed once.
        drop(unsafe { Box::from_raw(self.list) });
    }
}

impl Default for Reads {
    fn default() -> Self {
        Reads {
            kept: FastMap::default(),
            grave: Vec::new(),
            era: 1,
            epoch: 0,
            check: 1,
            now: AtEpoch::default(),
            old: Vec::new(),
            structure: 0,
            first_hint: FastMap::default(),
            query_hint: FastMap::default(),
            all_kept: None,
            tag_first: Vec::new(),
        }
    }
}

impl Reads {
    fn bury(&mut self) {
        for p in self.grave.drain(..) {
            // SAFETY: made by Box::into_raw, out of `kept`, freed once; called from `&mut self` points only.
            drop(unsafe { Box::from_raw(p) });
        }
    }
}

impl Drop for Reads {
    fn drop(&mut self) {
        self.bury();
        for (_, p) in self.kept.drain() {
            // SAFETY: as `bury`.
            drop(unsafe { Box::from_raw(p) });
        }
    }
}

/// More kept copies than this are dropped at the next write or entry (entities long gone are never asked again).
const KEPT_MAX: usize = 4096;

/// `host.entities`.
#[derive(Default)]
pub struct Entities {
    reads: RefCell<Reads>,
    /// The moment of the module (`host::moment`) this façade last looked at: at another one, what it handed out
    /// before is referenced by nothing any more (`settle`).
    seen: std::cell::Cell<u64>,
}

impl Entities {
    /// A write (`&mut self`: no reference handed out is alive). The copies stay: they are checked against the
    /// world's epoch when they are read.
    pub fn enter(&mut self) {
        self.seen.set(crate::host::moment());
        Self::entered(self.reads.get_mut());
    }

    /// No reference handed out before lives: what was set aside for them is freed, the copies may change in place.
    fn entered(r: &mut Reads) {
        r.era += 1;
        r.old.clear();
        r.bury();
        if r.kept.len() > KEPT_MAX {
            let (era, check) = (r.era, r.check + 1);
            *r = Reads::default();
            r.era = era;
            r.check = check;
        }
    }

    /// The first read since the moment moved (a new entry of the core, a call that may have run other kits — both
    /// through `&mut Host`, so no reference handed out before is alive): what [`Entities::enter`] does.
    #[inline]
    fn settle(&self) {
        let now = crate::host::moment();
        if self.seen.get() != now {
            self.seen.set(now);
            Self::entered(&mut self.reads.borrow_mut());
        }
    }

    fn wrote(&mut self) {
        self.enter();
    }

    /// Reads the world's epoch (a direct link): when it moved, what was read at the last one is set aside and the
    /// kept copies are to be checked again.
    fn sync(&self) {
        self.settle();
        let e = hot::epoch();
        let mut r = self.reads.borrow_mut();
        if e != r.epoch {
            r.epoch = e;
            r.structure = hot::structure();
            r.check += 1;
            // What a reference may point to (the arenas) is set aside until the next `&mut self`; the tables only
            // hold pointers and are emptied in place (they keep their memory from one epoch to the next).
            let now = &mut r.now;
            if !now.lists.is_empty() || !now.values.is_empty() {
                let was = AtEpoch {
                    lists: std::mem::take(&mut now.lists),
                    values: std::mem::take(&mut now.values),
                    ..AtEpoch::default()
                };
                now.absent.clear();
                now.components.clear();
                now.all = None;
                now.firsts.clear();
                now.queries.clear();
                r.old.push(was);
            } else {
                now.absent.clear();
                now.components.clear();
                now.all = None;
                now.firsts.clear();
                now.queries.clear();
            }
        }
    }

    fn keep_list(&self, list: Vec<Entity>) -> &[Entity] {
        let p = self
            .reads
            .borrow_mut()
            .now
            .lists
            .keep(list.into_boxed_slice());
        // SAFETY: kept in the arena, which is only dropped through `&mut self`.
        unsafe { &*p }
    }

    fn keep_value(&self, v: Json) -> &Json {
        let p = self.reads.borrow_mut().now.values.keep(Box::new(v));
        // SAFETY: as `keep_list`.
        unsafe { &*p }
    }

    /// The kept copy of `id` when it is current (checked at this epoch).
    fn current(&self, id: &str) -> Option<*mut Kept> {
        let (mut p, check, era) = {
            let r = self.reads.borrow();
            (*r.kept.get(id)?, r.check, r.era)
        };
        // SAFETY: a live box of `kept` (and below: changed in place only when no reference to it was handed out
        // since the last `&mut self`, else a changed copy takes its place, as in `apply`).
        unsafe {
            if (*p).seen == check {
                return Some(p);
            }
            // The entity's row of the zone says its revision without a call: the copy is that revision, or only
            // the base moved since (then the row has the four numbers to write in the copy's transform).
            let (rev, shape, base) = crate::columns::revision(id)?;
            if rev != (*p).rev {
                let (fields, floats) = base.filter(|_| shape == (*p).other && shape != 0)?;
                if (*p).lent == era {
                    let copy = Box::into_raw(Box::new(Kept {
                        entity: (*p).entity.clone(),
                        ..*p
                    }));
                    let mut r = self.reads.borrow_mut();
                    r.grave.push(p);
                    r.kept.insert(id.to_string(), copy);
                    p = copy;
                }
                let done = (*p).entity.components.iter_mut().find(|(k, _)| k == "transform").is_some_and(|(_, t)| pose_exact_into(t, fields, floats))
                    // The fields the columns mirror, as the row has them now (a game without such columns: nothing).
                    && (!crate::columns::any_mirrors() || crate::columns::readable_row(id).is_some_and(|row| crate::columns::mirrors_into(&mut (*p).entity, row)));
                if !done {
                    // Half written at worst: the core sends it whole.
                    (*p).rev = 0;
                    (*p).other = 0;
                    return None;
                }
                (*p).rev = rev;
            }
            (*p).seen = check;
            Some(p)
        }
    }

    /// The copy `p` was just made the entity `id` as the core has it (an answer of the core applied): what its row
    /// says of its last change that is more than a move, for the next reads.
    fn stamp(p: *mut Kept) {
        // SAFETY: a live box of `kept`.
        unsafe {
            (*p).other = match crate::columns::revision(&(*p).entity.id) {
                Some((rev, shape, _)) if rev == (*p).rev => shape,
                _ => 0,
            };
        }
    }

    /// One delta of the core (src/hot.rs) into the kept copies: `id` when the caller knows which entity it is
    /// about (else it is in the delta: a list). `None`: no entity.
    fn apply(&self, r: &mut Reader, id: Option<&str>) -> Option<*mut Kept> {
        let status = r.u8().ok()?;
        let mut reads = self.reads.borrow_mut();
        let reads = &mut *reads;
        match status {
            hot::D_WHOLE => {
                let (rev, entity) = hot::read_whole(r).ok()?;
                if let Some(p) = reads.kept.get(&entity.id).copied() {
                    // SAFETY: a live box of `kept`.
                    unsafe {
                        if (*p).rev == rev {
                            // The copy held is that revision already (the core had no hint of it): keep it, what
                            // was read at this epoch may point to it.
                            (*p).seen = reads.check;
                            (*p).lent = reads.era;
                            return Some(p);
                        }
                    }
                }
                let fresh = Box::into_raw(Box::new(Kept {
                    entity,
                    rev,
                    seen: reads.check,
                    lent: reads.era,
                    other: 0,
                }));
                Self::stamp(fresh);
                // SAFETY: a live box.
                let key = unsafe { (*fresh).entity.id.clone() };
                if let Some(was) = reads.kept.insert(key, fresh) {
                    reads.grave.push(was);
                }
                Some(fresh)
            }
            hot::D_SAME | hot::D_CHANGES => {
                let id = match id {
                    Some(id) => id,
                    None => r.str().ok()?,
                };
                let mut p = *reads.kept.get(id)?;
                // SAFETY: a live box of `kept`; changed in place only when no reference to it was handed out
                // since the last `&mut self` (`lent`), else a changed copy takes its place.
                unsafe {
                    if status == hot::D_CHANGES {
                        if (*p).lent == reads.era {
                            let copy = Box::into_raw(Box::new(Kept {
                                entity: (*p).entity.clone(),
                                ..*p
                            }));
                            reads.grave.push(p);
                            reads.kept.insert(id.to_string(), copy);
                            p = copy;
                        }
                        (*p).rev = hot::read_changes(r, &mut (*p).entity).ok()?;
                        Self::stamp(p);
                    }
                    (*p).seen = reads.check;
                    (*p).lent = reads.era;
                    if (*p).other == 0 {
                        Self::stamp(p);
                    }
                }
                Some(p)
            }
            _ => {
                if let Some(id) = id {
                    if let Some(was) = reads.kept.remove(id) {
                        reads.grave.push(was);
                    }
                    reads.now.absent.insert(id.to_string(), ());
                }
                None
            }
        }
    }

    /// The current copy of `id`: the kept one when the world did not change since it was checked, else what the
    /// core says changed since its revision.
    fn fresh(&self, id: &str) -> Option<*mut Kept> {
        if let Some(p) = self.current(id) {
            // SAFETY: a live box of `kept`.
            unsafe { (*p).lent = self.reads.borrow().era };
            return Some(p);
        }
        let have = {
            let r = self.reads.borrow();
            if r.now.absent.contains_key(id) {
                return None;
            }
            // SAFETY: as above.
            r.kept.get(id).map_or(0, |p| unsafe { (**p).rev })
        };
        let bytes = call(crate::func!(fh::GET_IF), |w| {
            w.str(id).u64(have);
        });
        let got = self.apply(&mut Reader::new(&bytes), Some(id));
        if got.is_none() && have != 0 && bytes.first() != Some(&hot::D_ABSENT) {
            // A delta this copy could not take: ask for the whole entity.
            let bytes = call(crate::func!(fh::GET_IF), |w| {
                w.str(id).u64(0);
            });
            return self.apply(&mut Reader::new(&bytes), Some(id));
        }
        got
    }

    pub fn get(&self, id: &str) -> Option<&Entity> {
        self.sync();
        // SAFETY: a kept copy is freed or changed only after the next `&mut self` (see `apply`).
        self.fresh(id).map(|p| unsafe { &(*p).entity })
    }

    /// A copy of the entity; what was changed in it is written back to the core when the guard drops.
    pub fn get_mut(&mut self, id: &str) -> Option<EntityMut<'_>> {
        self.wrote();
        self.sync();
        let kept = self.fresh(id)?;
        Some(EntityMut {
            entity: None,
            kept,
            of: self as *mut Entities,
            _of: std::marker::PhantomData,
        })
    }

    /// The revision of the entity `id`, read in its row of the zone (`core.revision@1`): it moves with every change
    /// of the entity (a component, a tag, a pose). `None`: the row cannot say here (no row, no such column) — the
    /// caller acts as if the entity changed. The same as the core's `Entities::revision`.
    pub fn revision(&self, id: &str) -> Option<u64> {
        crate::columns::revision(id).map(|(rev, _, _)| rev)
    }

    /// The last change of the entity `id` that is more than a pose or an in-place field (`core.shape@1`, the core's
    /// `Rev::other`): a roster that follows the athletes moving every tick re-reads only when this moves. `None`: as
    /// [`Entities::revision`].
    pub fn other_revision(&self, id: &str) -> Option<u64> {
        crate::columns::revision(id).map(|(_, other, _)| other)
    }

    /// Whether the engine owns this entity's pose. Ask the authoritative binding, not its tags.
    pub fn is_held(&self, id: &str) -> bool {
        call(crate::func!(f::IS_HELD), |w| {
            w.str(id);
        })
        .first()
            == Some(&1)
    }

    /// Whether an entity has a tag (false without such an entity), read by the core: no copy of the entity fetched.
    pub fn has_tag(&self, id: &str, tag: &str) -> bool {
        crate::direct::has_tag(id, tag)
            .unwrap_or_else(|| self.get(id).is_some_and(|e| e.has_tag(tag)))
    }

    pub fn has(&self, id: &str) -> bool {
        // The entity's row of the zone says so with a few loads (a row is an entity of this world: the core takes
        // and leaves them as entities come and go). No row here (an id that is no `e<number>`, a table that was
        // full, no table): the core is asked by a direct link — and nothing is kept of the answer: the call costs
        // less than a table of answers looked up and filled.
        if crate::columns::has_row(id) {
            return true;
        }
        // A direct link when the id packs (docs/kit-modules.md §7), else the bridge.
        crate::direct::has(id).unwrap_or_else(|| {
            call(crate::func!(f::HAS), |w| {
                w.str(id);
            })
            .first()
                == Some(&1)
        })
    }

    /// The answer of a query asked before at this structure of the world: the same entities, each brought up to
    /// date (those that changed: one call for all). `None`: not asked at this structure.
    fn same_structure(&self, key: &[u8]) -> Option<Vec<*const Kept>> {
        let ids: Vec<String> = {
            let r = self.reads.borrow();
            let (at, ids) = r.query_hint.get(key)?;
            if *at != r.structure {
                return None;
            }
            ids.clone()
        };
        let stale: Vec<&String> = ids.iter().filter(|id| self.current(id).is_none()).collect();
        match stale.len() {
            0 => {}
            1 => {
                self.fresh(stale[0])?;
            }
            n => {
                let mut w = crate::host::writer();
                w.u32(n as u32);
                {
                    let r = self.reads.borrow();
                    for id in &stale {
                        // SAFETY: a live box of `kept`.
                        w.str(id)
                            .u64(r.kept.get(*id).map_or(0, |p| unsafe { (**p).rev }));
                    }
                }
                let bytes = call_core_ok(crate::func!(fh::GET_MANY_IF), &w);
                let mut r = Reader::new(&bytes);
                for id in &stale {
                    self.apply(&mut r, Some(id))?;
                }
            }
        }
        let era = self.reads.borrow().era;
        let mut found = Vec::with_capacity(ids.len());
        for id in &ids {
            let p = self.current(id)?;
            // SAFETY: a live box of `kept`.
            unsafe { (*p).lent = era };
            found.push(p as *const Kept);
        }
        Some(found)
    }

    /// The existing QUERY_IDS ABI selects in the shared core; meshes and other components stay there.
    pub fn query_ids(&self, q: &Query) -> Vec<String> {
        Reader::new(&call(crate::func!(f::QUERY_IDS), |w| write_query(w, q)))
            .strs()
            .unwrap_or_default()
    }

    pub fn query(&self, q: &Query) -> Vec<&Entity> {
        self.sync();
        let mut w = crate::host::writer();
        write_query(&mut w, q);
        // SAFETY (all the `&(**p).entity` below): as `get`.
        if let Some(hit) = self.reads.borrow().now.queries.get(w.bytes()) {
            return hit.iter().map(|p| unsafe { &(**p).entity }).collect();
        }
        let key = w.bytes().to_vec();
        if let Some(found) = self.same_structure(&key) {
            self.reads
                .borrow_mut()
                .now
                .queries
                .insert(key, found.clone());
            return found.into_iter().map(|p| unsafe { &(*p).entity }).collect();
        }
        {
            let r = self.reads.borrow();
            let hints: Vec<(&String, u64)> = r
                .query_hint
                .get(&key)
                .into_iter()
                .flat_map(|(_, ids)| ids)
                .filter_map(|id| r.kept.get(id).map(|p| (id, unsafe { (**p).rev })))
                .collect();
            w.u32(hints.len() as u32);
            for (id, rev) in hints {
                w.str(id).u64(rev);
            }
        }
        let bytes = call_core_ok(crate::func!(fh::QUERY_IF), &w);
        let mut r = Reader::new(&bytes);
        let n = r.u32().unwrap_or(0) as usize;
        let mut found: Vec<*const Kept> = Vec::with_capacity(n);
        for _ in 0..n {
            match self.apply(&mut r, None) {
                Some(p) => found.push(p),
                None => {
                    // An answer this module cannot read (never from a core of this contract): the plain query.
                    let bytes = call(crate::func!(f::QUERY), |w| write_query(w, q));
                    return self.keep_list(read_entities(&bytes)).iter().collect();
                }
            }
        }
        let mut reads = self.reads.borrow_mut();
        let ids = found
            .iter()
            .map(|p| unsafe { (**p).entity.id.clone() })
            .collect();
        let at = reads.structure;
        reads.query_hint.insert(key.clone(), (at, ids));
        reads.now.queries.insert(key, found.clone());
        found.into_iter().map(|p| unsafe { &(*p).entity }).collect()
    }

    pub fn first(&self, q: &Query) -> Option<&Entity> {
        self.sync();
        // The first entity of ONE tag (`player()`, what most kits ask every tick): answered at this structure of
        // the world, it is the same entity — asked by its id, with no query written, hashed or copied.
        let tag = (q.kind.is_none() && q.any.is_empty() && q.none.is_empty() && q.all.len() == 1)
            .then(|| q.all[0].as_str());
        if let Some(tag) = tag {
            let known = {
                let r = self.reads.borrow();
                r.tag_first
                    .iter()
                    .find(|(t, at, _)| *at == r.structure && t == tag)
                    .map(|(_, _, id)| id.clone())
            };
            match known {
                Some(None) => return None,
                Some(Some(id)) => {
                    if let Some(p) = self.fresh(&id) {
                        // SAFETY: as `get`.
                        return Some(unsafe { &(*p).entity });
                    }
                }
                None => {}
            }
        }
        let got = self.first_asked(q);
        if let Some(tag) = tag {
            let mut r = self.reads.borrow_mut();
            let (at, id) = (
                r.structure,
                got.map(|e| std::rc::Rc::<str>::from(e.id.as_str())),
            );
            match r.tag_first.iter_mut().find(|(t, _, _)| t == tag) {
                Some(slot) => (slot.1, slot.2) = (at, id),
                None => r.tag_first.push((tag.to_string(), at, id)),
            }
        }
        got
    }

    /// [`Entities::first`] by the query itself (its answer of this epoch, its hint of this structure, the core).
    fn first_asked(&self, q: &Query) -> Option<&Entity> {
        let mut w = crate::host::writer();
        write_query(&mut w, q);
        // SAFETY (the `&(*p).entity` below): as `get`.
        if let Some(hit) = self.reads.borrow().now.firsts.get(w.bytes()) {
            return hit.map(|p| unsafe { &(*p).entity });
        }
        let key = w.bytes().to_vec();
        {
            // Answered at this structure of the world: the same entity (or none), asked by its id.
            let known = {
                let r = self.reads.borrow();
                r.first_hint
                    .get(&key)
                    .filter(|(at, _)| *at == r.structure)
                    .map(|(_, id)| id.clone())
            };
            if let Some(id) = known {
                let got = match &id {
                    Some(id) => self.fresh(id).map(|p| p as *const Kept),
                    None => None,
                };
                if got.is_some() == id.is_some() {
                    self.reads.borrow_mut().now.firsts.insert(key, got);
                    return got.map(|p| unsafe { &(*p).entity });
                }
            }
        }
        let hint = {
            let r = self.reads.borrow();
            r.first_hint
                .get(&key)
                .and_then(|(_, id)| id.as_ref())
                .and_then(|id| r.kept.get(id).map(|p| (id.clone(), unsafe { (**p).rev })))
        };
        match &hint {
            Some((id, rev)) => w.str(id).u64(*rev),
            None => w.str("").u64(0),
        };
        let bytes = call_core_ok(crate::func!(fh::FIRST_IF), &w);
        let got: Option<*const Kept> = if bytes.first() == Some(&hot::D_ABSENT) {
            None
        } else {
            self.apply(
                &mut Reader::new(&bytes),
                hint.as_ref().map(|(id, _)| id.as_str()),
            )
            .map(|p| p as *const Kept)
        };
        let mut reads = self.reads.borrow_mut();
        let at = reads.structure;
        reads.first_hint.insert(
            key.clone(),
            (at, got.map(|p| unsafe { (*p).entity.id.clone() })),
        );
        reads.now.firsts.insert(key, got);
        got.map(|p| unsafe { &(*p).entity })
    }

    /// The structure of the world (the core's `Entities::structure`): a value that changes whenever the list of
    /// entities, a kind or a tag does — what a kit may key what it derived from the list and the tags by. A load
    /// of the world's epoch; when it moved, a direct link.
    pub fn structure(&self) -> u64 {
        self.sync();
        self.reads.borrow().structure
    }

    pub fn by_tag(&self, tag: &str) -> Vec<&Entity> {
        self.query(&Query::tag(tag))
    }

    pub fn all(&self) -> &[Entity] {
        self.sync();
        if let Some(p) = self.reads.borrow().now.all {
            // SAFETY: as `keep_list`.
            return unsafe { &*p };
        }
        if let Some(p) = self.refresh_all() {
            self.reads.borrow_mut().now.all = Some(p);
            // SAFETY: owned by `all_kept`, changed or dropped only at a later era (`&mut self`).
            return unsafe { &*p };
        }
        let bytes = call(crate::func!(f::ALL), |_| {});
        let list = Box::into_raw(read_entities(&bytes).into_boxed_slice());
        let mut reads = self.reads.borrow_mut();
        let reads = &mut *reads;
        // SAFETY: just made.
        let n = unsafe { (&*list).len() };
        if let Some(was) = reads.all_kept.replace(AllKept {
            list,
            revs: vec![0; n],
            others: vec![0; n],
            structure: reads.structure,
            lent: reads.era,
        }) {
            if was.lent == reads.era {
                // A reference to the list replaced may live until the next `&mut self`: it goes with what was read.
                let was = std::mem::ManuallyDrop::new(was);
                // SAFETY: ownership moves to the arena, which frees it once.
                reads.now.lists.keep(unsafe { Box::from_raw(was.list) });
            }
        }
        reads.now.all = Some(list as *const [Entity]);
        // SAFETY: as above.
        unsafe { &*list }
    }

    /// The kept list brought up to date in place (see [`AllKept`]); `None`: none kept, the structure changed, it
    /// may still be referenced, or the core's answer does not fit — the caller reads the whole list again.
    fn refresh_all(&self) -> Option<*const [Entity]> {
        let mut w = crate::host::writer();
        {
            let mut r = self.reads.borrow_mut();
            let r = &mut *r;
            let era = r.era;
            let a = r.all_kept.as_mut()?;
            if a.structure != r.structure || a.lent == r.era {
                return None;
            }
            // SAFETY: owned by `all_kept`, no reference to it lives (handed out in an earlier era).
            let list = unsafe { &mut *a.list };
            // The rows of the zone first: an entity whose row says the revision held is current, one whose row
            // says only its base moved gets the four numbers. When that covers the whole list: no call.
            let mut current = true;
            for (i, e) in list.iter_mut().enumerate() {
                current = match crate::columns::revision(&e.id) {
                    Some((rev, _, _)) if rev == a.revs[i] => true,
                    Some((rev, shape, Some((fields, floats))))
                        if shape == a.others[i] && shape != 0 =>
                    {
                        let done = e.components.iter_mut().find(|(k, _)| k == "transform").is_some_and(|(_, t)| pose_exact_into(t, fields, floats))
                            // SAFETY: a row of the table read just above.
                            && (!crate::columns::any_mirrors() || crate::columns::readable_row(&e.id).is_some_and(|row| unsafe { crate::columns::mirrors_into(e, row) }));
                        a.revs[i] = if done { rev } else { 0 };
                        done
                    }
                    _ => false,
                };
                if !current {
                    break;
                }
            }
            if current {
                a.lent = era;
                return Some(a.list as *const [Entity]);
            }
            w.u32(list.len() as u32);
            for (e, rev) in list.iter().zip(&a.revs) {
                w.str(&e.id).u64(*rev);
            }
        }
        let bytes = call_core_ok(crate::func!(fh::GET_MANY_IF), &w);
        let mut r = Reader::new(&bytes);
        let mut reads = self.reads.borrow_mut();
        let reads = &mut *reads;
        let era = reads.era;
        let a = reads.all_kept.as_mut()?;
        // SAFETY: as above; this is the only reference.
        let list = unsafe { &mut *a.list };
        let mut ok = true;
        for (i, (e, rev)) in list.iter_mut().zip(a.revs.iter_mut()).enumerate() {
            match r.u8().ok() {
                Some(hot::D_SAME) => {}
                Some(hot::D_CHANGES) => match hot::read_changes(&mut r, e) {
                    Ok(now) => *rev = now,
                    Err(_) => ok = false,
                },
                Some(hot::D_WHOLE) => match hot::read_whole(&mut r) {
                    Ok((now, entity)) if entity.id == e.id => {
                        *e = entity;
                        *rev = now;
                    }
                    _ => ok = false,
                },
                _ => ok = false,
            }
            if !ok {
                break;
            }
            // What the entity's row says of its last change that is more than a move, for the next reads.
            a.others[i] = match crate::columns::revision(&e.id) {
                Some((now, shape, _)) if now == *rev => shape,
                _ => 0,
            };
        }
        if !ok {
            reads.all_kept = None;
            return None;
        }
        a.lent = era;
        Some(a.list as *const [Entity])
    }

    /// The neighbours of a point (the core's `Entities::near`): the id and the distance of every entity with a
    /// transform within `radius` on the ground for which every condition holds, in the order of the registry. A
    /// kit module of the common memory asks the core by a direct link — scalars and a result buffer, no entity is
    /// read here; elsewhere the same search (`near_one`) walks this module's copies.
    pub fn near(&self, x: f64, z: f64, radius: f64, conds: &[Cond]) -> Vec<(String, f64)> {
        if let Some(found) = near_link::ask(x, z, radius, conds) {
            return found;
        }
        self.all()
            .iter()
            .filter_map(|e| near_one(e, x, z, radius, conds).map(|d| (e.id.clone(), d)))
            .collect()
    }

    pub fn player(&self) -> Option<&Entity> {
        self.first(&Query::tag("player"))
    }

    pub fn set(&mut self, id: &str, component: &str, value: Json) {
        self.wrote();
        let epoch = self.reads.get_mut().epoch;
        let bytes = call(crate::func!(fh::SET_IF), |w| {
            w.str(id).str(component).value(&value).u64(epoch);
        });
        let mut r = Reader::new(&bytes);
        let (before, after, epoch) = (
            r.u64().unwrap_or(0),
            r.u64().unwrap_or(0),
            r.u64().unwrap_or(0),
        );
        // A component set is more than a move: the entity's `core.shape@1` is its new revision.
        self.wrote_own(id, [before, after, epoch, after], |e| {
            match e.components.iter_mut().find(|(k, _)| k == component) {
                Some(slot) => hot::assign(&mut slot.1, &value),
                None => e.components.push((component.to_string(), value.clone())),
            }
            true
        });
        // The component read alone at this epoch (an entity this module holds no copy of) is this value now.
        let reads = self.reads.get_mut();
        if let Some(i) = reads
            .now
            .components
            .get(id)
            .and_then(|l| l.iter().position(|(k, _)| k == component))
        {
            let p = reads.now.values.keep(Box::new(value));
            if let Some(list) = reads.now.components.get_mut(id) {
                list[i].1 = Some(p);
            }
        }
    }

    /// Writes fields of one component of the entity in place (the core's `Entities::set_fields`: same answer, same
    /// component stored). Where a column of the game schema mirrors each field (a kit module of the common
    /// memory), the values cross as scalars by the direct link `w_fields`: no value is built, encoded or copied,
    /// and the copies this module keeps take the same write. Elsewhere the same write is made on a copy of the
    /// component, set as a value.
    pub fn set_fields(&mut self, id: &str, fields: &[(&Field, FieldVal)], whole: bool) -> bool {
        self.wrote();
        let Some(first) = fields.first() else {
            return false;
        };
        let component = first.0.component;
        if fields.iter().any(|(f, _)| f.component != component) {
            return false;
        }
        let epoch = self.reads.get_mut().epoch;
        match crate::columns::write_fields(id, fields, whole, epoch) {
            Some(Ok(answer)) => {
                self.wrote_own(id, answer, |e| {
                    e.components
                        .iter_mut()
                        .find(|(k, _)| k == component)
                        .is_some_and(|(_, c)| {
                            fields_into(c, fields.iter().map(|(f, v)| (f.name, *v)), whole)
                                .is_some()
                        })
                });
                self.reads.get_mut().now.components.remove(id);
                true
            }
            Some(Err(())) => false,
            None => {
                let Some(mut c) = self.component(id, component).cloned() else {
                    return false;
                };
                if fields_into(&mut c, fields.iter().map(|(f, v)| (f.name, *v)), whole).is_none() {
                    return false;
                }
                self.set(id, component, c);
                true
            }
        }
    }

    /// Writes fields of one component of the entity, adding and removing keys (the core's `Entities::merge_fields`:
    /// same answer, same component stored): what `set(id, component, { ...component, ...fields })` would store, a
    /// field given `None` deleted. When every field is given and the component holds them all, it is
    /// [`Entities::set_fields`] (in place, scalars by the direct link where a column mirrors them); else the same
    /// write is made on a copy of the component, set as a value. False: no such entity, or the component is no
    /// object — nothing is written.
    pub fn merge_fields(&mut self, id: &str, fields: &[(&Field, Option<FieldVal>)]) -> bool {
        let Some(first) = fields.first() else {
            return false;
        };
        let component = first.0.component;
        if fields.iter().any(|(f, _)| f.component != component) {
            return false;
        }
        let plain: Option<Vec<(&Field, FieldVal)>> =
            fields.iter().map(|(f, v)| v.map(|v| (*f, v))).collect();
        if plain.is_some_and(|p| self.set_fields(id, &p, false)) {
            return true;
        }
        let Some(mut c) = self.component(id, component).cloned() else {
            return false;
        };
        if !fields_merge(&mut c, fields.iter().map(|(f, v)| (f.name, *v))) {
            return false;
        }
        self.set(id, component, c);
        true
    }

    /// A field of a component when it is a text (`None`: no such entity, component or field, or not a text), read
    /// in the component.
    pub fn field_text(&self, id: &str, f: &Field) -> Option<&str> {
        entity::field_text(self.component(id, f.component), f.name)
    }

    /// `set_fields` for several entities in one crossing to the core (`kithost.world@1.5` `w_fields_many`): each
    /// entry `(id, fields, whole)`, answered as `set_fields` answers it.
    #[allow(
        clippy::type_complexity,
        reason = "lint of a newer clippy than the source; copied as is (docs/sources.md)"
    )]
    pub fn set_fields_many(
        &mut self,
        entries: &[(&str, &[(&Field, FieldVal)], bool)],
    ) -> Vec<bool> {
        self.wrote();
        let epoch = self.reads.get_mut().epoch;
        let answers = crate::columns::write_fields_many(entries, epoch);
        let mut done = Vec::with_capacity(entries.len());
        for ((id, fields, whole), answer) in entries.iter().zip(answers) {
            let (id, whole) = (*id, *whole);
            let Some(first) = fields.first() else {
                done.push(false);
                continue;
            };
            let component = first.0.component;
            if fields.iter().any(|(f, _)| f.component != component) {
                done.push(false);
                continue;
            }
            done.push(match answer {
                Some(Ok(answer)) => {
                    self.wrote_own(id, answer, |e| {
                        e.components
                            .iter_mut()
                            .find(|(k, _)| k == component)
                            .is_some_and(|(_, c)| {
                                fields_into(c, fields.iter().map(|(f, v)| (f.name, *v)), whole)
                                    .is_some()
                            })
                    });
                    self.reads.get_mut().now.components.remove(id);
                    true
                }
                Some(Err(())) => false,
                // Not by the direct link: as `set_fields` (the component as a value).
                None => self.set_fields(id, fields, whole),
            });
        }
        done
    }

    /// A field of a component as a number (the core's `Entities::field`): a load in the entity's row when a column
    /// of the game schema mirrors it, else read in the component.
    pub fn field(&self, id: &str, f: &Field) -> Option<f64> {
        if let Some((at, code, row)) = crate::columns::mirror_row(id, f) {
            if code == gripforge_abi::rows::mirror::T_F64 {
                // SAFETY: a column of the schema, within the row.
                let v: f64 = unsafe { gripforge_abi::rows::get(row, at) };
                return (!v.is_nan()).then_some(v);
            }
        }
        field_num(self.component(id, f.component), f.name)
    }

    /// A field of a component as a boolean (see [`Entities::field`]).
    pub fn field_flag(&self, id: &str, f: &Field) -> Option<bool> {
        if let Some((at, code, row)) = crate::columns::mirror_row(id, f) {
            if code == gripforge_abi::rows::mirror::T_BOOL {
                // SAFETY: as `field`.
                let b: u8 = unsafe { gripforge_abi::rows::get(row, at) };
                return (b < gripforge_abi::rows::mirror::NO_FLAG).then_some(b != 0);
            }
        }
        field_flag(self.component(id, f.component), f.name)
    }

    /// Whether a field of a component is the text `text` (`None`: no such field, or not a text): the atom of a
    /// column of the game schema that mirrors it compared in the entity's row, else read in the component.
    pub fn field_text_is(&self, id: &str, f: &Field, text: &str) -> Option<bool> {
        use gripforge_abi::rows::mirror;
        if let Some((at, code, row)) = crate::columns::mirror_row(id, f) {
            if code == mirror::T_ATOM {
                // SAFETY: as `field`.
                let a: u32 = unsafe { gripforge_abi::rows::get(row, at) };
                return (a != mirror::NO_ATOM).then(|| a == crate::direct::atom(text));
            }
        }
        entity::field_text(self.component(id, f.component), f.name).map(|t| t == text)
    }

    /// Whether the component holds the field, whatever its value: a number column that mirrors it says so in the
    /// entity's row (`rows::mirror::F64_ABSENT`), else read in the component.
    pub fn field_present(&self, id: &str, f: &Field) -> bool {
        use gripforge_abi::rows::mirror;
        if let Some((at, code, row)) = crate::columns::mirror_row(id, f) {
            if code == mirror::T_F64 {
                // SAFETY: as `field`.
                let bits: u64 = unsafe { gripforge_abi::rows::get(row, at) };
                return bits != mirror::F64_ABSENT;
            }
        }
        entity::field_value(self.component(id, f.component), f.name).is_some()
    }

    /// A field of a component as three numbers (see [`Entities::field`]).
    pub fn field_vec3(&self, id: &str, f: &Field) -> Option<[f64; 3]> {
        if let Some((at, code, row)) = crate::columns::mirror_row(id, f) {
            if code == gripforge_abi::rows::T_F64X3 {
                // SAFETY: as `field`.
                let v: [f64; 3] = unsafe {
                    [
                        gripforge_abi::rows::get(row, at),
                        gripforge_abi::rows::get(row, at + 8),
                        gripforge_abi::rows::get(row, at + 16),
                    ]
                };
                return (!v[0].is_nan() && !v[1].is_nan() && !v[2].is_nan()).then_some(v);
            }
        }
        field_vec3(self.component(id, f.component), f.name)
    }

    /// A write of this module reached the core, which answered the entity's revision before and after it and the
    /// world's epoch after it when nothing else had changed since this module last looked (`SET_IF`, `hot_pose`):
    /// what was read stays current, and the copies of that entity are brought up to date here by `apply` (false:
    /// it cannot — the copy is then asked again). No answer of that kind (an epoch of 0): the world moved, the
    /// next read sees it by the epoch.
    ///
    /// The copies of that entity that were its revision `before` are, with the write applied, its revision
    /// `after` — whatever else changed in the world (`other`: its `core.shape@1` after the write).
    fn wrote_own(
        &mut self,
        id: &str,
        [before, after, epoch, other]: [u64; 4],
        mut apply: impl FnMut(&mut Entity) -> bool,
    ) {
        let r = self.reads.get_mut();
        if epoch != 0 {
            r.epoch = epoch;
        }
        if after == 0 {
            return;
        }
        if let Some(p) = r.kept.get(id).copied() {
            // SAFETY: a live box of `kept`; `&mut self`: no reference to it is alive.
            let k = unsafe { &mut *p };
            if k.rev == before && apply(&mut k.entity) {
                k.rev = after;
                k.other = other;
            } else {
                k.seen = 0;
                k.other = 0;
            }
        }
        if let Some(a) = r.all_kept.as_mut() {
            // SAFETY: owned by `all_kept`; `&mut self`: no reference to it is alive.
            let list = unsafe { &mut *a.list };
            let done = list.iter().position(|e| e.id == id).is_some_and(|i| {
                let ok = a.revs[i] == before && apply(&mut list[i]);
                a.revs[i] = if ok { after } else { 0 };
                a.others[i] = if ok { other } else { 0 };
                ok
            });
            if !done {
                r.now.all = None;
            }
        }
    }

    pub fn component(&self, id: &str, component: &str) -> Option<&Json> {
        self.sync();
        if let Some(p) = self.current(id) {
            // SAFETY: as `get`.
            unsafe {
                (*p).lent = self.reads.borrow().era;
                return (*p).entity.component(component);
            }
        }
        if self.reads.borrow().now.absent.contains_key(id) {
            return None;
        }
        if let Some(hit) = self.reads.borrow().now.component(id, component) {
            // SAFETY: as `keep_list`.
            return hit.map(|p| unsafe { &*p });
        }
        // Absent or a scalar: a direct link.
        let scalar = crate::direct::component(id, component);
        if scalar.is_none() && crate::columns::table().is_some() {
            // An object, an array or a long string, where the entity rows are in reach: the whole entity is kept
            // (one read), and its row then says, without a call, whether the copy is still current.
            return self.fresh(id).and_then(|p| unsafe {
                // SAFETY: as `get`.
                (*p).lent = self.reads.borrow().era;
                (*p).entity.component(component)
            });
        }
        // Elsewhere: the bridge, for that component alone.
        let value = scalar.unwrap_or_else(|| {
            let bytes = call(crate::func!(f::COMPONENT), |w| {
                w.str(id).str(component);
            });
            Reader::new(&bytes).opt_value().ok().flatten()
        });
        let got = value.map(|v| self.keep_value(v));
        let mut r = self.reads.borrow_mut();
        let at = got.map(|v| v as *const Json);
        match r.now.components.get_mut(id) {
            Some(list) => list.push((component.to_string(), at)),
            None => {
                r.now
                    .components
                    .insert(id.to_string(), vec![(component.to_string(), at)]);
            }
        }
        got
    }

    pub fn tag(&mut self, id: &str, tags: &[&str]) {
        self.wrote();
        call(crate::func!(f::TAG), |w| {
            w.str(id).strs(tags);
        });
    }

    pub fn untag(&mut self, id: &str, tags: &[&str]) {
        self.wrote();
        call(crate::func!(f::UNTAG), |w| {
            w.str(id).strs(tags);
        });
    }

    pub fn transform(&self, id: &str) -> Option<Pos> {
        // The entity's row of the zone (common memory): a few loads, no call.
        if let Some([x, y, z, yaw]) = crate::columns::pose(id) {
            return Some(Pos { x, y, z, yaw });
        }
        self.sync();
        if let Some(p) = self.current(id) {
            // SAFETY: a live box of `kept`.
            return Pos::of(unsafe { (*p).entity.component("transform") });
        }
        {
            let r = self.reads.borrow();
            if let Some(hit) = r.now.component(id, "transform") {
                // SAFETY: as `keep_list`.
                return hit.and_then(|p| Pos::of(Some(unsafe { &*p })));
            }
            if r.now.absent.contains_key(id) {
                return None;
            }
        }
        if let Some(t) = crate::direct::transform(id) {
            return t.map(|[x, y, z, yaw]| Pos { x, y, z, yaw });
        }
        let bytes = call(crate::func!(f::TRANSFORM), |w| {
            w.str(id);
        });
        let mut r = Reader::new(&bytes);
        if !r.bool().unwrap_or(false) {
            return None;
        }
        Some(Pos {
            x: r.f64().ok()?,
            y: r.f64().ok()?,
            z: r.f64().ok()?,
            yaw: r.f64().ok()?,
        })
    }

    /// `setTransform(id, partial)`: merges into the current transform.
    pub fn set_transform(&mut self, id: &str, patch: &Json) {
        self.wrote();
        // A patch of numbers among x, y, z, yaw, as the JS model writes them: the typed write (no value crosses).
        if let Some((mask, fields)) = pose_patch(patch) {
            if self.write_pose(id, mask, fields) {
                return;
            }
        }
        call(crate::func!(f::SET_TRANSFORM), |w| {
            w.str(id).value(patch);
        });
    }

    /// The typed write of a transform by a direct link; false: no link carried it (the caller makes the call).
    fn write_pose(&mut self, id: &str, mask: u32, fields: [f64; 4]) -> bool {
        let epoch = self.reads.get_mut().epoch;
        match hot::pose(id, mask, fields, epoch) {
            Some(Ok(Some(answer))) => {
                let on = |i: usize| (mask & 1 << i != 0).then_some(fields[i]);
                let set = [on(0), on(1), on(2), on(3)];
                self.wrote_own(id, answer, |e| {
                    e.components
                        .iter_mut()
                        .find(|(k, _)| k == "transform")
                        .is_some_and(|(_, t)| pose_into(t, set).is_some())
                });
                self.reads.get_mut().now.components.remove(id);
                true
            }
            Some(Ok(None)) => true,
            // Refused: the call answers the reason (the module stops with it).
            Some(Err(())) => false,
            None => hot::set_pose(id, mask, fields),
        }
    }

    /// Typed write of the transform (the same method as the core's `Entities::set_pose`): the fields given, with
    /// no value built, encoded or copied — a direct link; the bridge for an id that does not pack.
    pub fn set_pose(
        &mut self,
        id: &str,
        x: Option<f64>,
        y: Option<f64>,
        z: Option<f64>,
        yaw: Option<f64>,
    ) {
        self.wrote();
        let all = [x, y, z, yaw];
        let mask = all
            .iter()
            .enumerate()
            .fold(0, |m, (i, v)| m | (v.is_some() as u32) << i);
        if self.write_pose(id, mask, all.map(|v| v.unwrap_or(0.0))) {
            return;
        }
        let mut patch = Map::new();
        for (k, v) in ["x", "y", "z", "yaw"].into_iter().zip(all) {
            if let Some(v) = v {
                patch.insert(k.to_string(), crate::js::num(v));
            }
        }
        call(crate::func!(f::SET_TRANSFORM), |w| {
            w.str(id).value(&Json::Object(patch));
        });
    }

    /// Whether the entity `id` exists and has the tag (the core's `Entities::tagged`).
    pub fn tagged(&self, id: &str, tag: &str) -> bool {
        self.get(id).is_some_and(|e| e.has_tag(tag))
    }

    pub fn next_id(&self) -> u64 {
        let bytes = call(crate::func!(f::NEXT_ID), |_| {});
        Reader::new(&bytes).u64().unwrap_or(0)
    }

    pub fn serialize(&self) -> Json {
        let bytes = call(crate::func!(f::SERIALIZE), |_| {});
        Reader::new(&bytes).value().unwrap_or(Json::Null)
    }

    /// The guard of `get_mut` wrote its changes and the core says nothing else changed in the world meanwhile:
    /// the world's new epoch stands for what was read, less what those changes touch (components only: a change of
    /// tags moves the structure, read again with the epoch).
    fn wrote_alone(&mut self, id: &str, epoch: u64) {
        let r = self.reads.get_mut();
        r.epoch = epoch;
        r.now.components.remove(id);
        r.now.all = None;
    }
}

/// The fields of a `set_transform` patch when the typed write stores exactly what the patch would: an object whose
/// keys are among x, y, z, yaw, in that order, each a number as `js::num` writes it (an integral value as an
/// integer) or null. Anything else (another key, a float that holds an integer, a string) is `None`: the value
/// goes to the core as it is.
fn pose_patch(patch: &Json) -> Option<(u32, [f64; 4])> {
    let o = patch.as_object()?;
    let (mut mask, mut fields, mut next) = (0u32, [0f64; 4], 0usize);
    for (k, v) in o {
        let i = ["x", "y", "z", "yaw"].iter().position(|n| n == k)?;
        if i < next {
            return None;
        }
        next = i + 1;
        fields[i] = match v {
            Json::Null => f64::NAN,
            Json::Number(n) => {
                let f = n.as_f64()?;
                if crate::js::num(f) != *v {
                    return None;
                }
                f
            }
            _ => return None,
        };
        mask |= 1 << i;
    }
    (mask != 0).then_some((mask, fields))
}

/// What `get_mut` hands: the entity, read in the copy this module keeps until it is written to — a kit that only
/// reads through the guard copies nothing. At the first `DerefMut` the entity is copied; what then differs is
/// written back to the core on drop (`PATCH`: the components set, the tags; the whole entity when a list of changes
/// cannot say it).
pub struct EntityMut<'a> {
    /// The copy the kit writes in (`None`: not written to yet, read in `kept`).
    entity: Option<Entity>,
    /// The kept copy this one reads and is made from (nothing else reads, changes or frees it while the guard
    /// borrows the entities).
    kept: *mut Kept,
    of: *mut Entities,
    _of: std::marker::PhantomData<&'a mut Entities>,
}

impl std::ops::Deref for EntityMut<'_> {
    type Target = Entity;
    fn deref(&self) -> &Entity {
        match &self.entity {
            Some(e) => e,
            // SAFETY: a live box of `kept`, borrowed with the entities for `'a`.
            None => unsafe { &(*self.kept).entity },
        }
    }
}

impl std::ops::DerefMut for EntityMut<'_> {
    fn deref_mut(&mut self) -> &mut Entity {
        // SAFETY: as `deref`.
        self.entity
            .get_or_insert_with(|| unsafe { (*self.kept).entity.clone() })
    }
}

impl Drop for EntityMut<'_> {
    fn drop(&mut self) {
        let Some(mut entity) = self.entity.take() else {
            return;
        };
        // SAFETY: the guard borrows the entities for `'a`: `of` and the kept copy are alive and nothing else uses
        // them.
        let (of, kept) = unsafe { (&mut *self.of, &mut *self.kept) };
        let Some((tags, set)) = hot::changes(&kept.entity, &entity) else {
            let v = entity_value(&entity);
            call(crate::func!(f::PUT), |w| {
                w.str(&kept.entity.id).value(&v);
            });
            return;
        };
        if !tags && set.is_empty() {
            return;
        }
        let epoch = of.reads.get_mut().epoch;
        let bytes = call(crate::func!(fh::PATCH), |w| {
            w.str(&entity.id).u64(kept.rev).u64(epoch).bool(tags);
            if tags {
                w.strs(&entity.tags);
            }
            w.u32(set.len() as u32);
            for (k, v) in set {
                w.str(k).value(v);
            }
        });
        let mut r = Reader::new(&bytes);
        let (rev, epoch) = (r.u64().unwrap_or(0), r.u64().unwrap_or(0));
        if rev != 0 {
            // The copy was current: with the changes it is the entity as the core has it now.
            std::mem::swap(&mut kept.entity, &mut entity);
            kept.rev = rev;
            Entities::stamp(kept);
            if epoch != 0 && !tags {
                of.wrote_alone(&kept.entity.id, epoch);
            }
        }
    }
}

/// `host.player_scopes`: the co-op player scopes live in the core (`acting_player` reads them there).
#[derive(Default)]
pub struct PlayerScopes {}

impl PlayerScopes {
    pub fn push(&mut self, id: String) {
        if crate::direct::scope_push(&id) {
            return;
        }
        call(crate::func!(f::SCOPE_PUSH), |w| {
            w.str(&id);
        });
    }
    pub fn pop(&mut self) -> Option<String> {
        if let Some(id) = crate::direct::scope_pop() {
            return id;
        }
        let bytes = call(crate::func!(f::SCOPE_POP), |_| {});
        Reader::new(&bytes)
            .opt_str()
            .ok()
            .flatten()
            .map(str::to_string)
    }
}

impl Host {
    /// Drops every read the façades keep (after a call that may run other kits: anything may have changed).
    #[inline]
    pub(crate) fn reads_stale(&mut self) {
        // Each façade drops what it read when it is next read (`host::moved`): nothing is walked here.
        crate::host::moved();
    }
}

/// The neighbour search by the direct link `w_near` (layout: gripforge-kitcall src/near.rs).
mod near_link {
    use super::Cond;

    #[cfg(feature = "common-memory")]
    pub(super) fn ask(x: f64, z: f64, radius: f64, conds: &[Cond]) -> Option<Vec<(String, f64)>> {
        use gripforge_abi::near as n;
        use std::cell::RefCell;
        #[cfg(target_arch = "wasm32")]
        #[link(wasm_import_module = "gf:kithost.world@^1.3")]
        extern "C" {
            fn w_near(spec: *const u64, out: *mut u64, cap: u32) -> u32;
        }
        #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
        use crate::native::link::w_near;
        thread_local! {
            /// The result buffer: one place, kept from one search to the next.
            static OUT: RefCell<Vec<u64>> = RefCell::new(vec![0; 64 * n::HIT]);
        }
        if conds.len() > n::MAX_CONDS {
            return None;
        }
        let atom = |s: &str| crate::direct::atom(s) as u64;
        let mut spec = [0u64; n::HEAD + n::COND * n::MAX_CONDS];
        spec[0] = x.to_bits();
        spec[1] = z.to_bits();
        spec[2] = radius.to_bits();
        spec[3] = conds.len() as u64;
        let (rows, _) = spec[n::HEAD..].as_chunks_mut::<{ n::COND }>();
        for (c, w) in conds.iter().zip(rows) {
            let (kind, a, b, v) = match *c {
                Cond::Above {
                    component,
                    field,
                    min,
                } => (n::ABOVE, component, field, min),
                Cond::Number { component, field } => (n::NUMBER, component, field, 0.0),
                Cond::Text { component } => (n::TEXT, component, "", 0.0),
                Cond::TextNot { component, value } => (n::TEXT_NOT, component, value, 0.0),
                Cond::Tag(t) => (n::TAG, t, "", 0.0),
                Cond::NoTag(t) => (n::NO_TAG, t, "", 0.0),
            };
            w[0] = kind as u64 | atom(a) << 32;
            w[1] = atom(b);
            w[2] = v.to_bits();
        }
        OUT.with(|o| {
            let mut o = o.borrow_mut();
            // SAFETY: an import wired by the calls host to the export `gfx_w_near` of the core, which reads the
            // question on this module's stack and writes at most `cap` records in this module's buffer — in the
            // memory both share.
            let mut found =
                unsafe { w_near(spec.as_ptr(), o.as_mut_ptr(), (o.len() / n::HIT) as u32) };
            if found != n::UNANSWERED && found as usize * n::HIT > o.len() {
                o.resize(found as usize * n::HIT, 0);
                found = unsafe { w_near(spec.as_ptr(), o.as_mut_ptr(), (o.len() / n::HIT) as u32) };
            }
            if found == n::UNANSWERED || found as usize * n::HIT > o.len() {
                return None;
            }
            let (hits, _) = o.as_chunks::<{ n::HIT }>();
            let near = |h: &[u64; n::HIT]| (id_of(h[0] as u32), f64::from_bits(h[1]));
            Some(hits.iter().take(found as usize).map(near).collect())
        })
    }

    #[cfg(not(feature = "common-memory"))]
    pub(super) fn ask(_: f64, _: f64, _: f64, _: &[Cond]) -> Option<Vec<(String, f64)>> {
        None
    }

    /// `e<number>`: the id a row's number stands for (`rows::id_number`, the other way).
    #[cfg_attr(not(feature = "common-memory"), allow(dead_code))]
    pub(super) fn id_of(number: u32) -> String {
        let (mut digits, mut at, mut n) = ([0u8; 10], 10, number);
        loop {
            at -= 1;
            digits[at] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        let mut id = String::with_capacity(11 - at);
        id.push('e');
        // The digits are ASCII.
        id.push_str(std::str::from_utf8(&digits[at..]).unwrap_or(""));
        id
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn an_id_from_its_number() {
            for n in [1u32, 9, 10, 12, 4096, u32::MAX - 1] {
                let id = super::id_of(n);
                assert_eq!(id, format!("e{n}"));
                assert_eq!(gripforge_abi::rows::id_number(&id), Some(n));
            }
        }
    }
}
