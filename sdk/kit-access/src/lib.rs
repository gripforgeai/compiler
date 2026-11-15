//! gripforge-kit-access: the common part of the typed Rust accessors of the kits, and what a scene posed in code keeps
//! (docs/design/build-gripforge.md).
//!
//! The accessors themselves are one crate per kit, **generated** from its `kit.api.json` by tools/kit-api (language
//! `rust-access`) in the kit's own folder: `kits/<kit>/access/rust`, crate `gripforge-kit-access-<kit>` (`core.light` →
//! `gripforge-kit-access-core-light`), shipped in the kit's published package. Each has a function per method of its
//! services, and per element it exposes to scenes (`expose` `for: ["scene"]`) its typed properties (`Props`, defaults
//! of the declaration) and its four calls `create`, `set`, `remove`, `get`. Every function calls the kit **through the
//! host** ([`Host::call`]): no line of a kit is linked, the kits stay modules loaded apart (native library or wasm).
//! Each accessor depends on this crate by version only (`gripforge-kit-access = "<version>"`); the workspace and a
//! generated game point it to their copy with `[patch.crates-io]`.
//!
//! This crate keeps what they share:
//! - [`Host`]: the interface of the host the accessors need: the calls between kits and the engine's tree
//!   (`kithost.scene@1`). Served by the host side `Kx` (feature `kithost`) and the guest side `Kx` (feature `guest`).
//! - [`Spec`], [`Method`], [`Trs`], `json!` / [`Json`]: what an accessor's calls take.
//! - [`Posed`]: what the code of a scene (`init_scene/<scene>.rs`, written by `gfscene code`) posed, given back at the
//!   exit in the order dev.builder (ex-dev.builder) gives it back.
//!
//! Who uses it: every kit's accessor, the code `gfscene code` writes, a template's `init_scene.rs`, a game's own Rust.

pub use serde_json::{json, Value as Json};

/// The whole transform of a node as the engine's tree takes it: position, rotation (quaternion x, y, z, w), scale.
pub type Trs = [f64; 10];

/// What the accessors need of the host. Object safe: `&mut dyn Host` works as well as a concrete `Kx`.
pub trait Host {
    /// Calls `method` of the kit service `service` (the calls between modules; a method the game overrides is served
    /// by the game).
    fn call(&mut self, service: &str, method: &str, args: &[Json]) -> Result<Json, String>;
    /// An object of the engine's tree by its path; 0 when there is none.
    fn find(&mut self, path: &str) -> u64;
    /// A new object of `class` (empty: the plain node) named `name` under `parent` (0: the root); 0 when it could not.
    fn create(&mut self, parent: u64, class: &str, name: &str) -> u64;
    /// Sets an object's world transform; false when it could not.
    fn set_transform(&mut self, h: u64, t: &Trs) -> bool;
    /// Frees an object this code created.
    fn destroy(&mut self, h: u64) -> bool;
}

/// One method of a kit's service, named for a later call (what [`Posed`] calls to give an object back).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Method {
    /// The service (`core.light`).
    pub service: &'static str,
    /// The method (`remove`).
    pub name: &'static str,
}

/// Where an element is made: what `create(spec)` receives besides the properties (docs/design/dev-builder-sections.md).
#[derive(Clone, Debug, PartialEq)]
pub struct Spec<'a> {
    /// The scene's id.
    pub scene: &'a str,
    /// The node's path; empty for an element of the scene itself.
    pub node: &'a str,
    /// The node's stable id, when it has one.
    pub id: Option<&'a str>,
    /// The node's resolved transform (position, rotation, scale); `None` for the scene.
    pub transform: Option<([f64; 3], [f64; 4], [f64; 3])>,
    /// The engine node to make it under (0 or `None`: the scene's root).
    pub parent: Option<u64>,
}

impl Spec<'_> {
    /// The JSON the kit's `create` receives: `{ element, scene, node, id, props, transform, parent }`.
    pub fn to_json(&self, element: &str, props: Json) -> Json {
        let transform = self.transform.map_or(
            Json::Null,
            |(p, r, s)| json!({ "position": p, "rotation": r, "scale": s }),
        );
        let parent = self
            .parent
            .filter(|h| *h != 0)
            .map_or(Json::Null, |h| json!(h));
        json!({ "element": element, "scene": self.scene, "node": self.node, "id": self.id, "props": props,
            "transform": transform, "parent": parent })
    }
}

/// What the code of one scene posed, to give it back when the scene is left; and what a kit refused.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Posed {
    /// The scene's id.
    pub scene: String,
    /// The engine nodes held, in the order they were posed: (path, handle, made by this code).
    pub nodes: Vec<(String, u64, bool)>,
    /// The objects placed through a kit (lights, cameras): (method that lets it go, node path, handle).
    pub placed: Vec<(Method, String, Json)>,
    /// The kits' elements made: (method that lets it go, node path, handle).
    pub made: Vec<(Option<Method>, String, Json)>,
    /// What a kit refused (`place_failed`): `<what> <path>: <reason>`.
    pub problems: Vec<String>,
}

