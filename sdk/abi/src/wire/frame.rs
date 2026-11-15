//! The delta frame of a shared memory region (docs/wire-format.md « Frame »), v1. Little-endian.
//!
//! Header, 48 bytes, fixed:
//! `0 magic u32 "GFW1"`, `4 version u16`, `6 flags u16` (bit 0: keyframe — the reader drops what it kept),
//! `8 seq u32` (odd while the writer writes: a reader of a SharedArrayBuffer retries), `12 length u32` (header
//! included), `16 tick u64`, `24 elapsed f64`, `32 renderTime f64`, `40 entities u32` (alive), `44 sections u32`.
//!
//! Then the sections, each `kind u16, 0 u16, length u32`, its body padded to 4 bytes:
//! - 1 STRINGS: n, then n strings (the next ids of the stream's table, from 0 at a keyframe);
//! - 2 SPAWN: n × (slot, id ref, kind ref, tag count, tag refs) — slot: the entity's index in the region;
//! - 3 DESPAWN: n × slot;
//! - 4 TRANSFORM: u32 n, then n × `{ slot u32, x f32, y f32, z f32, yaw f32 }` (20 bytes, aligned: read in place);
//! - 5 VELOCITY: u32 n, then n × `{ slot u32, vx f32, vy f32, vz f32 }` (16 bytes);
//! - 6 TAGS: n × (slot, count, refs) — the whole list when it changed;
//! - 7 COMPONENT: n × (slot, name ref, 0 removed | 1 + value);
//! - 8 VARS: n × (path ref, 0 unset | 1 + value);
//! - 9 EVENTS: n × (topic ref, seq, tick, time f64, source ref, entity ref + 1 or 0, actor ref + 1 or 0, payload value);
//! - 10 VIEWS: n × (name ref, value) — the views that changed;
//! - 11 STATE: n × (key ref, value) — `phase`, `hudVars` when they changed;
//! - 12 PORTS: n × (port ref, value) — the calls of the browser ports (`audio`, `display`, `pointer`, `saves`).
//!
//! Counts, slots and refs are LEB128 varints but in TRANSFORM and VELOCITY; values are `value.rs` (tags 0-9).
//! A transform is sent when one of its four floats changed (as f32), a component when its encoded bytes changed.

use crate::wire::{put_uvar, Cursor};
use std::collections::HashMap;

pub const MAGIC: u32 = u32::from_le_bytes(*b"GFW1");
pub const VERSION: u16 = 1;
pub const HEADER: usize = 48;
pub const KEYFRAME: u16 = 1;

pub const STRINGS: u16 = 1;
pub const SPAWN: u16 = 2;
pub const DESPAWN: u16 = 3;
pub const TRANSFORM: u16 = 4;
pub const VELOCITY: u16 = 5;
pub const TAGS: u16 = 6;
pub const COMPONENT: u16 = 7;
pub const VARS: u16 = 8;
pub const EVENTS: u16 = 9;
pub const VIEWS: u16 = 10;
pub const STATE: u16 = 11;
pub const PORTS: u16 = 12;

// Values may be arbitrarily large or unique (serialized saves, thumbnails, chat).
// Interning all of them retained every historical value on both sides of the stream.
const VALUE_STRING_MAX: usize = 256;
const TABLE_BYTES: usize = 1024 * 1024;
const TABLE_ENTRIES: usize = 16 * 1024;

/// The string table of a stream: append-only between two keyframes.
#[derive(Default)]
pub struct Strings {
    ids: HashMap<String, u32>,
    pub list: Vec<String>,
    /// The first id not sent yet.
    sent: usize,
    bytes: usize,
}

impl Strings {
    pub fn id(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.ids.get(s) {
            return id;
        }
        let id = self.list.len() as u32;
        self.ids.insert(s.to_string(), id);
        self.list.push(s.to_string());
        self.bytes = self.bytes.saturating_add(s.len());
        id
    }

    /// Reuse existing refs, but keep large or continually changing values inline.
    fn value_id(&mut self, s: &str) -> Option<u32> {
        if let Some(&id) = self.ids.get(s) {
            return Some(id);
        }
        if s.len() > VALUE_STRING_MAX
            || self.list.len() >= TABLE_ENTRIES
            || self.bytes.saturating_add(s.len()) > TABLE_BYTES
        {
            return None;
        }
        Some(self.id(s))
    }
}

