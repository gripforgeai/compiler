//! The digest of the game's code (`sources` of a project): what tells the development mode that the `game.main` it
//! loads was compiled from the files the project declares now (docs/design/sources-du-jeu.md). FNV-1a 64 over, for
//! each declared file in the declared order: its path in the normal form (crate::sources::normal), a zero byte, its
//! length in bytes (u64, little-endian), its bytes. Written `fnv1a64:` and 16 lowercase hexadecimal digits.
//!
//! This file has no `use` and only `const fn`s: gripforge-build writes it as it is into the crate of `game.main`
//! (`src/sources_digest.rs`), which computes the digest of the bytes it compiles (`include_bytes!`) and keeps it in the
//! custom section `gripforge.sources` (wasm) or the symbol `GRIPFORGE_SOURCES` (native library). The same function
//! here, in the compiler and in the page (core/gateway-web/dev-project.mjs `sourcesDigest`), checked against the same
//! values (core/scene-format/tests/sources-digest.json).

/// The length of a digest, in bytes of text.
pub const DIGEST_LEN: usize = 24;

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

const fn eat(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// The digest of the files `(path in the normal form, bytes)`, in the declared order.
pub const fn digest(files: &[(&str, &[u8])]) -> [u8; DIGEST_LEN] {
    let mut h = OFFSET;
    let mut i = 0;
    while i < files.len() {
        let (path, bytes) = files[i];
        h = eat(h, path.as_bytes());
        h = eat(h, &[0]);
        h = eat(h, &(bytes.len() as u64).to_le_bytes());
        h = eat(h, bytes);
        i += 1;
    }
    let mut out = *b"fnv1a64:0000000000000000";
    let hex = b"0123456789abcdef";
    let mut d = 0;
    while d < 16 {
        out[8 + d] = hex[((h >> (60 - 4 * d)) & 0xf) as usize];
        d += 1;
    }
    out
}
