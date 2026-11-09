//! Level, EXP and stat-point math (Plan/16 §7-§10, §18.3 `progression`).
//!
//! * stat points (WP-MOVER-07): [`gp_total_for`], [`restat_one_refund`], [`restat_one_low`];
//! * experience and levels (WP-MOVER-08, 16 §8): [`next_exp`], [`limit_exp`], [`exp_percent_x100`],
//!   [`exp_factor`] over [`ExpFactorInputs`], [`level_up_math`] (the pure core of `CMover::AddExperience`) and
//!   [`skill_points_on_level`];
//! * the death EXP penalty (WP-MOVER-10, 16 §10): [`die_penalty_rates`], [`death_exp_loss`],
//!   [`recovery_exp_adjust`], [`is_after_death`], [`exp_after_loss`].
//!
//! Everything follows the original arithmetic type by type (16 §0.2): `EXPINTEGER` is `i64`, `EXPFLOAT` `f64`,
//! `float` is `f32` with every operation rounded, `(EXPINTEGER)(x)` truncates toward zero.

use crate::consts::dst::DST_EXPERIENCE;
use crate::consts::{MAX_JOB_LEVEL, MAX_LEGEND_LEVEL, MAX_LEVEL, RESTATE_LOW};
use crate::data::RulesData;
use crate::dst::DstTable;
use crate::jobs::{is_base, is_expert, is_hero, is_master, is_pro};

/// The base value every stat is reset to by the restat items (`ReState`, `m_nStr = ... = 15`).
pub const RESTAT_BASE: i32 = 15;

/// The stat points a character of `level` (or of its `death_level`, whichever is higher: the restat uses the
/// highest level ever reached) has earned in total (`CMover::ReState`, `Mover.cpp:2001-2034`, 16 §7.3):
///
/// `GP = sum over i = 1 .. L' - 1 of  lp_point(i + 1)  +  1 if (master or hero) and i > 59  +  12 if hero and i == 120`
///
/// with `L' = max(level, death_level)` and `lp_point` the `expCharacter` column (2 for levels 2..199). The
/// extra points depend on the **current** job, as in the original.
pub fn gp_total_for(d: &RulesData, level: i32, death_level: i32, job: i32) -> i32 {
    let top = level.max(death_level);
    let legend = is_master(d, job) || is_hero(d, job);
    let hero = is_hero(d, job);
    let mut gp = 0_i32;
    let mut i = 1;
    while i < top {
        let lp = d.exp_row(i + 1).map_or(0, |r| r.lp_point as i32);
        gp = gp.wrapping_add(lp);
        if legend && i > 59 {
            gp = gp.wrapping_add(1);
        }
        if hero && i == MAX_LEVEL {
            gp = gp.wrapping_add(12);
        }
        i += 1;
    }
    gp
}

/// The refund of `ReStateOne` for a stat of `value`: everything above the base.
pub fn restat_one_refund(value: i32) -> i32 {
    value.wrapping_sub(RESTAT_BASE)
}

/// `ReStateOneLow`: `(refund, new_value)` for a stat of `value`: with at least `RESTATE_LOW + 15` the cheap
/// scroll takes `RESTATE_LOW` (50) back, otherwise everything above the base.
pub fn restat_one_low(value: i32) -> (i32, i32) {
    if value >= RESTATE_LOW + RESTAT_BASE {
        (RESTATE_LOW, value - RESTATE_LOW)
    } else {
        (value.wrapping_sub(RESTAT_BASE), RESTAT_BASE)
    }
}

// ---- experience tables (16 §8.1) --------------------------------------------------------------------------

/// `GetMaxExp1()` for a mover of `level`: the EXP needed to go from `level` to `level + 1`, i.e. the `EXP` column of
/// `expCharacter` row `level + 1`. A level beyond the table gives `i64::MAX` (nobody levels up there; the original
/// read outside its array).
pub fn next_exp(d: &RulesData, level: i32) -> i64 {
    d.exp_row(level.wrapping_add(1))
        .map_or(i64::MAX, |r| r.exp1)
}

