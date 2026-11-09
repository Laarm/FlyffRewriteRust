//! Derived stats (Plan/16 §5; F-113 movement speed, F-120 derived stats, F-121 potion rule).
//!
//! Every function ports one C++ getter of `CMover` (`MoverParam.cpp`, `MoverAttack.cpp`) with its numeric
//! types (§5.1): each `float` operation is one `f32` operation, `int` operands are converted with `as f32`
//! where C++ converts them (`int op float` is a float operation), `(int)x` is `x as i32` (truncation toward
//! zero, saturating), integer arithmetic on DST-derived values wraps like the C `int`. There is no `powf` at
//! runtime: `(int)powf(opt, 1.5f)` is the [`UPGRADE_FLAT`] table (values above +20 use `libm`).
//!
//! The functions take a [`StatInputs`] (what the server reads from a `Mover` and the browser from its
//! replicas) and the [`RulesData`] tables; none of them touches a clock or a global. Random defense takes an
//! injected [`GameRng`] (PL-013: `random_below(0) == 0`).

use crate::RulesConfig;
use crate::consts::{
    MAX_HR, MIN_HR, NULL_CHGPARAM,
    dst::{
        _EARTH, _ELECTRICITY, _FIRE, _WATER, _WIND, AR_FAR, AR_HRANGE, AR_HWAND, AR_LONG, AR_RANGE,
        AR_SHORT, AR_WAND, DST_ABILITY_MAX, DST_ABILITY_MIN, DST_ADJ_HITRATE, DST_ADJDEF,
        DST_ADJDEF_RATE, DST_ATTACKSPEED, DST_ATTACKSPEED_RATE, DST_AXE_DMG, DST_BLOCK_MELEE,
        DST_BLOCK_RANGE, DST_BOW_DMG, DST_BOWMASTER_DMG, DST_CHR_CHANCECRITICAL, DST_CHR_DMG,
        DST_CRITICAL_BONUS, DST_DEX, DST_FP_DEC_RATE, DST_FP_MAX, DST_FP_MAX_RATE, DST_FP_RECOVERY,
        DST_HAWKEYE_RATE, DST_HP_MAX, DST_HP_MAX_RATE, DST_HP_RECOVERY, DST_INT, DST_KNUCKLE_DMG,
        DST_KNUCKLEMASTER_DMG, DST_MONSTER_DMG, DST_MP_DEC_RATE, DST_MP_MAX, DST_MP_MAX_RATE,
        DST_MP_RECOVERY, DST_ONEHANDMASTER_DMG, DST_PARRY, DST_PVP_DMG, DST_RESIST_EARTH,
        DST_RESIST_ELECTRICITY, DST_RESIST_FIRE, DST_RESIST_WATER, DST_RESIST_WIND, DST_SPEED,
        DST_SPELL_RATE, DST_STA, DST_STR, DST_SWD_DMG, DST_TWOHANDMASTER_DMG, DST_YOY_DMG,
        DST_YOYOMASTER_DMG, WT_MAGIC_WAND, WT_MELEE_AXE, WT_MELEE_KNUCKLE, WT_MELEE_STAFF,
        WT_MELEE_STICK, WT_MELEE_SWD, WT_MELEE_YOYO, WT_RANGE_BOW,
    },
};
use crate::data::RulesData;
use crate::dst::DstTable;
use flyff_common::defs::itemkind::{
    IK3_AXE, IK3_BOOTS, IK3_BOW, IK3_CHEERSTICK, IK3_COLLECTER, IK3_GAUNTLET, IK3_HELMET,
    IK3_KNUCKLEHAMMER, IK3_SHIELD, IK3_STAFF, IK3_SUIT, IK3_SWD, IK3_WAND, IK3_YOYO,
    WEAPON_ULTIMATE,
};
use flyff_common::defs::neuz::AII_MONSTER;
use flyff_common::rng::GameRng;
use flyff_data::props::{item::ItemProp, job::JobProp, mover::MoverProp};

// ---- data shared with the game crate (single definition, re-exported by `flyff_game::mover`) --------------

/// `m_nStr`, `m_nSta`, `m_nDex`, `m_nInt`: the base stats (the DST parameters are added by the getters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BaseStats {
    /// `m_nStr`.
    pub str: i32,
    /// `m_nSta`.
    pub sta: i32,
    /// `m_nDex`.
    pub dex: i32,
    /// `m_nInt`.
    pub int: i32,
}

/// `m_nDefenseMin/Max`, `m_nAdjHitRate`, `m_nAdjParry`: the values `update_param()` refreshes after every
/// equipment change (16 §13.4, ref/10 §5.2). NPCs: the defense is `natural_armor`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EquipCache {
    /// `m_nDefenseMin`.
    pub defense_min: i32,
    /// `m_nDefenseMax`.
    pub defense_max: i32,
    /// `m_nAdjHitRate`.
    pub adj_hit_rate: i32,
    /// `m_nAdjParry`.
    pub adj_parry: i32,
}

/// The per-prop back-end multipliers (`MoverProp::m_f*_Rate`, F-014; written by doc 23). `1.0` unless a GM
/// override exists. They are **not** `[rates]` config keys (16 §3.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonsterRates {
    /// `m_fHitPoint_Rate`.
    pub hp: f32,
    /// `m_fAttackPower_Rate`.
    pub attack: f32,
    /// `m_fDefence_Rate`.
    pub defence: f32,
    /// `m_fExp_Rate`.
    pub exp: f32,
    /// `m_fItemDrop_Rate`.
    pub item_drop: f32,
    /// `m_fPenya_Rate`.
    pub penya: f32,
}

impl Default for MonsterRates {
    fn default() -> Self {
        MonsterRates {
            hp: 1.0,
            attack: 1.0,
            defence: 1.0,
            exp: 1.0,
            item_drop: 1.0,
            penya: 1.0,
        }
    }
}

// ---- tables ----------------------------------------------------------------------------------------------

/// `(int)powf(opt, 1.5f)` for `opt` in `0..=20` (16 §5.1). Larger options use `libm::powf` (see
/// [`upgrade_flat`]); the data never reaches them.
pub const UPGRADE_FLAT: [i32; 21] = [
    0, 1, 2, 5, 8, 11, 14, 18, 22, 27, 31, 36, 41, 46, 52, 58, 64, 70, 76, 82, 89,
];

/// `fPlusValue[18]` of `GetAttackSpeedPlusValue` (`MoverAttack.cpp:73-90`).
pub const ATTACK_SPEED_PLUS: [f32; 18] = [
    0.08, 0.16, 0.24, 0.32, 0.40, 0.48, 0.56, 0.64, 0.72, 0.80, 0.88, 0.96, 1.04, 1.12, 1.20, 1.30,
    1.38, 1.50,
];

/// The base speed of a player in world units per 1/60 s frame (`MoverProp.speed` of `MI_MALE` /
/// `MI_FEMALE`, 16 §5.11); used when the player's prop is missing.
pub const PLAYER_BASE_SPEED: f32 = 0.1;

