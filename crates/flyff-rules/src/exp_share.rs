//! Kill-EXP distribution math (Plan/16 §9; `Mover.cpp:5985-6370`): the solo level-gap penalty, one attacker's
//! share of the damage ledger and the even split of the special bosses. The walk over the ledger and the
//! messages are `flyff_game::stats::exp_share`; the party member shares (WP-MOVER-20) extend this module.
//!
//! The numeric types are those of the source: the raw EXP is `EXPFLOAT` (`f64`), the shares and the flight EXP
//! `float` (`f32`), the EXP of one grant `EXPINTEGER` (`i64`).

use crate::data::RulesData;
use crate::progression::limit_exp;
use flyff_common::defs::obj::{MI_BIGMUSCLE, MI_CLOCKWORK1, MI_KRRR, MI_LOADCLOCKWORK};

/// The radius in which a special boss splits its EXP evenly (`SubAroundExp( .., 50.0f )`).
pub const AROUND_EXP_RANGE: f32 = 50.0;
/// The most users one even split serves (`MAX_AROUND_USER`).
pub const MAX_AROUND_USER: usize = 512;
/// An attacker earns EXP only within this distance of the corpse (`IsValidArea( pEnemy, 64.0f )`, strict `<`).
pub const KILL_EXP_RANGE: f32 = 64.0;

/// The bosses whose EXP is split evenly among the users around the corpse (`AttackArbiter.cpp:938-944`): the
/// ClockWorks, Big Muscle and Krrr, and the Load Clockwork when `features.better_clockwork_alpha` is on.
pub fn is_around_exp_boss(mover_id: u32, better_clockwork_alpha: bool) -> bool {
    mover_id == MI_CLOCKWORK1
        || mover_id == MI_BIGMUSCLE
        || mover_id == MI_KRRR
        || (better_clockwork_alpha && mover_id == MI_LOADCLOCKWORK)
}

/// The level-gap factor of a solo kill (`AddExperienceSolo`): `d = player - monster`; `d` 1..=2 gives 0.7, 3..=4
/// gives 0.4, 5 and more 0.1, `d <= 0` gives `None` (no penalty). Applied to the EXP and the flight EXP alike.
pub fn solo_level_factor(player_level: i32, monster_level: i32) -> Option<f32> {
    let d = player_level.wrapping_sub(monster_level);
    if d <= 0 {
        None
    } else if d <= 2 {
        Some(0.7)
    } else if d <= 4 {
        Some(0.4)
    } else {
        Some(0.1)
    }
}

/// `fExpValuePerson = (float)(fExpValue * (float(hits) / float(total)))`: the attacker's share of the damage
/// ledger. `hits` is the legacy `int` (the party sum goes through a `DWORD` and back), `total` the `DWORD` sum of
/// the ledger; a zero `total` gives 0.
pub fn person_share(f_exp: f64, hits: u32, total: u32) -> f32 {
    if total == 0 {
        return 0.0;
    }
    (f_exp * f64::from(hits as i32 as f32 / total as f32)) as f32
}

/// The EXP and flight EXP of one solo grant after the level-gap factor and the per-level cap
/// (`AddExperienceSolo`, `Mover.cpp:6320-6335`): `f_exp *= (EXPFLOAT)factor`, `f_fxp *= factor`, then
/// `min(f_exp, limit_exp[player level])`.
pub fn solo_share(
    d: &RulesData,
    f_exp: f64,
    f_fxp: f32,
    player_level: i32,
    monster_level: i32,
) -> (f64, f32) {
    let (mut exp, mut fxp) = (f_exp, f_fxp);
    if let Some(f) = solo_level_factor(player_level, monster_level) {
        exp *= f64::from(f);
        fxp *= f;
    }
    let cap = limit_exp(d, player_level) as f64;
    if exp > cap {
        exp = cap;
    }
    (exp, fxp)
}

/// `GetProp()->nExpValue / (EXPINTEGER)nMaxList`: one user's equal share of a special boss (i64 division; 0 users
/// give 0).
pub fn around_split(exp_value: i64, users: usize) -> i64 {
    match i64::try_from(users) {
        Ok(n) if n > 0 => exp_value / n,
        _ => 0,
    }
}

/// The flight level is derived: 1 from character level 20 on, else 0 (`GetFlightLv`, 16 §0.13).
pub fn flight_level(level: i32) -> i32 {
    i32::from(level >= 20)
}

// ---- the party shares (Plan/16 §9.5; WP-MOVER-20; src:_Common/Mover.cpp:6372-6567) -------------------------------

/// A party member must be within this distance of the credited attacker to count (`IsValidArea( pUser, 64.0f )`
/// of `GetPartyMemberFind`, strict `<` on the squared distance).
pub const PARTY_MEMBER_RANGE: f32 = KILL_EXP_RANGE;
/// A party of this many members in range earns the full-party bonus of the contribution mode.
pub const FULL_PARTY_SIZE: usize = 8;
/// Members this many levels below the highest one in range earn nothing (`nMaxLevel10 = max - 20`).
pub const LEVEL_CUTOFF: i32 = 20;

/// The non-Korean table of `GetExperienceReduceFactor` (`LANG_KOR` has another one; this server is ENG).
const REDUCE_FACTORS: [f32; 10] = [0.8, 0.8, 0.6, 0.35, 0.2, 0.12, 0.08, 0.04, 0.02, 0.01];

