//! The physics module's entries (docs/kit-modules.md §7.10): what the core calls of a physics module that is a
//! library of its own — a relocatable wasm of the common memory on the web (`physics.rapier`), the same functions
//! linked natively elsewhere. One place for the signatures: the module checks its exports against these types, the
//! core calls them through these types.
//!
//! **Nothing is copied across.** The world is the module's, a block of the memory every module shares; it is named
//! by its address (`w`). A name goes in as the address and length of the caller's bytes. What the module answers
//! is written where the caller says (`out`: the caller's stack, a kit's buffer), or is a list of records the
//! module keeps in its world, a block of a shared contract each (`abi.physics.poses`, `abi.physics.events`,
//! `abi.physics.names`, `abi.physics.hits`: a `TableHead`, then the rows at the stride it announces) that the caller
//! binds and reads where they are, names included (the bytes of the module's own strings) — valid until the next call
//! that changes the world.
//!
//! Every function is `unsafe extern "C"`: the addresses are of the shared memory, valid for what each says.

pub use super::abi_records::*;

/// The version of these entries (the module answers it by `gfp_abi`; the core refuses another major).
///
/// Major 2 requires the fixed-step order on physics links; an older server would silently ignore it.
/// Minors: 2 the ninth number of `add_character` (the skin ratio); 3 the core announces its own version to a
/// module of minor 3 or newer (`gfp_set` with [`SET_CORE_ABI`]), so that a module reads the words a core of an older
/// minor never wrote (the skin ratio) only from a core that wrote them: without the announcement the module reads the
/// 8 numbers of minor 1 and takes the default for the rest, never a word beyond the caller's buffer; 4 the collision
/// layers (two more words of a body description, [`DESC_WORDS_4`], read only from a core of minor 4) and the
/// questions of the layers (the optional entry `gfp_query`, [`Query`]); 5 the sensors (one more word,
/// [`DESC_WORDS_5`], read only from a core of minor 5); 6 the shape table (the entry `gfp_shapes`, [`Shapes`]: the
/// colliders published in the module's memory, read in place by the navigation and the debugger), required; 7 the
/// cone joint (`gfp_joint` reads [`super::JOINT_WORDS`] = 21 words: swing and twist after the 17 of 2.5); 8 the
/// vehicles (the optional entry `gfp_vehicle`, [`Vehicle`]; a module of 2.7 is still linked, without vehicles).
///
/// History since, the value unchanged (docs/decisions/versions-dev.md: the number is given at the tag):
/// - #1731, the generic floating of core.physics: the optional entry `gfp_fluid` ([`Fluid`]: a body's buoyancy, a
///   fluid source), and `set_buoyancy` 0x53c / `set_fluid` 0x53d of `kithost.physics`. A module without the entry is
///   still linked: nothing floats in it. Rebuild the physics module and the player.
/// - the bus of the ports (docs/decisions/2026-11-18-bus-ports.md, lecteur vierge, lot 2): the optional entry
///   `gfp_bus` ([`Bus`]: a capability's order, question, list of capabilities), the link's orders `BUS_ASK` (40) and
///   from `BUS_FIRST` (0x1000) on carried as they are, `bus_order` 0x5f0 / `bus_ask` 0x5f1 / `bus_capabilities` 0x5f2
///   of `kithost.physics`; an order of unknown kind is said once and counted. A module without the entry is still
///   linked: no capability. Rebuild the player and the physics module (the last time for a capability).
/// - the mailboxes (docs/decisions/2026-11-19-canaux-directs.md, #1761, step 1): `gfp_bus` answers `what` =
///   `bus::MAILBOXES` (3) with the module's boxes (block, kind, name), which the core writes once in the directory
///   it lays in the shared zone (`zone::state::MAILBOXES`, `mailbox::Table`); a kit writes its orders straight into
///   `physics/orders` and reads `physics/state` in place (`PhysicsPort::mailboxes`). Rebuild the player, the
///   physics module and the kits core.physics, locomotion.vehicle, debug.inspector.
/// - the old tunnels gone (#1761, step 2; refonte: no compatibility): the optional entries `gfp_vehicle` and
///   `gfp_fluid` (`abi::OPTIONAL` is now `gfp_query`, `gfp_joint`, `gfp_bus`), `gfp_bus` `what` ORDER / ASK, the
///   link's orders 26-29, `BUS_ASK` (40) and `BUS_FIRST` (0x1000 on), `kithost.physics` 0x539-0x53d and 0x5f0-0x5f1,
///   `Answer::Bus`. A vehicle, its commands, a passenger, a buoyancy and a fluid go only into `physics/orders`; a
///   vehicle's state is read only in `physics/state`. Rebuild the player, the physics module and every kit.
/// - the pose carries a quaternion (docs/design/pose-quaternion.md, #1739, lots 1-2): the published record of a body
///   (`pose` of `abi.physics.bodies`) is x, y, z, qx, qy, qz, qw (7 numbers, the body's whole rotation; a character's,
///   its cap) instead of x, y, z, yaw: the record grows by 24 bytes. Rebuild the physics module and the player.
/// - the intention read by the physics (docs/design/intention-physique.md, lot 1): the registry of intentions of the
///   physics module (base intentions `velocity`, `target`, `stop` given at once, a kit's own registered from its
///   instance on the physics' thread), the code `intent_run` 0x1007 in `physics/orders`, `character_state` 0x1008 in
///   `physics/state`, the dated journal `physics/events` (`motion_event` 0x1009, a third box of the module, of the
///   kind `state`). Added only, the value unchanged; no structure of this crate changed (`drive` 0x1004 stays until
///   lot 3b). Rebuild the physics module and the kits that launch or register intentions; not the player.
///   Then (same lot, PR #1776): the base intentions `follow`, `force`, `jump`, `burst`, `set_velocity` and the leash
///   of `velocity`, run on characters, kinematic and dynamic bodies; `run` carries the address of the launching kit's
///   own box of events (`intent_guest::Inbox`), where the physics also delivers them; `intents_state` 0x100a in
///   `physics/state` (the debugger's `physics.intents`); `mailbox::MAGIC` made public (a writer handed a bare box
///   address checks it). The native host calls `gfx_kit_role` like the web loader. The value unchanged.
///   Then (lot 3a, PR to come on `feat/intention-lot3`): the `reset` of `intent_run` removes only the intentions of
///   an older generation (the next holder of a body launches for the new generation, its message may arrive before
///   the owner's reset), and `character_state` is published for every body the registry knows (driven, or reset and
///   not gone), so a kit that takes a body over reads its generation. The words unchanged, the value unchanged.
///   Rebuild the physics module and the kits locomotion.mode.grind, grapple, mount, parachute; not the player.
///   Then (20/11, branch `feat/camera-rendu-10hz`): the heading of a character is the physics' (docs/design/
///   intention-physique.md « Le cap »): the base intention `turn` (group `turn`), `IntentStep` grows by `yaw`,
///   `out_yaw`, `out_yaw_mode` at its end (the physics allocates it: a kit's own step reads the same prefix), the
///   published pose of a character carries the rotation of its body, a yaw written on a character is ordered as
///   `SET_ROTATION`. The value unchanged. Rebuild the physics module, the core (`kithost.core`) and every kit that
///   launches or registers intentions (core.locomotion first); not the player.
///   Then (20/11, branch `feat/entrees-datees`, docs/decisions/2026-11-20-entrees-datees-et-intentions.md): `turn`
///   takes `follow`, `pointer`, `scale`, `anchor` at the end of its parameters (defaults 0): bound to the input, the
///   physics reads a `PointerFollow` block and the pointer total in place at each step (`intent::follow_now_at`). The
///   value unchanged. Rebuild the physics module, core.locomotion, locomotion.first_person; not the player, not the core.
///   Then (20/11, branch `feat/entrees-hors-tick`, docs/design/etat-des-actions-hors-tick.md): `velocity` takes
///   `input` (3 × address, map), `frame` (follow, pointer, scale, anchor) and `inputSpeed` at the end of its parameters
///   (defaults 0, 13 → 24 words): bound to the input, the physics reads the horizontal target in `ActionState` blocks
///   (`intent::action_now_at`) at each step; `turn` adds the look's stick named by its `PointerFollow` block
///   (`intent::stick_of`). The value unchanged. Rebuild the physics module, core.locomotion, core.input,
///   locomotion.third_person, locomotion.first_person; not the player.
/// - the counters of the physics in F9 (bugs of the night of 20/11, PR feat/physique-compteurs-f9): two padding words of
///   `physics_link::PhysicsLink` become counters the serving side writes, `skipped` (steps never played under
///   saturation) and `dropped_ms` (physics time thrown away); the layout and the value unchanged. Rebuild the physics
///   module and the core for the counters to move (an older module leaves them at 0).
/// - F9's slow and fast motion reach the physics on its own loop (bugs of the night of 20/11, PR
///   feat/physique-vitesse-f9): the link's order `SET_SPEED` (41, one number 0.05 to 8), a setting the loop applies
///   to the wall time it plays (a physics on the user's thread keeps it unused: the slowed tick already plays fewer
///   steps). Added only, the value unchanged; an older physics module counts it as unknown. Rebuild the physics
///   module and the core.
/// - a hidden page holds the physics of a solo game (bugs of the night of 20/11, PR feat/physique-onglet-cache): the
///   link's order `SET_HELD` (42, one number: 1 held, 0 let go), a setting the loop applies to its own step (no time
///   played while held, no catch-up after). Added only, the value unchanged. Rebuild the physics module and the core.
/// - 20/11 (feat/abi-dev, docs/decisions/2026-11-20-abi-dev.md): the value is `ABI_DEV` in the repository, the
///   number of core/abi/abi-versions.json is written at the tag only. Rebuild every binary once (player, modules,
///   gateways): a binary of before answers its old number and is refused.
/// - 20/11 (feat/abi-contrat-lot2, #1872, ABI par contrat lot 2, docs/decisions/2026-11-20-abi-par-contrat.md): the
///   published state (`PubHead`, `BodyRec`, `SlotInfo`) becomes the contracts `abi.physics.bodies`, `moving`,
///   `slots` (a `TableHead` per block, read at the stride it announces); the words of a body description, a body
///   state, a simple shape, a hit, a joint, a vehicle, a buoyancy, a fluid become contracts of words (indices
///   generated); the shape table becomes `abi.physics.shape_table` + `abi.physics.shapes`, complete (the
///   characters' capsules, a compound as its parts, the codes of the classic shapes); a physics module declares its
///   contracts (`SERVE_CONTRACTS`, checked at load, web and native). The value unchanged. Rebuild the physics module,
///   kithost.core, the native player and every kit module (the façade declares the words and the shapes).
///   Then (same PR): `gfp_poses`, `gfp_events`, `gfp_overlap`, `gfp_query` answer the address of a block of the
///   contracts `abi.physics.poses`, `events`, `names`, `hits` (no more `n` out, no more `PoseRec`, `EventRec`,
///   `NameRef`, `HitRec` with pointers and `usize`: one layout in wasm32 and natively), `gfp_raycast` writes one
///   row of `abi.physics.names`; the answers of the link's tickets are a block of `abi.physics.answers` (`TableHead`,
///   rows at its stride, checked once by the side that serves); the sensors' block of active contacts (#1856) is
///   the contract `abi.sensor.contacts`. The value unchanged. Rebuild the physics module, kithost.core, the native
///   player and every kit module (the façade declares `abi.sensor.contacts`); not the web player.
/// - 20/11 (feat/abi-contrat-lot2c, #1885, end of the lot 2, docs/decisions/2026-11-20-abi-par-contrat.md): the link itself
///   (`PhysicsLink` after the loop's head: rings, answers, `set`, `sim`, `shapes`, the counters) becomes the contract
///   `abi.physics.link` and its published set (`PubSet`) `abi.physics.set` (a `TableHead` each, every address u64 in
///   wasm32 as natively, checked once by the side that serves); `IntentStep` becomes `abi.intent.step` (one block per
///   running intention, `params`, `memo`, `others` as u64); the parameters of the base intentions become contracts of
///   words `abi.intent.<name>` (`velocity` 13 → 24 words written in its history); the table of the revision keys
///   becomes `abi.physics.revisions`. The value unchanged. Rebuild the physics modules (rapier, null), kithost.core,
///   the native player and every kit module (the façade declares `abi.intent.step` and the parameters); not the web
///   player.
pub const ABI: u32 = crate::ABI_DEV;