const NO_JOB: JobProp = JobProp {
    attack_speed: 0.0,
    factor_max_hp: 0.0,
    factor_max_mp: 0.0,
    factor_max_fp: 0.0,
    factor_def: 0.0,
    factor_hp_recovery: 0.0,
    factor_mp_recovery: 0.0,
    factor_fp_recovery: 0.0,
    melee_swd: 0.0,
    melee_axe: 0.0,
    melee_staff: 0.0,
    melee_stick: 0.0,
    melee_knuckle: 0.0,
    magic_wand: 0.0,
    melee_yoyo: 0.0,
    melee_bow: 0.0,
    blocking: 0.0,
    critical: 0.0,
};

/// `(int)pow((float)opt, 1.5f)` for a positive option (0 otherwise): the flat bonus of an upgraded item
/// (`GetHitMinMax`, `SumEquipDefenseAbility`, `GetItemATKPower`).
pub fn upgrade_flat(opt: i32) -> i32 {
    if opt <= 0 {
        return 0;
    }
    match UPGRADE_FLAT.get(opt as usize) {
        Some(v) => *v,
        None => libm::powf(opt as f32, 1.5_f32) as i32,
    }
}

/// `GetAttackSpeedPlusValue(n)` (`MoverAttack.cpp:73`): `PLUS[clamp(n / 10, 0, 17)]`.
pub fn attack_speed_plus_value(n: i32) -> f32 {
    let i = (n / 10).clamp(0, ATTACK_SPEED_PLUS.len() as i32 - 1);
    ATTACK_SPEED_PLUS.get(i as usize).copied().unwrap_or(0.0)
}

// ---- inputs ----------------------------------------------------------------------------------------------

/// A weapon (or armour piece) as the formulas see it: the prop plus the item instance's upgrade level and
/// expiry (16 §5.2).
#[derive(Clone, Copy, Debug)]
pub struct WeaponView<'a> {
    /// The item prop.
    pub prop: &'a ItemProp,
    /// `GetAbilityOption()`: the upgrade level.
    pub ability_option: i32,
    /// `prop.refer_stat1 == WEAPON_ULTIMATE` (ultimate weapons count as +10, `GetItemMultiplier`).
    pub refer_ultimate: bool,
    /// `CItemElem::expired`.
    pub expired: bool,
}

impl<'a> WeaponView<'a> {
    /// A view of `prop` with upgrade level `ability_option`; `refer_ultimate` is read from the prop.
    pub fn new(prop: &'a ItemProp, ability_option: i32, expired: bool) -> Self {
        WeaponView {
            prop,
            ability_option,
            refer_ultimate: prop.refer_stat1 == WEAPON_ULTIMATE,
            expired,
        }
    }
}

/// Everything the derived-stat formulas read about one mover (16 §5.2). Built by the server from a `Mover`
/// and by the browser from its replicas.
#[derive(Clone, Copy, Debug)]
pub struct StatInputs<'a> {
    /// `IsPlayer()`.
    pub is_player: bool,
    /// `GetLevel()`.
    pub level: i32,
    /// `GetJob()` (`-1` for NPCs).
    pub job: i32,
    /// `m_nStr` ... `m_nInt`.
    pub base: BaseStats,
    /// The DST aggregate.
    pub dst: &'a DstTable,
    /// The cached equipment values.
    pub equip_cache: EquipCache,
    /// The item of the active hand for the attack in question (`GetWeaponItem(part)`); `None` = bare
    /// hands, where the formulas use the `II_WEA_HAN_HAND` prop of [`RulesData::hand`]. For an NPC: the
    /// attack item (`m_dwVirtItem`, or the skill's `nParam >> 16`) as a view with option 0.
    pub weapon: Option<WeaponView<'a>>,
    /// The item in the *other* hand. Together with [`StatInputs::weapon`] both being present is
    /// `IsDualWeapon()`.
    pub left_weapon: Option<WeaponView<'a>>,
    /// The mover prop: NPC numbers; players: `MI_MALE` / `MI_FEMALE` (resists 0).
    pub mover_prop: &'a MoverProp,
    /// The NPC per-prop back-end rates (`mv.rates`, 1.0).
    pub rates: MonsterRates,
    /// `SM_ATTACK_UP | SM_ATTACK_UP1` (×1.2 ATK, composed by doc 17).
    pub sm_attack_up: bool,
}

impl StatInputs<'_> {
    /// `IsDualWeapon()`: items in both hands.
    pub fn is_dual_weapon(&self) -> bool {
        self.weapon.is_some() && self.left_weapon.is_some()
    }
}

/// What the character window shows (`__STATISTICS`, 16 §5.12).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    /// `GetSpeedRun`: `clamp(param(SPEED, 0), 0, set_max_speed)`.
    pub speed_run: i32,
    /// `GetCriticalRate`: `param(CRITICAL_BONUS, 0)`.
    pub critical_rate: i32,
    /// `GetSpellCastSpeed`: `clamp(param(SPELL_RATE, 0), 0, set_max_cast)`.
    pub spell_cast_speed: i32,
    /// `GetHitRate`: `param(ADJ_HITRATE, 0)`.
    pub hit_rate: i32,
    /// `GetBlockMelee`: `clamp(param(BLOCK_MELEE, 0), 0, max_block_value)`.
    pub block_melee: i32,
    /// `GetBlockRange`.
    pub block_range: i32,
    /// `GetPvMDmg`: `param(MONSTER_DMG, 0)`.
    pub pvm_dmg: i32,
    /// `GetPvPDmg`: `max(0, param(PVP_DMG, 0))`.
    pub pvp_dmg: i32,
}

/// The numbers of the character window plus the ones the server reuses (16 §5.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DerivedStats {
    /// `GetStr()`.
    pub str: i32,
    /// `GetSta()`.
    pub sta: i32,
    /// `GetDex()`.
    pub dex: i32,
    /// `GetInt()`.
    pub int: i32,
    /// `GetMaxHitPoint()`.
    pub max_hp: i32,
    /// `GetMaxManaPoint()`.
    pub max_mp: i32,
    /// `GetMaxFatiguePoint()`.
    pub max_fp: i32,
    /// `GetHPRecovery()`.
    pub hp_recovery: i32,
    /// `GetMPRecovery()`.
    pub mp_recovery: i32,
    /// `GetFPRecovery()`.
    pub fp_recovery: i32,
    /// `GetHitMinMax` minimum, active hand, generic attack.
    pub attack_min: i32,
    /// `GetHitMinMax` maximum.
    pub attack_max: i32,
    /// `CalcDefense(AF_GENERIC, rand = false)` without the Event.lua term.
    pub defense_shown: i32,
    /// `GetHR()`.
    pub hit_rating: i32,
    /// `GetParrying()`.
    pub parry: i32,
    /// `GetCriticalProb()` without the party bonus.
    pub critical_prob: i32,
    /// `GetAttackSpeed()`.
    pub attack_speed: f32,
    /// `GetCastingAniSpeed()`.
    pub casting_ani_speed: f32,
    /// `GetSpeed(base)` for the mover's base speed.
    pub move_speed: f32,
    /// The `__STATISTICS` getters.
    pub statistics: Statistics,
}

