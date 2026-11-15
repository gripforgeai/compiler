//! The hooks seen from a kit module (`game.engine.hooks`, docs/modding.md « Les hooks »): `decide`, and the short path of
//! a hook nobody taps — `kithost.services@1.6` `hook_tapped` is asked first, and nothing is written nor sent when no
//! tap listens. Used by `Host::call_service*` (kx_world.rs) for `game.engine.hooks` `run` / `decide`, and by the kits
//! (`kx.hook_tapped`, `kx.decide`).

use crate::host::{call_core, writer, Host};
use crate::state::call;
use gripforge_abi::{services as fs, Reader};
use serde_json::{json, Value as Json};

impl Host {
    /// Whether a tap may listen to `hook`: a kit asks before it builds the payload of a frequent hook. A core that
    /// refuses the call (before 1.6): yes (the full path, the safe one).
    pub fn hook_tapped(&self, hook: &str) -> bool {
        let mut w = writer();
        w.str(hook);
        call_core(crate::func!(fs::HOOK_TAPPED), &w)
            .map_or(true, |b| Reader::new(&b).bool().unwrap_or(true))
    }

    /// 1.3 `decide(services, hook, payload)`: nothing is written for a hook nobody taps.
    pub fn decide(&mut self, hook: &str, payload: Json) -> Json {
        if !self.hook_tapped(hook) {
            return untapped_answer(payload);
        }
        self.reads_stale();
        let bytes = call(crate::func!(fs::DECIDE), |w| {
            w.str(hook).value(&payload);
        });
        self.reads_stale();
        Reader::new(&bytes).value().unwrap_or(Json::Null)
    }

    /// `game.engine.hooks` `run` / `decide` (JSON arguments) of a hook nobody taps: the answer of an empty pipeline,
    /// made here. `None`: any other call, the full path.
    pub(crate) fn untapped_json(
        &self,
        capability: &str,
        method: &str,
        args: &[Json],
    ) -> Option<Json> {
        let hook = args.first()?.as_str()?;
        (is_decide(capability, method) && !self.hook_tapped(hook))
            .then(|| untapped_answer(args.get(1).cloned().unwrap_or(Json::Null)))
    }

    /// [`Host::untapped_json`] with typed arguments: the payload is turned into a value only for the answer.
    pub(crate) fn untapped_args(
        &self,
        capability: &str,
        method: &str,
        args: &[crate::typed::Arg],
    ) -> Option<Json> {
        let crate::typed::Arg::Str(hook) = args.first()? else {
            return None;
        };
        (is_decide(capability, method) && !self.hook_tapped(hook))
            .then(|| untapped_answer(args.get(1).map_or(Json::Null, crate::typed::Arg::to_json)))
    }
}

fn is_decide(capability: &str, method: &str) -> bool {
    capability == "game.engine.hooks" && matches!(method, "run" | "decide")
}

/// The answer of `decide` for a hook nobody taps (the same bytes as the core's `untapped_answer`).
fn untapped_answer(payload: Json) -> Json {
    let payload = match payload {
        Json::Object(fields) => fields,
        _ => serde_json::Map::new(),
    };
    json!({ "ok": true, "payload": payload, "cancelled": false, "reason": null, "by": null, "notes": [] })
}