/// The oldest version a loader accepts ([`check_version`]); `ABI_DEV` in the repository (at the tag 2.7): a joint is 21 words (a module of 2.6 would read 17
/// and take a cone for an unknown kind). No module of an older minor is kept working (the refonte breaks and asks
/// for the current version).
pub const MIN_ABI: u32 = crate::ABI_DEV;

/// Whether a module answering `version` (its `gfp_abi`) may be linked: the major of [`ABI`], minor at least that of
/// [`MIN_ABI`]. The error says the version found and the one asked.
pub fn check_version(version: u32) -> Result<(), String> {
    let (major, minor) = (version >> 16, version & 0xffff);
    if major != ABI >> 16 || minor < MIN_ABI & 0xffff {
        return Err(format!(
            "physics {major}.{minor}, this host needs {}.{} or newer of major {}",
            MIN_ABI >> 16,
            MIN_ABI & 0xffff,
            ABI >> 16
        ));
    }
    Ok(())
}

/// The `what` of `gfp_set` by which the core tells its version (`x` = its [`ABI`]); 0 and 1 are manual stepping and
/// gravity. Sent only to a module that answers minor 3 or newer: an older one takes any other `what` for the gravity.
pub const SET_CORE_ABI: u32 = 2;

/// No name (an exclusion not given, a hit without an entity): the length.
pub const NO_NAME: usize = usize::MAX;

