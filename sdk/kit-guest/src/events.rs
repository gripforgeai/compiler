//! The event queue as a kit module reads it (docs/kit-modules.md §7 step 5; layout in bytes: gripforge-kitcall
//! src/events.rs). The core wakes the kit once (`KIT_EVENTS`) with its first delivery; the kit copies the event from
//! the zone, runs the handler, then asks the core for its next delivery (`ev_next`, a direct link) until the core
//! answers that the next one in the order of the bus is not its own.

use crate::bus::GameEvent;
use crate::host::Kx;
use crate::kit::KitModule;
use crate::zone::{self, zone_copy};
use gripforge_abi::events as layout;
use gripforge_abi::{Ctx, Reader};
use std::cell::RefCell;

#[cfg(any(target_arch = "wasm32", feature = "common-memory"))]
mod raw {
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.services@^1")]
    extern "C" {
        pub fn ev_next() -> u64;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    pub use crate::native::link::ev_next;
}

#[cfg(not(any(target_arch = "wasm32", feature = "common-memory")))]
mod raw {
    /// Off wasm there is no core to ask (the façade is only checked there).
    pub unsafe fn ev_next() -> u64 {
        0
    }
}

/// The body of an event record.
pub fn decode_event(bytes: &[u8]) -> Result<GameEvent, String> {
    let mut r = Reader::new(bytes);
    let (seq, tick, time) = (r.u64()?, r.u64()?, r.f64()?);
    let topic = r.string()?;
    let source = r.string()?;
    let entity = r.opt_str()?.map(str::to_string);
    let actor = r.opt_str()?.map(str::to_string);
    // A typed payload (a block that crossed) is kept as it is: its JSON value is made only if the handler reads it so.
    let payload = match r.kept()? {
        Some(k) => crate::bus::Payload::typed(k),
        None => r.value()?.into(),
    };
    Ok(GameEvent {
        topic,
        payload,
        seq,
        tick,
        time,
        source,
        entity,
        actor,
    })
}

/// `KIT_EVENTS`: the deliveries of this wake-up, one handler at a time, each entered as a hook of its own (the
/// context of the core, what the façades read made stale).
pub fn drain(m: &dyn KitModule, kx: &mut Kx, r: &mut Reader) -> Result<(), String> {
    let mut event: Option<GameEvent> = None;
    let mut buf: Vec<u8> = Vec::new();
    let mut record: Vec<u8> = Vec::new();
    let (mut seq, mut at, mut len, mut tag) = (r.u64()?, r.u32()?, r.u32()?, r.string()?);
    loop {
        if event.as_ref().map(|e| e.seq) != Some(seq) {
            buf.resize(len as usize, 0);
            if !zone_copy(at, &mut buf) {
                return Err(format!(
                    "kitcall: event record {at:#x}+{len} is outside the zone"
                ));
            }
            event = Some(decode_event(&buf)?);
        }
        if let Some(e) = &event {
            m.on_event(kx, &tag, e);
        }
        // SAFETY: an import wired by the calls host to the core's `gfx_ev_next`.
        let next = unsafe { raw::ev_next() };
        if next == 0 {
            return Ok(());
        }
        record.resize((next >> 32) as usize, 0);
        if !zone_copy(next as u32, &mut record) {
            return Err("kitcall: delivery record outside the zone".into());
        }
        let mut d = Reader::new(&record);
        let ctx = Ctx::read(&mut d)?;
        kx.host.enter(ctx);
        (seq, at, len, tag) = (d.u64()?, d.u32()?, d.u32()?, d.string()?);
    }
}

// ---- the queue, written and read by the kit itself ---------------------------------------------------------------

/// Copies `out.len()` bytes of the zone from `off`, word by word through the direct links (no JavaScript between
/// the modules; merged, each word is a load).
#[allow(
    clippy::chunks_exact_to_as_chunks,
    reason = "lint of a newer clippy than the source; copied as is (docs/sources.md)"
)]
pub fn zone_get(off: u32, out: &mut [u8]) {
    let mut chunks = out.chunks_exact_mut(8);
    let mut at = off;
    for c in &mut chunks {
        c.copy_from_slice(&zone::zone_read_u64(at).to_le_bytes());
        at += 8;
    }
    for b in chunks.into_remainder() {
        *b = zone::zone_read_u8(at);
        at += 1;
    }
}