/// `m_aExpCharacter[level].nLimitExp`: the largest raw EXP one kill may give at `level` (applied before the
/// multipliers). 0 for a level the table does not have.
pub fn limit_exp(d: &RulesData, level: i32) -> i64 {
    d.exp_row(level).map_or(0, |r| r.limit_exp)
}

/// `GetExpPercent()` times 100: `(int)(exp * 10000 / next_exp(level))`, `0..=9999` while `exp < next_exp`. A level
/// without a table row (or a row of 0) gives 0.
pub fn exp_percent_x100(d: &RulesData, level: i32, exp: i64) -> i32 {
    let max = next_exp(d, level);
    if max <= 0 || max == i64::MAX {
        return 0;
    }
    (exp.wrapping_mul(10_000) / max) as i32
}

/// The skill points `InitLevel` grants (16 §11.3), defined in [`crate::jobs`].
pub use crate::jobs::sp_init_for;

/// The skill points of one level-up for a non-master, non-hero character: `(level - 1) / 20 + 2` (2 for levels
/// 2..=20, 3, 4, 5, 6, 7 for 101..=120; `MoverParam.cpp:1415`).
pub fn skill_points_on_level(level: i32) -> i32 {
    (level.wrapping_sub(1) / 20).wrapping_add(2)
}

// ---- the PK EXP hook (16 §16) ------------------------------------------------------------------------------

/// What [`pk_exp_step`] computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PkExpStep {
    /// `m_dwPKExp` afterwards.
    pub pk_exp: u32,
    /// `m_dwPKPropensity` afterwards.
    pub propensity: u32,
    /// The propensity was lowered (the caller broadcasts it): `pk_exp / level_exp` was not 0.
    pub changed: bool,
}

/// The PK propensity hook of `AddExperience` (`MoverParam.cpp:1288-1310`, 16 §16): a chaotic player that earns `n`
/// EXP from a monster adds it to `pk_exp` (`DWORD` arithmetic), and every full `level_exp` of it (the
/// `PKSetting` `LEVEL_EXP` of the player's level, 4 400 000 by default) lowers the propensity by one; the
/// remainder stays. A propensity that would go below 0 becomes 0 (the legacy underflow guard compares with the
/// old value). A `level_exp` of 0 changes nothing but the sum.
pub fn pk_exp_step(pk_exp: u32, propensity: u32, n: i64, level_exp: u32) -> PkExpStep {
    let sum = pk_exp.wrapping_add(n as u32);
    if level_exp == 0 {
        return PkExpStep {
            pk_exp: sum,
            propensity,
            changed: false,
        };
    }
    let sub = sum / level_exp;
    if sub == 0 {
        return PkExpStep {
            pk_exp: sum,
            propensity,
            changed: false,
        };
    }
    let mut lowered = propensity.wrapping_sub(sub);
    if propensity <= lowered {
        lowered = 0;
    }
    PkExpStep {
        pk_exp: sum % level_exp,
        propensity: lowered,
        changed: true,
    }
}

// ---- GetExpFactor (16 §8.3) -------------------------------------------------------------------------------

/// The inputs of `CMover::GetExpFactor` that are not in the DST table (16 §8.3). The server fills it from the
/// buff list, the mode word, `[rates]` and the event services (docs 17, 22); the neutral [`Default`] is factor 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpFactorInputs {
    /// `IsMode(EVENT_OLDBOY_MODE)`: x1.1.
    pub oldboy_mode: bool,
    /// How many of the 13 `II_SYS_SYS_SCR_AMPES{A,B,C,D}{,1,2}` / `AMPESE` buffs (plus the three special
    /// `AMPESS{,1,2}`) are active.
    pub ampes_count: u32,
    /// One of the special `AMPESS*` buffs is active.
    pub ampes_special: bool,
    /// `II_SYS_SYS_SCR_AMPES`: x1.5.
    pub ampes_legacy: bool,
    /// `II_SYS_SYS_SCR_AMPEM`: x2.0.
    pub ampem: bool,
    /// `II_SYS_SYS_SCR_GLVCHEER`: x1.10.
    pub glv_cheer: bool,
    /// `II_CHEERUP` (only without `glv_cheer`).
    pub cheer_up: bool,
    /// `EventLua.GetCheerExpFactor()`: replaces the 1.05 of `II_CHEERUP` when it is larger.
    pub lua_cheer_factor: f32,
    /// `CEventGeneric::GetExpFactor()` (doc 22, `propEvent.inc`).
    pub generic_event: f32,
    /// `EventLua.GetExpFactor()` (doc 22; x20 in this repack).
    pub lua_event: f32,
    /// `prj.m_fMonsterExpRate` (`rates.monster_exp`, 1.0 when unset).
    pub monster_exp_rate: f32,
    /// `ILordEvent::GetEFactor()` (doc 22).
    pub lord_e_factor: f32,
    /// `II_SYS_SYS_LS_CHEERING`: x1.1.
    pub ls_cheering: bool,
    /// `II_SYS_SYS_VOTE_THANKS`: x1.05.
    pub vote_thanks: bool,
    /// `EventLua.GetWeatherEventExpFactor()`, only while the environment effect is on (`None` = off).
    pub weather_factor: Option<f32>,
}

