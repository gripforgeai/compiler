//! The event queue of the kit logic modules (docs/kit-modules.md §7, step 5): the buffer `EVENTS` of the shared zone
//! (src/zone.rs) and the numbers of the functions that read it.
//!
//! The bus stays the core's (`Host::flush`): same order, same rounds. What changes is how a kit module receives its
//! events. The core writes an event **once**, in binary, in the queue; a kit is woken by one call
//! ([`KIT_EVENTS`]) that names its first delivery, runs the handler, then asks for the next one by a direct link
//! (`gf:kithost.services@^1` / `ev_next`, the core's `gfx_ev_next`): as long as the next delivery **in the order of
//! the bus** is for this kit, it stays in the same wake-up; the subscribers of the core that come in between
//! (triggers, the recorder) run in the core at their exact place during that call. When the next delivery
//! is for another kit, `ev_next` answers 0 and the wake-up ends.
//!
//! ## `EVENTS` (id 3 of the zone) — every integer little-endian, offsets from the start of the buffer
//!
//! | at | type | field |
//! |---|---|---|
//! | 0 | u32 | `head`: where the next record is written (≥ 32, 8-byte aligned) |
//! | 4 | u32 | `records`: records written so far (wraps) |
//! | 8 | u32 | `wraps`: times the queue went back to its start |
//! | 12 | u32 | `answer`: offset in the zone of the body of the answer of the last direct callback ([`REC_ANSWER`]) |
//! | 16 | u32 | `answer_len`: its length |
//! | 20 | 12 bytes | reserved, 0 |
//! | 32 | … | the records |
//!
//! A record: `len u32` (bytes of the body), `kind u32` ([`REC_EVENT`], [`REC_DELIVERY`]), the body,
//! padding to 8 bytes. The core and the kits both append records (one thread: who
//! writes reads `head`, writes the record, then writes `head`). A record is valid until the queue wraps over it: a reader copies it when it is given its offset (the
//! offset and length of the **body** in the zone), never later.
//!
//! - **Event** body: `seq u64`, `tick u64`, `time f64`, `topic string`, `source string`, `entity string?`,
//!   `actor string?`, `payload value` (the encoding of the calls: `string` = u32 count + UTF-8, `string?` = a bool
//!   then the string, `value` = a tagged value).
//! - **Delivery** body: the context of the entry (48 bytes, [`crate::Ctx`]), `seq u64` (the event), `event u32`
//!   (offset of its body in the zone), `len u32`, `tag string` (the handler: `Kx::on(pattern, tag)`).
//!
//! - **Arguments** of a callback entered directly ([`REC_ARGS`]): `name string, args list<value>` (the bytes of the
//!   hook `callback` of `kithost.kit` after the context). **Answer** ([`REC_ANSWER`]): the value answered, or the
//!   message of the error.
//!
//! ## Callbacks by the direct entry (a tap in code of an interceptable hook, `Kx::invoke`)
//!
//! A function a kit handed over (`Kx::callback(name)`: the tap of a hook of `game.engine.hooks`, a callback given to a
//! service) is called back **synchronously**: who asks waits for the answer. When the kit has a direct entry
//! (`gfx_kit_hook`) the core writes the arguments as a record, enters the kit there (hook `callback`, the place of
//! the record as the argument: `offset | len << 32`), and the kit writes its answer as a record and its place in
//! the header (`answer`, `answer_len`); the entry answers 0 (a value) or [`HOOK_FAILED`] (the message of the
//! error). No JavaScript, no buffer of the bridge. Arguments or an answer past [`DIRECT_MAX`] take the bridge.
//!
//! A typed event (`name@version` with a fixed binary schema, declared in the manifest of the module that emits it)
//! has the same header and its payload as the bytes of the schema instead of a value; the kits of GripForge all
//! declare the schema `value`.

/// `kithost.kit@1.1`: (ctx, seq u64, event u32, len u32, tag string) — the first delivery of a wake-up; the kit
/// runs the handler then reads the following ones with `ev_next`. Only called on a kit whose `INFO` says
/// `events: 1`; the others keep `ON_EVENT`.
pub const KIT_EVENTS: u32 = 0xa0;

/// Bytes of the header of the buffer.
pub const HEADER: u32 = 32;
pub const HEAD: u32 = 0;
pub const RECORDS: u32 = 4;
pub const WRAPS: u32 = 8;
/// Bytes before the body of a record (`len`, `kind`).
pub const REC_HEADER: u32 = 8;
pub const REC_EVENT: u32 = 1;
pub const REC_DELIVERY: u32 = 2;
/// The number of `ev_next` among the direct links counted by the core (`kitcall::count_direct_fn`).
pub const DIRECT_EV_NEXT: u32 = 192;
pub const REC_ARGS: u32 = 3;
pub const REC_ANSWER: u32 = 4;
/// The header fields of the answer of the last direct callback.
pub const ANSWER: u32 = 12;
pub const ANSWER_LEN: u32 = 16;
/// The direct entry answered: the callback answered an error (the answer is its message).
pub const HOOK_FAILED: u32 = 3;
/// Arguments larger than this take the bridge (they are copied word by word through the links).
pub const DIRECT_MAX: usize = 4096;
