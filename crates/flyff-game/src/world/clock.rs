//! The in-game clock and `GameTimer` (Plan/15 §17.1, WP-WORLD-12; ref/19 §20; F-094).
//!
//! Port of `CGameTimer` (`src:_Common/timer.cpp:141-208`): `TIMESPEED = 10`, so one game second is 10 ms of
//! real time (1 game hour = 36 s, 1 game day = 14.4 min, a month has 30 days). Every server start begins at game
//! day 4, 16:00 (**KEEP-QUIRK**: the original CoreServer restarted the clock at every boot): the base of
//! [`GAME_CLOCK_BASE_MS`] is `(3 * 24 + 15) * 3600 * 10`. All arithmetic is 64-bit (**FIX** of the 24.8-day
//! `long` wrap, 01 §9). `GameMs` is tick-derived (PL-012), so the clock stretches with late ticks.
//!
//! [`InGameTime`] is a pure function of `GameMs`. `Worlds.game_time` caches it once per second: the 1 s slot item
//! `game_clock` ([`game_clock`], slot item 1 of 10 §5.4, tick `n % 15 == 9`) recomputes it, so the respawn job
//! (P1, `n % 15 == 3`) reads the value computed up to 14 ticks earlier, as the original read
//! `g_GameTimer.Compute()`'s once-per-second sample. `GameTimer{current_time_ms}` is computed live from the tick
//! clock for the join sequence (the original `GetCurrentTime()` read `timeGetTime()` directly).

use crate::ctx::{GameCtx, Services};
use crate::world::Worlds;
use flyff_common::GameMs;
use flyff_proto::pb;

/// `TIMESPEED`: milliseconds of real time per game second.
pub const TIMESPEED: u64 = 10;

/// The game-time offset of a server start: day 4, 16:00 (`SetCurrentTime(3, 15)`: `(3 * 24 + 15) * 3600 *
/// TIMESPEED` ms).
pub const GAME_CLOCK_BASE_MS: u64 = (3 * 24 + 15) * 3600 * TIMESPEED;

/// The in-game time of day (`CGameTimer` fields).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InGameTime {
    /// The clock value sent to the client: `GAME_CLOCK_BASE_MS + game_ms`.
    pub clock_ms: u64,
    /// Game seconds `0..60`.
    pub sec: u8,
    /// Game minutes `0..60`.
    pub min: u8,
    /// Game hour `1..=24`.
    pub hour: u8,
    /// Game day of the month `1..=30`.
    pub day: u8,
}

impl InGameTime {
    /// The in-game time at tick time `now`.
    pub fn at(now: GameMs) -> InGameTime {
        let clock_ms = GAME_CLOCK_BASE_MS.saturating_add(now.0);
        let t = clock_ms / TIMESPEED; // game seconds
        InGameTime {
            clock_ms,
            sec: (t % 60) as u8,
            min: ((t / 60) % 60) as u8,
            hour: ((t / 3600) % 24) as u8 + 1,
            day: ((t / 86_400) % 30) as u8 + 1,
        }
    }
}

impl Default for InGameTime {
    fn default() -> Self {
        InGameTime::at(GameMs(0))
    }
}

/// `Worlds.game_time = InGameTime::at(now)` (the body of the slot item).
pub fn recompute_game_time(w: &mut Worlds, now: GameMs) {
    w.game_time = InGameTime::at(now);
}

/// Slot item 1 `game_clock` of the 1 s slot (10 §5.4 as amended by 15 §17.1): the once-per-second sample.
pub fn game_clock(_svc: &mut Services, ctx: &mut GameCtx<'_>) {
    recompute_game_time(ctx.worlds, ctx.clock.game_ms);
}

/// `GameTimer{current_time_ms}` of the join sequence (step 4): the live clock of the tick, in milliseconds as an
/// `f64` (the client advances it locally with its server-time estimate).
pub fn game_timer_msg(now: GameMs) -> pb::GameTimer {
    pb::GameTimer {
        current_time_ms: InGameTime::at(now).clock_ms as f64,
    }
}