/// `GetExperienceReduceFactor( monsterLevel, maxLevel )`: `delta = maxLevel - monsterLevel` clamped to 9; a
/// positive delta indexes the table (0.8 at 1 ... 0.01 from 9), anything else keeps the full EXP.
pub fn reduce_factor(monster_level: i32, max_level: i32) -> f32 {
    let delta = max_level.wrapping_sub(monster_level);
    if delta > 0 {
        let d = usize::try_from(delta.min(9)).unwrap_or(0);
        REDUCE_FACTORS.get(d).copied().unwrap_or(1.0)
    } else {
        1.0
    }
}

/// `nMaxLevel10` of `GetPartyMemberFind`: the highest level in range minus 20, at least 0. A member must be
/// **above** it to earn a share.
pub fn party_level_floor(max_level: i32) -> i32 {
    if 0 < max_level.wrapping_sub(LEVEL_CUTOFF) {
        max_level - LEVEL_CUTOFF
    } else {
        0
    }
}

/// `fMaxMemberLevel`: the sum of the squared levels, accumulated in `f32` in member order.
fn level_square_sum(levels: &[i32]) -> f32 {
    levels
        .iter()
        .fold(0.0_f32, |s, &l| s + (l as f32) * (l as f32))
}

/// The share of `level` in the squared-level sum, `(float)L * (float)L / fMaxMemberLevel`.
fn level_share(level: i32, sum: f32) -> f32 {
    ((level as f32) * (level as f32)) / sum
}

/// `AddExperiencePartyLevel` (`Mover.cpp:6503-6545`): the EXP of each member in range, `None` for a member at or
/// below the level floor (it gets nothing, not even a message).
///
/// `f_exp` is the raw EXP of the party share after the reduce factor (`EXPFLOAT`, `f64`); `levels` the levels of
/// the members in range in party order. `add = (f_exp * 0.2f * (n - 1)) as f32`; each member earns
/// `((f_exp + add) * (L*L / S))` truncated to `i64`.
pub fn party_level_mode(f_exp: f64, levels: &[i32], max_level10: i32) -> Vec<Option<i64>> {
    let n = levels.len() as i32;
    let sum = level_square_sum(levels);
    let add = ((f_exp * f64::from(0.2_f32)) * f64::from(n.wrapping_sub(1))) as f32;
    levels
        .iter()
        .map(|&l| {
            (max_level10 < l)
                .then(|| ((f_exp + f64::from(add)) * f64::from(level_share(l, sum))) as i64)
        })
        .collect()
}

/// The inputs of `AddExperiencePartyContribution`.
#[derive(Clone, Copy, Debug)]
pub struct ContributionInput<'a> {
    /// The raw EXP of the party share after the reduce factor.
    pub f_exp: f64,
    /// The levels of the members in range, party order.
    pub levels: &'a [i32],
    /// The damage of each of these members on the monster (`GetEnemyHit`), 0 when it did not hit.
    pub hits: &'a [i32],
    /// The size of the **whole** party (`pParty->m_nSizeofMember`, not the members in range: KEEP-QUIRK).
    pub party_size: usize,
    /// `nMaxLevel10`.
    pub max_level10: i32,
    /// The monster's maximum HP.
    pub monster_max_hp: i32,
}

/// `AddExperiencePartyContribution` (`Mover.cpp:6407-6501`): the EXP of each member in range, `None` for a
/// member at or below the level floor. Attackers earn their damage share
/// (`min(100, hits * 100 / maxHp)` percent, an `f32` expression) plus the level-weighted bonus, the option
/// bonus (two or more attackers) and the full-party bonus (eight in range); non-attackers earn the last three.
pub fn party_contribution_mode(i: &ContributionInput<'_>) -> Vec<Option<i64>> {
    let n = i.levels.len();
    let attackers = i.hits.iter().take(n).filter(|&&h| h != 0).count();
    let sum = level_square_sum(i.levels);
    let party_minus_1 = (i.party_size as i32).wrapping_sub(1);
    let add = ((i.f_exp * f64::from(0.2_f32)) * f64::from(party_minus_1)) as f32;
    let full = if n == FULL_PARTY_SIZE {
        (i.f_exp * f64::from(0.1_f32)) as f32
    } else {
        0.0
    };
    let option = if attackers > 1 {
        (i.f_exp * f64::from(attackers as f32) / f64::from(100.0_f32)) as f32
    } else {
        0.0
    };
    i.levels
        .iter()
        .enumerate()
        .map(|(k, &l)| {
            if l <= i.max_level10 {
                return None;
            }
            let r = level_share(l, sum);
            let hits = i.hits.get(k).copied().unwrap_or(0);
            if hits != 0 {
                let mut c = (hits as f32) * 100.0 / (i.monster_max_hp as f32);
                if 100.0 < c {
                    c = 100.0;
                }
                // `double + float + float + float`: every float term is widened before it is added
                let own = i.f_exp * f64::from(c / 100.0_f32);
                Some((own + f64::from(add * r) + f64::from(option) + f64::from(full)) as i64)
            } else {
                // `float + float + float`: the sum of the bonuses stays in single precision
                let bonus: f32 = add * r + option + full;
                Some(bonus as i64)
            }
        })
        .collect()
}
