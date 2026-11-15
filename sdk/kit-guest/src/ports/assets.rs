//! `kx.assets()` of a kit module: the asset bindings of the core (`kithost.assets@1`), one call per read. Split out
//! of physics.rs as is.

use super::codec::{assets_fn as a, *};
use super::{args, ask, AssetPort};

/// The asset bindings of the core (`kithost.assets@1`), one call per read.
#[derive(Default)]
pub struct AssetsProxy {}

impl AssetsProxy {
    pub fn enter(&mut self) {}
}

impl AssetPort for AssetsProxy {
    fn url(&self, slot: &str) -> Option<String> {
        ask(
            crate::func!(a::URL),
            &args(|w| {
                w.str(slot);
            }),
            r_opt_string,
        )
    }
    fn bound(&self, slot: &str) -> bool {
        ask(
            crate::func!(a::BOUND),
            &args(|w| {
                w.str(slot);
            }),
            |r| r.bool(),
        )
    }
    fn variants(&self, slot: &str) -> Vec<String> {
        ask(
            crate::func!(a::VARIANTS),
            &args(|w| {
                w.str(slot);
            }),
            |r| r.strs(),
        )
    }
    fn bytes(&self, slot: &str) -> Option<Vec<u8>> {
        ask(
            crate::func!(a::BYTES),
            &args(|w| {
                w.str(slot);
            }),
            |r| {
                if !r.bool()? {
                    return Ok(None);
                }
                let n = r.u32()? as usize;
                let b = r
                    .bytes
                    .get(r.at..r.at + n)
                    .ok_or("kitcall: message too short (bytes)")?
                    .to_vec();
                r.at += n;
                Ok(Some(b))
            },
        )
    }
}
