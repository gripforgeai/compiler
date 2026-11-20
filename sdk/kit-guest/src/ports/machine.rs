//! The machine of a kit module (platform.rs of the kit host): save stores, clock, language, target, audio output,
//! display, pointer — all held by the core (`kithost.platform@1`).

use super::codec::{platform_fn as f, *};
use super::{args, ask, tell, Arena};
use std::cell::Cell;

// ---- save stores -------------------------------------------------------------------------------------------------

/// A save store of the core (`StoreHandle` of platform.rs): the local one or the cloud one, by name — the core's
/// handle stays shared there, so what a kit module writes is what every other kit and the page read.
#[derive(Clone, Debug, PartialEq)]
pub struct StoreHandle {
    which: u8,
}

impl StoreHandle {
    pub fn kind(&self) -> &'static str {
        let k = ask(
            crate::func!(f::STORE_KIND),
            &args(|w| {
                w.u8(self.which);
            }),
            |r| r.string(),
        );
        match k.as_str() {
            "local" => "local",
            "cloud" => "cloud",
            "memory" => "memory",
            _ => Box::leak(k.into_boxed_str()),
        }
    }
    pub fn get(&self, key: &str) -> Option<String> {
        ask(
            crate::func!(f::STORE_GET),
            &args(|w| {
                w.u8(self.which).str(key);
            }),
            r_opt_string,
        )
    }
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let err = ask(
            crate::func!(f::STORE_SET),
            &args(|w| {
                w.u8(self.which).str(key).str(value);
            }),
            r_opt_string,
        );
        err.map_or(Ok(()), Err)
    }
    pub fn remove(&self, key: &str) {
        tell(
            crate::func!(f::STORE_REMOVE),
            &args(|w| {
                w.u8(self.which).str(key);
            }),
        );
    }
    pub fn keys(&self, prefix: &str) -> Vec<String> {
        ask(
            crate::func!(f::STORE_KEYS),
            &args(|w| {
                w.u8(self.which).str(prefix);
            }),
            |r| r.strs(),
        )
    }
}

/// `Host::stores`: `local` always, the cloud one when the core has one (`get("cloud")` asks).
#[derive(Clone)]
pub struct SaveStores {
    pub local: StoreHandle,
}

impl Default for SaveStores {
    fn default() -> Self {
        SaveStores {
            local: StoreHandle { which: STORE_LOCAL },
        }
    }
}

impl SaveStores {
    /// `runtime.core.saveStore(kind)`: `cloud` is the cloud store or nothing, anything else the local one.
    pub fn get(&self, kind: &str) -> Option<StoreHandle> {
        if kind != "cloud" {
            return Some(self.local.clone());
        }
        ask(
            crate::func!(f::STORE_HAS),
            &args(|w| {
                w.u8(STORE_CLOUD);
            }),
            |r| r.bool(),
        )
        .then_some(StoreHandle { which: STORE_CLOUD })
    }
}

// ---- clock, language, target -------------------------------------------------------------------------------------

/// `Platform::clock` of the core.
#[derive(Default)]
pub struct ClockProxy {}

impl ClockProxy {
    pub fn now_ms(&self) -> Option<f64> {
        ask(crate::func!(f::NOW), &args(|_| {}), |r| {
            Ok(if r.bool()? { Some(r.f64()?) } else { None })
        })
    }
    pub fn tz_offset(&self, ms: f64) -> f64 {
        ask(
            crate::func!(f::TZ_OFFSET),
            &args(|w| {
                w.f64(ms);
            }),
            |r| r.f64(),
        )
    }
}

/// `Host::platform`: `target` and `locale` are read once (the first entry: they do not change once mounted); the
/// date and its offset once per entry of the core (fixed during a hook, as the tick).
pub struct Platform {
    pub clock: ClockProxy,
    pub locale: String,
    pub target: String,
    loaded: bool,
    now: Cell<Option<(Option<f64>, f64)>>,
}

impl Default for Platform {
    fn default() -> Self {
        Platform {
            clock: ClockProxy {},
            locale: "en".into(),
            target: "web".into(),
            loaded: false,
            now: Cell::new(None),
        }
    }
}

