//! Social formulas shared by the server and the browser (Plan/21 S6, §4.5; WP-SOCIAL-04): the party level, EXP
//! and point arithmetic of `CDPCoreSrvr::OnAddPartyExp` (`src:CORESERVER/DPCoreSrvr.cpp:744-826`).
//!
//! The integer widths are those of the source: the monster level, the party EXP and the points are `int` (`i32`),
//! the rate `s_fPartyExpRate` is a `float`, the "EXP up" factor 1.5 a `double`. The gate that decides whether a
//! kill feeds the party at all (`CParty::GetPoint`) is [`party_points_gate`]; the game glue (leader buffs, the
//! `PartyExp` broadcast) is `flyff_game::social::party::points`.

use flyff_data::exp_table::ExpPartyRow;

// couples, campus and honor titles (WP-SOCIAL-21..23, task couple-campus-honor)
mod relation;
pub use relation::*;

// the guild siege: kill points, chips, refunds, prize shares, ranking (WP-SOCIAL-17, -18, task siege-b)
pub mod siege;

// the 1:1 guild combat: the tender refunds and the chips of a round (WP-SOCIAL-25, -26, task gc11)
pub mod gc1to1;

/// `MAX_PARTY_MEMBERS` (`MAX_PTMEMBER_SIZE_SPECIAL`): the real party cap.
pub const MAX_PARTY_MEMBERS: usize = 8;
/// `MAX_PARTYLEVEL`: a normal party stops gaining levels here.
pub const MAX_PARTY_LEVEL: i32 = 10;
/// The party level a new party starts at.
pub const PARTY_START_LEVEL: i32 = 1;
/// The minimum party level to become an advanced party (`CHANGETROUP`, ref/14 §4.3).
pub const ADVANCED_MIN_LEVEL: i32 = 10;
/// Skill points an advanced party gains per level (`__MA_VER11_01`).
pub const ADVANCED_LEVEL_POINTS: i32 = 15;
/// A kill feeds the party only while `average level - monster level` stays below this (`GetPoint`).
pub const PARTY_POINTS_LEVEL_GAP: i32 = 5;

/// `m_nKindTroup`: a normal party or an advanced (named) one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PartyKind {
    /// 0: a normal party, capped at [`MAX_PARTY_LEVEL`].
    #[default]
    Normal = 0,
    /// 1: an advanced party, no level cap, party skills.
    Advanced = 1,
}

impl PartyKind {
    /// The wire value (`PartyInfo.kind_troupe`).
    pub const fn wire(self) -> i32 {
        self as i32
    }
}

/// The inputs of one party-EXP gain (`OnAddPartyExp`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartyExpInput {
    /// The party kind.
    pub kind: PartyKind,
    /// The party level before the kill.
    pub level: i32,
    /// The party EXP before the kill.
    pub exp: i32,
    /// The party skill points before the kill.
    pub point: i32,
    /// The level of the dead monster.
    pub monster_level: i32,
    /// `rates.party_exp` (`PartyExpRate`, 10.0 in the repack).
    pub rate: f32,
    /// The leader has the `II_SYS_SYS_SCR_SUPERLEADERPARTY` item buff: twice the EXP.
    pub super_leader: bool,
    /// The leader has a `PARTYEXPUP` buff: 1.5 times the EXP.
    pub leader_exp_up: bool,
}

/// `advanced_party_next_exp(level) = ((50 + level) * level / 13) * 10` in `i32` arithmetic: 460 at level 10, 1070
/// at 20, 3840 at 50.
pub fn advanced_party_next_exp(level: i32) -> i32 {
    (50_i32.wrapping_add(level))
        .wrapping_mul(level)
        .wrapping_div(13)
        .wrapping_mul(10)
}

/// The party EXP one kill adds: `(monLv / 25 + 1) * 10`, times the rate (truncated), doubled for a super leader,
/// times 1.5 (truncated) with an EXP-up buff.
pub fn party_exp_gain(i: &PartyExpInput) -> i32 {
    let mut add = (i.monster_level / 25).wrapping_add(1).wrapping_mul(10);
    add = (add as f32 * i.rate) as i32;
    if i.super_leader {
        add = add.wrapping_mul(2);
    }
    if i.leader_exp_up {
        add = (f64::from(add) * 1.5) as i32;
    }
    add
}

/// The kill-feeds-the-party gate of `CParty::GetPoint`: the integer average level of the members in range minus
/// the monster level must stay below [`PARTY_POINTS_LEVEL_GAP`]. `member_count` 0 never passes.
pub fn party_points_gate(total_level: i32, member_count: i32, monster_level: i32) -> bool {
    if member_count <= 0 {
        return false;
    }
    (total_level / member_count).wrapping_sub(monster_level) < PARTY_POINTS_LEVEL_GAP
}

