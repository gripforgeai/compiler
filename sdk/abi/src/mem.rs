//! The common memory of the trusted kit modules (docs/kit-modules.md, « Common memory »): the core and the kit
//! modules built relocatable import ONE memory and use ONE allocator (the core's), so bytes are handed from one
//! module to another by their address, never copied.
//!
//! What crosses a call is [`Bytes`]: a `Vec<u8>` given away with its capacity. The module that receives it owns
//! it and frees it with the same allocator.

/// A `Vec<u8>` handed to another module of the common memory (pointer, length, capacity: 12 bytes, written by the
/// callee where the caller said).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bytes {
    pub ptr: u32,
    pub len: u32,
    pub cap: u32,
}

impl Bytes {
    /// Gives `v` away: whoever calls [`Bytes::take`] on the result owns it.
    pub fn give(v: Vec<u8>) -> Bytes {
        let mut v = core::mem::ManuallyDrop::new(v);
        Bytes {
            ptr: v.as_mut_ptr() as usize as u32,
            len: v.len() as u32,
            cap: v.capacity() as u32,
        }
    }

    /// Takes the bytes back as a `Vec`.
    ///
    /// # Safety
    /// `self` comes from one [`Bytes::give`] made in the same memory with the same allocator, taken once.
    pub unsafe fn take(self) -> Vec<u8> {
        if self.cap == 0 {
            return Vec::new();
        }
        Vec::from_raw_parts(
            self.ptr as usize as *mut u8,
            self.len as usize,
            self.cap as usize,
        )
    }
}

/// The entry of a kit module of the common memory (`gfx_mem_enter`, called by the core through its function
/// table) and the core's `gfx_mem_call` (a kit module calls a core interface): the arguments are read where they
/// are, the answer is given in `out`; the status is that of the calls (0 `ok`, else `out` holds the message).
pub type Enter = unsafe extern "C" fn(func: u32, args: *const u8, len: u32, out: *mut Bytes) -> u32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        assert_eq!(core::mem::size_of::<Bytes>(), 12);
        assert_eq!(Bytes::give(Vec::new()).cap, 0);
        assert!(unsafe { Bytes::default().take() }.is_empty());
    }
}
