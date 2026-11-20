//! The methods of `Kx` and `Host` on the ports: physics, input, assets, stores, platform, audio, display, pointer
//! (the same as host.rs of the kit host; the ports are the proxies of src/ports.rs).

use crate::host::{Host, Kx};
use crate::ports::{AssetPort, InputPort, Physics, StoreHandle};

impl Host {
    /// `runtime.core.saveStore(kind)`: the local store, or the cloud one when the core has one.
    pub fn save_store(&self, kind: &str) -> Option<StoreHandle> {
        self.stores.get(kind)
    }

    /// A new entry of the core (`Host::enter`): the copies the ports kept are dropped (the input read, the date,
    /// the recorded calls); the target and the language are read the first time.
    pub fn enter_ports(&mut self) {
        self.physics.enter();
        self.assets.enter();
        self.platform.enter();
        self.display.enter();
        self.pointer.enter();
    }
}

impl Kx<'_> {
    /// `ctx.physics`.
    pub fn physics(&mut self) -> &mut dyn Physics {
        &mut self.host.physics
    }
    /// `ctx.input`: the neutral transport of the devices (the actions are core.input').
    pub fn input(&mut self) -> &mut InputPort {
        &mut self.host.input
    }
    /// `ctx.assets`.
    pub fn assets(&self) -> &dyn AssetPort {
        &self.host.assets
    }
}