/// The contact events of the last step, kept by the module ([`Fns::events`]: the block; contract
/// `abi.physics.events`; until #1872 `EventRec`).
pub use gripforge_contracts::abi_physics_events as events;
/// The poses of the last step, kept by the module ([`Fns::poses`]: the block): every body that is not fixed (contract
/// `abi.physics.poses`, core/abi/contracts/abi.physics.poses.json; until #1872 `PoseRec`, two words of `usize`).
pub use gripforge_contracts::abi_physics_poses as poses;
/// A name's length in a row (`Str::len`) when there is none (a hit without an entity).
pub const NO_NAME_LEN: u32 = u32::MAX;

/// The words of a body description (`add_body`; contract `abi.physics.body_desc`, core/abi/contracts/
/// abi.physics.body_desc.json): kind, shape (code, three numbers), position, rotation, mass, friction, restitution,
/// damping, ccd, sleep, locks, gravity scale, ground probe, then the layers and the sensor flag (with defaults). The
/// index of each word is generated (`body_desc::at`), read by its accessor (`body_desc::mass(words)`…). A mesh's
/// vertices and indices are given beside, where they are.
pub use gripforge_contracts::abi_physics_body_desc as body_desc;
/// The words of a body state (`body`, `set_body`; contract `abi.physics.body_state`): position, rotation, linear and
/// angular velocity, sleeping — the layout of the direct link `ph_body` of the kits.
pub use gripforge_contracts::abi_physics_body_state as body_state;
/// The words of a box (`add_box`, the orders ADD_STATIC_BOX / ADD_DYNAMIC_BOX; contract `abi.physics.box_desc`): a
/// static box the words before `mass`.
pub use gripforge_contracts::abi_physics_box_desc as box_desc;
/// The words of a character (`add_character`, the order ADD_CHARACTER; contract `abi.physics.character_desc`): a core
/// that never announced minor 2 wrote the words before `skin_ratio` only (`REQUIRED_WORDS`).
pub use gripforge_contracts::abi_physics_character_desc as character_desc;
/// The words of an overlap (`overlap`, the orders OVERLAP_SPHERE / OVERLAP_BOX; contracts `abi.physics.overlap_*`).
pub use gripforge_contracts::{
    abi_physics_overlap_box as overlap_box, abi_physics_overlap_sphere as overlap_sphere,
};
/// The words of a ray (`raycast`, the order RAYCAST; contract `abi.physics.ray`) and of its hit (`abi.physics.ray_hit`).
pub use gripforge_contracts::{abi_physics_ray as ray, abi_physics_ray_hit as ray_hit};
/// The words of a body description a core of this build writes (all its fields).
pub const DESC_WORDS: usize = body_desc::WORDS;
/// The words of a body state.
pub const BODY_WORDS: usize = body_state::WORDS;