// ---- movement speed (F-113) ------------------------------------------------------------------------------

/// `CMover::GetSpeed(fSrcSpeed)` for the sim's per-frame speed (`MoverParam.cpp:3303-3358`, F-113).
///
/// * `src` is the base speed of the mover (`sim.params.base_speed` = `MoverProp.speed`; players `0.1` per
///   1/60 s frame) and `speed_factor` the AI multiplier (`m_fSpeedFactor`, doc 18);
/// * `crr_speed` is the forced speed `m_fCrrSpd`: non-zero wins over everything. The original field is dropped
///   with the ghost replay (31 §5.1, PL-006/PL-007), callers pass `0.0`; the parameter stays for oracle parity;
/// * a `DST_SPEED` override (`chg != NULL_CHGPARAM`) is a root counter: the mover stands still (`0.0`);
/// * a non-zero `DST_SPEED` adjustment is a percentage capped from above at `set_max_speed`
///   (`gameplay.set_max_speed`, 150; **KEEP**: no lower cap) and the result is floored at `0.0`.
///
/// Evaluation order and rounding as the C++ (`fSrcSpeed + fSrcSpeed * (nAdjValue / 100.0f)` in `f32`).
pub fn move_speed(
    src: f32,
    speed_factor: f32,
    crr_speed: f32,
    dst: &DstTable,
    set_max_speed: i32,
) -> f32 {
    let s = src * speed_factor;
    if crr_speed != 0.0 {
        return crr_speed;
    }
    if dst.chg(DST_SPEED) != NULL_CHGPARAM {
        return 0.0;
    }
    let mut a = dst.adj(DST_SPEED);
    if a != 0 {
        if a > set_max_speed {
            a = set_max_speed;
        }
        let r = s + s * (a as f32 / 100.0_f32);
        return if r < 0.0 { 0.0 } else { r };
    }
    if s < 0.0 { 0.0 } else { s }
}

/// The base speed `m_pActMover->m_fSpeed` of a mover: `prop.speed`, halved for `AII_MONSTER`
/// (`CreateObj.cpp:696-705`); players without a prop speed use [`PLAYER_BASE_SPEED`].
pub fn base_speed(inp: &StatInputs<'_>) -> f32 {
    let mut s = inp.mover_prop.speed;
    if inp.is_player && s <= 0.0 {
        s = PLAYER_BASE_SPEED;
    }
    if inp.mover_prop.ai == AII_MONSTER {
        s /= 2.0_f32;
    }
    s
}

// ---- base stat getters -----------------------------------------------------------------------------------

/// `GetStr()` / `GetSta()` / `GetDex()` / `GetInt()` (`MoverParam.cpp:3218-3300`): the base stat plus the DST
/// parameter, at least 1 (`__JEFF_11`). **KEEP**: an override (`chg`) is *added* to the base, because the
/// parameter is read with base 0 (16 §5.1).
pub fn stat(dst: &DstTable, stat_dst: u32, base: i32) -> i32 {
    let n = base.wrapping_add(dst.param(stat_dst, 0));
    if n < 1 { 1 } else { n }
}

/// `GetStr()`.
pub fn get_str(inp: &StatInputs<'_>) -> i32 {
    stat(inp.dst, DST_STR, inp.base.str)
}

/// `GetSta()`.
pub fn get_sta(inp: &StatInputs<'_>) -> i32 {
    stat(inp.dst, DST_STA, inp.base.sta)
}

/// `GetDex()`.
pub fn get_dex(inp: &StatInputs<'_>) -> i32 {
    stat(inp.dst, DST_DEX, inp.base.dex)
}

/// `GetInt()`.
pub fn get_int(inp: &StatInputs<'_>) -> i32 {
    stat(inp.dst, DST_INT, inp.base.int)
}

/// The job factors of `job`: its row, else the vagrant's, else zeros (`GetJobProp` returns NULL for an
/// unknown job and the original crashes; the rules answer with the documented fallback).
fn job_prop(d: &RulesData, job: i32) -> &JobProp {
    d.job(job)
        .or_else(|| d.job(0))
        .map_or(&NO_JOB, |row| &row.prop)
}

// ---- max HP / MP / FP (§5.3) -----------------------------------------------------------------------------

/// `GetMaxOriginHitPoint(bOriginal)` of an NPC: `(int)(dwAddHp * monsterHitpointRate * m_fHitPoint_Rate)`
/// (`__S1108_BACK_END_SYSTEM`; `prj.m_fMonsterHitpointRate` is 1.0, `mv.rates.hp` the per-prop GM multiplier).
pub fn npc_max_hp_origin(add_hp: u32, monster_hp_rate: f32, prop_hp_rate: f32) -> i32 {
    c_int(add_hp as f32 * monster_hp_rate * prop_hp_rate)
}

/// `(int)x` of the original x86 build for a value that may be out of range: truncation toward zero, and the
/// "integer indefinite" `0x80000000` (`i32::MIN`) for NaN and for anything outside the `int` range, where a
/// Rust `as` cast would saturate. Only data with a `dwAddHp` of `NULL_ID` (`=` in `propMover.txt`) reaches it.
pub fn c_int(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0_f32 || x < -2_147_483_648.0_f32 {
        i32::MIN
    } else {
        x as i32
    }
}

/// `GetMaxOriginManaPoint` of an NPC: `L * 2 + INT * 8 + 22` (integers).
pub fn npc_max_mp_origin(level: i32, int: i32) -> i32 {
    level
        .wrapping_mul(2)
        .wrapping_add(int.wrapping_mul(8))
        .wrapping_add(22)
}

/// `GetMaxOriginFatiguePoint` of an NPC: `L * 2 + STR * 7 + STA * 2 + DEX * 4` (integers).
pub fn npc_max_fp_origin(level: i32, str_: i32, sta: i32, dex: i32) -> i32 {
    level
        .wrapping_mul(2)
        .wrapping_add(str_.wrapping_mul(7))
        .wrapping_add(sta.wrapping_mul(2))
        .wrapping_add(dex.wrapping_mul(4))
}

/// `GetMaxOriginHitPoint(original)`: a player's `a = (JP.max_hp * L) / 2.0f`,
/// `b = a * ((L + 1.0f) / 4.0f) * (1.0f + sta / 50.0f) + sta * 10.0f`, `(int)(b + 80.0f)`, with `sta` the base
/// stat when `original` (the rate composites use it) and `GetSta()` otherwise; an NPC's `prop.add_hp` product.
pub fn max_hp_origin(inp: &StatInputs<'_>, d: &RulesData, original: bool) -> i32 {
    if !inp.is_player {
        return npc_max_hp_origin(inp.mover_prop.add_hp, 1.0, inp.rates.hp);
    }
    let sta = if original { inp.base.sta } else { get_sta(inp) };
    let jp = job_prop(d, inp.job);
    let a = (jp.factor_max_hp * inp.level as f32) / 2.0_f32;
    let b = a * ((inp.level as f32 + 1.0_f32) / 4.0_f32) * (1.0_f32 + sta as f32 / 50.0_f32)
        + (sta as f32 * 10.0_f32);
    let max_hp = b + 80.0_f32;
    max_hp as i32
}