impl Platform {
    /// A new entry of the core after the date was read (`ports::keeps`), and the first entry: the date is read
    /// again; target and language the first time.
    pub fn enter(&mut self) {
        self.now.set(None);
        if !self.loaded {
            self.loaded = true;
            let (target, locale) = ask(crate::func!(f::INFO), &args(|_| {}), |r| {
                Ok((r.string()?, r.string()?))
            });
            self.target = target;
            self.locale = locale;
        }
    }
    fn now(&self) -> (Option<f64>, f64) {
        if let Some(n) = self.now.get() {
            return n;
        }
        if let Some(n) = direct_now() {
            super::keeping();
            self.now.set(Some(n));
            return n;
        }
        let n = ask(crate::func!(f::NOW), &args(|_| {}), |r| {
            let has = r.bool()?;
            let ms = r.f64()?;
            Ok((has.then_some(ms), r.f64()?))
        });
        super::keeping();
        self.now.set(Some(n));
        n
    }
    /// `Date.now()`; `None` without a clock.
    pub fn now_ms(&self) -> Option<f64> {
        self.now().0
    }
    /// `-new Date().getTimezoneOffset()`.
    pub fn local_offset(&self) -> f64 {
        self.now().1
    }
}

// ---- audio, display, pointer -------------------------------------------------------------------------------------

/// `Host::display` (`DisplayPort` of the core).
#[derive(Default)]
pub struct DisplayProxy {
    kept: Arena<Vec<String>>,
}

impl DisplayProxy {
    pub fn enter(&mut self) {
        self.kept.clear();
    }
    pub fn fullscreen_available(&self) -> bool {
        ask(crate::func!(f::DISPLAY_AVAILABLE), &args(|_| {}), |r| {
            r.bool()
        })
    }
    pub fn needs_gesture(&self) -> bool {
        ask(crate::func!(f::DISPLAY_NEEDS_GESTURE), &args(|_| {}), |r| {
            r.bool()
        })
    }
    pub fn request_fullscreen(&mut self) {
        tell(crate::func!(f::DISPLAY_REQUEST), &args(|_| {}));
    }
    pub fn exit_fullscreen(&mut self) {
        tell(crate::func!(f::DISPLAY_EXIT), &args(|_| {}));
    }
    pub fn recorded(&self) -> &[String] {
        self.kept
            .keep(ask(crate::func!(f::DISPLAY_RECORDED), &args(|_| {}), |r| {
                r.strs()
            }))
    }
    pub fn drain(&mut self) -> Vec<String> {
        ask(crate::func!(f::DISPLAY_DRAIN), &args(|_| {}), |r| r.strs())
    }
}

/// `Host::pointer` (`PointerPort` of the core).
#[derive(Default)]
pub struct PointerProxy {
    kept: Arena<Vec<String>>,
}

impl PointerProxy {
    pub fn enter(&mut self) {
        self.kept.clear();
    }
    pub fn pointer_available(&self) -> bool {
        ask(crate::func!(f::POINTER_AVAILABLE), &args(|_| {}), |r| {
            r.bool()
        })
    }
    pub fn needs_gesture(&self) -> bool {
        ask(crate::func!(f::POINTER_NEEDS_GESTURE), &args(|_| {}), |r| {
            r.bool()
        })
    }
    pub fn lock(&mut self) {
        tell(crate::func!(f::POINTER_LOCK), &args(|_| {}));
    }
    pub fn unlock(&mut self) {
        tell(crate::func!(f::POINTER_UNLOCK), &args(|_| {}));
    }
    pub fn recorded(&self) -> &[String] {
        self.kept
            .keep(ask(crate::func!(f::POINTER_RECORDED), &args(|_| {}), |r| {
                r.strs()
            }))
    }
    pub fn drain(&mut self) -> Vec<String> {
        ask(crate::func!(f::POINTER_DRAIN), &args(|_| {}), |r| r.strs())
    }
}

/// The date by direct link (`kithost.platform@1.1`: two scalars, no message) — in the common memory; else `None`.
#[cfg(feature = "common-memory")]
fn direct_now() -> Option<(Option<f64>, f64)> {
    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "gf:kithost.platform@^1.1")]
    extern "C" {
        fn pf_now() -> f64;
        fn pf_offset() -> f64;
    }
    #[cfg(all(feature = "common-memory", not(target_arch = "wasm32")))]
    use crate::native::link::{pf_now, pf_offset};
    // SAFETY: imports wired by the calls host to the core's exports of the same names.
    let (ms, offset) = unsafe { (pf_now(), pf_offset()) };
    Some(((!ms.is_nan()).then_some(ms), offset))
}
#[cfg(not(feature = "common-memory"))]
fn direct_now() -> Option<(Option<f64>, f64)> {
    None
}
