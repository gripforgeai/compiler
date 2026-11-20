//! The engine's tree as a kit reaches it (`kithost.scene@1`, core/abi/interfaces/scene.gfi): the gateway's own scene
//! access — Godot nodes, Unity GameObjects, Unreal actors, Bevy entities — given to a kit module by handle.
//!
//! Who uses it: the core serves it (gripforge-kit-api `SceneAccess`, src/kitcall/scene.rs) over the port the gateway
//! plugs in (`Host::scene`); a kit module calls it through its façade (gripforge-kit-guest `SceneProxy`). Each gateway
//! answers with its engine's own tools on the engine's own objects: nothing is copied, nothing is mirrored.
//!
//! Space: a transform is world space in the canonical space of the kits (metres, right-handed, +Y up); the gateway
//! converts (Unity's left-handed axes, Unreal's centimetres and +Z up), as it does for the entities.

/// A transform: position xyz, rotation quaternion xyzw, scale xyz.
pub type Trs = [f64; 10];

/// The identity transform.
pub const TRS_IDENTITY: Trs = [0., 0., 0., 0., 0., 0., 1., 1., 1., 1.];

/// What a gateway serves of its engine's tree. Every method is about one object, by its handle (the engine's own id;
/// 0 is none). The defaults answer "nothing": an engine that has no tree wired answers none, and the kit falls back.
pub trait ScenePort {
    /// The engine that serves (`godot`, `unity`, `unreal`, `bevy`); empty when none.
    fn engine(&self) -> &str {
        ""
    }
    /// Where a kit creates: the root the game named (`rootNode`), else the current scene.
    fn root(&mut self) -> u64 {
        0
    }
    /// An object by its path in the tree (relative to the current scene, or absolute).
    fn find(&mut self, _path: &str) -> u64 {
        0
    }
    /// The objects that carry `tag` (Godot group, Unity tag, Unreal actor tag, Bevy marker), in the tree's order.
    fn tagged(&mut self, _tag: &str) -> Vec<u64> {
        Vec::new()
    }
    /// Its world transform.
    fn transform(&mut self, _h: u64) -> Option<Trs> {
        None
    }
    /// Sets its world transform with the engine's own setter; false when it has none.
    fn set_transform(&mut self, _h: u64, _t: &Trs) -> bool {
        false
    }
    /// Shown or hidden (its own flag, not its ancestors').
    fn visible(&mut self, _h: u64) -> Option<bool> {
        None
    }
    fn set_visible(&mut self, _h: u64, _on: bool) -> bool {
        false
    }
    /// A new object of `class` (the engine's class or prefab / scene path; empty: its plain node) named `name`,
    /// under `parent` (0: [`ScenePort::root`]); 0 when it could not.
    fn create(&mut self, _parent: u64, _class: &str, _name: &str) -> u64 {
        0
    }
    /// Frees an object (the core asks only for what a kit created).
    fn destroy(&mut self, _h: u64) -> bool {
        false
    }
    /// Its parent in the tree (0: none).
    fn parent(&mut self, _h: u64) -> u64 {
        0
    }
    /// The camera the engine draws with now (0: none): what a kit names by [`drawn::CAMERA`].
    fn camera(&mut self) -> u64 {
        0
    }
    /// The technical node of the view of entity `e<entity>` (`kithost.scene@1.3`): the object drawing its look under
    /// the root its reflection poses, whose transform the logic never rewrites (0: no view, or no such node).
    fn entity_node(&mut self, _entity: u64) -> u64 {
        0
    }
    /// The technical pose of the node of an entity's view itself (`kithost.scene@1.4` `set_node_turn`): laid after
    /// the entity's place, yaw and reflection, replacing the one it had (identity: none); false when `node` is not the
    /// node of an entity's view. The relative form of [`ScenePort::set_transform`] on that node: it holds no place.
    fn set_node_turn(&mut self, _node: u64, _t: &Trs) -> bool {
        false
    }
}

/// The pose of an entity as the last frame drew it (`kithost.scene@1.1` `watch_pose`): one record of the zone per
/// watched entity, written on the render side after each drawn frame, read in place at the tick (docs/boucles-et-fils.md:
/// written at the render, read at the tick). Nothing is written while nobody watches.
///
/// The block at the offset `watch_pose` answers follows the contract `abi.scene.drawn` (core/abi/contracts/
/// abi.scene.drawn.json: a `TableHead`, then one row `frame`, `position`, `rotation`), read and written through the
/// generated accessors of gripforge-contracts; only the kinds of watch stay here.
///
/// History: until #1865 (20/11) a record of 64 bytes at positions written here (`FRAME`, `POSITION`,
/// `ROTATION`, `BYTES`); rebuild kithost.core, the native player and the kit modules.
pub mod drawn {
    /// What a watch names: an engine object by handle (`kithost.scene@1` handles).
    pub const OBJECT: u32 = 1;
    /// A kit entity by the number of its id (`e12` → 12): its pose as drawn (interpolated, its yaw as a turn about +Y).
    pub const ENTITY: u32 = 2;
    /// The camera the engine draws with ([`super::ScenePort::camera`]), whichever it is at that frame; handle 0.
    pub const CAMERA: u32 = 3;
}

// A yaw applied at the render (`kithost.scene@1.2` `claim_render_yaw`): a kit claims, once, that the yaw an entity is
// drawn with comes from a look read at the render, not from its ticks — the body of a first-person player turns in
// the same frame as the mouse. The render side applies it on the drawn pose of the entity (its interpolated position
// kept): `yaw = anchor - look × sensitivity`, the look read in place in the zone: the x of a neutral `PointerFollow`
// block (`zone::pointer_follow`, `sum + (pointer - taken) × gain`) a kit published (#1766: no kit named here). Nothing is computed while no entity is claimed. The tick reads the yaw the frame
// drew back through [`drawn`] (`watch_pose` of the same entity): one yaw, applied once.
//
// The block at the offset `claim_render_yaw` answers follows the contract `abi.scene.render_yaw` (core/abi/contracts/
// abi.scene.render_yaw.json: a `TableHead`, then one row `anchor`, `sensitivity`, `source`), read and written through
// the generated accessors of gripforge-contracts.
//
// History: until #1766 `source` was a code (`LOOK = 1` named core.input's look); until #1865 (20/11)
// a record of 24 bytes at positions written here (module `render_yaw`, removed); rebuild kithost.core, the native
// player and the kit modules.

/// No tree wired: every read answers none.
#[derive(Default)]
pub struct NullScene;

impl ScenePort for NullScene {}
