//! An event read from a message of the calls (one file for the façade and the core, which includes it
//! with `#[path]`). When the value came as a fixed-layout block (docs/kit-modules.md §9.7) the event is made
//! from the block **in place** — one copy, of the payload and of the texts it keeps — instead of a whole value
//! built first and read after.

use crate::bus::GameEvent;
use gripforge_abi::fixed::View;
use gripforge_abi::Reader;
use serde_json::Value as Json;

/// `GameEvent::from_json` on a block: the same event.
pub(crate) fn event_of(v: View) -> Option<GameEvent> {
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    Some(GameEvent {
        topic: text("topic")?,
        payload: v
            .get("payload")
            .and_then(|p| p.to_json().ok())
            .unwrap_or(Json::Null)
            .into(),
        seq: v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0),
        tick: v.get("tick").and_then(|x| x.as_u64()).unwrap_or(0),
        time: v.get("time").and_then(|x| x.as_f64()).unwrap_or(0.0),
        source: text("source").unwrap_or_else(|| "runtime".to_string()),
        entity: text("entity"),
        actor: text("actor"),
    })
}

/// The event the next value of a message holds (`None`: not an event).
pub(crate) fn read_event(r: &mut Reader) -> Result<Option<GameEvent>, String> {
    Ok(match r.fixed()? {
        Some(block) => event_of(block.view()),
        None => GameEvent::from_json(&r.value()?),
    })
}