/// `GetMaxOriginManaPoint(original)`: a player's
/// `(int)(((((L * 2.0f) + (int * 8.0f)) * f) + 22.0f) + (int * f))`; an NPC's integer formula.
pub fn max_mp_origin(inp: &StatInputs<'_>, d: &RulesData, original: bool) -> i32 {
    let int = if original { inp.base.int } else { get_int(inp) };
    if !inp.is_player {
        return npc_max_mp_origin(inp.level, int);
    }
    let factor = job_prop(d, inp.job).factor_max_mp;
    ((((inp.level as f32 * 2.0_f32) + (int as f32 * 8.0_f32)) * factor + 22.0_f32)
        + (int as f32 * factor)) as i32
}

/// `GetMaxOriginFatiguePoint(original)`: a player's `(int)((((L * 2.0f) + (sta * 6.0f)) * f) + (sta * f))`;
/// an NPC's integer formula over STR, STA and DEX.
pub fn max_fp_origin(inp: &StatInputs<'_>, d: &RulesData, original: bool) -> i32 {
    let (sta, str_, dex) = if original {
        (inp.base.sta, inp.base.str, inp.base.dex)
    } else {
        (get_sta(inp), get_str(inp), get_dex(inp))
    };
    if !inp.is_player {
        return npc_max_fp_origin(inp.level, str_, sta, dex);
    }
    let factor = job_prop(d, inp.job).factor_max_fp;
    (((inp.level as f32 * 2.0_f32) + (sta as f32 * 6.0_f32)) * factor + (sta as f32 * factor))
        as i32
}

/// The final maximum of a pool (`GetMaxHitPoint` / `GetMaxManaPoint` / `GetMaxFatiguePoint`,
/// `MoverParam.cpp:2843-2900`): `n = param(max_dst, origin)`, `n = (int)(n * (1.0f + (float)param(rate_dst, 0) /
/// 100))` (one `f32` product), at least 1.
pub fn final_max(dst: &DstTable, max_dst: u32, rate_dst: u32, origin: i32) -> i32 {
    let n = dst.param(max_dst, origin);
    let factor = 1.0_f32 + dst.param(rate_dst, 0) as f32 / 100.0_f32;
    let n = (n as f32 * factor) as i32;
    if n < 1 { 1 } else { n }
}

/// `GetMaxHitPoint()`.
pub fn max_hp(inp: &StatInputs<'_>, d: &RulesData) -> i32 {
    final_max(
        inp.dst,
        DST_HP_MAX,
        DST_HP_MAX_RATE,
        max_hp_origin(inp, d, false),
    )
}

/// `GetMaxManaPoint()`.
pub fn max_mp(inp: &StatInputs<'_>, d: &RulesData) -> i32 {
    final_max(
        inp.dst,
        DST_MP_MAX,
        DST_MP_MAX_RATE,
        max_mp_origin(inp, d, false),
    )
}

/// `GetMaxFatiguePoint()`.
pub fn max_fp(inp: &StatInputs<'_>, d: &RulesData) -> i32 {
    final_max(
        inp.dst,
        DST_FP_MAX,
        DST_FP_MAX_RATE,
        max_fp_origin(inp, d, false),
    )
}

// ---- regeneration amounts (§5.4) -------------------------------------------------------------------------

/// `GetHPRecovery()`: `v = (int)(L / 3.0f + maxHP / (500.0f * L) + STA * f)`, `v = (int)(v - v * 0.1f)`,
/// then `param(DST_HP_RECOVERY, v)`; `f` is the job factor, 1.0 for NPCs.
pub fn hp_recovery(inp: &StatInputs<'_>, d: &RulesData) -> i32 {
    let f = if inp.is_player {
        job_prop(d, inp.job).factor_hp_recovery
    } else {
        1.0_f32
    };
    let l = inp.level;
    let v = ((l as f32 / 3.0_f32)
        + (max_hp(inp, d) as f32 / (500.0_f32 * l as f32))
        + (get_sta(inp) as f32 * f)) as i32;
    let v = (v as f32 - (v as f32 * 0.1_f32)) as i32;
    inp.dst.param(DST_HP_RECOVERY, v)
}

/// `GetMPRecovery()`: `v = (int)(((L * 1.5f) + maxMP / (500.0f * L) + INT * f) * 0.2f)`, then the 10 % cut and
/// `param(DST_MP_RECOVERY, v)`.
pub fn mp_recovery(inp: &StatInputs<'_>, d: &RulesData) -> i32 {
    let f = if inp.is_player {
        job_prop(d, inp.job).factor_mp_recovery
    } else {
        1.0_f32
    };
    let l = inp.level;
    let v = (((l as f32 * 1.5_f32)
        + (max_mp(inp, d) as f32 / (500.0_f32 * l as f32))
        + (get_int(inp) as f32 * f))
        * 0.2_f32) as i32;
    let v = (v as f32 - (v as f32 * 0.1_f32)) as i32;
    inp.dst.param(DST_MP_RECOVERY, v)
}

/// `GetFPRecovery()`: `v = (int)(((L * 2.0f) + maxFP / (500.0f * L) + STA * f) * 0.2f)`, then the 10 % cut and
/// `param(DST_FP_RECOVERY, v)`.
pub fn fp_recovery(inp: &StatInputs<'_>, d: &RulesData) -> i32 {
    let f = if inp.is_player {
        job_prop(d, inp.job).factor_fp_recovery
    } else {
        1.0_f32
    };
    let l = inp.level;
    let v = (((l as f32 * 2.0_f32)
        + (max_fp(inp, d) as f32 / (500.0_f32 * l as f32))
        + (get_sta(inp) as f32 * f))
        * 0.2_f32) as i32;
    let v = (v as f32 - (v as f32 * 0.1_f32)) as i32;
    inp.dst.param(DST_FP_RECOVERY, v)
}

// ---- hit rating, parry, hit chance (§5.5) ----------------------------------------------------------------

/// `GetHR()`: a player's `GetDex()`, an NPC's `prop.dwHR`.
pub fn hit_rating(inp: &StatInputs<'_>) -> i32 {
    if inp.is_player {
        get_dex(inp)
    } else {
        inp.mover_prop.hr as i32
    }
}

/// `GetParrying()`: a player's `(int)(DEX * 0.5f + param(DST_PARRY, adj_parry))`, an NPC's `prop.dwER`.
pub fn parry(inp: &StatInputs<'_>) -> i32 {
    if inp.is_player {
        ((get_dex(inp) as f32 * 0.5_f32)
            + inp.dst.param(DST_PARRY, inp.equip_cache.adj_parry) as f32) as i32
    } else {
        inp.mover_prop.er as i32
    }
}