/// The words of a body state (contract `abi.physics.body_state`), every index generated.
pub fn state_words(
    p: [f64; 3],
    q: [f64; 4],
    v: [f64; 3],
    w: [f64; 3],
    sleeping: bool,
) -> [f64; BODY_WORDS] {
    use body_state::at;
    let mut s = [0.0; BODY_WORDS];
    s[at::POSITION..at::POSITION + 3].copy_from_slice(&p);
    s[at::ROTATION..at::ROTATION + 4].copy_from_slice(&q);
    s[at::LINVEL..at::LINVEL + 3].copy_from_slice(&v);
    s[at::ANGVEL..at::ANGVEL + 3].copy_from_slice(&w);
    s[at::SLEEPING] = sleeping as u8 as f64;
    s
}

/// The words of a character (contract `abi.physics.character_desc`), every index generated: what `add_character` and
/// the order ADD_CHARACTER carry.
pub fn character_words(
    position: [f64; 3],
    radius: f64,
    height: f64,
    max_slope_deg: f64,
    step_height: f64,
    snap_distance: f64,
    skin_ratio: f64,
) -> [f64; character_desc::WORDS] {
    use character_desc::at;
    let mut w = [0.0; character_desc::WORDS];
    w[at::POSITION..at::POSITION + 3].copy_from_slice(&position);
    w[at::RADIUS] = radius;
    w[at::HEIGHT] = height;
    w[at::MAX_SLOPE_DEG] = max_slope_deg;
    w[at::STEP_HEIGHT] = step_height;
    w[at::SNAP_DISTANCE] = snap_distance;
    w[at::SKIN_RATIO] = skin_ratio;
    w
}

