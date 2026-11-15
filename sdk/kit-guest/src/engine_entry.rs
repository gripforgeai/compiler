//! `game.engine.contract@1` on the kit side (kit module ABI 1.2, docs/modules-de-kit.md): the three functions the core asks
//! of the game.engine module, answered by its `KitModule::engine_*` methods. Any other kit answers the defaults.

use crate::host::{KitModule, Kx};
use gripforge_abi::{kit, Reader, Writer};
use serde_json::Value as Json;

/// The functions without a context: true when `func` was one of them (its answer written).
pub(crate) fn plain(
    func: u32,
    m: &dyn KitModule,
    r: &mut Reader,
    w: &mut Writer,
) -> Result<bool, String> {
    match func {
        kit::ENGINE_PREPARE_PROJECT => {
            w.value(&m.engine_prepare_project(&r.value()?));
        }
        kit::ENGINE_KIT_APIS => {
            m.engine_kit_apis(&r.value()?);
        }
        kit::ENGINE_COLLECTION_OPS => {
            let mut list = match r.value()? {
                Json::Array(l) => l,
                _ => Vec::new(),
            };
            m.engine_collection_ops(&mut list, &r.value()?);
            w.value(&Json::Array(list));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// `engine_untapped(ctx, hook) -> bool`.
pub(crate) fn untapped(
    m: &dyn KitModule,
    kx: &mut Kx,
    r: &mut Reader,
    w: &mut Writer,
) -> Result<(), String> {
    let hook = r.string()?;
    w.bool(m.engine_untapped(kx, &hook));
    Ok(())
}
