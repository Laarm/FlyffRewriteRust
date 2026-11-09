//! `CalcDefense` (Plan/17 §5.9; `MoverAttack.cpp:595-707`; WP-COMBAT-02).

use super::{AttackInfo, CombatCx, CombatantView};
use crate::consts::dst::DST_RESIST_MAGIC;
use crate::stats;
use flyff_common::defs::skill::SI_BLD_DOUBLE_ARMORPENETRATE;
use flyff_common::rng::GameRng;

/// `CMover::CalcDefense(info, bRandom)` for the victim `def`: `(int)(core * GetDEFMultiplier) +
/// (victim is a player ? EventLua.GetDefensePower() : 0)`.
///
/// `core` (`CalcDefenseCore`): a magic skill gives `GetParam(DST_RESIST_MAGIC, 0)`; an autoattack (or any attack
/// between two players) the generic `((Lv * 2 + STA / 2) / 2.8 - 4) + (STA - 14) * factor` plus a quarter of the
/// item defense plus `DST_ADJDEF`, at least 0; an NPC victim `resist_magic / 7 + 1` (wand) or `natural_armor /
/// 7 + 1`; a player victim of a monster skill the formula of `CalcDefensePlayer`. The multiplier is the NPC
/// `m_fDefence_Rate` times 0.5 for Armor Penetrate times `1 + ADJDEF_RATE / 100`.
///
/// `att` is `None` for the character-window value (`GetShowDefense`); `rng` is `None` for the average item
/// defense (`bRandom = FALSE`) and otherwise draws `x_random(max - min)` when the victim wears armour with a
/// range.
pub fn calc_defense(
    def: &dyn CombatantView,
    att: Option<&dyn CombatantView>,
    info: &AttackInfo<'_>,
    cx: &CombatCx<'_>,
    rng: Option<&mut dyn GameRng>,
) -> i32 {
    let flags = info.flags;
    let core = if flags.intersects(super::AtkFlags::MAGICSKILL) {
        def.param(DST_RESIST_MAGIC, 0)
    } else {
        let generic = flags.intersects(super::AtkFlags::GENERIC)
            || (def.is_player() && att.is_some_and(|a| a.is_player()));
        if generic {
            stats::defense_generic(def.stat(), def.rules(), rng)
        } else if !def.is_player() {
            stats::defense_npc(def.mover_prop(), flags.intersects(super::AtkFlags::MAGIC))
        } else {
            stats::defense_player_vs_npc(def.stat(), rng)
        }
    };
    let armor_penetrate = info.skill_id() == SI_BLD_DOUBLE_ARMORPENETRATE;
    let mult = stats::def_multiplier(def.stat(), armor_penetrate);
    let mut d = (core as f32 * mult) as i32;
    if def.is_player() {
        d = d.wrapping_add(cx.event_defense_power);
    }
    d
}
