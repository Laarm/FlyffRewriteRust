//! Monotonic clock for telemetry timestamps (not for game logic).

// `Instant` is disallowed in determinism tiers (game time comes from the tick clock); telemetry and
// health checks are observation-only and never feed back into the simulation.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use std::time::Instant;

/// A monotonic nanosecond counter anchored at construction time; cheap to copy around via `Arc`.
#[derive(Clone, Copy, Debug)]
pub struct MonoClock {
    base: Instant,
}

impl MonoClock {
    /// Starts a clock at "now" (= 0 ns).
    pub fn start() -> Self {
        Self {
            base: Instant::now(), // det-ok: telemetry clock, observation only
        }
    }

    /// Nanoseconds since [`MonoClock::start`] (saturating at `u64::MAX`, ~584 years).
    pub fn now_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}
