// `game.engine.contract@1`: the functions of `kithost.kit@1.2` (ABI 2.2) only the game.engine module serves, included in
// `crate::kit` (docs/modules-de-kit.md).

/// 1.2, `game.engine.contract@1` (game.engine only, `info.engine` says it): (project value) → value, the project with
/// its mods applied and its registries resolved.
pub const ENGINE_PREPARE_PROJECT: u32 = 40;
/// 1.2, `game.engine.contract@1`: (list value, ops value) → value, the list with the deferred operations of a mod.
pub const ENGINE_COLLECTION_OPS: u32 = 41;
/// 1.2, `game.engine.contract@1`: (ctx, hook string) → bool, no tap listens to the hook now.
pub const ENGINE_UNTAPPED: u32 = 42;
/// 1.3, `game.engine.contract@1`: (apis value) → nothing, the `kit.api.json` of the kits of the lock by kit id, given once
/// at mount before `ENGINE_PREPARE_PROJECT` (what the engine reads of the other kits). A module before 1.3 does not
/// answer it: it keeps game.engine's own API only.
pub const ENGINE_KIT_APIS: u32 = 43;
/// The name a module exports in `info.engine` when it serves the three functions above.
pub const ENGINE_CONTRACT: &str = "game.engine.contract@1";
