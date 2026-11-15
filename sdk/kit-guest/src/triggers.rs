//! Façade of src/triggers.rs: the trigger engine lives in the core (`kithost.services@1`); a kit registers its
//! verbs and rules with `Kx`, and reads or removes them here.

use crate::state::call;
use gripforge_abi::{services as f, Reader};

/// `host.triggers.firing` (a field of the core): the rule whose actions are running, asked on `clone()`.
#[derive(Default)]
pub struct Firing {}

impl Firing {
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> Option<String> {
        let bytes = call(crate::func!(f::TRIGGER_FIRING), |_| {});
        Reader::new(&bytes)
            .opt_str()
            .ok()
            .flatten()
            .map(str::to_string)
    }
}

/// `host.triggers`.
#[derive(Default)]
pub struct Triggers {
    pub firing: Firing,
}

impl Triggers {
    pub fn remove(&mut self, id: &str) {
        call(crate::func!(f::TRIGGER_REMOVE), |w| {
            w.str(id);
        });
    }
    pub fn action_types(&self) -> Vec<String> {
        let bytes = call(crate::func!(f::ACTION_TYPES), |_| {});
        Reader::new(&bytes).strs().unwrap_or_default()
    }
    pub fn condition_types(&self) -> Vec<String> {
        let bytes = call(crate::func!(f::CONDITION_TYPES), |_| {});
        Reader::new(&bytes).strs().unwrap_or_default()
    }
    /// The verb answered by `kit` (an index of the host, `Kx::kit` for the kit that calls): game.engine registers the
    /// verbs of the kits that declare them (`kithost.services@1.3`).
    pub fn register_action(&mut self, verb: &str, kit: usize) {
        call(crate::func!(f::REGISTER_VERB_FOR), |w| {
            w.str(verb).u32(kit as u32).bool(false);
        });
    }
    pub fn register_condition(&mut self, verb: &str, kit: usize) {
        call(crate::func!(f::REGISTER_VERB_FOR), |w| {
            w.str(verb).u32(kit as u32).bool(true);
        });
    }
}