impl Posed {
    /// Nothing posed yet for `scene`.
    pub fn new(scene: &str) -> Self {
        Posed {
            scene: scene.to_string(),
            ..Default::default()
        }
    }

    /// A node of the engine's tree: the game's own at `path` when there is one (adopted, left as it is), else made
    /// under `parent`; its whole transform set. Gives its handle (0: the engine could not make it).
    pub fn node(
        &mut self,
        h: &mut (impl Host + ?Sized),
        parent: u64,
        path: &str,
        name: &str,
        t: Trs,
    ) -> u64 {
        let found = h.find(path);
        let (handle, created) = if found != 0 {
            (found, false)
        } else {
            (h.create(parent, "", name), true)
        };
        if handle == 0 {
            return 0;
        }
        h.set_transform(handle, &t);
        self.nodes.push((path.to_string(), handle, created));
        handle
    }

    /// An object placed through a kit (`remove` lets it go), or what the kit said when it refused.
    pub fn place(&mut self, remove: Method, path: &str, what: &str, r: Result<Json, String>) {
        match r {
            Ok(handle) => self.placed.push((remove, path.to_string(), handle)),
            Err(e) => self.problems.push(format!("{what} {path}: {e}")),
        }
    }

    /// An element made for a kit's section (`remove` lets it go, when the kit has one).
    pub fn make(
        &mut self,
        remove: Option<Method>,
        path: &str,
        what: &str,
        r: Result<Json, String>,
    ) {
        match r {
            Ok(handle) => self.made.push((remove, path.to_string(), handle)),
            Err(e) => self.problems.push(format!("{what} {path}: {e}")),
        }
    }

    /// The engine node held at `path`, if any.
    pub fn handle(&self, path: &str) -> Option<u64> {
        self.nodes
            .iter()
            .find(|(p, _, _)| p == path)
            .map(|(_, h, _)| *h)
    }

    /// Gives the scene back, as dev.builder does when it is left: the elements (the last made first), then the
    /// lights and cameras (each kit frees what it made and gives the game's own back), then the nodes this code made
    /// (children first).
    pub fn leave(self, h: &mut (impl Host + ?Sized)) {
        for (m, _, handle) in self.made.iter().rev() {
            if let Some(m) = m {
                let _ = h.call(m.service, m.name, std::slice::from_ref(handle));
            }
        }
        for (m, _, handle) in &self.placed {
            let _ = h.call(m.service, m.name, std::slice::from_ref(handle));
        }
        for (_, handle, created) in self.nodes.iter().rev() {
            if *created {
                h.destroy(*handle);
            }
        }
    }
}

/// The host side: a module loaded in memory by the kit host (tests, a game embedded in a gateway).
#[cfg(feature = "kithost")]
impl Host for gripforge_kit_api::host::Kx<'_> {
    fn call(&mut self, service: &str, method: &str, args: &[Json]) -> Result<Json, String> {
        gripforge_kit_api::host::Kx::call(self, service, method, args)
    }
    fn find(&mut self, path: &str) -> u64 {
        self.scene().find(path)
    }
    fn create(&mut self, parent: u64, class: &str, name: &str) -> u64 {
        self.scene().create(parent, class, name)
    }
    fn set_transform(&mut self, h: u64, t: &Trs) -> bool {
        self.scene().set_transform(h, t)
    }
    fn destroy(&mut self, h: u64) -> bool {
        self.scene().destroy(h)
    }
}

/// The guest side: a game module built as its own native library or wasm, like a kit.
#[cfg(feature = "guest")]
impl Host for gripforge_kit_guest::host::Kx<'_> {
    fn call(&mut self, service: &str, method: &str, args: &[Json]) -> Result<Json, String> {
        gripforge_kit_guest::host::Kx::call(self, service, method, args)
    }
    fn find(&mut self, path: &str) -> u64 {
        use gripforge_abi::ports::ScenePort;
        self.scene().find(path)
    }
    fn create(&mut self, parent: u64, class: &str, name: &str) -> u64 {
        use gripforge_abi::ports::ScenePort;
        self.scene().create(parent, class, name)
    }
    fn set_transform(&mut self, h: u64, t: &Trs) -> bool {
        use gripforge_abi::ports::ScenePort;
        self.scene().set_transform(h, t)
    }
    fn destroy(&mut self, h: u64) -> bool {
        use gripforge_abi::ports::ScenePort;
        self.scene().destroy(h)
    }
}
