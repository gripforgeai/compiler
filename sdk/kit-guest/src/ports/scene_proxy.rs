//! The engine's tree of a kit module (`kithost.scene@1`, core/abi/interfaces/scene.gfi): each method one call to the
//! core, which answers with the port the gateway plugged (gripforge-kit-api src/scene.rs). Handles are the engine's
//! own ids, kept by the kit as plain numbers; nothing of the engine's objects is copied into the module.

use super::codec::{scene_fn as f, *};
use super::{args, ask};
use gripforge_abi::ports::{ScenePort, Trs};

/// `kx.scene()`: the engine's tree, through the core.
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneProxy;

fn r_trs(r: &mut gripforge_abi::Reader) -> R<Option<Trs>> {
    if !r.bool()? {
        return Ok(None);
    }
    let mut t = [0.; 10];
    for v in &mut t {
        *v = r.f64()?;
    }
    Ok(Some(t))
}

impl SceneProxy {
    /// The object at `path`, else the first tagged `tag`, else a new one (`class`, `name`) under the game's root:
    /// `(handle, adopted)` (gripforge-kit-api `SceneAccess::adopt`).
    pub fn adopt(&self, path: &str, tag: &str, class: &str, name: &str) -> (u64, bool) {
        ask(
            crate::func!(f::ADOPT),
            &args(|w| {
                w.str(path).str(tag).str(class).str(name);
            }),
            |r| Ok((r.u64()?, r.bool()?)),
        )
    }
    /// The engine that serves, as an owned text (the trait's `engine` lends nothing across a call).
    pub fn engine_name(&self) -> String {
        ask(crate::func!(f::ENGINE), &args(|_| {}), |r| r.string())
    }
}

impl ScenePort for SceneProxy {
    fn root(&mut self) -> u64 {
        ask(crate::func!(f::ROOT), &args(|_| {}), |r| r.u64())
    }
    fn find(&mut self, path: &str) -> u64 {
        ask(
            crate::func!(f::FIND),
            &args(|w| {
                w.str(path);
            }),
            |r| r.u64(),
        )
    }
    fn tagged(&mut self, tag: &str) -> Vec<u64> {
        ask(
            crate::func!(f::TAGGED),
            &args(|w| {
                w.str(tag);
            }),
            |r| (0..r.u32()?).map(|_| r.u64()).collect(),
        )
    }
    fn transform(&mut self, h: u64) -> Option<Trs> {
        ask(
            crate::func!(f::TRANSFORM),
            &args(|w| {
                w.u64(h);
            }),
            r_trs,
        )
    }
    fn set_transform(&mut self, h: u64, t: &Trs) -> bool {
        ask(
            crate::func!(f::SET_TRANSFORM),
            &args(|w| {
                w.u64(h);
                t.iter().for_each(|v| {
                    w.f64(*v);
                });
            }),
            |r| r.bool(),
        )
    }
    fn visible(&mut self, h: u64) -> Option<bool> {
        ask(
            crate::func!(f::VISIBLE),
            &args(|w| {
                w.u64(h);
            }),
            |r| {
                if r.bool()? {
                    r.bool().map(Some)
                } else {
                    Ok(None)
                }
            },
        )
    }
    fn set_visible(&mut self, h: u64, on: bool) -> bool {
        ask(
            crate::func!(f::SET_VISIBLE),
            &args(|w| {
                w.u64(h).bool(on);
            }),
            |r| r.bool(),
        )
    }
    fn create(&mut self, parent: u64, class: &str, name: &str) -> u64 {
        ask(
            crate::func!(f::CREATE),
            &args(|w| {
                w.u64(parent).str(class).str(name);
            }),
            |r| r.u64(),
        )
    }
    fn destroy(&mut self, h: u64) -> bool {
        ask(
            crate::func!(f::DESTROY),
            &args(|w| {
                w.u64(h);
            }),
            |r| r.bool(),
        )
    }
    fn parent(&mut self, h: u64) -> u64 {
        ask(
            crate::func!(f::PARENT),
            &args(|w| {
                w.u64(h);
            }),
            |r| r.u64(),
        )
    }
    fn entity_node(&mut self, entity: u64) -> u64 {
        ask(
            crate::func!(f::ENTITY_NODE),
            &args(|w| {
                w.u64(entity);
            }),
            |r| r.u64(),
        )
    }
    fn set_node_turn(&mut self, node: u64, t: &Trs) -> bool {
        ask(
            crate::func!(f::SET_NODE_TURN),
            &args(|w| {
                w.u64(node);
                t.iter().for_each(|v| {
                    w.f64(*v);
                });
            }),
            |r| r.bool(),
        )
    }
}

impl crate::host::Kx<'_> {
    /// `ctx.scene`: the engine's tree (`kithost.scene@1`), by handle, through the core.
    pub fn scene(&self) -> SceneProxy {
        SceneProxy
    }
}