impl Default for ExpFactorInputs {
    fn default() -> Self {
        ExpFactorInputs {
            oldboy_mode: false,
            ampes_count: 0,
            ampes_special: false,
            ampes_legacy: false,
            ampem: false,
            glv_cheer: false,
            cheer_up: false,
            lua_cheer_factor: 1.0,
            generic_event: 1.0,
            lua_event: 1.0,
            monster_exp_rate: 1.0,
            lord_e_factor: 1.0,
            ls_cheering: false,
            vote_thanks: false,
            weather_factor: None,
        }
    }
}

/// `CMover::GetExpFactor()` (`MoverParam.cpp:4191-4336`): the f32 product in the order of the source.
/// `DST_EXPERIENCE` is read from `dst` (`GetParam(DST_EXPERIENCE, 0)`).
pub fn exp_factor(i: &ExpFactorInputs, level: i32, dst: &DstTable) -> f32 {
    let mut f = 1.0_f32;
    if i.oldboy_mode {
        f *= 1.1;
    }
    match i.ampes_count {
        0 => {}
        1 => f *= 1.5,
        2 => {
            if i.ampes_special || level < 61 {
                f *= 2.0;
            } else {
                f *= 1.5;
            }
        }
        _ => {
            if i.ampes_special || level < 41 {
                f *= 2.5;
            } else if level < 61 {
                f *= 2.0;
            } else {
                f *= 1.5;
            }
        }
    }
    if i.ampes_legacy {
        f *= 1.5;
    }
    if i.ampem {
        f *= 2.0;
    }
    if i.glv_cheer {
        f *= 1.10;
    } else if i.cheer_up {
        if i.lua_cheer_factor > 1.05 {
            f *= i.lua_cheer_factor;
        } else {
            f *= 1.05;
        }
    }
    f *= i.generic_event;
    f *= i.lua_event;
    f *= i.monster_exp_rate;
    let dst_exp = dst.param(DST_EXPERIENCE, 0);
    if 0 < dst_exp {
        f *= 1.0 + dst_exp as f32 / 100.0;
    }
    f *= i.lord_e_factor;
    if i.ls_cheering {
        f *= 1.1;
    }
    if i.vote_thanks {
        f *= 1.05;
    }
    if let Some(w) = i.weather_factor {
        f *= w;
    }
    f
}

// ---- AddExperience (16 §8.2) ------------------------------------------------------------------------------

/// The job tier flags `AddExperience` branches on (`IsBaseJob/IsExpert/IsPro/IsMaster/IsHero`; all false for a
/// job the table does not list).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tier {
    /// `IsBaseJob()`: the vagrant (cap 15).
    pub base: bool,
    /// `IsExpert()` (cap 60).
    pub expert: bool,
    /// `IsPro()` (cap 120, sticks at 99.99 %).
    pub pro: bool,
    /// `IsMaster()` (cap 120, EXP halved).
    pub master: bool,
    /// `IsHero()` (cap 129, EXP halved).
    pub hero: bool,
}

