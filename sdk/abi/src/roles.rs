//! The roles the core calls (docs/design/noyau-sans-noms-de-kits.md): the core never names a kit. A kit takes a role
//! by serving it as a capability (`services_for`, and a `provides` entry of its kit.api.json); the core calls the
//! role through the services it already has (`has_service`, `call_service`). One kit per role: the last one mounted
//! decides and the host says so in its logs. Nothing here is binary: a role is a name.
//!
//! Used by: the kit host (the session's `actions` op), the C gateway (`gfk_input_specs`), the kits that take a role.

/// What every role name starts with: never a family of kits, so never taken for a kit id.
pub const PREFIX: &str = "role.";

/// The table of the input actions of the game: `register(specs, owner)`, `actions()` (the JSON array of the actions,
/// in the order of their records in the zone).
pub const INPUT: &str = "role.input";

/// The debugger: the `expose` entries for `debug`, the counters' blocks (`counters`), the recorder's operations.
pub const DEBUG: &str = "role.debug";

/// The scene reader: the `expose` entries for `scene` (the elements a scene file may hold).
pub const SCENE: &str = "role.scene";

/// The passports of the assets: the `expose` entries for `passport`, `register` / `bind` of the passports an engine
/// reads.
pub const PASSPORT: &str = "role.passport";

/// The settings of the player: the kit that holds it publishes each setting's value in the game state as
/// `<its id>.value.<setting id>` (core.settings: `core.settings.value.audio.music.level`).
pub const SETTINGS: &str = "role.settings";

/// The physics of the world: the kit that holds it has the host's physics limits read from its own config at the
/// mount (`maxBodies`…), before any `setup`.
pub const PHYSICS: &str = "role.physics";

/// The mover of the bodies a kit drives (a character's capsule): `teleport(entity, [x, y, z])` answers `true` when it
/// moved that entity itself (its body, its fall, its stages); anything else: the core moves the entity.
pub const TELEPORT: &str = "role.teleport";

/// The replication of the game state between peers: `send(kind, body, target)` (`target`: `"all"` or a peer).
pub const REPLICATION: &str = "role.replication";

/// The role that reads the `expose` entries for `reader` (`debug`, `scene`, `passport`…): `role.<reader>`. A new
/// reader of `expose` needs nothing of the core.
pub fn expose_reader(reader: &str) -> String {
    format!("{PREFIX}{reader}")
}

/// Whether a capability is a role.
pub fn is_role(capability: &str) -> bool {
    capability.starts_with(PREFIX)
}
