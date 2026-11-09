//! The injected [`Clock`] (Plan/24 §7.3).
//!
//! `flyff-assets` is tier R (`Instant` and `SystemTime` are disallowed), so every timing in this crate
//! (build durations, the 1 s prewarm batching window, shutdown deadlines, the 10 min write-error pause,
//! `finished_at` of the report) comes from this trait. `flyff-server` implements it with a monotonic
//! clock plus the wall clock; `flyff_common::rt::MonoClock` already implements the monotonic half.

use flyff_common::rt::MonoClock;

/// Time source of the asset framework.
pub trait Clock: Send + Sync {
    /// Monotonic milliseconds since an arbitrary origin (never goes backwards).
    fn now_ms(&self) -> u64;

    /// Wall-clock Unix seconds, used only for `finished_at` in `prepare-report.json`. The default is
    /// `0` ("unknown") so a monotonic-only clock stays a valid implementation.
    fn unix_s(&self) -> u64 {
        0
    }
}

impl Clock for MonoClock {
    fn now_ms(&self) -> u64 {
        self.now_ns() / 1_000_000
    }
}