/// The words of a box (contract `abi.physics.box_desc`): a static box sends `box_desc::REQUIRED_WORDS` of them.
pub fn box_words(center: [f64; 3], size: [f64; 3], mass: f64) -> [f64; box_desc::WORDS] {
    use box_desc::at;
    let mut w = [0.0; box_desc::WORDS];
    w[at::CENTER..at::CENTER + 3].copy_from_slice(&center);
    w[at::SIZE..at::SIZE + 3].copy_from_slice(&size);
    w[at::MASS] = mass;
    w
}

/// The words of a ray (contract `abi.physics.ray`).
pub fn ray_words(origin: [f64; 3], dir: [f64; 3], max_toi: f64) -> [f64; ray::WORDS] {
    use ray::at;
    let mut w = [0.0; ray::WORDS];
    w[at::ORIGIN..at::ORIGIN + 3].copy_from_slice(&origin);
    w[at::DIR..at::DIR + 3].copy_from_slice(&dir);
    w[at::MAX_TOI] = max_toi;
    w
}

/// The words of an overlap by a sphere (contract `abi.physics.overlap_sphere`).
pub fn overlap_sphere_words(center: [f64; 3], radius: f64) -> [f64; overlap_sphere::WORDS] {
    use overlap_sphere::at;
    let mut w = [0.0; overlap_sphere::WORDS];
    w[at::CENTER..at::CENTER + 3].copy_from_slice(&center);
    w[at::RADIUS] = radius;
    w
}

/// The words of an overlap by a box (contract `abi.physics.overlap_box`): no rotation, the identity and `rotated` 0.
pub fn overlap_box_words(
    center: [f64; 3],
    size: [f64; 3],
    rotation: Option<[f64; 4]>,
) -> [f64; overlap_box::WORDS] {
    use overlap_box::at;
    let mut w = [0.0; overlap_box::WORDS];
    w[at::CENTER..at::CENTER + 3].copy_from_slice(&center);
    w[at::SIZE..at::SIZE + 3].copy_from_slice(&size);
    w[at::ROTATION..at::ROTATION + 4].copy_from_slice(&rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]));
    w[at::ROTATED] = rotation.is_some() as u8 as f64;
    w
}

