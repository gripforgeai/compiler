//! Measures only (feature `entry-probe`): parts of an entry of this kit module run alone, as many times as the core
//! asks, so that their cost is the slope of the step against that count (docs/kit-modules.md §9.14). No hook runs.

use crate::host::{instance, Host};
use gripforge_abi::{self as kc, Ctx};

/// What [`crate::host::tick`] does for a `func` that carries a probe bit. Answers 2 (nothing of the kit ran).
///
/// # Safety
/// As [`crate::host::tick`].
pub unsafe fn tick(func: u32, ctx: *const kc::fast::HookCtx) -> u32 {
    let n = (*ctx).frame;
    if func & kc::fast::PROBE_ENTER != 0 {
        // The façade's prologue of a per-tick hook: the instance found, what it read before dropped.
        let ctx: Ctx = (*ctx).into();
        let Some(ptr) = instance(ctx.inst, ctx.kit) else {
            return 1;
        };
        let host: &mut Host = &mut *ptr;
        host.enter(ctx);
        std::hint::black_box(&host.tick);
    } else if func & kc::fast::PROBE_LINK != 0 {
        for _ in 0..n {
            std::hint::black_box(crate::direct::link_probe());
        }
    } else if func & kc::fast::PROBE_SVC != 0 {
        crate::svc::probe(n);
    } else if func & kc::fast::PROBE_ALLOC != 0 {
        for _ in 0..n {
            drop(std::hint::black_box(Box::new([0u64; 6])));
        }
    }
    2
}
