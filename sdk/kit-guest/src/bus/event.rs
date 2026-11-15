//! A bus event (runtime/bus.ts `GameEvent`) — shared word for word by the core and the kit logic modules
//! (the file is the kit façade's, the core includes it).
//!
//! The payload is **typed** where it crosses between the modules (docs/kit-modules.md §10.17): the emitter writes it
//! once as a fixed-layout block (§9.7: every field with its type — integer, float, flag, text, list, dictionary),
//! the core keeps that block, records it and delivers it as it is, and a reader reads its fields in place
//! ([`Payload::num`], [`Payload::text`], [`Payload::flag`]). The JSON value is made only when something asks for it
//! — `&*payload`, `payload.get(..)`, any method of `serde_json::Value` (a mod, the triggers, the history, a kit that
//! reads it as JSON): once, then kept. For the authors of kits nothing changes: the payload reads as the JSON value
//! it always was.

use crate::kept::{self, Kept};
use serde_json::{json, Value as Json};
use std::sync::OnceLock;

/// The payload of an event: a JSON value, or a typed block whose JSON value is made the first time it is read.
#[derive(Default)]
pub struct Payload {
    json: OnceLock<Json>,
    typed: Option<Kept>,
}

impl Payload {
    /// A payload kept typed (a block that crossed): no JSON made.
    pub fn typed(k: Kept) -> Payload {
        Payload {
            json: OnceLock::new(),
            typed: Some(k),
        }
    }
    /// The block, when the payload is typed (to deliver it as it is).
    pub fn kept(&self) -> Option<&Kept> {
        self.typed.as_ref()
    }
    /// Whether the JSON value was made (the tests and the counters).
    pub fn materialized(&self) -> bool {
        self.json.get().is_some()
    }
    fn json(&self) -> &Json {
        self.json.get_or_init(|| match &self.typed {
            Some(k) => {
                MADE.with(|m| m.set(m.get() + 1));
                kept::to_json(k)
            }
            None => Json::Null,
        })
    }
    /// The JSON value, owned.
    pub fn into_json(mut self) -> Json {
        self.json();
        self.json.take().unwrap_or(Json::Null)
    }
    /// The JSON values made from typed payloads by this module so far (what a reader asked of them as JSON).
    pub fn made() -> u64 {
        MADE.with(|m| m.get())
    }
    /// A field that is a number (`payload[key].as_f64()`), read in place when typed.
    pub fn num(&self, key: &str) -> Option<f64> {
        match (&self.typed, self.json.get()) {
            (Some(k), None) => kept::num(k, key),
            _ => self.json().get(key)?.as_f64(),
        }
    }
    /// A field that is a flag.
    pub fn flag(&self, key: &str) -> Option<bool> {
        match (&self.typed, self.json.get()) {
            (Some(k), None) => kept::flag(k, key),
            _ => self.json().get(key)?.as_bool(),
        }
    }
    /// A field that is a text, read in place when typed.
    pub fn text(&self, key: &str) -> Option<&str> {
        match (&self.typed, self.json.get()) {
            (Some(k), None) => kept::text(k, key),
            _ => self.json().get(key)?.as_str(),
        }
    }
    /// Whether the payload is a dictionary holding `key` (whatever its value).
    pub fn has(&self, key: &str) -> bool {
        match (&self.typed, self.json.get()) {
            (Some(k), None) => kept::has(k, key),
            _ => self.json().get(key).is_some(),
        }
    }
}

thread_local! {
    static MADE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl From<Json> for Payload {
    fn from(v: Json) -> Payload {
        let json = OnceLock::new();
        let _ = json.set(v);
        Payload { json, typed: None }
    }
}

impl std::ops::Deref for Payload {
    type Target = Json;
    fn deref(&self) -> &Json {
        self.json()
    }
}

impl std::ops::DerefMut for Payload {
    /// A payload changed in place is a JSON value from then on (the block no longer says it).
    fn deref_mut(&mut self) -> &mut Json {
        self.json();
        self.typed = None;
        self.json.get_mut().expect("made above")
    }
}

impl Clone for Payload {
    /// A typed payload stays a block (the clone is a count: the bus records and delivers it); a JSON one is copied.
    #[allow(
        unreachable_code,
        reason = "`Kept` is an empty enum in the native kit host (typed payloads exist in the wasm guest only)"
    )]
    fn clone(&self) -> Payload {
        match &self.typed {
            Some(k) => Payload::typed(k.clone()),
            None => Payload::from(self.json().clone()),
        }
    }
}

impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.json(), f)
    }
}

impl PartialEq for Payload {
    fn eq(&self, other: &Payload) -> bool {
        self.json() == other.json()
    }
}

impl PartialEq<Json> for Payload {
    fn eq(&self, other: &Json) -> bool {
        self.json() == other
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameEvent {
    pub topic: String,
    pub payload: Payload,
    pub seq: u64,
    pub tick: u64,
    pub time: f64,
    pub source: String,
    pub entity: Option<String>,
    /// The co-op player in scope when the event was emitted (runtime/bus.ts `actor`).
    pub actor: Option<String>,
}

impl GameEvent {
    pub fn to_json(&self) -> Json {
        let mut o = json!({
            "topic": self.topic,
            "payload": *self.payload,
            "seq": self.seq,
            "tick": self.tick,
            "time": self.time,
            "source": self.source,
        });
        if let Some(e) = &self.entity {
            o["entity"] = Json::String(e.clone());
        }
        if let Some(a) = &self.actor {
            o["actor"] = Json::String(a.clone());
        }
        o
    }

    pub fn from_json(v: &Json) -> Option<GameEvent> {
        Some(GameEvent {
            topic: v.get("topic")?.as_str()?.to_string(),
            payload: v.get("payload").cloned().unwrap_or(Json::Null).into(),
            seq: v.get("seq").and_then(Json::as_u64).unwrap_or(0),
            tick: v.get("tick").and_then(Json::as_u64).unwrap_or(0),
            time: v.get("time").and_then(Json::as_f64).unwrap_or(0.0),
            source: v
                .get("source")
                .and_then(Json::as_str)
                .unwrap_or("runtime")
                .to_string(),
            entity: v.get("entity").and_then(Json::as_str).map(str::to_string),
            actor: v.get("actor").and_then(Json::as_str).map(str::to_string),
        })
    }
}
