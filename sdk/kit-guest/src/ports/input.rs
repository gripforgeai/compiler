//! `kx.input()` of a kit module: the neutral transport of the devices (`kithost.input@2`). The core holds no table of
//! actions: core.input takes the raw records once per tick and writes the actions in the `INPUT` buffer of the
//! zone; the pointer total is read there too (`pointer()`), in place.

use super::codec::input_fn as f;
use super::{args, ask, tell};

/// One raw record of a device (`RawInput` of the core, gripforge-kit-api src/ports/raw_input.rs): kind 0 keyboard,
/// 1 mouse, 2 gamepad, 3 touch, 4 gyroscope, 5 virtual; link 0 none, 1 connected, 2 disconnected, 3 release all.
#[derive(Clone, Debug, PartialEq)]
pub struct RawRecord {
    pub kind: u8,
    pub index: u16,
    pub code: String,
    pub x: f64,
    pub y: f64,
    pub link: u8,
}

#[derive(Default)]
pub struct InputPort;

impl InputPort {
    /// The raw device records since the last take, taken (`raw_take`): core.input reads them once per tick.
    pub fn take_raw(&mut self) -> Vec<RawRecord> {
        ask(crate::func!(f::RAW_TAKE), &args(|_| {}), |r| {
            let n = r.u32()?;
            let mut out = Vec::with_capacity(n as usize);
            for _ in 0..n {
                out.push(RawRecord {
                    kind: r.u8()?,
                    index: r.u32()? as u16,
                    code: r.string()?,
                    x: r.f64()?,
                    y: r.f64()?,
                    link: r.u8()?,
                });
            }
            Ok(out)
        })
    }

    /// One record from a kit that is a device (an on-screen pad), as a device of the host.
    pub fn push_raw(&mut self, r: &RawRecord) {
        tell(
            crate::func!(f::PUSH_RAW),
            &args(|w| {
                w.u8(r.kind)
                    .u32(r.index as u32)
                    .str(&r.code)
                    .f64(r.x)
                    .f64(r.y)
                    .u8(r.link);
            }),
        );
    }

    /// The pointer total (x right, y down) the host's devices added since the start, read in place in the zone.
    pub fn pointer(&self) -> (f64, f64) {
        let at = crate::zone::input().offset + crate::zone::layout::input::POINTER;
        (
            crate::zone::zone_read_f64(at),
            crate::zone::zone_read_f64(at + 8),
        )
    }
}
