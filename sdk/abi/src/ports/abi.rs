//! The physics module's entries (docs/kit-modules.md §7.10): what the core calls of a physics module that is a
//! library of its own — a relocatable wasm of the common memory on the web (`physics.rapier`), the same functions
//! linked natively elsewhere. One place for the signatures: the module checks its exports against these types, the
//! core calls them through these types.
//!
//! **Nothing is copied across.** The world is the module's, a block of the memory every module shares; it is named
//! by its address (`w`). A name goes in as the address and length of the caller's bytes. What the module answers
//! is written where the caller says (`out`: the caller's stack, a kit's buffer), or is a list of records the
//! module keeps in its world ([`PoseRec`], [`EventRec`], [`NameRef`]) that the caller reads where they are, names
//! included (the bytes of the module's own strings) — valid until the next call that changes the world.
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
/// colliders published in the module's memory, read in place by the navigation and the debugger), required.
pub const ABI: u32 = (2 << 16) | 6;

/// The oldest version a loader accepts ([`check_version`]): 2.6, the shape table is required. No module of an older
/// minor is kept working (the refonte breaks and asks for the current version).
pub const MIN_ABI: u32 = (2 << 16) | 6;

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

/// A body's pose of the last step, kept by the module ([`Fns::poses`]): every body that is not fixed.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PoseRec {
    pub name: *const u8,
    pub name_len: usize,
    /// 1: the body is a character's (its yaw is not the body's to give).
    pub character: u32,
    pub _pad: u32,
    pub p: [f64; 3],
    pub yaw: f64,
}

/// A contact event of the last step, kept by the module ([`Fns::events`]).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EventRec {
    pub a: *const u8,
    pub a_len: usize,
    pub b: *const u8,
    pub b_len: usize,
    pub begin: u32,
    pub _pad: u32,
    pub impulse: f64,
}

/// The words of a body description (`add_body`): kind, shape (kind, three numbers), position (3), rotation (4),
/// mass, friction, restitution, linear damping, angular damping, ccd, can sleep, lock rotation, lock tilt, gravity
/// scale, ground probe. A mesh's vertices and indices are given beside, where they are.
pub const DESC_WORDS: usize = 23;
/// The words of a body state (`body`, `set_body`): position (3), rotation (4), linear velocity (3), angular
/// velocity (3), sleeping (0 or 1) — the layout of the direct link `ph_body` of the kits.
pub const BODY_WORDS: usize = 14;

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
/// `what` 0 set_position, 1 apply_impulse, 2 apply_force, 3 apply_torque_impulse, 4 set_character_position.
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
/// normal at `out` (7 numbers), its entity's name at `name` (address, length — [`NO_NAME`] when none).
pub type Raycast = unsafe extern "C" fn(
    w: usize,
    args: *const f64,
    ex: *const u8,
    ex_len: usize,
    out: *mut f64,
    name: *mut NameRef,
) -> u32;
/// `what` 0 a sphere (`args` centre, radius), 1 a box (centre, size, then a rotation when `rotated` is 1): the names
/// found, in order, as records the module keeps; their number at `n`.
pub type Overlap = unsafe extern "C" fn(
    w: usize,
    what: u32,
    args: *const f64,
    rotated: u32,
    ex: *const u8,
    ex_len: usize,
    n: *mut usize,
) -> *const NameRef;
/// The poses of every body that is not fixed, in the module's order (records it keeps; their number at `n`).
pub type Poses = unsafe extern "C" fn(w: usize, n: *mut usize) -> *const PoseRec;
/// The contact events of the last step (records it keeps; their number at `n`).
pub type Events = unsafe extern "C" fn(w: usize, n: *mut usize) -> *const EventRec;
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
        }
    }
}