impl Tier {
    /// The tier of `job` in the job table.
    pub fn of(d: &RulesData, job: i32) -> Tier {
        Tier {
            base: is_base(d, job),
            expert: is_expert(d, job),
            pro: is_pro(d, job),
            master: is_master(d, job),
            hero: is_hero(d, job),
        }
    }

    /// `IsMaster() || IsHero()`.
    pub fn legend(&self) -> bool {
        self.master || self.hero
    }
}

/// The fields `AddExperience` reads and writes (a copy of the mover's progression).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressState {
    /// `m_nLevel`.
    pub level: i32,
    /// `m_nExp1`.
    pub exp: i64,
    /// `m_nRemainGP`.
    pub stat_points: i32,
    /// `m_nDeathLevel`.
    pub death_level: i32,
    /// `m_nAngelExp`.
    pub angel_exp: i64,
    /// `m_nAngelLevel`.
    pub angel_level: i32,
}

/// One call of `AddExperience`: the raw EXP and the switches (16 §8.2 signature).
pub struct ExpInput<'a> {
    /// `nExp`.
    pub n: i64,
    /// `bFirstCall`: the factor, the rest-point bonus and the angel buff apply only to the first call.
    pub first_call: bool,
    /// `bMultiPly`: apply [`ExpInput::factor`] (first call only).
    pub multiply: bool,
    /// The result of [`exp_factor`] (read when `first_call && multiply`).
    pub factor: f32,
    /// `GetAddExpAfterApplyRestPoint(nExp)` (doc 22, M7); called with the halved `n` when the factor applies.
    pub rest_bonus: &'a mut dyn FnMut(i64) -> i64,
    /// The angel buff, when the mover has one: `(int)(float)itemProp->nAdjParamVal1`, or 100 when the buff has no
    /// item prop (first call only).
    pub angel: Option<i32>,
}

/// One real level-up (`bLevelUp`) of an `AddExperience` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelStep {
    /// The level reached.
    pub level: i32,
    /// The stat points this level gives (`LP`, +1 for masters and heroes) before a regained level takes them back.
    pub gp: i32,
    /// `m_nDeathLevel >= level`: the level was lost to a death and is regained: no GP, no SP, no hooks.
    pub regained: bool,
}

/// The angel experience that changed (doc 19 sends `Angel{ANGEL_INFO, exp, level, complete}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AngelUpdate {
    /// `m_nAngelExp` after the grant.
    pub exp: i64,
    /// `m_nAngelLevel`.
    pub level: i32,
    /// The angel experience reached its maximum.
    pub complete: bool,
}

/// What an `AddExperience` call did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelUpOutcome {
    /// The return value of `AddExperience`: a level-up happened, or a tier cap reset the EXP (the callers treat it
    /// as a level-up, KEEP).
    pub changed: bool,
    /// The real level-ups in order (the carry of one call can cross several levels).
    pub steps: Vec<LevelStep>,
    /// The EXP that went into `exp` in the first pass (after halving, factor and angel): the amount the PK hook
    /// of 16 §16 adds to `m_dwPKExp`.
    pub added: i64,
    /// The level before the call (the PK hook reads `GetLevel()` before the level-up).
    pub level_before: i32,
    /// The angel experience gain, if the angel buff took part.
    pub angel: Option<AngelUpdate>,
}

