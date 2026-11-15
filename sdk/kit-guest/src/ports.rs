//! Façade of src/ports.rs and src/platform.rs of the kit host: the ports of the `KitContext` (physics, input,
//! assets) and the machine (save stores, clock, language, target, audio, display, pointer), each a call to the core
//! (`kithost.physics@1`, `kithost.input@2`, `kithost.assets@1`, `kithost.platform@1`).
//!
//! The pure types and traits (`V3`, `BodyDesc`, `Shape`, `PhysicsPort`, `AssetPort`…) are the
//! files of this crate (src/ports/), which the core compiles too.
//!
//! What a read returns by reference (`device()`, `captured()`, `recorded()`…) is a copy kept in an append-only
//! [`Arena`] of the proxy, emptied by a `&mut` method or when the core enters the module again (`enter`).

use crate::host::call_core_ok;
use gripforge_abi::{Reader, Writer};
use std::cell::{Cell, UnsafeCell};

// The pure types are compiled once, in the contract (src/ports/port_types.rs is that file): a kit module and the
// crates it links (the game.engine model) then name one `Shape`, one `BodyDesc`.
pub use gripforge_abi::ports::*;

/// The numbers and the encoding of the port interfaces (shared with the core).
pub mod codec;

mod assets;
mod drawn_pose;
mod input;
mod machine;
mod physics;
// Not `render_yaw`: that name is the layout of gripforge-abi, re-exported above by `pub use gripforge_abi::ports::*`.
mod render_yaw_claim;
mod scene_proxy;

pub use assets::AssetsProxy;
pub use drawn_pose::{DrawnPose, DrawnPoseView, CAMERA, ENTITY, OBJECT};
pub use input::{InputPort, RawRecord};
pub use machine::{
    AudioProxy, ClockProxy, DisplayProxy, Platform, PointerProxy, SaveStores, StoreHandle,
};
pub use physics::PhysicsProxy;
pub use render_yaw_claim::{RenderYaw, LOOK};
pub use scene_proxy::SceneProxy;

// ---- helpers -----------------------------------------------------------------------------------------------------

thread_local! {
    /// Counts what the ports of this module keep until the next entry (a copy handed out by reference, a read of
    /// the input, the date): an instance drops the copies of its ports on an entry only when this moved since it
    /// last did (`Host::enter`) — most hooks keep nothing, and their entry does not touch the ports.
    static KEEPS: Cell<u64> = const { Cell::new(1) };
}

/// See `KEEPS`.
#[inline]
pub(crate) fn keeps() -> u64 {
    KEEPS.with(Cell::get)
}

/// A port keeps something until the next entry.
#[inline]
pub(crate) fn keeping() {
    KEEPS.with(|k| k.set(k.get().wrapping_add(1)));
}

/// Copies handed out by reference: never moved (each boxed), only dropped by `clear` (`&mut`) or with the arena.
pub struct Arena<T> {
    items: UnsafeCell<Vec<*mut T>>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            items: UnsafeCell::new(Vec::new()),
        }
    }
}

impl<T> Arena<T> {
    pub fn keep(&self, v: T) -> &T {
        keeping();
        let p = Box::into_raw(Box::new(v));
        // SAFETY: the vector holds raw pointers only (no reference into it is handed out); the boxes it points to
        // are freed by `clear` or `drop`, which take `&mut self`, so no `&T` given by `keep` outlives them.
        unsafe {
            (*self.items.get()).push(p);
            &*p
        }
    }
    pub fn clear(&mut self) {
        for p in self.items.get_mut().drain(..) {
            // SAFETY: made by Box::into_raw in `keep`, freed once.
            drop(unsafe { Box::from_raw(p) });
        }
    }
}

impl<T> Drop for Arena<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// A call answering bytes, read with `f`; a malformed answer is a bug of the contract (the module stops).
pub(crate) fn ask<T>(
    func: crate::uses::Func,
    w: &Writer,
    f: impl FnOnce(&mut Reader) -> Result<T, String>,
) -> T {
    let bytes = call_core_ok(func, w);
    let mut r = Reader::new(&bytes);
    f(&mut r)
        .unwrap_or_else(|e| panic!("kitcall: bad answer of core function {:#x}: {e}", func.id()))
}

/// A call without an answer.
pub(crate) fn tell(func: crate::uses::Func, w: &Writer) {
    call_core_ok(func, w);
}

/// Writes the arguments of a call.
pub(crate) fn args(f: impl FnOnce(&mut Writer)) -> Writer {
    let mut w = crate::host::writer();
    f(&mut w);
    w
}
