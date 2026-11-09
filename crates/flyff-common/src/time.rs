//! Time base types (Plan/01 §9, 10 §4.2).
//!
//! * [`Tick`] — one 15 Hz server tick (66.667 ms) = exactly four simulation frames,
//! * [`SimFrame`] — one 1/60 s simulation frame (`4·tick + i`),
//! * [`GameMs`] — in-game milliseconds derived from the tick count (never from a wall clock),
//! * [`UnixMs`] / [`UnixTime`] — wall-clock instants (UTC), sampled once per tick by the runtime.

use serde::{Deserialize, Serialize};

/// Simulation frames per tick.
pub const FRAMES_PER_TICK: u64 = 4;

/// One server tick (15 Hz).
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub u64);

/// One 1/60 s simulation frame; `SimFrame(4·n + i)` is sub-frame `i` of tick `n`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct SimFrame(pub u64);

/// In-game milliseconds, `floor(tick · 1000 / 15)` (the only "now" for in-game timers, PL-012).
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct GameMs(pub u64);

/// Wall-clock milliseconds since the Unix epoch (UTC).
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct UnixMs(pub i64);

/// Wall-clock **seconds** since the Unix epoch (UTC): persisted times (13 §4) and wire `*_at` fields.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct UnixTime(pub i64);

impl Tick {
    /// Ticks per second.
    pub const PER_SECOND: u64 = 15;

    /// `floor(n · 1000 / 15)`: `Tick(1)` → 66, `Tick(3)` → 200, `Tick(15)` → 1000, `Tick(900)` → 60000.
    pub const fn game_ms(self) -> GameMs {
        GameMs(self.0.saturating_mul(1000) / 15)
    }

    /// First simulation frame of this tick (`4n`).
    pub const fn first_frame(self) -> SimFrame {
        SimFrame(self.0.saturating_mul(FRAMES_PER_TICK))
    }

    /// Number of frames completed once this tick has run (`4(n + 1)`).
    pub const fn frames_completed_after(self) -> SimFrame {
        SimFrame(self.0.saturating_add(1).saturating_mul(FRAMES_PER_TICK))
    }

    /// The smallest tick `n` with `game_ms(n) >= ms`.
    pub const fn first_at_or_after(ms: GameMs) -> Tick {
        Tick(ms.0.saturating_mul(15).saturating_add(999) / 1000)
    }

    /// The tick at which a timer armed on this tick with `delay_ms` fires (always at least the
    /// next tick).
    pub const fn after_ms(self, delay_ms: u64) -> Tick {
        let target = Tick::first_at_or_after(GameMs(self.game_ms().0.saturating_add(delay_ms)));
        let next = self.0.saturating_add(1);
        if target.0 > next { target } else { Tick(next) }
    }

    /// The following tick.
    pub const fn next(self) -> Tick {
        Tick(self.0.saturating_add(1))
    }
}

impl SimFrame {
    /// The tick this frame belongs to.
    pub const fn tick(self) -> Tick {
        Tick(self.0 / FRAMES_PER_TICK)
    }

    /// Sub-frame index `0..4` inside the tick.
    pub const fn sub_frame(self) -> u8 {
        (self.0 % FRAMES_PER_TICK) as u8
    }
}

impl GameMs {
    /// Zero (tick 0).
    pub const ZERO: GameMs = GameMs(0);
}

impl UnixMs {
    /// Whole seconds (floor toward negative infinity).
    pub const fn to_unix_time(self) -> UnixTime {
        UnixTime(self.0.div_euclid(1000))
    }
}

impl UnixTime {
    /// The same instant in milliseconds (saturating).
    pub const fn to_unix_ms(self) -> UnixMs {
        UnixMs(self.0.saturating_mul(1000))
    }
}