/// The pure core of `CMover::AddExperience` (`MoverParam.cpp:1057-1481`, 16 §8.2) without any side effect besides
/// `st`: halving for masters and heroes on **every** pass including the carry (C16-6), the factor and the
/// rest-point bonus, the angel buff, the tier caps, the level-up with GP, the regained-level rule and the carry
/// recursion (as a loop). The checks that need the mover (`MODE_EXPUP_STOP`, dead) are the caller's; `n <= 0`
/// returns an unchanged state. HP/MP/FP refills, skill points and the hooks are applied by the caller from the
/// returned [`LevelStep`]s.
///
/// `i64 -> f32 -> i64` is kept on purpose: `(EXPINTEGER)(nExp * GetExpFactor())` loses precision for large `n`
/// (123 456 789 x 20 = 2 469 135 872, not 2 469 135 780).
pub fn level_up_math(
    d: &RulesData,
    tier: Tier,
    st: &mut ProgressState,
    inp: ExpInput<'_>,
) -> LevelUpOutcome {
    let mut out = LevelUpOutcome {
        changed: false,
        steps: Vec::new(),
        added: 0,
        level_before: st.level,
        angel: None,
    };
    let ExpInput {
        n,
        mut first_call,
        multiply,
        factor,
        rest_bonus,
        angel,
    } = inp;
    if n <= 0 {
        return out;
    }
    let mut n = n;
    let mut outermost = true;
    // The first pass decides the return value; later passes (the carry recursion) cannot change it.
    let mut result = false;
    loop {
        if tier.legend() {
            n /= 2;
        }
        if first_call && multiply {
            let add = rest_bonus(n);
            n = ((n as f32) * factor) as i64;
            n = n.wrapping_add(add);
        }
        if first_call && let Some(pct) = angel {
            let pct = if pct <= 0 || pct > 100 { 100 } else { pct };
            let max_angel = d
                .exp_row(st.angel_level)
                .map_or(0, |r| r.exp1)
                .wrapping_div(100)
                .wrapping_mul(i64::from(pct));
            if st.angel_exp < max_angel {
                n /= 2;
                st.angel_exp = st.angel_exp.wrapping_add(n);
                let complete = st.angel_exp > max_angel;
                if complete {
                    st.angel_exp = max_angel;
                }
                out.angel = Some(AngelUpdate {
                    exp: st.angel_exp,
                    level: st.angel_level,
                    complete,
                });
            }
        }
        // 5: the tier caps, before adding (a vagrant at 15 and an expert at 60 earn nothing; a pro or master above
        // 120 and a hero above 129 are put back)
        let capped = if (tier.base && st.level >= MAX_JOB_LEVEL)
            || (tier.expert && st.level >= MAX_JOB_LEVEL + crate::consts::MAX_EXP_LEVEL)
        {
            st.exp = 0;
            Some(true)
        } else if (tier.pro || tier.master) && st.level > MAX_LEVEL {
            st.level = MAX_LEVEL;
            Some(true)
        } else if tier.hero && st.level > MAX_LEGEND_LEVEL {
            st.level = MAX_LEGEND_LEVEL;
            Some(true)
        } else {
            None
        };
        if let Some(r) = capped {
            if outermost {
                result = r;
            }
            break;
        }
        // 6: add
        st.exp = st.exp.wrapping_add(n);
        if outermost {
            out.added = n;
        }
        // 8: level up?
        let next = st.level.wrapping_add(1);
        let (next_exp1, lp) = d
            .exp_row(next)
            .map_or((i64::MAX, 0), |r| (r.exp1, r.lp_point as i32));
        if st.exp < next_exp1 {
            break;
        }
        let cap = if tier.hero {
            MAX_LEGEND_LEVEL
        } else {
            MAX_LEVEL
        };
        if next > cap {
            st.level = cap;
            st.exp = next_exp1.wrapping_sub(1);
            break;
        }
        let gp = lp.wrapping_add(i32::from(tier.legend()));
        st.stat_points = st.stat_points.wrapping_add(gp);
        let mut carry = st.exp.wrapping_sub(next_exp1);
        st.exp = 0;
        st.level = next;
        let mut level_up = true;
        if tier.base && st.level > MAX_JOB_LEVEL {
            st.level = MAX_JOB_LEVEL;
            level_up = false;
        } else if tier.expert && st.level > MAX_JOB_LEVEL + crate::consts::MAX_EXP_LEVEL {
            st.level = MAX_JOB_LEVEL + crate::consts::MAX_EXP_LEVEL;
            level_up = false;
        } else if (tier.pro || tier.master) && st.level > MAX_LEVEL {
            st.level = MAX_LEVEL;
            st.exp = next_exp1.wrapping_sub(1);
            if outermost {
                result = false;
            }
            break;
        } else if tier.hero && st.level > MAX_LEGEND_LEVEL {
            st.level = MAX_LEGEND_LEVEL;
            st.exp = next_exp1.wrapping_sub(1);
            if outermost {
                result = false;
            }
            break;
        }
        if level_up {
            let regained = st.death_level >= st.level;
            if regained {
                st.stat_points = st.stat_points.wrapping_sub(gp);
            }
            out.steps.push(LevelStep {
                level: st.level,
                gp,
                regained,
            });
        } else {
            st.stat_points = st.stat_points.wrapping_sub(gp);
            st.exp = 0;
            carry = 0;
        }
        if outermost {
            result = true;
        }
        if carry > 0 {
            n = carry;
            first_call = false;
            outermost = false;
            continue;
        }
        break;
    }
    out.changed = result;
    out
}

