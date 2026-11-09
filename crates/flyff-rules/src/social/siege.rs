//! The arithmetic of the guild siege (Plan/21 §7.5-7.6; WP-SOCIAL-17, -18; `src:_Common/eveschool.cpp`): the kill
//! points, the red chips, the 98 % refund of the bidders that were not admitted, the prize shares, the average
//! level and the ranking rules of the guilds. Pure functions over plain numbers, bit-exact with the original x86
//! build (`float` arithmetic in the order of the C++ expression), so the browser and the server agree.
//!
//! Where the original used `float` the functions use `f32` in the same operation order: the chip formula is **not**
//! the `f64` expression of Plan/21 §7.6 (the two agree for 1..=24 bidders and differ from 25: 1574 / 449 / 224
//! chips against 1575 / 450 / 225), and the refund of a not admitted bid is **not** `MulDiv` (that is the cancel
//! refund of [`super::muldiv`]).

use std::cmp::Ordering;

/// The base of a kill (`int nGetPoint = 2`, `CGuildCombat::GetPoint`).
pub const KILL_BASE_POINTS: i32 = 2;
/// The points of a Holy Cross support kill (`bSupport`: `nGetPoint = 1`).
pub const SUPPORT_KILL_POINTS: i32 = 1;
/// The ranks that get red chips (`if( i >= 3 ) break`).
pub const CHIP_RANKS: usize = 3;
/// The share of the base chip count per rank (`fChipNum *= 0.7f / 0.2f / 0.1f`).
pub const CHIP_RANK_SHARES: [f32; CHIP_RANKS] = [0.7, 0.2, 0.1];

/// `GetPoint`: the points of one kill. `master` = the attacker is the master of his guild, `defender` = the victim is
/// the defender of his guild, `last_life` = the attacker has exactly one life left (read after the victim's death
/// was booked), `support` = a Holy Cross support kill (exactly 1 point). The "killed a guild other than the last
/// time" bonus of the original never applies (KQ-20: `uKillidGuild` is never set), so it is not a parameter.
///
/// `kill_points(true, true, true, false) == 5`, the maximum.
pub fn kill_points(master: bool, defender: bool, last_life: bool, support: bool) -> i32 {
    if support {
        return SUPPORT_KILL_POINTS;
    }
    KILL_BASE_POINTS + i32::from(master) + i32::from(defender) + i32::from(last_life)
}

/// `GuildCombatResultRanking`: the red chips of the guild ranked `rank` (0 = first) when `n_bidders` guilds bid in
/// all (admitted or not, KQ-39 i) and the minimum bid is `join_penya`:
/// `(join_penya * n_bidders) as f32 * 0.9f * 0.00001f * 0.1f * [0.7f, 0.2f, 0.1f][rank]`, truncated. The caller
/// awards `max(1, chips)` to every online selected player. A rank beyond the third has no share in the original
/// (it never gets chips); the unscaled base is returned for it.
///
/// `siege_chips(100_000_000, 2, 0) == 126`, `(.., 2, 1) == 36`, `(.., 2, 2) == 18`.
pub fn siege_chips(join_penya: i32, n_bidders: usize, rank: usize) -> i32 {
    // `int * size_t` of the 32-bit original wraps from 43 bidders on (4.3e9 penya); the product is exact here
    let product =
        i64::from(join_penya).saturating_mul(i64::try_from(n_bidders).unwrap_or(i64::MAX));
    let mut chips = product as f32 * 0.9_f32 * 0.00001_f32 * 0.1_f32;
    if let Some(share) = CHIP_RANK_SHARES.get(rank) {
        chips *= *share;
    }
    c_int(chips)
}

/// `(int)` of a non-negative `float`: truncation, saturating (the original's value range never leaves `int`).
fn c_int(x: f32) -> i32 {
    if x.is_nan() { 0 } else { x as i32 }
}

/// `SendGuildCombatResult`: the refund of a bid that was not admitted,
/// `(int)(dwPenya * (NOTREQUESTPERCENT / 100.0f))`, mailed to the master of the guild. `f32` throughout:
/// `not_admitted_refund(100_000_000, 98) == 98_000_000`, `(100_000_001, 98) == 98_000_000` (the bid rounds to
/// 1e8 as a `float`), `(2_147_483_647, 98) == 2_104_534_016`.
pub fn not_admitted_refund(bid: u32, percent: i32) -> i32 {
    let rate = percent as f32 / 100.0_f32;
    c_int(bid as f32 * rate)
}

