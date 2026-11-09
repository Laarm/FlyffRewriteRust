//! The ATK and damage multipliers, the level-gap penalty and the Link Attack bonus (Plan/17 §5.8, §5.13;
//! `MoverAttack.cpp:757-832, 985-1129, 1301-1336`; WP-COMBAT-03).

use super::flags::{Hand, rank_skips_level_gap};
use super::rng::x_random;
use super::{AttackInfo, BUFF_SKILL, CombatConfig, CombatCx, CombatantView};
use crate::consts::dst::{CHS_DOUBLE, DST_ATKPOWER_RATE, DST_MONSTER_DMG, DST_PVP_DMG};
use flyff_common::defs::skill::{
    SI_ACR_BOW_AIMEDSHOT, SI_ACR_BOW_JUNKBOW, SI_ACR_BOW_SILENTSHOT, SI_ACR_SUP_DARKILLUSION,
    SI_JST_YOYO_BACKSTAB, SI_JST_YOYO_HITOFPENYA, SI_JST_YOYO_VATALSTAB, SI_PSY_PSY_SPRITBOMB,
};
use flyff_common::rng::GameRng;

/// `GetATKMultiplier(defender, flags)` (this = the attacker), `f32`: `(1 + ATKPOWER_RATE / 100)`, a player
/// times 1.2 under `SM_ATTACK_UP` / `SM_ATTACK_UP1` and plus `PVP_DMG` (player victim) or `MONSTER_DMG` percent
/// when positive; an NPC times `monster_hit_rate * m_fAttackPower_Rate`.
pub fn atk_multiplier(att: &dyn CombatantView, def: &dyn CombatantView, cx: &CombatCx<'_>) -> f32 {
    let rate = 1.0_f32 + att.param(DST_ATKPOWER_RATE, 0) as f32 / 100.0_f32;
    let mut m = 1.0_f32;
    m *= rate;
    let mut dst_dmg = 0;
    if att.is_player() {
        if att.sm_attack_up() {
            m *= 1.2_f32;
        }
        dst_dmg = if def.is_player() {
            att.param(DST_PVP_DMG, 0)
        } else {
            att.param(DST_MONSTER_DMG, 0)
        };
    } else {
        let (attack_rate, _) = att.npc_rates();
        m *= cx.monster_hit_rate * attack_rate;
    }
    if 0 < dst_dmg {
        m += m * dst_dmg as f32 / 100.0_f32;
    }
    m
}

/// The level-gap penalty `cos(pi * d / 32)` for `d` levels of difference, `d` capped at 15
/// (`MAX_OVER_ATK - 1`): computed in `f64` with `pi = 3.1415926535` as the original does, rounded to `f32` last
/// (KQ-05). `d <= 0` gives 1.0.
pub fn level_gap_factor(d: i32) -> f32 {
    if d <= 0 {
        return 1.0;
    }
    let d = d.min(15);
    // the original's own truncated constant (`const double pi = 3.1415926535`), not `PI`
    #[allow(clippy::approx_constant)]
    const PI: f64 = 3.141_592_653_5;
    let radian = (PI * f64::from(d)) / f64::from(32.0_f32);
    libm::cos(radian) as f32
}

