//! Façade of src/bus.rs: the event and subscriber types are shared with the core (src/bus/event.rs,
//! src/bus/subscriber.rs, which the core compiles too); the bus lives in the core (`kithost.services@1`).
mod event;
pub use event::{GameEvent, Payload};
mod subscriber;
pub use subscriber::Subscriber;

use crate::state::call;
use gripforge_abi::{services as f, Reader};

/// `host.bus`: the subscriptions are made in the core.
#[derive(Default)]
pub struct Bus {}

impl Bus {
    /// Subscribes; returns the subscription id (for `off`).
    pub fn on(
        &mut self,
        pattern: &str,
        subscriber: Subscriber,
        priority: f64,
        once: bool,
        owner: Option<&str>,
    ) -> u64 {
        let bytes = call(crate::func!(f::BUS_ON), |w| {
            w.str(pattern);
            match &subscriber {
                Subscriber::Kit { kit, tag } => w.u8(0).u32(*kit as u32).str(tag),
                Subscriber::Triggers => w.u8(1),
                Subscriber::Recorder => w.u8(3),
                Subscriber::Noop => w.u8(4),
            };
            w.f64(priority).bool(once).opt_str(owner);
        });
        Reader::new(&bytes).u64().unwrap_or(0)
    }

    /// The contexts of game.engine's stack a subscription hears in (`kithost.services@1.7`, docs/contextes.md):
    /// empty is `default`, `all` every context. A core before 1.7 refuses it: the subscription stays on `default`.
    pub fn set_contexts<S: AsRef<str>>(&mut self, id: u64, names: &[S]) -> bool {
        let mut w = crate::host::writer();
        w.u64(id).strs(names);
        crate::host::call_core(crate::func!(f::BUS_CONTEXTS), &w).is_ok()
    }

    /// The context on top of game.engine's stack (game.engine says it when its stack changes).
    /// A core before 1.7 refuses it: nothing is filtered there.
    pub fn set_context_top(&mut self, name: &str) {
        let mut w = crate::host::writer();
        w.str(name);
        let _ = crate::host::call_core(crate::func!(f::CONTEXT_TOP), &w);
    }

    pub fn off(&mut self, id: u64) {
        call(crate::func!(f::BUS_OFF), |w| {
            w.u64(id);
        });
    }
}

impl crate::host::Kx<'_> {
    /// Subscribes `on_event(tag)` to a topic pattern in some contexts of game.engine's stack (docs/contextes.md):
    /// `["dialogue"]`, `["default", "dialogue"]`, `["all"]` (whatever is on top). Empty: `default`, as `on`.
    pub fn on_in<S: AsRef<str>>(&mut self, pattern: &str, tag: &str, contexts: &[S]) -> u64 {
        let id = self.on_with(pattern, tag, 0.0, false);
        if !contexts.is_empty() {
            self.host.bus.set_contexts(id, contexts);
        }
        id
    }
}