#[derive(Default, Clone)]
struct Kept {
    transform: Option<[f32; 4]>,
    velocity: Option<[f32; 3]>,
    tags: Vec<u32>,
    /// name ref → the encoded value last sent.
    components: Vec<(u32, Vec<u8>)>,
    seen: u64,
    /// The tags and the components as they were given last frame, in order: a frame that gives the same again is
    /// compared with these and nothing is encoded (`same`).
    #[cfg(feature = "json")]
    tag_names: Vec<String>,
    #[cfg(feature = "json")]
    given: Vec<(String, serde_json::Value)>,
}

/// The same value, written the same way: objects key by key in their order, floats bit for bit. Two values that are
/// `same` encode to the same bytes — what the writer compares in place of encoding every value of every frame.
#[cfg(feature = "json")]
pub fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value::*;
    match (a, b) {
        (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Number(x), Number(y)) => {
            x == y
                && x.is_f64() == y.is_f64()
                && x.as_f64().map(f64::to_bits) == y.as_f64().map(f64::to_bits)
        }
        (String(x), String(y)) => x == y,
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
        (Object(x), Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((kx, x), (ky, y))| kx == ky && same(x, y))
        }
        _ => false,
    }
}

/// One section being written: its count and body.
#[derive(Default)]
struct Section {
    n: u32,
    body: Vec<u8>,
}

/// Writes the frames of one stream. The caller hands the state of this frame (entities, vars, events…); the writer
/// keeps what it sent and writes only what changed.
#[derive(Default)]
pub struct FrameWriter {
    pub strings: Strings,
    /// entity id → slot.
    slots: HashMap<String, u32>,
    kept: Vec<Option<Kept>>,
    free: Vec<u32>,
    vars: HashMap<u32, Vec<u8>>,
    state: HashMap<u32, Vec<u8>>,
    frames: u64,
    keyframe: bool,
    /// The table required by the current keyframe. A large live world must not
    /// trigger another compaction every frame merely because it exceeds the floor.
    dictionary_base: (usize, usize),
    // This frame.
    spawn: Section,
    despawn: Section,
    transform: Section,
    velocity: Section,
    tags: Section,
    component: Section,
    var: Section,
    events: Section,
    views: Section,
    st: Section,
    ports: Section,
    scratch: Vec<u8>,
    alive: u32,
    /// What was given last frame, in the order it was given, and where this frame is in it: the entity ids with
    /// their slot, the vars, the state. A caller that hands the same world in the same order (a game at rest) is
    /// answered by comparisons in place — no string is hashed, no value encoded.
    #[cfg(feature = "json")]
    order: Vec<(String, u32)>,
    order_at: usize,
    #[cfg(feature = "json")]
    vars_given: Vec<(String, Option<serde_json::Value>)>,
    vars_at: usize,
    #[cfg(feature = "json")]
    state_given: Vec<(String, serde_json::Value)>,
}

#[cfg(feature = "json")]
impl FrameWriter {
    pub fn new() -> FrameWriter {
        FrameWriter {
            keyframe: true,
            ..Default::default()
        }
    }

    /// The next frame is a keyframe: everything is sent again, the reader starts over.
    pub fn reset(&mut self) {
        *self = FrameWriter::new();
    }

    /// Structural names (entity ids, topics) require refs. When churn grows their
    /// table, the caller must reset between frames and resend all cached views/state.
    pub fn needs_reset(&self) -> bool {
        self.strings.list.len() > TABLE_ENTRIES.max(self.dictionary_base.0.saturating_mul(2))
            || self.strings.bytes > TABLE_BYTES.max(self.dictionary_base.1.saturating_mul(2))
    }

    /// Encodes `v` in `self.scratch` (no allocation once warm); the caller compares and copies when it changed.
    fn encode(&mut self, v: &serde_json::Value) {
        self.scratch.clear();
        let strings = &mut self.strings;
        crate::wire::value::encode_with(&mut self.scratch, v, &mut |s| strings.value_id(s));
    }

