//! Façade of src/timers.rs: the timers live in the core (`kithost.services@1`); a kit sets them with `Kx::after` /
//! `Kx::every` and cancels them here.

use crate::state::call;
use gripforge_abi::services as f;

/// `host.timers`.
#[derive(Default)]
pub struct Timers {}

impl Timers {
    pub fn cancel(&mut self, id: u64) {
        call(crate::func!(f::TIMER_CANCEL), |w| {
            w.u64(id);
        });
    }
}
