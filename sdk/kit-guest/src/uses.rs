//! What a kit module really calls of the core, written into its own binary (docs/kit-modules.md « Contracts »).
//!
//! Every function number of a core interface the façade calls is a [`Func`], and a `Func` is made only by [`func!`]
//! (`func!(world::GET)`): the mention leaves in the module's data a marker of 12 bytes, `gf.uses:` then the number
//! (u32 little-endian), read by a volatile load where the number is used. Code the linker drops (a part of the
//! façade the kit never reaches) takes its markers with it, so what stays in the data is what this kit calls.
//! tools/kit-modules/contracts.mjs reads the markers (and the `gf:kithost.*` imports, the direct links) and writes
//! the `consumes` lines of the module (custom section `gripforge.calls`): each interface at the smallest minor that
//! has every function the kit uses (`since` of kits/kitcall/interfaces/*.gfi).
//!
//! A raw `u32` cannot reach the core: `call_core` and the helpers take a `Func`, so a call the markers miss does
//! not compile.

/// A function number of a core interface, marked in the module's data ([`func!`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Func(u32);

impl Func {
    /// The number (what crosses to the core).
    #[inline(always)]
    pub const fn id(self) -> u32 {
        self.0
    }
    /// Made by [`func!`] only (the number is a constant).
    #[doc(hidden)]
    #[inline(always)]
    pub fn marked<const F: u32>() -> Func {
        #[cfg(target_arch = "wasm32")]
        // SAFETY: a read of a byte of a constant of this module; volatile, so the marker stays in the data while
        // this code is linked.
        unsafe {
            core::ptr::read_volatile(Marker::<F>::BYTES.as_ptr());
        }
        Func(F)
    }
}

/// The prefix of a marker (tools/kit-modules/contracts.mjs `USES_MAGIC`).
pub const MAGIC: &[u8; 8] = b"gf.uses:";

#[doc(hidden)]
pub struct Marker<const F: u32>;

impl<const F: u32> Marker<F> {
    pub const BYTES: &'static [u8; 12] = &[
        MAGIC[0],
        MAGIC[1],
        MAGIC[2],
        MAGIC[3],
        MAGIC[4],
        MAGIC[5],
        MAGIC[6],
        MAGIC[7],
        F as u8,
        (F >> 8) as u8,
        (F >> 16) as u8,
        (F >> 24) as u8,
    ];
}

/// `func!(world::GET)`: the [`Func`] of a constant function number, marked as used by this module.
#[macro_export]
macro_rules! func {
    ($n:expr) => {{
        const F: u32 = $n;
        $crate::uses::Func::marked::<F>()
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_marker_is_the_magic_then_the_number() {
        assert_eq!(super::Marker::<0x524>::BYTES, b"gf.uses:\x24\x05\0\0");
        assert_eq!(
            crate::func!(gripforge_abi::world::GET).id(),
            gripforge_abi::world::GET
        );
    }
}