/// Copies `src` into the zone at `off`, word by word.
#[allow(
    clippy::chunks_exact_to_as_chunks,
    reason = "lint of a newer clippy than the source; copied as is (docs/sources.md)"
)]
pub fn zone_put(off: u32, src: &[u8]) {
    let mut chunks = src.chunks_exact(8);
    let mut at = off;
    for c in &mut chunks {
        zone::zone_write_u64(at, u64::from_le_bytes(c.try_into().unwrap()));
        at += 8;
    }
    for b in chunks.remainder() {
        zone::zone_write_u8(at, *b);
        at += 1;
    }
}

/// Appends a record to the queue (the same steps as the core's `write`): the offset in the zone and the length of
/// its body, `None` when it cannot hold in the buffer.
pub fn ring_write(kind: u32, body: &[u8]) -> Option<(u32, u32)> {
    let ev = zone::events();
    let len = body.len() as u32;
    let total = (layout::REC_HEADER + len + 7) & !7;
    if ev.size < layout::HEADER || total > ev.size - layout::HEADER {
        return None;
    }
    let mut head = zone::zone_read_u32(ev.offset + layout::HEAD).max(layout::HEADER);
    if head + total > ev.size {
        head = layout::HEADER;
        zone::zone_write_u32(
            ev.offset + layout::WRAPS,
            zone::zone_read_u32(ev.offset + layout::WRAPS).wrapping_add(1),
        );
    }
    let at = ev.offset + head;
    zone::zone_write_u64(at, len as u64 | (kind as u64) << 32);
    zone_put(at + layout::REC_HEADER, body);
    zone::zone_write_u32(ev.offset + layout::HEAD, head + total);
    zone::zone_write_u32(
        ev.offset + layout::RECORDS,
        zone::zone_read_u32(ev.offset + layout::RECORDS).wrapping_add(1),
    );
    Some((at + layout::REC_HEADER, len))
}

thread_local! {
    /// Buffers of the direct callbacks, reused (they nest).
    static POOL: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}

/// The hook `callback` entered directly (a tap of an interceptable hook, a function this kit handed over): the
/// arguments are the record at `place` (`offset | len << 32`: `name, values`); the answer is written as a record,
/// its place in the header of the queue. Answers the status of the direct entry.
pub fn serve_callback(m: &dyn KitModule, kx: &mut Kx, place: u64) -> u32 {
    let mut b = POOL.with(|p| p.borrow_mut().pop()).unwrap_or_default();
    b.clear();
    b.resize((place >> 32) as usize, 0);
    zone_get(place as u32, &mut b);
    let parsed = (|| {
        let mut r = Reader::new(&b);
        Ok::<_, String>((r.string()?, r.values()?))
    })();
    let answered = parsed.and_then(|(name, args)| m.callback(kx, &name, &args));
    b.clear();
    let status = match answered {
        Ok(v) => {
            gripforge_abi::put_value(&mut b, &v);
            0
        }
        Err(msg) => {
            b.extend_from_slice(msg.as_bytes());
            layout::HOOK_FAILED
        }
    };
    let status = match ring_write(layout::REC_ANSWER, &b) {
        Some((off, len)) => {
            zone::zone_write_u64(
                zone::events().offset + layout::ANSWER,
                off as u64 | (len as u64) << 32,
            );
            status
        }
        None => 4,
    };
    POOL.with(|p| p.borrow_mut().push(b));
    status
}

/// Whether an event of `topic` would be heard by anybody: a kit handler, a rule, a mission, or the engine through
/// the gateway's recorder (`kithost.services@1.5` `has_audience`, `Host::has_audience` of the core). A kit asks before
/// it builds a frequent event: nothing is made for an event that goes nowhere. A core before 1.5 refuses the call:
/// the answer is then yes (the safe one).
pub fn has_audience(topic: &str) -> bool {
    let mut w = crate::host::writer();
    w.str(topic);
    crate::host::call_core(crate::func!(gripforge_abi::services::HAS_AUDIENCE), &w)
        .map_or(true, |b| {
            gripforge_abi::Reader::new(&b).bool().unwrap_or(true)
        })
}
