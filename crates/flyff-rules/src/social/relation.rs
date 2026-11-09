//! Couple, campus and honor-title rules shared by the server and the browser (Plan/21 §11-§13, WP-SOCIAL-21..23):
//! the pure arithmetic of `CCoupleProperty::GetLevel` / `GetSkill`, `CCampusHelper::GetMaxPupilNum` and the
//! constants of `Campus.h` / `CampusHelper.h`. Wasm-safe: no I/O, no clock, integer arithmetic only.
//!
//! The game glue (the services, the messages, the persistence) is `flyff_game::social::{couple, campus, honor}`.

// ---- couples ---------------------------------------------------------------------------------------------------

/// `CCouple::eMaxLevel`: a couple stops gaining EXP at this level (648 couple minutes with the shipped table).
pub const COUPLE_MAX_LEVEL: i32 = 21;
/// `m_cbProcessCouple` ticks when it passes 60, i.e. every 61st cadence step (61 x 16 ticks = 65.07 s).
pub const COUPLE_TICK_STEPS: u32 = 61;
/// The proposal cooldown in seconds (`SetPropose( idProposer, now + 86400 )`; none with `features.no_couple_cd`).
pub const COUPLE_COOLDOWN_S: i64 = 86_400;

/// `CCoupleProperty::GetLevel(exp)`: the index of the FIRST threshold with `exp < threshold`, or 1 when there is
/// none. `level_exp[i]` is the EXP that reaches level `i + 1` (`couple.inc` `Level`, already divided by
/// `gameplay.couple_rate` by the loader): with the shipped table 13 -> 1, 14 -> 2, 647 -> 20, 648 -> 21.
///
/// KEEP: the first threshold wins, so a table whose later rows drop (the shipped file has `100000` rows after
/// level 31) is read in file order, exactly as the original does. A negative EXP is level 1.
pub fn couple_level(level_exp: &[i32], exp: i32) -> i32 {
    match level_exp.iter().position(|&threshold| exp < threshold) {
        Some(i) => i32::try_from(i).unwrap_or(i32::MAX).max(1),
        None => 1,
    }
}

/// The EXP a couple of EXP `exp` still needs for the next level (`level_exp[level] - exp`); `None` at
/// [`COUPLE_MAX_LEVEL`] (and for a table that ends before the next level).
pub fn couple_exp_to_next(level_exp: &[i32], exp: i32) -> Option<i32> {
    let level = couple_level(level_exp, exp);
    if level >= COUPLE_MAX_LEVEL {
        return None;
    }
    let next = *level_exp.get(usize::try_from(level).ok()?)?;
    (next > exp).then(|| next - exp)
}

/// The couple buff items of `level` (`CCoupleProperty::GetSkill`, `CUser::ActiveCoupleBuff`): `kinds` are the
/// `SkillKind` items, `rows` the `SkillLevel` rows `(level, level of each kind)`. The item of kind `i` with row
/// value `k > 0` is `kinds[i] + k - 1`; a level without a row of its own inherits the nearest row below it.
/// 1-5 POWER_01; 6-10 + BLESS_01; 11-15 POWER_02 + BLESS_01; 16-20 POWER_03 + BLESS_02 + MIRACLE_01; 21 POWER_04 +
/// BLESS_02 + MIRACLE_01.
pub fn couple_buff_items(kinds: &[u32], rows: &[(i32, Vec<i32>)], level: i32) -> Vec<u32> {
    let Some(best) = rows
        .iter()
        .map(|(l, _)| *l)
        .filter(|l| *l >= 1 && *l <= level)
        .max()
    else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for (_, per_kind) in rows.iter().filter(|(l, _)| *l == best) {
        for (kind, &k) in kinds.iter().zip(per_kind) {
            if k > 0 {
                items.push(kind.saturating_add(u32::try_from(k - 1).unwrap_or(0)));
            }
        }
    }
    items
}

/// `CTimeSpan::GetDays/GetHours/GetMinutes` of the rest of a proposal cooldown (`TID_GAME_COUPLE_M00` shows
/// "%d days %d hours %d minutes"): the whole days, the hours of the day and the minutes of the hour.
pub fn couple_cooldown_parts(seconds: i64) -> (i64, i64, i64) {
    let s = seconds.max(0);
    (s / 86_400, s % 86_400 / 3_600, s % 3_600 / 60)
}

// ---- campus ----------------------------------------------------------------------------------------------------

/// `MIN_MASTER_LEVEL`: a master needs this level.
pub const CAMPUS_MASTER_LEVEL: i32 = 91;
/// `COMPLETE_PUPIL_LEVEL`: a pupil graduates (is removed without penalty) at this level.
pub const CAMPUS_GRADUATION_LEVEL: i32 = 75;
/// `REMOVE_CAMPUS_POINT`: what the requester of a removal pays.
pub const CAMPUS_REMOVE_POINTS: i32 = 5;
/// `MAX_PUPIL_NUM`.
pub const CAMPUS_MAX_PUPILS: usize = 3;
/// `MIN_LV2_POINT`: the master points from which two pupils are allowed.
pub const CAMPUS_LV2_POINTS: i32 = 41;
/// `MIN_LV3_POINT`: the master points from which three pupils are allowed.
pub const CAMPUS_LV3_POINTS: i32 = 101;

/// `CCampusHelper::GetMaxPupilNum` by the master's campus points: 1 for 0..=40, 2 for 41..=100, 3 from 101 and 0
/// below zero (a master in debt cannot teach).
pub fn campus_max_pupils(points: i32) -> usize {
    if points < 0 {
        0
    } else if points < CAMPUS_LV2_POINTS {
        1
    } else if points < CAMPUS_LV3_POINTS {
        2
    } else {
        CAMPUS_MAX_PUPILS
    }
}

/// `IsMasterLevel`: the level condition of a master.
pub fn campus_master_capable(level: i32) -> bool {
    level >= CAMPUS_MASTER_LEVEL
}

/// `IsPupilLevel`: a vagrant or a first-class job (`job < MAX_EXPERT`).
pub fn campus_pupil_capable(job: i32, max_expert: i32) -> bool {
    job < max_expert
}

// ---- honor titles ----------------------------------------------------------------------------------------------

/// `MAX_HONOR_TITLE`: the counters of a character and the entries of `HonorListAck`; title ids at or above it
/// are never stored.
pub const HONOR_TITLE_COUNT: usize = 150;
/// `CheckHonorTime`: more than this many milliseconds since the mark counts one hour of collecting or vending.
pub const HONOR_HOUR_MS: u64 = 3_600_000;
/// `CheckHonorTime` runs on every 90th pass of the 1 s timer.
pub const HONOR_CHECK_SECONDS: u32 = 90;

/// The counter after `SetHonorAdd`: `value` when given, else one more than `old`. `None` when the result would
/// not fit below `i32::MAX` (`SetHonorCount` ignores a value at or above it).
pub fn honor_next_count(old: i32, value: Option<i32>) -> Option<i32> {
    let new = match value {
        Some(v) => v,
        None => old.checked_add(1)?,
    };
    (new < i32::MAX).then_some(new)
}
