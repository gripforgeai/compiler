//! The animation layers as a kit module sees them (the logic lives in the core: gripforge_kithost::anim_layers,
//! reached through `kithost.world@1.6`, see `Kx::play_anim_once_layered` and the others in kx_world.rs).
/// `ANIM_LAYERS_COMPONENT`: the game's table of action layers of an entity.
pub const COMPONENT: &str = "anim.layers";
pub const MOVEMENTS: [&str; 4] = ["free", "slow", "lock", "root"];
