//! gripforge-sources-digest: the digest of the game's code (the files `sources` of a `project.gripforge` declares),
//! one function for every reader. Its home is the compiler's public repository (decision of Damien, 21/11: the common
//! code lives with the compiler, the kits repository imports it): gripforge-build, the common core of the format
//! (`gripforge_scene_format::sources_digest`, a re-export) and, through module-kits, dev.builder and the page.
//!
//! - [`digest`] / [`DIGEST_LEN`]: the function, `const`, over `(path in the normal form, bytes)` in the declared order.
//! - [`SOURCE`]: the text of the function's file, which gripforge-build writes as it is into the crate of `game.main`
//!   (the module computes the digest of the bytes it compiles, no dependency on this crate).
//! - [`VALUES`]: the shared values (`tests/values.json`) a caller in another language checks its binding against.

mod sources_digest;

pub use sources_digest::{digest, DIGEST_LEN};

/// The text of `src/sources_digest.rs`, written as it is into the crate of `game.main` (`src/sources_digest.rs`).
pub const SOURCE: &str = include_str!("sources_digest.rs");

/// The shared values (`{ note, cases: [{ files: [{ path, text }], digest }] }`): every binding of [`digest`] gives them.
pub const VALUES: &str = include_str!("../tests/values.json");