/// One kill's effect on the party record, `(level, exp, point)` after it; `None` when the kill changes nothing: a
/// normal party at [`MAX_PARTY_LEVEL`] or above gains nothing (and a party level without a table row, which the
/// original would index out of range). At most one level is gained per kill.
///
/// `exp_party` is `GameData.exp.party` (`expParty`, row index = current level, row 0 a placeholder).
pub fn party_add_exp(i: &PartyExpInput, exp_party: &[ExpPartyRow]) -> Option<(i32, i32, i32)> {
    let add = party_exp_gain(i);
    let (mut level, mut exp, mut point) = (i.level, i.exp.wrapping_add(add), i.point);
    match i.kind {
        PartyKind::Normal => {
            if level >= MAX_PARTY_LEVEL {
                return None;
            }
            let row = exp_party.get(usize::try_from(level).ok()?)?;
            let need = i32::try_from(row.exp).unwrap_or(i32::MAX);
            if exp >= need {
                exp = exp.wrapping_sub(need);
                point = point.wrapping_add(i32::try_from(row.point).unwrap_or(i32::MAX));
                level = level.wrapping_add(1);
            }
        }
        PartyKind::Advanced => {
            let need = advanced_party_next_exp(level);
            if exp >= need {
                exp = exp.wrapping_sub(need);
                level = level.wrapping_add(1);
                point = point.wrapping_add(ADVANCED_LEVEL_POINTS);
            }
        }
    }
    Some((level, exp, point))
}

// ---- guilds (Plan/21 §5; WP-SOCIAL-09) -------------------------------------------------------------------------

use flyff_data::social::guild::GuildLevelRow;

/// `MAX_GUILD_LEVEL`: a guild never rises above this level.
pub const MAX_GUILD_LEVEL: i32 = 50;
/// The most members of the ranks Kingpin, Captain and Supporter (`sm_anMaxMemberLvSize`); the Master is always 1
/// and the Rookie cap is the guild's member limit (21 §5.2).
pub const GUILD_RANK_CAPS: [usize; 4] = [1, 5, 10, 20];
/// A guild created from a party needs at least this many members (21 §5.4).
pub const GUILD_CREATE_MIN_PARTY: usize = 3;
/// The highest logo a request may set (`CUSTOM_LOGO_MAX`).
pub const GUILD_LOGO_MAX: u32 = 27;
/// The logos from this number on are for game masters only.
pub const GUILD_LOGO_GM_FROM: u32 = 21;
/// A guild needs this level before its master sets the logo.
pub const GUILD_LOGO_MIN_LEVEL: i32 = 2;
/// A guild needs this level before its master gives nicknames.
pub const GUILD_NICKNAME_MIN_LEVEL: i32 = 10;
/// The longest notice in bytes (`MAX_BYTE_NOTICE` is 128 with the terminator).
pub const GUILD_NOTICE_MAX: usize = 127;
/// A rank's daily salary stays below this (`GuildPenyaReq`).
pub const GUILD_SALARY_LIMIT: u32 = 1_000_000;
/// The rejoin cooldown after leaving or being kicked (2 days, only with `features.remove_guild_cooldown = false`).
pub const GUILD_REJOIN_COOLDOWN_S: i64 = 172_800;
/// The longest guild name in characters (UD-21).
pub const GUILD_NAME_MAX: usize = 16;

/// `CGuild::AddContribution`'s level step (21 §5.3): the guild at `level` with `pxp` contribution points and `gold`
/// penya pays the cost of the NEXT level and rises by exactly one. `table` is `GameData.guild.levels`: row `i` is the
/// cost of level `i + 1`, so the next level `level + 1` is `table[level]` (the off-by-one trap: the member limit of
/// `level` is `table[level - 1]`). `None` when the guild is at [`MAX_GUILD_LEVEL`], cannot pay yet or the row is
/// missing; otherwise the points and penya that remain (never two rows at once).
pub fn guild_level_up(
    level: i32,
    pxp: u32,
    gold: u32,
    table: &[GuildLevelRow],
) -> Option<(u32, u32)> {
    if !(1..MAX_GUILD_LEVEL).contains(&level) {
        return None;
    }
    let next = table.get(usize::try_from(level).ok()?)?;
    if pxp < next.pxp_count || gold < next.penya {
        return None;
    }
    Some((pxp - next.pxp_count, gold - next.penya))
}

/// The member limit of a guild level (`CGuildTable::GetMaxMemeber`): row `level - 1`; `None` outside the table.
pub fn guild_max_members(level: i32, table: &[GuildLevelRow]) -> Option<u32> {
    let row = usize::try_from(level.checked_sub(1)?).ok()?;
    table.get(row).map(|r| r.max_member)
}

/// How many members `rank` (0 Master .. 4 Rookie) may have in a guild whose member limit is `max_members`
/// (`GetMaxMemberLvSize`); a rank outside `0..=4` has none.
pub fn guild_rank_cap(rank: u8, max_members: usize) -> usize {
    match rank {
        0..=3 => GUILD_RANK_CAPS.get(usize::from(rank)).copied().unwrap_or(0),
        4 => max_members,
        _ => 0,
    }
}