/// `GetAdjHitRate()`: `param(DST_ADJ_HITRATE, adj_hit_rate)`.
pub fn adj_hit_rate(inp: &StatInputs<'_>) -> i32 {
    inp.dst.param(DST_ADJ_HITRATE, inp.equip_cache.adj_hit_rate)
}

/// The hit chance in percent of `GetAttackResult` (`MoverAttack.cpp:331-379`) before the roll
/// `rng.below(100) < hr`: the NPC→player, player→NPC and "otherwise" (PvP, NPC→NPC) formulas, plus
/// `GetAdjHitRate()`, clamped to `[MIN_HR 20, 96]`. The `ONEKILL` mode and event-spawn bypasses are doc 17's.
pub fn hit_chance_percent(att: &StatInputs<'_>, def: &StatInputs<'_>) -> i32 {
    let hr = hit_rating(att);
    let denom = hr.wrapping_add(parry(def)) as f32;
    let (l, dl) = (att.level, def.level);
    // `(int)` of the x86 build: a zero denominator gives inf / NaN, which converts to `0x80000000`, not to a
    // saturated value (`c_int`); the clamp then answers `MIN_HR`
    let mut hit = if !att.is_player && def.is_player {
        c_int(
            ((hr as f32 * 1.5_f32) / denom)
                * 2.0_f32
                * ((l as f32 * 0.5_f32) / (l as f32 + dl as f32 * 0.3_f32))
                * 100.0_f32,
        )
    } else if att.is_player && !def.is_player {
        c_int(
            ((hr as f32 * 1.6_f32) / denom)
                * 1.5_f32
                * ((l as f32 * 1.2_f32) / (l.wrapping_add(dl) as f32))
                * 100.0_f32,
        )
    } else {
        c_int(
            ((hr as f32 * 1.6_f32) / denom)
                * 1.2_f32
                * ((l as f32 * 1.2_f32) / (l.wrapping_add(dl) as f32))
                * 100.0_f32,
        )
    };
    hit = hit.wrapping_add(adj_hit_rate(att));
    hit.clamp(MIN_HR, MAX_HR)
}

// ---- defense (§5.6) --------------------------------------------------------------------------------------

/// `GetDefenseByItem(bRandom)`: with a generator `min + rng.below(max - min)` (`xRandom`, 0 when
/// `max <= min`), without one `min + (max - min) / 2`.
pub fn defense_by_item(cache: &EquipCache, rng: Option<&mut dyn GameRng>) -> i32 {
    let n = cache.defense_max.wrapping_sub(cache.defense_min);
    match rng {
        Some(r) => {
            let extra = if n > 0 {
                r.random_below(n as u32) as i32
            } else {
                0
            };
            cache.defense_min.wrapping_add(extra)
        }
        None => cache.defense_min.wrapping_add(n / 2),
    }
}

/// `SumEquipDefenseAbility` (`MoverParam.cpp:1944-2001`) over the equipped armour: for each piece with
/// `kind2 ∈ {IK2_ARMOR, IK2_ARMORETC}` (the caller filters) `min += (int)(ability_min * f) + flat`,
/// `max` likewise, then `param(ABILITY_MIN/MAX, ·)`. Returns `(defense_min, defense_max)` for
/// [`EquipCache`]; used by `update_param` (WP-MOVER-11).
pub fn equip_defense_sum<'a>(
    d: &RulesData,
    dst: &DstTable,
    armour: impl IntoIterator<Item = WeaponView<'a>>,
) -> (i32, i32) {
    let (mut min, mut max) = (0_i32, 0_i32);
    for w in armour {
        let flat = upgrade_flat(w.ability_option);
        let f = item_multiplier(d, w.prop, w.ability_option, w.expired, w.refer_ultimate);
        min = min
            .wrapping_add((w.prop.ability_min as f32 * f) as i32)
            .wrapping_add(flat);
        max = max
            .wrapping_add((w.prop.ability_max as f32 * f) as i32)
            .wrapping_add(flat);
    }
    (
        dst.param(DST_ABILITY_MIN, min),
        dst.param(DST_ABILITY_MAX, max),
    )
}

/// The generic defense of `CalcDefenseCore` (`bGeneric`, also forced when both are players):
/// `(int)((((L * 2 + STA / 2) / 2.8f) - 4) + ((STA - 14) * f))`, `+= defense_by_item / 4`, `+= param(ADJDEF)`,
/// at least 0; `f` is the job's `fFactorDef`, 1.0 for NPCs. `rng` selects the random item defense.
pub fn defense_generic(inp: &StatInputs<'_>, d: &RulesData, rng: Option<&mut dyn GameRng>) -> i32 {
    let sta = get_sta(inp);
    let f = if inp.is_player {
        job_prop(d, inp.job).factor_def
    } else {
        1.0_f32
    };
    let core = inp.level.wrapping_mul(2).wrapping_add(sta / 2);
    let base = (((core as f32 / 2.8_f32) - 4.0_f32) + (sta.wrapping_sub(14) as f32 * f)) as i32;
    let mut def = base.wrapping_add(defense_by_item(&inp.equip_cache, rng) / 4);
    def = def.wrapping_add(inp.dst.param(DST_ADJDEF, 0));
    def.max(0)
}

/// `CalcDefenseNPC`: `AF_MAGIC` → `(int)(prop.resist_magic / 7.0f + 1)`, else
/// `(int)(prop.natural_armor / 7.0f + 1)`.
pub fn defense_npc(prop: &MoverProp, magic: bool) -> i32 {
    let v = if magic {
        prop.resist_magic as f32
    } else {
        prop.natural_armor as f32
    };
    (v / 7.0_f32 + 1.0_f32) as i32
}

/// `CalcDefensePlayer` against an NPC attacker (the only reachable branch; PvP is always generic):
/// `(int)((item_defense / 4 + ADJDEF) + ((L + STA / 2 + DEX) / 2.8f) - 4 + L * 2)`, at least 0.
pub fn defense_player_vs_npc(inp: &StatInputs<'_>, rng: Option<&mut dyn GameRng>) -> i32 {
    let a = (defense_by_item(&inp.equip_cache, rng) / 4).wrapping_add(inp.dst.param(DST_ADJDEF, 0));
    let sum = inp
        .level
        .wrapping_add(get_sta(inp) / 2)
        .wrapping_add(get_dex(inp));
    let v = ((a as f32 + (sum as f32 / 2.8_f32)) - 4.0_f32) + (inp.level.wrapping_mul(2) as f32);
    (v as i32).max(0)
}