    fn value(&mut self, v: &serde_json::Value) -> Vec<u8> {
        self.encode(v);
        self.scratch.clone()
    }

    /// Starts a frame.
    pub fn begin(&mut self) {
        self.frames += 1;
        self.alive = 0;
        self.order_at = 0;
        self.vars_at = 0;
    }

    /// The slot of entity `id` when it comes where it came last frame (no hash of its id); else `None`.
    fn slot_in_order(&mut self, id: &str) -> Option<u32> {
        let at = self.order_at;
        self.order_at += 1;
        match self.order.get(at) {
            Some((was, slot)) if was == id => Some(*slot),
            _ => None,
        }
    }

    /// An entity alive this frame that the caller knows unchanged since the last frame (same place in the order,
    /// same drawn transform, velocity, tags and components): only marked seen, nothing compared nor encoded.
    /// False when it is not where it was last frame: the caller then gives it whole ([`FrameWriter::entity`]).
    pub fn same(&mut self, id: &str) -> bool {
        let at = self.order_at;
        let Some((was, slot)) = self.order.get(at) else {
            return false;
        };
        if was != id {
            return false;
        }
        let Some(kept) = self.kept.get_mut(*slot as usize).and_then(Option::as_mut) else {
            return false;
        };
        kept.seen = self.frames;
        self.order_at += 1;
        self.alive += 1;
        true
    }