// ---- death EXP penalty (16 §10) ---------------------------------------------------------------------------

/// `GetDieDecExp(level)` lookup in a `DiePenalty.inc` table: the first row `i` with `level <= rows[i].level`
/// (row 0) or `rows[i-1].level < level <= rows[i].level`; no such row means the last one (`Mover.cpp:7171-7215`).
fn penalty_row(rows: &[(i32, i32)], level: i32) -> Option<i32> {
    let mut i = 0;
    while let Some(&(row_level, _)) = rows.get(i) {
        let above_previous = i == 0 || rows.get(i - 1).is_some_and(|p| p.0 < level);
        if above_previous && level <= row_level {
            break;
        }
        i += 1;
    }
    rows.get(i).or_else(|| rows.last()).map(|r| r.1)
}

/// `GetDieDecExp(level)`: `(revival HP rate, EXP loss fraction, level-down flag)` of `DiePenalty.inc` (values
/// are percent x 100, so `8000` is 0.8). The level-down flag is reported but ignored by the caller (KQ-09). Without a
/// table the defaults of `SubDieDecExp` apply: rate 0.1, no loss.
pub fn die_penalty_rates(d: &RulesData, level: i32) -> (f32, f32, bool) {
    let t = &d.die_penalty;
    let rate = penalty_row(&t.revival, level).map_or(0.1, |v| v as f32 / 10_000.0);
    let dec = penalty_row(&t.dec_exp, level).map_or(0.0, |v| v as f32 / 10_000.0);
    let lvdown = penalty_row(&t.level_down, level).is_some_and(|v| v != 0);
    (rate, dec, lvdown)
}

/// `GetDieDecExpRate`, the resurrection part: a revival that restores only `v` percent of the loss (the resurrection
/// skill's `DST_RECOVERY_EXP`): `dec - dec * ((100 - v) / 100.0f)` in `f32` (C16-5). `v == 0` leaves `dec` alone.
pub fn recovery_exp_adjust(dec: f32, v: u32) -> f32 {
    if v == 0 {
        return dec;
    }
    // `(float)(100 - dwDestParam)`: unsigned arithmetic, as in the source
    let add_dec = 100_u32.wrapping_sub(v) as f32 / 100.0;
    dec - dec * add_dec
}

/// `(EXPINTEGER)(m_aExpCharacter[level + 1].nExp1 * fPercent)`: the EXP a penalty of `dec` (a fraction) takes away,
/// with the product in `f32` (KEEP). Goldens: the 13 547 310 EXP of level 60 at 5 % lose 677 365 (the real table
/// charges level 60 only 4 %: 541 892), the 22 353 256 174 of level 120 at 1.5 % lose 335 298 848 (the exact
/// product is 335 298 842.6).
pub fn death_exp_loss(next_exp: i64, dec: f32) -> i64 {
    ((next_exp as f32) * dec) as i64
}

/// `CMover::DecExperience` without level-down: `exp - loss`, floored at 0; level 1 never loses anything.
pub fn exp_after_loss(level: i32, exp: i64, loss: i64) -> i64 {
    if loss < 0 || level.wrapping_sub(1) < 1 {
        return exp;
    }
    exp.wrapping_sub(loss).max(0)
}

/// `CMover::IsAfterDeath()`: the character is below the level or EXP it had at its last death penalty (a lost
/// level is not yet regained).
pub fn is_after_death(level: i32, exp: i64, death_level: i32, death_exp: i64) -> bool {
    if death_level > level {
        return true;
    }
    death_level == level && (exp == 0 || death_exp > exp)
}