/// `GetDEFMultiplier`: `1.0f`, times the NPC's `m_fDefence_Rate`, times `0.5f` for
/// `SI_BLD_DOUBLE_ARMORPENETRATE` (`armor_penetrate`), times `1.0f + param(ADJDEF_RATE, 0) / 100.0f`.
pub fn def_multiplier(inp: &StatInputs<'_>, armor_penetrate: bool) -> f32 {
    let mut f = 1.0_f32;
    if !inp.is_player {
        f *= inp.rates.defence;
    }
    if armor_penetrate {
        f *= 0.5_f32;
    }
    f *= 1.0_f32 + inp.dst.param(DST_ADJDEF_RATE, 0) as f32 / 100.0_f32;
    f
}

/// The defense the UI shows: `(int)(defense_generic(rand = false) * GetDEFMultiplier)`, without the
/// Event.lua term of the server (KQ-11).
pub fn defense_shown(inp: &StatInputs<'_>, d: &RulesData) -> i32 {
    (defense_generic(inp, d, None) as f32 * def_multiplier(inp, false)) as i32
}

// ---- items: upgrade multiplier, ATK ranges (§5.7) --------------------------------------------------------

/// `GetExpUpItem(kind3, option)` (`Project.cpp:5311-5350`): the percent bonus of an upgrade level from the
/// `expUpitem` table; 0 for option 0, for an unknown item kind or an option outside the table.
pub fn exp_up_item(d: &RulesData, kind3: u32, option: i32) -> i32 {
    if option <= 0 {
        return 0;
    }
    let column = match kind3 {
        IK3_SWD | IK3_CHEERSTICK | IK3_WAND | IK3_AXE | IK3_KNUCKLEHAMMER | IK3_STAFF
        | IK3_YOYO | IK3_BOW | IK3_COLLECTER => 0,
        IK3_HELMET => 1,
        IK3_SUIT => 2,
        IK3_GAUNTLET => 3,
        IK3_BOOTS => 4,
        IK3_SHIELD => 5,
        _ => return 0,
    };
    d.upitem
        .get(column)
        .and_then(|row| row.get(option as usize))
        .copied()
        .unwrap_or(0)
}

/// `GetItemMultiplier(item)` (`MoverAttack.cpp:2260-2290`): an expired item 0.0; the upgrade level
/// (ultimate weapons count as +10); level 0 gives 1.0, otherwise `(100 + expUpitem) / 100.0f`.
/// **FIX PL-072**: levels above +10 read row 10 (the original indexed past the table).
pub fn item_multiplier(
    d: &RulesData,
    prop: &ItemProp,
    ability_option: i32,
    expired: bool,
    ultimate: bool,
) -> f32 {
    if expired {
        return 0.0;
    }
    let mut opt = ability_option;
    if ultimate || prop.refer_stat1 == WEAPON_ULTIMATE {
        opt = 10;
    }
    let mut f = 1.0_f32;
    if opt != 0 {
        let v = exp_up_item(d, prop.item_kind3, opt.min(10)).wrapping_add(100);
        f = f * v as f32 / 100.0_f32;
    }
    f
}

/// The ability line of an item tooltip: `(int)(ability_min * f) + flat(opt)` and the same for max, with no DST
/// (`GetItemATKPower` without the parameters; 45 §10.2 row 7a).
pub fn item_min_max(
    d: &RulesData,
    prop: &ItemProp,
    ability_option: i32,
    expired: bool,
    ultimate: bool,
) -> (i32, i32) {
    let f = item_multiplier(d, prop, ability_option, expired, ultimate);
    let add = upgrade_flat(ability_option);
    (
        ((prop.ability_min as f32 * f) as i32).wrapping_add(add),
        ((prop.ability_max as f32 * f) as i32).wrapping_add(add),
    )
}

/// `GetItemATKPower(&min, &max, itemProp, weapon)` (`MoverAttack.cpp:2293-2315`; skills and wands):
/// `(int)(param(ABILITY_MIN, prop.ability_min) * f) + flat(opt)` and the same for max; `f` is the weapon's
/// multiplier, 1.0 without one.
pub fn item_atk_power(
    inp: &StatInputs<'_>,
    d: &RulesData,
    prop: &ItemProp,
    weapon: Option<WeaponView<'_>>,
) -> (i32, i32) {
    let mut f = 1.0_f32;
    let mut add = 0;
    if let Some(w) = weapon {
        add = upgrade_flat(w.ability_option);
        f = item_multiplier(d, w.prop, w.ability_option, w.expired, w.refer_ultimate);
    }
    let min = inp.dst.param(DST_ABILITY_MIN, prop.ability_min as i32);
    let max = inp.dst.param(DST_ABILITY_MAX, prop.ability_max as i32);
    (
        ((min as f32 * f) as i32).wrapping_add(add),
        ((max as f32 * f) as i32).wrapping_add(add),
    )
}

/// `GetJobPropFactor` of the weapon formulas of [`weapon_atk`]: `None` when the switch of the C++ assigns
/// nothing (an unknown weapon type, or a bow without `features.bowdmg_propjob`); NPCs use 1.0.
fn weapon_factor(
    inp: &StatInputs<'_>,
    d: &RulesData,
    wtype: u32,
    cfg: &RulesConfig,
) -> Option<f32> {
    let pick = |f: fn(&JobProp) -> f32| {
        if inp.is_player {
            f(job_prop(d, inp.job))
        } else {
            1.0_f32
        }
    };
    match wtype {
        WT_MELEE_SWD => Some(pick(|j| j.melee_swd)),
        WT_MELEE_AXE => Some(pick(|j| j.melee_axe)),
        WT_MELEE_STAFF => Some(pick(|j| j.melee_staff)),
        WT_MELEE_STICK => Some(pick(|j| j.melee_stick)),
        WT_MELEE_KNUCKLE => Some(pick(|j| j.melee_knuckle)),
        WT_MAGIC_WAND if cfg.wand_autoattacks => Some(pick(|j| j.magic_wand)),
        // without `__WAND_AUTOATTACKS` the switch falls through to the yoyo formula (KEEP)
        WT_MAGIC_WAND | WT_MELEE_YOYO => Some(pick(|j| j.melee_yoyo)),
        WT_RANGE_BOW if cfg.bowdmg_propjob => Some(pick(|j| j.melee_bow)),
        _ => None,
    }
}

/// `GetPlusWeaponATK(wtype)` (`MoverAttack.cpp:395-440`): the weapon-type DST bonus plus, with the legend
/// mastery lines, `ONEHANDMASTER` for dual wield or the type's `*MASTER` bonus.
pub fn plus_weapon_atk(inp: &StatInputs<'_>, wtype: u32) -> i32 {
    let p = |dst: u32| inp.dst.param(dst, 0);
    let mut atk = match wtype {
        WT_MELEE_SWD => p(DST_SWD_DMG),
        WT_MELEE_AXE => p(DST_AXE_DMG),
        WT_MELEE_KNUCKLE => p(DST_KNUCKLE_DMG),
        WT_MELEE_YOYO => p(DST_YOY_DMG),
        WT_RANGE_BOW => p(DST_BOW_DMG),
        _ => 0,
    };
    if inp.is_dual_weapon() {
        atk = atk.wrapping_add(p(DST_ONEHANDMASTER_DMG));
    } else {
        atk = atk.wrapping_add(match wtype {
            WT_MELEE_KNUCKLE => p(DST_KNUCKLEMASTER_DMG),
            WT_MELEE_YOYO => p(DST_YOYOMASTER_DMG),
            WT_RANGE_BOW => p(DST_BOWMASTER_DMG),
            WT_MELEE_SWD | WT_MELEE_AXE => p(DST_TWOHANDMASTER_DMG),
            _ => 0,
        });
    }
    atk
}

