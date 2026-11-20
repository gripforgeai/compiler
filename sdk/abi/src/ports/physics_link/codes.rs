//! The numeric codes of the link: the orders' kinds, the published flags, the slot info and kind (split out of physics_link.rs, unchanged).

/// The kinds of the orders (the ring's record kind).
pub mod order {
    pub const ADD_BODY: u32 = 1;
    pub const ADD_CHARACTER: u32 = 2;
    pub const ADD_STATIC_BOX: u32 = 3;
    pub const ADD_DYNAMIC_BOX: u32 = 4;
    pub const REMOVE: u32 = 5;
    pub const SET_BODY: u32 = 6;
    pub const SET_POSITION: u32 = 7;
    pub const IMPULSE: u32 = 8;
    pub const FORCE: u32 = 9;
    pub const TORQUE_IMPULSE: u32 = 10;
    pub const MOVE_KINEMATIC: u32 = 11;
    pub const SET_CHARACTER_POSITION: u32 = 12;
    pub const SET_GRAVITY: u32 = 13;
    pub const SET_MANUAL: u32 = 14;
    /// A step asked by the user (manual stepping).
    pub const STEP: u32 = 15;
    /// Publish whether this body is grounded (a probe the user reads).
    pub const WATCH_GROUNDED: u32 = 16;
    /// The published state moves to a bigger set (its address).
    pub const GROW: u32 = 17;
    pub const SET_COLLISION_SPACE: u32 = 18;
    /// Field-level writes: one field of a body, applied in order on the physics' freshest state.
    pub const SET_LINVEL: u32 = 19;
    pub const SET_ANGVEL: u32 = 20;
    pub const SET_ROTATION: u32 = 21;
    /// Set the simulation-owned fixed step, seconds (no cross-thread field mutation).
    pub const SET_FIXED_DT: u32 = 22;
    /// A joint (1.9): the first body's slot, the joint's name, then the second body's slot and the joint's words.
    pub const ADD_JOINT: u32 = 23;
    /// Let go of a joint: its name.
    pub const REMOVE_JOINT: u32 = 24;
    /// Hold or release one body (1.11): the mode in the first number of the payload.
    pub const HOLD_BODY: u32 = 25;
    // 26-29 (a vehicle, its commands, a buoyancy, a fluid source) are gone (#1761): written straight into the
    // physics' box by the kits (docs/decisions/2026-11-19-canaux-directs.md).
    // Questions: a ticket.
    pub const MOVE_CHARACTER: u32 = 32;
    pub const CAST_CHARACTER: u32 = 33;
    pub const RAYCAST: u32 = 34;
    pub const OVERLAP_SPHERE: u32 = 35;
    pub const OVERLAP_BOX: u32 = 36;
    /// 2.4: every hit of a ray through layers (`physics_link/questions.rs`).
    pub const RAYCAST_ALL: u32 = 37;
    pub const SHAPE_CAST: u32 = 38;
    pub const OVERLAP_SHAPE: u32 = 39;
    /// The last question.
    pub const QUESTION_LAST: u32 = 39;
    /// The speed of the game time a debugger holds (F9's slow and fast motion): the physics on its own loop
    /// ([`super::LOOP`], `serve_loop`) multiplies the wall time it plays by the first number (0.05 to 8; 1: real
    /// time). A setting the physics applies to its own step, never a clock it reads (decision « horloges et boucles »
    /// of 19/11, correctif 1: `docs/decisions/2026-11-19-horloges-et-boucles.md`, brought by PR #1790). The front
    /// sends it whatever the mode; on the user's thread ([`super::LIVE`], `step_live`) it is kept and never used: the
    /// `dt` of a tick is already game time, the slowed tick plays fewer steps (multiplying it too would slow twice).
    pub const SET_SPEED: u32 = 41;
    /// Held (first number 1) or let go (0) while the page is hidden in a solo game: the physics on its own loop plays
    /// no time while held and starts again from the turn it is let go (no catch-up), as the tick does. A setting it
    /// applies to its own step, never a clock it reads.
    pub const SET_HELD: u32 = 42;
    // 40 and 0x1000 on (a question and the orders of a capability, the bus of #1754) are gone (#1761): the link
    // carries no capability; a kit writes it into the module's box.
}

/// What the published record of a body says (`bodies::Row::flags`, contract `abi.physics.bodies`).
pub mod flag {
    /// The record holds the body's state.
    pub const BODY: u32 = 1;
    pub const CHARACTER: u32 = 2;
    pub const SLEEPING: u32 = 4;
    /// `GROUNDED` is known (the body is watched).
    pub const GROUND_KNOWN: u32 = 8;
    pub const GROUNDED: u32 = 16;
    pub const CHARACTER_GROUNDED: u32 = 32;
    /// `cp` holds the character's position.
    pub const CHARACTER_AT: u32 = 64;
    /// `pose` holds the pose of the transforms (the body is listed).
    pub const POSE: u32 = 128;
    /// The pose is a character's (its yaw is not the body's to give).
    pub const POSE_CHARACTER: u32 = 256;
}

/// What the port has under a slot's name (`slots::Row::flags`, contract `abi.physics.slots`).
pub mod info {
    pub const HAS_BODY: u32 = 1;
    pub const HAS_CHARACTER: u32 = 2;
}

/// The kind of a slot (`slots::Row::kind`).
pub mod kind {
    pub const NONE: u32 = 0;
    pub const DYNAMIC: u32 = 1;
    pub const KINEMATIC: u32 = 2;
    pub const STATIC: u32 = 3;
    pub const CHARACTER: u32 = 4;
}
