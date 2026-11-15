//! The tracer of the interceptable hooks on the module side (`game.engine.hooks`, docs/mods-engine.md): the runs go to the
//! host's tracer (`kithost.services@1.4` `hook_trace_on` / `hook_trace_record`), so that the host reads them as it
//! reads its own (the `gripforge.debug/1` snapshot, `gfk_hook_trace`). Asked only when a hook with taps runs (rare);
//! a core before 1.4 answers no: nothing is kept, nothing changes in the run. The figures per hook (`stats`,
//! `kithost.services@1.6`) are what the debug.inspector panel reads; a core before 1.6 answers none.

use crate::host::{call_core, writer};
use gripforge_abi::{services as fs, Reader};
use serde_json::Value as Json;

/// Whether the host's tracer keeps runs (asked before a row is built).
pub fn on() -> bool {
    call_core(crate::func!(fs::HOOK_TRACE_ON), &writer())
        .ok()
        .and_then(|b| Reader::new(&b).bool().ok())
        .unwrap_or(false)
}

/// [`on`] for one run of `hook`, declared by `kit`: the host's watch of the hooks counts it only if its filter keeps
/// it. A core before 1.6 refuses the call: `on`.
pub fn on_for(hook: &str, kit: &str) -> bool {
    let mut w = writer();
    w.str(hook).str(kit);
    match call_core(crate::func!(fs::HOOK_TRACE_ON_FOR), &w) {
        Ok(b) => Reader::new(&b).bool().unwrap_or(false),
        Err(_) => on(),
    }
}

/// One run of a hook, kept by the host's tracer.
pub fn record(run: Json) {
    let mut w = writer();
    w.value(&run);
    let _ = call_core(crate::func!(fs::HOOK_TRACE_RECORD), &w);
}

/// The host's figures per hook since watched (`{ <hook>: { calls, ms, lastMs, maxMs, refused, cancelled } }`):
/// `watch` keeps them counting, `false` stops and forgets them; `filter` names the hooks to watch (hooks or kits,
/// `*` patterns, empty: all). `{}` from a core before 1.6.
pub fn stats(watch: bool, filter: &str) -> Json {
    let mut w = writer();
    w.bool(watch).str(filter);
    call_core(crate::func!(fs::HOOK_STATS), &w)
        .ok()
        .and_then(|b| Reader::new(&b).value().ok())
        .unwrap_or_else(|| Json::Object(Default::default()))
}