/// `GetWeaponATK(wtype)` (`MoverAttack.cpp:445-510`): `(int)((stat - 12) * factor + L * 1.2f)` with STR for
/// sword, axe, knuckle and yoyo, INT for staff, stick and (with `features.wand_autoattacks`) wand, DEX for the
/// bow (`features.bowdmg_propjob`, else 0); a wand without the switch uses the yoyo formula (KEEP). Then
/// `+ GetPlusWeaponATK(wtype)`.
pub fn weapon_atk(inp: &StatInputs<'_>, d: &RulesData, cfg: &RulesConfig, wtype: u32) -> i32 {
    let base = match weapon_factor(inp, d, wtype, cfg) {
        None => 0,
        Some(factor) => {
            let stat_value = match wtype {
                WT_MELEE_STAFF | WT_MELEE_STICK => get_int(inp),
                WT_MAGIC_WAND if cfg.wand_autoattacks => get_int(inp),
                WT_RANGE_BOW => get_dex(inp),
                _ => get_str(inp),
            };
            ((stat_value.wrapping_sub(12) as f32 * factor) + (inp.level as f32 * 1.2_f32)) as i32
        }
    };
    base.wrapping_add(plus_weapon_atk(inp, wtype))
}

/// The prop of the active hand: the weapon, else (players) the bare-hand item `II_WEA_HAN_HAND`.
fn active_prop<'x>(inp: &'x StatInputs<'_>, d: &'x RulesData) -> Option<&'x ItemProp> {
    match &inp.weapon {
        Some(w) => Some(w.prop),
        None if inp.is_player => d.hand.as_ref(),
        None => None,
    }
}

/// `GetHitMinMax(&min, &max)` (`MoverAttack.cpp:514-592`) for the hand of `inp.weapon`.
///
/// Player: `min = 2 * ability_min` (`2 * ability_max`), `param(ABILITY_MIN/MAX)`, `+ weapon_atk(type) +
/// param(CHR_DMG)`; with a weapon item `(int)(x * item_multiplier)` and `+ flat(opt)` for `opt > 0`.
/// NPC: `prop.atk_min/max` → `param(ABILITY_*)` → `param(CHR_DMG)` (an override **replaces**, KEEP) `+` the
/// attack item's ability range (`inp.weapon`, which the caller picked as `info.param >> 16` or `prop.atk1`).
pub fn attack_min_max(inp: &StatInputs<'_>, d: &RulesData, cfg: &RulesConfig) -> (i32, i32) {
    if inp.is_player {
        let Some(item) = active_prop(inp, d) else {
            return (0, 0);
        };
        let mut min = item.ability_min.wrapping_mul(2) as i32;
        let mut max = item.ability_max.wrapping_mul(2) as i32;
        min = inp.dst.param(DST_ABILITY_MIN, min);
        max = inp.dst.param(DST_ABILITY_MAX, max);
        let plus =
            weapon_atk(inp, d, cfg, item.weapon_type).wrapping_add(inp.dst.param(DST_CHR_DMG, 0));
        min = min.wrapping_add(plus);
        max = max.wrapping_add(plus);
        if let Some(w) = &inp.weapon {
            let f = item_multiplier(d, w.prop, w.ability_option, w.expired, w.refer_ultimate);
            min = (min as f32 * f) as i32;
            max = (max as f32 * f) as i32;
            if w.ability_option > 0 {
                let v = upgrade_flat(w.ability_option);
                min = min.wrapping_add(v);
                max = max.wrapping_add(v);
            }
        }
        (min, max)
    } else {
        let p = inp.mover_prop;
        let mut min = p.atk_min as i32;
        let mut max = p.atk_max as i32;
        min = inp.dst.param(DST_ABILITY_MIN, min);
        max = inp.dst.param(DST_ABILITY_MAX, max);
        min = inp.dst.param(DST_CHR_DMG, min);
        max = inp.dst.param(DST_CHR_DMG, max);
        if let Some(w) = &inp.weapon {
            min = min.wrapping_add(w.prop.ability_min as i32);
            max = max.wrapping_add(w.prop.ability_max as i32);
        }
        (min, max)
    }
}

// ---- attack speed, casting (§5.8) ------------------------------------------------------------------------

/// `GetAttackSpeed()` (`MoverAttack.cpp:161-205`): a player's
/// `A = (int)(JP.attack_speed + W.attack_speed * (4.0f * DEX + L / 8.0f) - 3.0f)` (187 from 188 up),
/// `s = (50.0f / (200.f - A)) / 2.0f + PLUS[clamp(A / 10, 0, 17)]`; an NPC's 1.0; then
/// `+= param(ATTACKSPEED) / 1000.0f`, `+= s * rate / 100` for a positive `ATTACKSPEED_RATE`, clamped to
/// `[0.1, 2.0]`. `W` is the active hand's prop (bare hands: `II_WEA_HAN_HAND`; no prop at all: factor 1.0).
pub fn attack_speed(inp: &StatInputs<'_>, d: &RulesData) -> f32 {
    let mut speed = 1.0_f32;
    if inp.is_player {
        let jp = job_prop(d, inp.job);
        let item = active_prop(inp, d).map_or(1.0_f32, |p| p.attack_speed);
        let mut a = (jp.attack_speed
            + (item * (4.0_f32 * get_dex(inp) as f32 + inp.level as f32 / 8.0_f32))
            - 3.0_f32) as i32;
        if 187.5_f32 <= a as f32 {
            a = 187;
        }
        speed = ((50.0_f32 / (200.0_f32 - a as f32)) / 2.0_f32) + attack_speed_plus_value(a);
    }
    speed += inp.dst.param(DST_ATTACKSPEED, 0) as f32 / 1000.0_f32;
    let rate = inp.dst.param(DST_ATTACKSPEED_RATE, 0);
    if 0 < rate {
        speed += speed * rate as f32 / 100.0_f32;
    }
    speed.clamp(0.1_f32, 2.0_f32)
}

/// `GetCastingAniSpeed()`: `clamp(1.0f + param(SPELL_RATE, 0) / 100.0f, 0.1, 2.0)`.
pub fn casting_ani_speed(dst: &DstTable) -> f32 {
    let s = 1.0_f32 + dst.param(DST_SPELL_RATE, 0) as f32 / 100.0_f32;
    s.clamp(0.1_f32, 2.0_f32)
}

