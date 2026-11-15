//! The core's `kit_time` (src/kit_time.rs of gripforge-kithost) as a kit module sees it: no frame clock is handed to
//! a module, so `frame_clock` answers `None` and a kit falls back to the platform's date (`ports::Platform::now_ms`).

/// The host's frame clock (ms) when it gave one: never in a kit module.
pub fn frame_clock() -> Option<f64> {
    None
}