/// `GetPrizePenya(0 / 1)`: `percent` % of the prize pool (the sum of the admitted bids), rounded like `MulDiv`
/// (halves away from zero). FIX: the original cast the sum to `int` first; here the arithmetic is 64 bits wide.
/// The shipped percentages are 0, so nobody ever wins penya (KQ-20).
pub fn prize_share(total: i64, percent: i32) -> i64 {
    let product = i128::from(total) * i128::from(percent);
    let rounded = if product >= 0 {
        (product + 50) / 100
    } else {
        (product - 50) / 100
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

/// The integer average level of the guild's players that count (`(float)( nSubLevel / nValidObjCount )`: the
/// division is an `int` division), `0.0` without one.
pub fn avg_level(levels: &[i32]) -> f32 {
    let n = i32::try_from(levels.len()).unwrap_or(i32::MAX);
    if n == 0 {
        return 0.0;
    }
    let sum: i32 = levels.iter().fold(0_i32, |a, l| a.wrapping_add(*l));
    (sum / n) as f32
}

/// [`avg_level`] as the chip ranking reads it: `1.0` when nobody counts (`fAvg = 1.0f`).
pub fn avg_level_for_ranking(levels: &[i32]) -> f32 {
    if levels.is_empty() {
        1.0
    } else {
        avg_level(levels)
    }
}

/// What the result ranks a guild by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuildScore {
    /// Σ (player points + lives) (`nGuildPoint` after `GuildCombatResult`).
    pub points: i32,
    /// Σ lives of all its selected players.
    pub lives: i32,
    /// The average level of the online players with lives left ([`avg_level`] or [`avg_level_for_ranking`]).
    pub avg_level: f32,
}

/// The total order of the results: [`Ordering::Less`] when `a` ranks before `b` (more points, then more lives,
/// then the higher average level).
pub fn rank_cmp(a: &GuildScore, b: &GuildScore) -> Ordering {
    b.points.cmp(&a.points).then(b.lives.cmp(&a.lives)).then(
        b.avg_level
            .partial_cmp(&a.avg_level)
            .unwrap_or(Ordering::Equal),
    )
}

/// The winner of `GuildCombatResult` among `scores` in rank order: the best by [`rank_cmp`], the FIRST of equal
/// guilds (rank order breaks the last tie). `None` for an empty list.
pub fn pick_winner(scores: &[GuildScore]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, s) in scores.iter().enumerate() {
        match best {
            None => best = Some(i),
            Some(b) => {
                let current = scores.get(b)?;
                if rank_cmp(s, current) == Ordering::Less {
                    best = Some(i);
                }
            }
        }
    }
    best
}

/// `true` when the bubble pass of `GuildCombatResultRanking` swaps `upper` (the earlier place) with `lower`:
/// fewer points, or equal points and fewer lives, or equal both and a lower average level.
pub fn ranking_swaps(upper: &GuildScore, lower: &GuildScore) -> bool {
    if upper.points != lower.points {
        return upper.points < lower.points;
    }
    if upper.lives != lower.lives {
        return upper.lives < lower.lives;
    }
    upper.avg_level < lower.avg_level
}

/// The order `GuildCombatResultRanking` gives `scores` (indices, best first): the partial bubble sort of the
/// original, `max_pass` = `MAXJOINGUILDSIZE` outer passes of `size - 1 - pass` comparisons each. With more than
/// `max_pass + 1` guilds the front of the list is not sorted completely (KEEP); equal guilds keep their order.
pub fn bubble_rank(scores: &[GuildScore], max_pass: usize) -> Vec<usize> {
    let n = scores.len();
    let mut order: Vec<usize> = (0..n).collect();
    for pass in 0..n.saturating_sub(1) {
        if pass >= max_pass {
            break;
        }
        for j in 0..(n - 1 - pass) {
            let (Some(&a), Some(&b)) = (order.get(j), order.get(j + 1)) else {
                break;
            };
            if let (Some(upper), Some(lower)) = (scores.get(a), scores.get(b))
                && ranking_swaps(upper, lower)
            {
                order.swap(j, j + 1);
            }
        }
    }
    order
}
