//! `UpgradeLimiter` (Plan/19 §12.1, PL-076): the pacing of the upgrade attempts.
//!
//! `Enchant`, `SmeltSafetyReq`, `UltimateEnchantWeapon`, `UltimateTransWeapon` and `PiercingSize` are refused when
//! less than `gameplay.upgrade_min_interval_ms` (600) passed since the previous attempt of **any** of them. The
//! original paced only in the client (`ENCHANT_TIME = 1 s`; the server throttle `Player_Smelt_Tick` was commented
//! out), so a modified client could send hundreds of attempts per second. The limiter lives in
//! [`PlayerItems::upgrade_limiter`](crate::item::PlayerItems) and is shared by the work packages of docs 19
//! ITEM-12 (here) and ITEM-13/14 (the ultimate and piercing handlers call [`UpgradeLimiter::accept`] first).

use flyff_common::GameMs;

/// The game time of the last accepted attempt. Runtime only (never persisted).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpgradeLimiter {
    last: Option<GameMs>,
}

impl UpgradeLimiter {
    /// Decides one attempt at `now`: `true` (and the time is remembered) when at least `min_interval_ms` passed
    /// since the previous accepted attempt, `false` (nothing remembered) when not. The first attempt is always
    /// accepted. A clock that went backwards counts as "not enough time" only up to the interval, never
    /// forever: the saturating difference is 0 for `now <= last`.
    pub fn accept(&mut self, now: GameMs, min_interval_ms: u32) -> bool {
        if let Some(last) = self.last
            && now.0.saturating_sub(last.0) < u64::from(min_interval_ms)
        {
            return false;
        }
        self.last = Some(now);
        true
    }

    /// The game time of the last accepted attempt.
    pub fn last(&self) -> Option<GameMs> {
        self.last
    }
}
