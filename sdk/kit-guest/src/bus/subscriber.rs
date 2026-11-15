//! Who receives a bus event (runtime/bus.ts) — shared word for word by the core and the kit logic modules
//! (the file is the kit façade's, the core includes it).

/// Who receives a dispatched event.
#[derive(Clone, Debug, PartialEq)]
pub enum Subscriber {
    /// The trigger engine (`*`, subscribed at construction).
    Triggers,
    /// A kit handler: `KitModule::on_event(kx, tag, event)`.
    Kit { kit: usize, tag: String },
    /// Appends to `Host::recorded` (parity traces, tests).
    Recorder,
    /// A subscription that does nothing (keeps subscription ids aligned with the TypeScript host).
    Noop,
}