pub type New = unsafe extern "C" fn() -> usize;
pub type Drop = unsafe extern "C" fn(w: usize);
pub type Abi = unsafe extern "C" fn() -> u32;
pub type Step = unsafe extern "C" fn(w: usize, dt: f64);
/// Takes what the kits left for the physics in the entity rows of the zone (the inbox, the forces: `world` is the
/// `WORLD` buffer, `force_at` the offset of `physics.force@1` in a row or `u32::MAX`), then steps when `step` is 1 and
/// stepping is not manual. Answers the steps made (0 or 1).
pub type Phase = unsafe extern "C" fn(
    w: usize,
    world: *mut u8,
    world_bytes: u32,
    force_at: u32,
    dt: f64,
    step: u32,
) -> u32;
pub type AddBody = unsafe extern "C" fn(
    w: usize,
    name: *const u8,
    len: usize,
    desc: *const f64,
    vertices: *const f32,
    nv: usize,
    indices: *const u32,
    ni: usize,
);
/// `args`: position (3), radius, height, max slope (deg), step height, snap distance.
pub type AddCharacter =
    unsafe extern "C" fn(w: usize, name: *const u8, len: usize, args: *const f64);
/// `what` 0 a static box (centre, size), 1 a dynamic box (centre, size, mass): `args` 6 or 7 numbers.
pub type AddBox =
    unsafe extern "C" fn(w: usize, what: u32, name: *const u8, len: usize, args: *const f64);
pub type CollisionSpace = unsafe extern "C" fn(
    w: usize,
    name: *const u8,
    len: usize,
    space: *const u8,
    space_len: usize,
) -> u32;
pub type Remove = unsafe extern "C" fn(w: usize, name: *const u8, len: usize);
/// `what` 0 has_body, 1 has_character, 2 grounded, 3 character_grounded, 4 manual_stepping (no name).
pub type Flag = unsafe extern "C" fn(w: usize, what: u32, name: *const u8, len: usize) -> u32;
pub type Mass = unsafe extern "C" fn(w: usize, name: *const u8, len: usize) -> f64;
/// 1 and [`BODY_WORDS`] numbers at `out`, or 0.
pub type Body = unsafe extern "C" fn(w: usize, name: *const u8, len: usize, out: *mut f64) -> u32;
pub type SetBody = unsafe extern "C" fn(w: usize, name: *const u8, len: usize, state: *const f64);
/// `what` 0 set_position, 1 apply_impulse, 2 apply_force, 3 apply_torque_impulse, 4 set_character_position, 5 hold_body (1.11: x the mode, 1 hold, 0 release, 2 release at rest).
pub type Vector =
    unsafe extern "C" fn(w: usize, what: u32, name: *const u8, len: usize, x: f64, y: f64, z: f64);
/// `args`: position (3), then the rotation (4) when `rotated` is 1.
pub type MoveKinematic =
    unsafe extern "C" fn(w: usize, name: *const u8, len: usize, args: *const f64, rotated: u32);
/// `what` 0 move_character (`args`: delta), 1 cast_character (`args`: origin, delta): 1 and the displacement and
/// grounded at `out` (4 numbers), or 0.
pub type Character = unsafe extern "C" fn(
    w: usize,
    what: u32,
    name: *const u8,
    len: usize,
    args: *const f64,
    out: *mut f64,
) -> u32;
/// 1 and the position at `out` (3), or 0.
pub type CharacterPosition =
    unsafe extern "C" fn(w: usize, name: *const u8, len: usize, out: *mut f64) -> u32;
/// `args`: origin, direction, max toi (7); the exclusion's name ([`NO_NAME`]: none). 0 no hit; 1 a hit: toi, point,
/// normal at `out` (7 numbers), its entity's name at `name` (one row of `abi.physics.names`: length
/// [`NO_NAME_LEN`] when none).
pub type Raycast = unsafe extern "C" fn(
    w: usize,
    args: *const f64,
    ex: *const u8,
    ex_len: usize,
    out: *mut f64,
    name: *mut names::Row,
) -> u32;
/// `what` 0 a sphere (`args` centre, radius), 1 a box (centre, size, then a rotation when `rotated` is 1): the names
/// found, in order: the address of the `abi.physics.names` block the module keeps (its count: the names).
pub type Overlap = unsafe extern "C" fn(
    w: usize,
    what: u32,
    args: *const f64,
    rotated: u32,
    ex: *const u8,
    ex_len: usize,
) -> *const u8;
/// The poses of every body that is not fixed, in the module's order: the address of the `abi.physics.poses` block it
/// keeps.
pub type Poses = unsafe extern "C" fn(w: usize) -> *const u8;
/// The contact events of the last step: the address of the `abi.physics.events` block it keeps.
pub type Events = unsafe extern "C" fn(w: usize) -> *const u8;
/// `what` 0 manual stepping (`x` 0 or 1), 1 gravity.
pub type Set = unsafe extern "C" fn(w: usize, what: u32, x: f64, y: f64, z: f64);