/// `GetDamageMultiplier(info)` (this = the attacker), `f32`, applied to the damage after defense (Plan/17 §5.8).
///
/// Skill hits: the burst split `1 / skill_count`; Dark Illusion x1.4 on Back Stab, Vatal Stab and Silent Shot;
/// Vatal Stab `x_random(100) < prob` x2; Aimed Shot `x_random(100) < prob` sets 4.0; Junk Bow `x_random(100) >
/// prob` sets 0 (the probability is `probability_pvp` between players; the comparisons are unsigned, so a `=`
/// probability of `NULL_ID` means always); Hit of Penya `dest_data1[0] / 100` unless `features.hop_scaling_fix`;
/// Spirit Bomb `1.5 * clamp(MP%, 10, 100) / 100` unless `features.improved_spiritbomb`. NPC attackers: the
/// berserk multiplier below the berserk HP percent. Player attackers: x0.60 against players and x0.75 for the
/// off hand; x2 against `CHS_DOUBLE` (Holy Cross). A victim above the attacker's level, when either side is an
/// NPC (guards and `SUPER` attackers excepted): [`level_gap_factor`].
///
/// Draws: the Vatal Stab / Aimed Shot / Junk Bow roll, nothing else.
pub fn damage_multiplier(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    info: &AttackInfo<'_>,
    cx: &CombatCx<'_>,
    rng: &mut dyn GameRng,
) -> f32 {
    let mut factor = 1.0_f32;
    let skill = info.skill_id();
    let mut prob: u32 = 0;
    if skill != 0
        && info.skill.prop.is_some()
        && let Some(add) = info.skill.add
    {
        prob = if att.is_player() && def.is_player() {
            add.probability_pvp
        } else {
            add.probability
        };
        if add.skill_count > 0 {
            factor /= add.skill_count as f32;
        }
    }

    if att.has_buff(BUFF_SKILL, SI_ACR_SUP_DARKILLUSION)
        && matches!(
            skill,
            SI_JST_YOYO_BACKSTAB | SI_JST_YOYO_VATALSTAB | SI_ACR_BOW_SILENTSHOT
        )
    {
        factor *= 1.4_f32;
    }

    match skill {
        SI_JST_YOYO_VATALSTAB => {
            if x_random(rng, 100) < prob {
                factor *= 2.0_f32;
            }
        }
        SI_ACR_BOW_AIMEDSHOT => {
            if x_random(rng, 100) < prob {
                factor = 4.0_f32;
            }
        }
        SI_ACR_BOW_JUNKBOW => {
            if x_random(rng, 100) > prob {
                factor = 0.0_f32;
            }
        }
        SI_JST_YOYO_HITOFPENYA if !cx.cfg.hop_scaling_fix => {
            if let Some(add) = info.skill.add {
                factor *= add.dest_data1[0] as f32 / 100.0_f32;
            }
        }
        _ => {}
    }

    if skill == SI_PSY_PSY_SPRITBOMB && !cx.cfg.improved_spiritbomb {
        let percent = (att.mp_percent() as f32).clamp(10.0_f32, 100.0_f32);
        factor = factor * 1.5_f32 * (percent / 100.0_f32);
    }

    if !att.is_player() {
        let ai = &att.mover_prop().ex.ai;
        if ai.berserk_hp > 0 && att.hp_percent() <= ai.berserk_hp {
            factor *= ai.berserk_dmg_mul;
        }
    } else {
        if def.is_player() {
            factor *= 0.60_f32;
        }
        if info.hand == Hand::Left {
            factor *= 0.75_f32;
        }
    }

    if att.is_player() && def.chr_state() & CHS_DOUBLE != 0 {
        factor *= 2.0_f32;
    }

    let mut delta = def.level().wrapping_sub(att.level());
    if !att.is_player() && rank_skips_level_gap(att.rank()) {
        delta = 0;
    }
    if delta > 0 && (!att.is_player() || !def.is_player()) {
        factor *= level_gap_factor(delta);
    }
    factor
}

/// The party state `CalcLinkAttackDamage` reads (Plan/17 §5.13), gathered by the caller from doc 21's party API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkAttackState {
    /// The attacker is a player and a member of a valid party.
    pub in_party: bool,
    /// The party is an advanced one (`m_nKindTroup == 1`).
    pub advanced: bool,
    /// `m_nModeTime[PARTY_LINKATTACK_MODE]` is running.
    pub link_mode: bool,
    /// The attacker is the party leader.
    pub is_leader: bool,
    /// `m_nModeTime[PARTY_PARSKILL_MODE]` is running (every member counts).
    pub parskill_mode: bool,
    /// `m_nSizeofMember`.
    pub party_size: i32,
    /// Members whose `m_idSetTarget` equals the leader's (both set) and who stand within 255 m of the attacker;
    /// the vanilla `N` (the leader counts itself). Only read without `features.better_link_attack`.
    pub joined_members: i32,
}

/// `CalcLinkAttackDamage(dmg)` (Plan/17 §5.13): the bonus damage of Link Attack, 0 unless `dmg > 0`, the attacker is
/// in an advanced party whose link mode runs, and (vanilla) is its leader. `features.better_link_attack`: `dmg *
/// 8 / 10` for every member; vanilla: `dmg * N / 10` with `N` the party size under the party-skill mode, else the
/// members sharing the leader's target.
pub fn link_attack_bonus(dmg: i32, party: &LinkAttackState, cfg: &CombatConfig) -> i32 {
    if dmg <= 0 || !party.in_party || !party.advanced || !party.link_mode {
        return 0;
    }
    if cfg.better_link_attack {
        return dmg.wrapping_mul(8) / 10;
    }
    if !party.is_leader {
        return 0;
    }
    let n = if party.parskill_mode {
        party.party_size
    } else {
        party.joined_members
    };
    dmg.wrapping_mul(n) / 10
}