/// `GetCastingTime(t)`: `max(0, t - t * param(SPELL_RATE, 0) / 100)` in integers.
pub fn casting_time(dst: &DstTable, casting_time: i32) -> i32 {
    let t =
        casting_time.wrapping_sub(casting_time.wrapping_mul(dst.param(DST_SPELL_RATE, 0)) / 100);
    t.max(0)
}

// ---- critical, block (§5.9) ------------------------------------------------------------------------------

/// `GetCriticalProb()` before the one-shot party bonus: `(int)((DEX / 10) * JP.critical)` (1.0 for NPCs; 0 for
/// every job in this data), `param(CHR_CHANCECRITICAL, ·)`, clamped to `[0, gameplay.set_max_crit]`.
pub fn critical_prob_base(inp: &StatInputs<'_>, d: &RulesData, cfg: &RulesConfig) -> i32 {
    let factor = if inp.is_player {
        job_prop(d, inp.job).critical
    } else {
        1.0_f32
    };
    let p = ((get_dex(inp) / 10) as f32 * factor) as i32;
    inp.dst
        .param(DST_CHR_CHANCECRITICAL, p)
        .clamp(0, cfg.set_max_crit.max(0))
}

/// The block percentage of `GetBlockFactor` (`features.improved_block_calculation`): a player's
/// `param(BLOCK_MELEE) + param(BLOCK_RANGE)`; PvP in the guild-war world (`gw_pvp`) is capped at
/// `max_block_value_gw` with **no** lower clamp, otherwise clamped to `[0, max_block_value]`. NPCs never block (0).
/// The roll and the outcome table are doc 17's.
pub fn block_value(inp: &StatInputs<'_>, cfg: &RulesConfig, gw_pvp: bool) -> i32 {
    if !inp.is_player {
        return 0;
    }
    let br = inp
        .dst
        .param(DST_BLOCK_MELEE, 0)
        .wrapping_add(inp.dst.param(DST_BLOCK_RANGE, 0));
    if gw_pvp {
        br.min(cfg.max_block_value_gw)
    } else {
        br.clamp(0, cfg.max_block_value.max(0))
    }
}

// ---- resists and misc (§5.10) ----------------------------------------------------------------------------

/// `GetResist(element)`: `param(dst_of(element), prop.resist_X) / 100.0f`; `NO_PROP` (and anything that is
/// not one of the five elements `_FIRE` .. `_EARTH`) gives 0.0. Players read their own prop (`MI_MALE`: 0).
pub fn resist(inp: &StatInputs<'_>, element: u32) -> f32 {
    let p = inp.mover_prop;
    let (dst, base) = match element {
        _FIRE => (DST_RESIST_FIRE, p.resist_fire),
        _WATER => (DST_RESIST_WATER, p.resist_water),
        _ELECTRICITY => (DST_RESIST_ELECTRICITY, p.resist_electricity),
        _WIND => (DST_RESIST_WIND, p.resist_wind),
        _EARTH => (DST_RESIST_EARTH, p.resist_earth),
        _ => return 0.0,
    };
    inp.dst.param(dst, base) as f32 / 100.0_f32
}

/// `GetReqMp(n)`: `n == 9_999_999` is special (unchanged), else `n - n * param(MP_DEC_RATE, 0) / 100`.
pub fn req_mp(dst: &DstTable, n: i32) -> i32 {
    if n == 9_999_999 {
        return n;
    }
    n.wrapping_sub(n.wrapping_mul(dst.param(DST_MP_DEC_RATE, 0)) / 100)
}

/// `GetReqFp(n)`: like [`req_mp`] with `DST_FP_DEC_RATE`.
pub fn req_fp(dst: &DstTable, n: i32) -> i32 {
    if n == 9_999_999 {
        return n;
    }
    n.wrapping_sub(n.wrapping_mul(dst.param(DST_FP_DEC_RATE, 0)) / 100)
}

/// `GetAttackRange(AR_*)` (`MoverMsg.cpp:140-166`): the range in metres (2, 3, 4, 10, 15, 6, 18; unknown 0),
/// times `(HAWKEYE_RATE + 100) / 100` for a positive `DST_HAWKEYE_RATE`.
pub fn attack_range(dst: &DstTable, ar: u32) -> f32 {
    let mut range = match ar {
        AR_SHORT => 2.0_f32,
        AR_LONG => 3.0_f32,
        AR_FAR => 4.0_f32,
        AR_RANGE => 10.0_f32,
        AR_WAND => 15.0_f32,
        AR_HRANGE => 6.0_f32,
        AR_HWAND => 18.0_f32,
        _ => 0.0_f32,
    };
    let rate = dst.param(DST_HAWKEYE_RATE, 0);
    if rate > 0 {
        range = range * rate.wrapping_add(100) as f32 / 100.0_f32;
    }
    range
}

// ---- __STATISTICS (§5.12) --------------------------------------------------------------------------------

/// The `__STATISTICS` getters (`MoverAttack.cpp:207-287`).
pub fn statistics(inp: &StatInputs<'_>, cfg: &RulesConfig) -> Statistics {
    let p = |dst: u32| inp.dst.param(dst, 0);
    Statistics {
        speed_run: p(DST_SPEED).clamp(0, cfg.set_max_speed.max(0)),
        critical_rate: p(DST_CRITICAL_BONUS),
        spell_cast_speed: p(DST_SPELL_RATE).clamp(0, cfg.set_max_cast.max(0)),
        hit_rate: p(DST_ADJ_HITRATE),
        block_melee: p(DST_BLOCK_MELEE).clamp(0, cfg.max_block_value.max(0)),
        block_range: p(DST_BLOCK_RANGE).clamp(0, cfg.max_block_value.max(0)),
        pvm_dmg: p(DST_MONSTER_DMG),
        pvp_dmg: p(DST_PVP_DMG).max(0),
    }
}

// ---- the aggregate ---------------------------------------------------------------------------------------

/// Every number of the character window in one call (16 §5.2); the browser's `derivedStats` and the server's
/// status snapshot use it.
pub fn derived_stats(inp: &StatInputs<'_>, d: &RulesData, cfg: &RulesConfig) -> DerivedStats {
    let (attack_min, attack_max) = attack_min_max(inp, d, cfg);
    DerivedStats {
        str: get_str(inp),
        sta: get_sta(inp),
        dex: get_dex(inp),
        int: get_int(inp),
        max_hp: max_hp(inp, d),
        max_mp: max_mp(inp, d),
        max_fp: max_fp(inp, d),
        hp_recovery: hp_recovery(inp, d),
        mp_recovery: mp_recovery(inp, d),
        fp_recovery: fp_recovery(inp, d),
        attack_min,
        attack_max,
        defense_shown: defense_shown(inp, d),
        hit_rating: hit_rating(inp),
        parry: parry(inp),
        critical_prob: critical_prob_base(inp, d, cfg),
        attack_speed: attack_speed(inp, d),
        casting_ani_speed: casting_ani_speed(inp.dst),
        move_speed: move_speed(base_speed(inp), 1.0, 0.0, inp.dst, cfg.set_max_speed),
        statistics: statistics(inp, cfg),
    }
}