/// The Windows `MulDiv(number, numerator, denominator)` (21 §7.3, `OutGuildCombat` refund, `src:_Common/eveschool.cpp:468-512`):
/// the product in 64 bits, divided by `denominator` and rounded to the nearest integer with halves away from zero;
/// `-1` for a zero divisor or a result outside `i32`. `muldiv(100_000_001, 80, 100) == 80_000_001` (the 80 % cancel
/// refund of a bid of 100 000 001 penya). Integer arithmetic only: the browser and the server agree bit for bit.
pub fn muldiv(number: i32, numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        return -1;
    }
    // a positive divisor simplifies the rounding (the sign moves to the multiplicand)
    let (number, denominator) = if denominator < 0 {
        (-i64::from(number), -i64::from(denominator))
    } else {
        (i64::from(number), i64::from(denominator))
    };
    let product = number * i64::from(numerator);
    let half = denominator / 2;
    let rounded = if product >= 0 {
        (product + half) / denominator
    } else {
        (product - half) / denominator
    };
    i32::try_from(rounded).unwrap_or(-1)
}

// ---- guild wars (Plan/21 §6; WP-SOCIAL-15, task guild-war) ----------------------------------------------------

/// The length of a guild war: it ends by the timeout rule two hours after `started_at` (`CGuildWar::GetEndTime`,
/// 21 §6.3).
pub const WAR_DURATION_S: i64 = 7_200;
/// Both guilds need this level for a declaration (`GUILDWARREQLV6`, `GUILDWAROHTERLV6`).
pub const WAR_MIN_LEVEL: i32 = 6;
/// The guild that is challenged needs this many members, offline ones included (`GUILDWARMEMBER10`); the declarer
/// has no minimum.
pub const WAR_MIN_TARGET_MEMBERS: usize = 10;
/// A side whose surrenders are MORE than this percentage of its size at the start of the war loses
/// (`CDPCacheSrvr::OnSurrender`, strict `> 70`).
pub const WAR_SURRENDER_PERCENT: i64 = 70;

/// What a war result does to the win points of both guilds (`CGuildWarMng::Result`, `_Common/guildwar.cpp:165-330`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarPoints {
    /// The points the winner gains: `min(raw gain, cap)`.
    pub gain: i32,
    /// The winner's points after the war (`W + gain`).
    pub winner: i32,
    /// The loser's points after the war (never below 0).
    pub loser: i32,
}

/// The uncapped gain of the winner with `w` points against a loser with `l` points. The C++ expression is
/// `(int)( W + (float)(10 + W) / (float)( (float)(W + 4) / (float)(L + 1) * 0.05 ) + 1 )`: single precision
/// except the `* 0.05` (a `double` literal: float times double is a double, cast back to float). Plain IEEE
/// operations, no libm, so the browser and the server agree with the compiled original bit for bit.
pub fn war_gain_raw(w: i32, l: i32) -> i32 {
    let ratio =
        (((w.saturating_add(4)) as f32 / (l.saturating_add(1)) as f32) as f64 * 0.05) as f32;
    ((w as f32) + ((w.saturating_add(10)) as f32 / ratio) + 1.0f32) as i32
}

/// The most a winner with `w` points can gain: `(int)( (10 + W * 0.05) * 5 )` in double precision.
pub fn war_gain_cap(w: i32) -> i32 {
    ((10.0f64 + w as f64 * 0.05) * 5.0) as i32
}

/// The win points after a war that one guild won (`winner` has `w` points, `loser` has `l`): the winner gains
/// `min(raw, cap)` (21 §6.4); the loser loses `(int)(10 + L * 0.02)` and stops at 0. Two fresh guilds: raw 51, cap 50,
/// the winner ends with 50 and the loser stays at 0. A truce or a draw changes nothing (the caller does not call this).
pub fn war_points(w: i32, l: i32) -> WarPoints {
    let gain = war_gain_raw(w, l).min(war_gain_cap(w));
    let loss = (10.0f64 + l as f64 * 0.02) as i32;
    WarPoints {
        gain,
        winner: w.saturating_add(gain),
        loser: l.saturating_sub(loss).max(0),
    }
}

/// `true` when a side with `surrenders` surrendered members out of `size` at the start of the war loses: the integer
/// ratio `surrenders * 100 / size` (widened to `i64`, `size` at least 1) is strictly above [`WAR_SURRENDER_PERCENT`].
/// 8 of 11 (72) ends the war, 7 of 10 (70) does not.
pub fn surrender_ends_war(surrenders: i32, size: i32) -> bool {
    i64::from(surrenders) * 100 / i64::from(size.max(1)) > WAR_SURRENDER_PERCENT
}

/// `IsWarTarget` (21 §6.3, `MoverAttack.cpp:2168-2176`): `events.guild_war` is on, both are in the SAME war (a war id
/// above 0; a surrendered member has 0) and their guilds differ. `0` is "no guild" / "no war".
pub fn is_war_target(event_on: bool, war_a: u32, guild_a: u32, war_b: u32, guild_b: u32) -> bool {
    event_on && war_a != 0 && war_a == war_b && guild_a != guild_b
}
