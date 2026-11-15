//! What the game.engine module asks of the other kits through the core (`kithost.services@1.3`, docs/modules-de-kit.md):
//! their scene and component hooks, by the kit's index in the host. A kit linked into the host calls
//! `Host::with_kit`; a kit module asks the core to do the same.

use crate::host::{Host, Kx};
use crate::state::call;
use gripforge_abi::services as fs;
use serde_json::Value as Json;

impl Host {
    /// The index of a mounted kit (the order of `kit_ids`, the host's own).
    pub fn kit_index(&self, id: &str) -> Option<usize> {
        self.kit_ids().iter().position(|k| k == id)
    }
    /// How many kits are mounted.
    pub fn kit_count(&self) -> usize {
        self.kit_ids().len()
    }
}

impl Kx<'_> {
    fn kit_hook(&mut self, kit: usize, hook: u8, args: impl FnOnce(&mut gripforge_abi::Writer)) {
        self.host.reads_stale();
        call(crate::func!(fs::KIT_HOOK), |w| {
            w.u32(kit as u32).u8(hook);
            args(w);
        });
        self.host.reads_stale();
    }
    /// `KitModule::scene_enter` of the kit `kit`.
    pub fn kit_scene_enter(&mut self, kit: usize, scene: &Json) {
        self.kit_hook(kit, fs::KIT_HOOK_SCENE_ENTER, |w| {
            w.value(scene);
        });
    }
    /// `KitModule::scene_exit` of the kit `kit`.
    pub fn kit_scene_exit(&mut self, kit: usize, scene: &Json) {
        self.kit_hook(kit, fs::KIT_HOOK_SCENE_EXIT, |w| {
            w.value(scene);
        });
    }
    /// `KitModule::component` of the kit `kit`.
    pub fn kit_component(
        &mut self,
        kit: usize,
        name: &str,
        entity: &str,
        value: &Json,
        instance: &Json,
    ) {
        self.kit_hook(kit, fs::KIT_HOOK_COMPONENT, |w| {
            w.str(name).str(entity).value(value).value(instance);
        });
    }
    /// `kithost.services@1.4` `project_docs`: the documents of a data collection of the project, replaced in the core
    /// (every kit reads them again). False when the core is older than 1.4.
    pub fn set_project_docs(&mut self, collection: &str, docs: &Json) -> bool {
        let mut w = crate::host::writer();
        w.str(collection).value(docs);
        let done = crate::host::call_core(crate::func!(fs::PROJECT_DOCS), &w).is_ok();
        if done {
            self.host.reread_project();
        }
        done
    }
    /// `kithost.services@1.4` `kit_config_set`: a dotted path of a mounted kit's config, in the project and in the kit.
    /// False when the core is older than 1.4.
    pub fn set_kit_config_path(&mut self, kit: &str, path: &str, value: &Json) -> bool {
        let mut w = crate::host::writer();
        w.str(kit).str(path).value(value);
        let done = crate::host::call_core(crate::func!(fs::KIT_CONFIG_SET), &w).is_ok();
        if done {
            self.host.reread_project();
        }
        done
    }
}
