//! Pure rules of the pockets, the inventory sort, the EXP scrolls and the angel, shared by the server handlers
//! (`flyff-game::item::{pockets, buffitems}`) and the browser (Plan/19 §3.4, §3.5, §5.4, §5.12; WP-ITEM-15, -20).
//!
//! Everything is plain integers: the server feeds them from its state, the browser from its replica. The
//! functions are total (no panic on any input).

/// `MAX_POCKET`: the pockets of a character.
pub const MAX_POCKET: usize = 3;

/// The slots of pocket `index`: `18 / 42 / 42` with `features.bigger_pocket`, the vanilla `6 / 24 / 24` without
/// (`POCKET_SLOT_SIZE_0`, `POCKET_SLOT_SIZE_1_2`); 0 for an index outside the three pockets.
pub fn pocket_capacity(index: usize, bigger_pocket: bool) -> u32 {
    match (index, bigger_pocket) {
        (0, true) => 18,
        (1 | 2, true) => 42,
        (0, false) => 6,
        (1 | 2, false) => 24,
        _ => 0,
    }
}

/// `CPocketController::IsAvailable(index)`: with `features.perm_extrabags` every pocket always; else pocket 0
/// always, pockets 1 and 2 while they exist and have not expired. An index outside the three pockets is never
/// available.
pub fn pocket_available(perm_extrabags: bool, index: usize, exists: bool, expired: bool) -> bool {
    if index >= MAX_POCKET {
        return false;
    }
    perm_extrabags || index == 0 || (exists && !expired)
}

/// The order `SortInventory` moves the items into: `items` are the usable items of the bag as `(slot, item_kind3)`
/// in slot order; the result is their slots ordered by kind ascending, **stable** on the slot (PL-153: the
/// original `std::sort` was not).
pub fn sort_order(items: &[(u16, u32)]) -> Vec<u16> {
    let mut v: Vec<(u16, u32)> = items.to_vec();
    v.sort_by_key(|&(_, kind)| kind);
    v.into_iter().map(|(slot, _)| slot).collect()
}

/// The level band of a banded EXP scroll (`AMPESA..E`): `(low, high)`; `None` for any other id (`AMPESS` has none).
pub fn exp_scroll_band(scroll: u32) -> Option<(i32, i32)> {
    use flyff_common::defs::item::*;
    match scroll {
        II_SYS_SYS_SCR_AMPESA => Some((1, 20)),
        II_SYS_SYS_SCR_AMPESB => Some((21, 40)),
        II_SYS_SYS_SCR_AMPESC => Some((41, 60)),
        II_SYS_SYS_SCR_AMPESD => Some((61, 80)),
        II_SYS_SYS_SCR_AMPESE => Some((81, 120)),
        _ => None,
    }
}

/// The band a level is in (the one printed when the buffs of the family are full); `None` above 120.
pub fn exp_level_band(level: i32) -> Option<(i32, i32)> {
    match level {
        ..=20 => Some((1, 20)),
        21..=40 => Some((21, 40)),
        41..=60 => Some((41, 60)),
        61..=80 => Some((61, 80)),
        81..=120 => Some((81, 120)),
        _ => None,
    }
}

/// How many EXP scroll buffs a player of `level` may hold with `special` special (`AMPESS*`) buffs active:
/// 3 below level 41; 2 at 41-60 with fewer than two specials; 1 up to 120 with none; else 3.
pub fn exp_scroll_max_tiers(level: i32, special: i32) -> i32 {
    if level < 41 {
        3
    } else if level < 61 && special < 2 {
        2
    } else if level <= 120 && special < 1 {
        1
    } else {
        3
    }
}

