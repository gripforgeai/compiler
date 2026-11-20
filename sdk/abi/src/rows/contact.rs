//! A contact in a result buffer (contract `abi.physics.contact`, words of u64): `a` (2 words), `b` (2 words), a value,
//! flags — the index of each word generated.

use gripforge_contracts::abi_physics_contact as c;
pub const WORDS: usize = c::WORDS;
pub const F_BEGAN: u64 = 1;

/// Writes one contact; false when a name does not pack (the caller answers by the call instead).
pub fn write(out: &mut [u64], a: &str, b: &str, value: f64, began: bool) -> bool {
    let (Some(a), Some(b)) = (super::pack16(a), super::pack16(b)) else {
        return false;
    };
    let w = &mut out[..WORDS];
    w[c::at::A..c::at::A + 2].copy_from_slice(&[a.0, a.1]);
    w[c::at::B..c::at::B + 2].copy_from_slice(&[b.0, b.1]);
    w[c::at::VALUE] = value.to_bits();
    w[c::at::FLAGS] = began as u64;
    true
}

/// Reads one contact: `(a, b, value, began)`.
pub fn read(w: &[u64]) -> Option<(String, String, f64, bool)> {
    c::check(w).ok()?;
    let ([a0, a1], [b0, b1]) = (c::a(w), c::b(w));
    Some((
        super::unpack16(a0, a1)?,
        super::unpack16(b0, b1)?,
        f64::from_bits(c::value(w)),
        c::flags(w) & F_BEGAN != 0,
    ))
}