    /// An entity alive this frame, with its drawn transform, velocity, tags and the components the readers want.
    pub fn entity<'a>(
        &mut self,
        id: &str,
        kind: &str,
        tags: &[String],
        transform: [f64; 4],
        velocity: Option<[f64; 3]>,
        components: impl Iterator<Item = (&'a str, &'a serde_json::Value)>,
    ) {
        self.alive += 1;
        let at = self.order_at;
        let known = self.slot_in_order(id);
        // The same tags as last frame: nothing to turn into refs.
        let same_tags = known
            .and_then(|s| self.kept[s as usize].as_ref())
            .is_some_and(|k| k.tag_names == tags);
        let tag_ids: Vec<u32> = if same_tags {
            Vec::new()
        } else {
            tags.iter().map(|t| self.strings.id(t)).collect()
        };
        let slot = match known.or_else(|| self.slots.get(id).copied()) {
            Some(s) => s,
            None => {
                let s = self.free.pop().unwrap_or(self.kept.len() as u32);
                if s as usize == self.kept.len() {
                    self.kept.push(None);
                }
                self.slots.insert(id.to_string(), s);
                let (idr, kr) = (self.strings.id(id), self.strings.id(kind));
                let b = &mut self.spawn.body;
                put_uvar(b, s as u64);
                put_uvar(b, idr as u64);
                put_uvar(b, kr as u64);
                put_uvar(b, tag_ids.len() as u64);
                for t in &tag_ids {
                    put_uvar(b, *t as u64);
                }
                self.spawn.n += 1;
                self.kept[s as usize] = Some(Kept {
                    tags: tag_ids.clone(),
                    tag_names: tags.to_vec(),
                    ..Kept::default()
                });
                s
            }
        };
        if known.is_none() {
            // Not where it was: the order of this frame is written from here on.
            self.order.truncate(at);
            self.order.push((id.to_string(), slot));
        }
        let frames = self.frames;
        let mut kept = self.kept[slot as usize].take().unwrap_or_default();
        kept.seen = frames;
        let t = transform.map(|v| v as f32);
        if kept.transform != Some(t) {
            kept.transform = Some(t);
            let b = &mut self.transform.body;
            b.extend_from_slice(&slot.to_le_bytes());
            for v in t {
                b.extend_from_slice(&v.to_le_bytes());
            }
            self.transform.n += 1;
        }
        let v = velocity.map(|v| v.map(|x| x as f32));
        if let Some(v) = v.filter(|v| kept.velocity != Some(*v)) {
            kept.velocity = Some(v);
            let b = &mut self.velocity.body;
            b.extend_from_slice(&slot.to_le_bytes());
            for x in v {
                b.extend_from_slice(&x.to_le_bytes());
            }
            self.velocity.n += 1;
        }
        if !same_tags && kept.tags != tag_ids {
            let b = &mut self.tags.body;
            put_uvar(b, slot as u64);
            put_uvar(b, tag_ids.len() as u64);
            for t in &tag_ids {
                put_uvar(b, *t as u64);
            }
            self.tags.n += 1;
            kept.tags = tag_ids;
        }
        if !same_tags {
            kept.tag_names = tags.to_vec();
        }
        // The components given as last frame, in the same order, are compared in place; from the first one that
        // differs the frame goes the long way (names to refs, values to bytes) and what was given is written again.
        let mut present: Vec<u32> = Vec::new();
        let mut matched = 0usize;
        let mut steady = true;
        let mut given: Vec<(String, serde_json::Value)> = Vec::new();
        for (name, value) in components {
            if steady {
                if kept
                    .given
                    .get(matched)
                    .is_some_and(|(n, v)| n == name && same(v, value))
                {
                    matched += 1;
                    continue;
                }
                steady = false;
                for (n, _) in &kept.given[..matched] {
                    present.push(self.strings.id(n));
                }
                given.extend(kept.given[..matched].iter().cloned());
            }
            given.push((name.to_string(), value.clone()));
            let nr = self.strings.id(name);
            present.push(nr);
            self.encode(value);
            let bytes = &self.scratch;
            match kept.components.iter_mut().find(|(n, _)| *n == nr) {
                Some((_, last)) if last == bytes => continue,
                Some((_, last)) => {
                    last.clear();
                    last.extend_from_slice(bytes);
                }
                None => kept.components.push((nr, bytes.clone())),
            }
            let b = &mut self.component.body;
            put_uvar(b, slot as u64);
            put_uvar(b, nr as u64);
            b.push(1);
            b.extend_from_slice(&self.scratch);
            self.component.n += 1;
        }
        if steady && matched == kept.given.len() {
            // Everything as it was: nothing left, nothing to write.
            self.kept[slot as usize] = Some(kept);
            return;
        }
        if steady {
            // The first ones as they were, the others gone.
            for (n, _) in &kept.given[..matched] {
                present.push(self.strings.id(n));
            }
            kept.given.truncate(matched);
        } else {
            kept.given = given;
        }
        let gone: Vec<u32> = kept
            .components
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !present.contains(n))
            .collect();
        for nr in gone {
            kept.components.retain(|(n, _)| *n != nr);
            let b = &mut self.component.body;
            put_uvar(b, slot as u64);
            put_uvar(b, nr as u64);
            b.push(0);
            self.component.n += 1;
        }
        self.kept[slot as usize] = Some(kept);
    }

    /// A var this frame (`None`: unset). Call it for every var, each frame: the vars not named are unset.
    pub fn var(&mut self, path: &str, value: Option<&serde_json::Value>) {
        // Given as last frame, where it was: compared in place.
        let at = self.vars_at;
        self.vars_at += 1;
        match self.vars_given.get_mut(at) {
            Some((was, last)) if was == path => {
                let unchanged = match (last.as_ref(), value) {
                    (Some(a), Some(b)) => same(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if unchanged {
                    return;
                }
                *last = value.cloned();
            }
            _ => {
                self.vars_given.truncate(at);
                self.vars_given.push((path.to_string(), value.cloned()));
            }
        }
        let pr = self.strings.id(path);
        match value {
            Some(v) => {
                self.encode(v);
                match self.vars.get_mut(&pr) {
                    Some(last) if *last == self.scratch => return,
                    Some(last) => {
                        last.clear();
                        last.extend_from_slice(&self.scratch);
                    }
                    None => {
                        self.vars.insert(pr, self.scratch.clone());
                    }
                }
                let b = &mut self.var.body;
                put_uvar(b, pr as u64);
                b.push(1);
                b.extend_from_slice(&self.scratch);
            }
            None => {
                if self.vars.remove(&pr).is_none() {
                    return;
                }
                put_uvar(&mut self.var.body, pr as u64);
                self.var.body.push(0);
            }
        }
        self.var.n += 1;
    }

    /// The caller knows no var changed since the last frame and gives none: those given last frame stay « as last
    /// frame » (nothing compared nor encoded).
    pub fn vars_kept(&mut self) {
        self.vars_at = self.vars_given.len();
    }

    /// `phase`, `hudVars`…: sent when it changed.
    pub fn state(&mut self, key: &str, value: &serde_json::Value) {
        match self.state_given.iter_mut().find(|(k, _)| k == key) {
            Some((_, last)) if same(last, value) => return,
            Some((_, last)) => *last = value.clone(),
            None => self.state_given.push((key.to_string(), value.clone())),
        }
        let kr = self.strings.id(key);
        self.encode(value);
        if self.state.get(&kr) == Some(&self.scratch) {
            return;
        }
        let bytes = self.scratch.clone();
        put_uvar(&mut self.st.body, kr as u64);
        self.st.body.extend_from_slice(&bytes);
        self.state.insert(kr, bytes);
        self.st.n += 1;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn event(
        &mut self,
        topic: &str,
        seq: u64,
        tick: u64,
        time: f64,
        source: &str,
        entity: Option<&str>,
        actor: Option<&str>,
        payload: &serde_json::Value,
    ) {
        let (tr, sr) = (self.strings.id(topic), self.strings.id(source));
        let er = entity.map(|e| self.strings.id(e) as u64 + 1).unwrap_or(0);
        let ar = actor.map(|a| self.strings.id(a) as u64 + 1).unwrap_or(0);
        let bytes = self.value(payload);
        let b = &mut self.events.body;
        put_uvar(b, tr as u64);
        put_uvar(b, seq);
        put_uvar(b, tick);
        b.extend_from_slice(&time.to_le_bytes());
        put_uvar(b, sr as u64);
        put_uvar(b, er);
        put_uvar(b, ar);
        b.extend_from_slice(&bytes);
        self.events.n += 1;
    }

    /// A view that changed (the presentation feed already sends only those).
    pub fn view(&mut self, name: &str, value: &serde_json::Value) {
        let nr = self.strings.id(name);
        let bytes = self.value(value);
        put_uvar(&mut self.views.body, nr as u64);
        self.views.body.extend_from_slice(&bytes);
        self.views.n += 1;
    }

    /// The calls of a browser port this frame (`audio`, `display`, `pointer`, `saves`).
    pub fn port(&mut self, port: &str, calls: &serde_json::Value) {
        let pr = self.strings.id(port);
        let bytes = self.value(calls);
        put_uvar(&mut self.ports.body, pr as u64);
        self.ports.body.extend_from_slice(&bytes);
        self.ports.n += 1;
    }

    /// Ends the frame: the entities not named this frame are despawned; writes the region in `out` (cleared).
    pub fn finish(
        &mut self,
        out: &mut Vec<u8>,
        seq: u32,
        tick: u64,
        elapsed: f64,
        render_time: f64,
    ) {
        // What was not given this frame is no longer « as last frame » (a slot freed below may be given again).
        self.order.truncate(self.order_at);
        self.vars_given.truncate(self.vars_at);
        let frames = self.frames;
        let mut dead: Vec<(String, u32)> = Vec::new();
        // Every entity known was given this frame (each once): none left, the table is not walked.
        if self.alive as usize != self.slots.len() {
            for (id, &slot) in &self.slots {
                if self.kept[slot as usize]
                    .as_ref()
                    .is_none_or(|k| k.seen != frames)
                {
                    dead.push((id.clone(), slot));
                }
            }
        }
        dead.sort_by_key(|d| d.1);
        for (id, slot) in dead {
            self.slots.remove(&id);
            self.kept[slot as usize] = None;
            self.free.push(slot);
            put_uvar(&mut self.despawn.body, slot as u64);
            self.despawn.n += 1;
        }
        out.clear();
        out.resize(HEADER, 0);
        let mut sections = 0u32;
        // New strings first: everything after refers to them.
        let mut strings = Section::default();
        for s in &self.strings.list[self.strings.sent..] {
            crate::wire::put_str(&mut strings.body, s);
            strings.n += 1;
        }
        self.strings.sent = self.strings.list.len();
        let fixed = |s: &mut Section| {
            // TRANSFORM and VELOCITY: a u32 count, records aligned on 4.
            // Written in front of the records where they are (the buffer keeps its room from frame to frame: nothing
            // allocated per frame once it is large enough).
            s.body.splice(0..0, s.n.to_le_bytes());
        };
        fixed(&mut self.transform);
        fixed(&mut self.velocity);
        let list: [(u16, &mut Section, bool); 12] = [
            (STRINGS, &mut strings, true),
            (SPAWN, &mut self.spawn, true),
            (DESPAWN, &mut self.despawn, true),
            (TRANSFORM, &mut self.transform, false),
            (VELOCITY, &mut self.velocity, false),
            (TAGS, &mut self.tags, true),
            (COMPONENT, &mut self.component, true),
            (VARS, &mut self.var, true),
            (EVENTS, &mut self.events, true),
            (VIEWS, &mut self.views, true),
            (STATE, &mut self.st, true),
            (PORTS, &mut self.ports, true),
        ];
        for (kind, s, counted) in list {
            if s.n > 0 {
                let mut head = Vec::new();
                if counted {
                    put_uvar(&mut head, s.n as u64);
                }
                let len = head.len() + s.body.len();
                out.extend_from_slice(&kind.to_le_bytes());
                out.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&(len as u32).to_le_bytes());
                out.extend_from_slice(&head);
                out.extend_from_slice(&s.body);
                while !out.len().is_multiple_of(4) {
                    out.push(0);
                }
                sections += 1;
            }
            s.n = 0;
            s.body.clear();
        }
        let flags = if std::mem::take(&mut self.keyframe) {
            self.dictionary_base = (self.strings.list.len(), self.strings.bytes);
            KEYFRAME
        } else {
            0
        };
        let len = out.len() as u32;
        let h = &mut out[..HEADER];
        h[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        h[4..6].copy_from_slice(&VERSION.to_le_bytes());
        h[6..8].copy_from_slice(&flags.to_le_bytes());
        h[8..12].copy_from_slice(&seq.to_le_bytes());
        h[12..16].copy_from_slice(&len.to_le_bytes());
        h[16..24].copy_from_slice(&tick.to_le_bytes());
        h[24..32].copy_from_slice(&elapsed.to_le_bytes());
        h[32..40].copy_from_slice(&render_time.to_le_bytes());
        h[40..44].copy_from_slice(&self.alive.to_le_bytes());
        h[44..48].copy_from_slice(&sections.to_le_bytes());
    }
}

// ─── the reader ─────────────────────────────────────────────────────────────

/// An entity as the reader keeps it.
#[derive(Clone, Debug, Default)]
pub struct MirrorEntity {
    pub id: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub transform: [f32; 4],
    pub velocity: Option<[f32; 3]>,
    #[cfg(feature = "json")]
    pub components: Vec<(String, serde_json::Value)>,
}

/// What a reader keeps of a stream: the frames applied one after the other give the writer's state.
#[derive(Default)]
pub struct Mirror {
    pub strings: Vec<String>,
    pub entities: Vec<Option<MirrorEntity>>,
    pub tick: u64,
    pub elapsed: f64,
    pub render_time: f64,
    #[cfg(feature = "json")]
    pub vars: Vec<(String, serde_json::Value)>,
    #[cfg(feature = "json")]
    pub state: Vec<(String, serde_json::Value)>,
    /// Of the last frame only: (topic, seq, tick, time, source, entity, actor, payload).
    #[cfg(feature = "json")]
    pub events: Vec<serde_json::Value>,
    #[cfg(feature = "json")]
    pub views: Vec<(String, serde_json::Value)>,
    #[cfg(feature = "json")]
    pub ports: Vec<(String, serde_json::Value)>,
    /// Of the last frame only: whether it was a keyframe (the reader started over), its `seq`, and the slots it
    /// spawned, despawned or changed (in section order, with repeats) — a reader that keeps its own copy of the
    /// entities updates just those.
    pub keyframe: bool,
    pub seq: u32,
    pub touched: Vec<usize>,
    /// The slots of `touched` changed by more than their transform or velocity (spawned, despawned, tags,
    /// components): a reader that keeps its own copy rebuilds just those, and only moves the others.
    pub touched_whole: Vec<usize>,
    /// Of the last frame only: the paths of the vars and the keys of the state it set or unset — a reader that keeps
    /// its own copy patches just those.
    pub changed_vars: Vec<String>,
    pub changed_state: Vec<String>,
}

#[cfg(feature = "json")]
impl Mirror {
    /// Applies one frame; an error names what was malformed (the mirror is then to be dropped).
    pub fn apply(&mut self, bytes: &[u8]) -> Result<(), String> {
        use crate::wire::value::decode;
        let mut c = Cursor::new(bytes);
        let bad = |what: &str| format!("wire frame: bad {what}");
        if c.u32().ok_or_else(|| bad("header"))? != MAGIC {
            return Err(bad("magic"));
        }
        let version =
            u16::from_le_bytes(c.take(2).ok_or_else(|| bad("header"))?.try_into().unwrap());
        if version != VERSION {
            return Err(format!("wire frame: version {version}"));
        }
        let flags = u16::from_le_bytes(c.take(2).ok_or_else(|| bad("header"))?.try_into().unwrap());
        let keyframe = flags & KEYFRAME != 0;
        if keyframe {
            *self = Mirror::default();
        }
        self.keyframe = keyframe;
        self.touched.clear();
        self.touched_whole.clear();
        self.changed_vars.clear();
        self.changed_state.clear();
        self.seq = c.u32().ok_or_else(|| bad("header"))?;
        let len = c.u32().ok_or_else(|| bad("header"))? as usize;
        self.tick = u64::from_le_bytes(c.take(8).ok_or_else(|| bad("header"))?.try_into().unwrap());
        self.elapsed = c.f64().ok_or_else(|| bad("header"))?;
        self.render_time = c.f64().ok_or_else(|| bad("header"))?;
        let _alive = c.u32();
        let sections = c.u32().ok_or_else(|| bad("header"))?;
        self.events.clear();
        self.views.clear();
        self.ports.clear();
        let bytes = bytes.get(..len).ok_or_else(|| bad("length"))?;
        let mut at = HEADER;
        for _ in 0..sections {
            let head = bytes.get(at..at + 8).ok_or_else(|| bad("section"))?;
            let kind = u16::from_le_bytes([head[0], head[1]]);
            let n = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
            let body = bytes
                .get(at + 8..at + 8 + n)
                .ok_or_else(|| bad("section length"))?;
            at = (at + 8 + n + 3) & !3;
            self.section(kind, body)
                .ok_or_else(|| format!("wire frame: bad section {kind}"))?;
        }
        let _ = decode;
        Ok(())
    }

    fn s(&self, id: u64) -> Option<String> {
        self.strings.get(id as usize).cloned()
    }

    fn section(&mut self, kind: u16, body: &[u8]) -> Option<()> {
        use crate::wire::value::decode;
        let mut c = Cursor::new(body);
        let fixed = matches!(kind, TRANSFORM | VELOCITY);
        let n = if fixed {
            c.u32()? as usize
        } else {
            c.uvar()? as usize
        };
        for _ in 0..n {
            match kind {
                STRINGS => {
                    let s = c.str()?.to_string();
                    self.strings.push(s);
                }
                SPAWN => {
                    let slot = c.uvar()? as usize;
                    let id = self.s(c.uvar()?)?;
                    let kind = self.s(c.uvar()?)?;
                    let k = c.uvar()? as usize;
                    let mut tags = Vec::with_capacity(k);
                    for _ in 0..k {
                        tags.push(self.s(c.uvar()?)?);
                    }
                    if self.entities.len() <= slot {
                        self.entities.resize(slot + 1, None);
                    }
                    self.entities[slot] = Some(MirrorEntity {
                        id,
                        kind,
                        tags,
                        ..MirrorEntity::default()
                    });
                    self.touched.push(slot);
                    self.touched_whole.push(slot);
                }
                DESPAWN => {
                    let slot = c.uvar()? as usize;
                    *self.entities.get_mut(slot)? = None;
                    self.touched.push(slot);
                    self.touched_whole.push(slot);
                }
                TRANSFORM => {
                    let slot = c.u32()? as usize;
                    let t = [c.f32()?, c.f32()?, c.f32()?, c.f32()?];
                    self.entities.get_mut(slot)?.as_mut()?.transform = t;
                    self.touched.push(slot);
                }
                VELOCITY => {
                    let slot = c.u32()? as usize;
                    let v = [c.f32()?, c.f32()?, c.f32()?];
                    self.entities.get_mut(slot)?.as_mut()?.velocity = Some(v);
                    self.touched.push(slot);
                }
                TAGS => {
                    let slot = c.uvar()? as usize;
                    let k = c.uvar()? as usize;
                    let mut tags = Vec::with_capacity(k);
                    for _ in 0..k {
                        tags.push(self.s(c.uvar()?)?);
                    }
                    self.entities.get_mut(slot)?.as_mut()?.tags = tags;
                    self.touched.push(slot);
                    self.touched_whole.push(slot);
                }
                COMPONENT => {
                    let slot = c.uvar()? as usize;
                    let name = self.s(c.uvar()?)?;
                    let set = c.u8()? == 1;
                    let value = if set {
                        Some(decode(&mut c, &self.strings)?)
                    } else {
                        None
                    };
                    self.touched.push(slot);
                    self.touched_whole.push(slot);
                    let e = self.entities.get_mut(slot)?.as_mut()?;
                    match (e.components.iter().position(|(k, _)| *k == name), value) {
                        (Some(i), Some(v)) => e.components[i].1 = v,
                        (None, Some(v)) => e.components.push((name, v)),
                        (Some(i), None) => {
                            e.components.remove(i);
                        }
                        (None, None) => {}
                    }
                }
                VARS => {
                    let path = self.s(c.uvar()?)?;
                    let set = c.u8()? == 1;
                    let value = if set {
                        Some(decode(&mut c, &self.strings)?)
                    } else {
                        None
                    };
                    self.changed_vars.push(path.clone());
                    upsert(&mut self.vars, path, value);
                }
                EVENTS => {
                    let topic = self.s(c.uvar()?)?;
                    let seq = c.uvar()?;
                    let tick = c.uvar()?;
                    let time = c.f64()?;
                    let source = self.s(c.uvar()?)?;
                    let entity = match c.uvar()? {
                        0 => None,
                        r => Some(self.s(r - 1)?),
                    };
                    let actor = match c.uvar()? {
                        0 => None,
                        r => Some(self.s(r - 1)?),
                    };
                    let payload = decode(&mut c, &self.strings)?;
                    let mut o = serde_json::json!({ "topic": topic, "payload": payload, "seq": seq, "tick": tick, "time": time, "source": source });
                    if let Some(e) = entity {
                        o["entity"] = e.into();
                    }
                    if let Some(a) = actor {
                        o["actor"] = a.into();
                    }
                    self.events.push(o);
                }
                VIEWS | STATE | PORTS => {
                    let key = self.s(c.uvar()?)?;
                    let value = decode(&mut c, &self.strings)?;
                    match kind {
                        VIEWS => self.views.push((key, value)),
                        STATE => {
                            self.changed_state.push(key.clone());
                            upsert(&mut self.state, key, Some(value))
                        }
                        _ => self.ports.push((key, value)),
                    }
                }
                _ => return Some(()), // a section of a later minor version: skipped
            }
        }
        Some(())
    }

    pub fn var(&self, path: &str) -> Option<&serde_json::Value> {
        self.vars.iter().find(|(k, _)| k == path).map(|(_, v)| v)
    }

    pub fn state_of(&self, key: &str) -> Option<&serde_json::Value> {
        self.state.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

#[cfg(feature = "json")]
fn upsert(
    list: &mut Vec<(String, serde_json::Value)>,
    key: String,
    value: Option<serde_json::Value>,
) {
    match (list.iter().position(|(k, _)| *k == key), value) {
        (Some(i), Some(v)) => list[i].1 = v,
        (None, Some(v)) => list.push((key, v)),
        (Some(i), None) => {
            list.remove(i);
        }
        (None, None) => {}
    }
}
