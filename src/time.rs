//! Monotonic clock for time management.
//!
//! Native targets re-export [`std::time::Instant`] unchanged, so this
//! costs nothing: it is the same type, not a wrapper around it.
//!
//! `wasm32-unknown-unknown` uses Performance.now()

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

#[cfg(target_arch = "wasm32")]
pub use wasm::Instant;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::ops::Add;
    use std::time::Duration;

    #[link(wasm_import_module = "env")]
    extern "C" {
        fn mimir_now_ms() -> f64;
    }

    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
    pub struct Instant(f64);

    impl Instant {
        pub fn now() -> Self {
            Self(unsafe { mimir_now_ms() })
        }

        pub fn elapsed(&self) -> Duration {
            let millis = (Self::now().0 - self.0).max(0.0);
            Duration::from_secs_f64(millis / 1000.0)
        }
    }

    impl Add<Duration> for Instant {
        type Output = Self;

        fn add(self, duration: Duration) -> Self {
            Self(self.0 + duration.as_secs_f64() * 1000.0)
        }
    }
}