/// The module's entries, by the names it exports.
#[derive(Clone, Copy)]
pub struct Fns {
    pub abi: Abi,
    pub new: New,
    pub drop: Drop,
    pub step: Step,
    pub phase: Phase,
    pub add_body: AddBody,
    pub add_character: AddCharacter,
    pub add_box: AddBox,
    pub remove: Remove,
    pub flag: Flag,
    pub mass: Mass,
    pub body: Body,
    pub set_body: SetBody,
    pub vector: Vector,
    pub move_kinematic: MoveKinematic,
    pub character: Character,
    pub character_position: CharacterPosition,
    pub raycast: Raycast,
    pub overlap: Overlap,
    pub poses: Poses,
    pub events: Events,
    pub set: Set,
    pub collision_space: CollisionSpace,
    /// `gfp_query` (2.4), when the module has it.
    pub query: Option<Query>,
    /// `gfp_joint` (2.5), when the module has it.
    pub joint: Option<Joint>,
    /// `gfp_bus` (dev), when the module has it.
    pub bus: Option<Bus>,
    /// `gfp_shapes` (2.6, required).
    pub shapes: Shapes,
}

/// The exports, in the order of [`Fns`]: what the loader looks up in the module and hands the core (`gf_physics_link`).
pub const NAMES: [&str; 24] = [
    "gfp_abi",
    "gfp_new",
    "gfp_drop",
    "gfp_step",
    "gfp_phase",
    "gfp_add_body",
    "gfp_add_character",
    "gfp_add_box",
    "gfp_remove",
    "gfp_flag",
    "gfp_mass",
    "gfp_body",
    "gfp_set_body",
    "gfp_vector",
    "gfp_move_kinematic",
    "gfp_character",
    "gfp_character_position",
    "gfp_raycast",
    "gfp_overlap",
    "gfp_poses",
    "gfp_events",
    "gfp_set",
    "gfp_collision_space",
    "gfp_shapes",
];

impl Fns {
    /// The entries from their places (in the order of [`NAMES`]): function table indices on wasm, addresses natively.
    ///
    /// # Safety
    /// Each place is the module's export of that name, with the signature of its type here.
    pub unsafe fn from_places(p: &[usize; 24]) -> Fns {
        use std::mem::transmute as t;
        Fns {
            abi: t::<usize, Abi>(p[0]),
            new: t::<usize, New>(p[1]),
            drop: t::<usize, Drop>(p[2]),
            step: t::<usize, Step>(p[3]),
            phase: t::<usize, Phase>(p[4]),
            add_body: t::<usize, AddBody>(p[5]),
            add_character: t::<usize, AddCharacter>(p[6]),
            add_box: t::<usize, AddBox>(p[7]),
            remove: t::<usize, Remove>(p[8]),
            flag: t::<usize, Flag>(p[9]),
            mass: t::<usize, Mass>(p[10]),
            body: t::<usize, Body>(p[11]),
            set_body: t::<usize, SetBody>(p[12]),
            vector: t::<usize, Vector>(p[13]),
            move_kinematic: t::<usize, MoveKinematic>(p[14]),
            character: t::<usize, Character>(p[15]),
            character_position: t::<usize, CharacterPosition>(p[16]),
            raycast: t::<usize, Raycast>(p[17]),
            overlap: t::<usize, Overlap>(p[18]),
            poses: t::<usize, Poses>(p[19]),
            events: t::<usize, Events>(p[20]),
            set: t::<usize, Set>(p[21]),
            collision_space: t::<usize, CollisionSpace>(p[22]),
            shapes: t::<usize, Shapes>(p[23]),
            query: None,
            joint: None,
            bus: None,
        }
    }
}