/// The bonus the `nth` (1-based) EXP scroll buff gives, in percent (50, 100 or 150): 1.5 for the first; the second
/// 2.0 below level 61 or with a special buff, else 1.5; the third 2.5 below level 41 or with a special, 2.0 below
/// level 61, else 1.5 (`MoverSkill.cpp:2451-2481`).
pub fn exp_scroll_percent(nth: i32, level: i32, special: i32) -> i32 {
    let factor: f32 = match nth {
        1 => 1.5,
        2 => {
            if special > 0 || level < 61 {
                2.0
            } else {
                1.5
            }
        }
        3 => {
            if special > 0 || level < 41 {
                2.5
            } else if level < 61 {
                2.0
            } else {
                1.5
            }
        }
        _ => 1.0,
    };
    ((factor - 1.0) * 100.0) as i32
}

/// The first free tier slot among the first `max_tiers` ones (`taken[i]` = tier `i` holds a buff), the offset to
/// add to the id of the scroll; `None` when all of them are taken.
pub fn exp_scroll_slot(taken: [bool; 3], max_tiers: i32) -> Option<usize> {
    let n = usize::try_from(max_tiers.clamp(0, 3)).unwrap_or(0);
    (0..n).find(|&i| !taken.get(i).copied().unwrap_or(true))
}

/// What using a banded EXP scroll (`AMPESA..E`, `AMPESS`) comes to, before the exclusivity with `AMPES`/`AMPEM`/`AMPI`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpScrollPlan {
    /// The level is outside the band of the scroll (`TID_GAME_EXP_NOTUSELEVEL`): result 99.
    WrongBand {
        /// The band of the scroll.
        band: (i32, i32),
    },
    /// A special buff runs and the scroll is not the special one: result 2.
    Limited,
    /// All tiers the level allows are taken (`TID_GAME_EXP_NOTUSE`): result 99.
    Full {
        /// The band of the player's level (`(0, 0)` above level 120).
        band: (i32, i32),
        /// The tiers the level allows.
        max_tiers: i32,
    },
    /// The buff `scroll + slot` is applied and the player is told `percent` percent.
    Apply {
        /// The offset added to the id of the scroll (0, 1 or 2).
        slot: usize,
        /// The bonus of this buff in percent (50, 100 or 150).
        percent: i32,
        /// The tiers the level allows.
        max_tiers: i32,
    },
}

/// Plans the use of a banded EXP scroll: `taken[i]` = tier `i` holds a buff (any family), `count` = the number of
/// EXP scroll buffs (several of them can share a tier), `special` = the number of `AMPESS*` buffs
/// (`MoverSkill.cpp:2259-2504`). The checks run in the order band, special, count.
pub fn exp_scroll_plan(
    scroll: u32,
    level: i32,
    taken: [bool; 3],
    count: i32,
    special: i32,
) -> ExpScrollPlan {
    use flyff_common::defs::item::II_SYS_SYS_SCR_AMPESS;
    if let Some((lo, hi)) = exp_scroll_band(scroll)
        && (level < lo || level > hi)
    {
        return ExpScrollPlan::WrongBand { band: (lo, hi) };
    }
    if special > 0 && scroll != II_SYS_SYS_SCR_AMPESS {
        return ExpScrollPlan::Limited;
    }
    let max_tiers = exp_scroll_max_tiers(level, special);
    if count < max_tiers {
        // the legacy fall-back: no free slot below the maximum cannot happen (every buff takes one), but the
        // original kept the id of the scroll in that case
        let slot = exp_scroll_slot(taken, max_tiers).unwrap_or(0);
        ExpScrollPlan::Apply {
            slot,
            percent: exp_scroll_percent(count + 1, level, special),
            max_tiers,
        }
    } else {
        ExpScrollPlan::Full {
            band: exp_level_band(level).unwrap_or((0, 0)),
            max_tiers,
        }
    }
}

/// The angel EXP an `AngelBuff` request needs: `exp1(angel level) / 100 * percent` in integers, with the
/// percent of the buff item read as 100 outside `1..=100`.
pub fn angel_goal(exp1: i64, percent: i32) -> i64 {
    let n = if (1..=100).contains(&percent) {
        i64::from(percent)
    } else {
        100
    };
    exp1 / 100 * n
}
