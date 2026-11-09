//! Hit or miss, critical chance and block (Plan/17 §5.10, §5.11; WP-COMBAT-02).

use super::flags::{AtkFlags, JobFactor};
use super::rng::x_random;
use super::{AttackInfo, CombatConfig, CombatCx, CombatantView};
use crate::consts::{MIN_HR, ONEKILL_MODE, dst::*};
use crate::stats::{self, c_int};
use flyff_common::rng::GameRng;

/// `GetAttackResult(victim)` (`MoverAttack.cpp:339-379`, only for `AF_GENERIC`: the melee start roll and the
/// ranged impact): `true` = hit. `ONEKILL_MODE` and (for players) an event-spawn monster victim always hit and
/// draw nothing; otherwise the draw is `x_random(100) < chance` with the chance of
/// [`stats::hit_chance_percent`] (NPC to player, player to NPC, otherwise; plus `GetAdjHitRate()`, clamped to
/// `[20, 96]`).
pub fn attack_result(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    event_spawn_victim: bool,
    rng: &mut dyn GameRng,
) -> bool {
    if att.mode() & ONEKILL_MODE != 0 {
        return true;
    }
    if att.is_player() && !def.is_player() && event_spawn_victim {
        return true;
    }
    let chance = stats::hit_chance_percent(att.stat(), def.stat());
    (x_random(rng, 100) as i32) < chance
}

/// `GetCriticalProb()` (`MoverAttack.cpp:711-753`): `(int)((DEX / 10) * fCritical)` (0 for every job of the data,
/// 1.0 for NPCs), `GetParam(DST_CHR_CHANCECRITICAL, ·)`, clamped to `[0, gameplay.set_max_crit]`, plus the
/// one-shot party bonus `party_size / 2` of a player whose `MVRF_CRITICAL` is armed (added **after** the clamp;
/// the caller clears the flag, see [`AttackInfo::party_crit_used`]).
pub fn critical_prob(v: &dyn CombatantView, cfg: &CombatConfig) -> i32 {
    let mut p = stats::critical_prob_base(v.stat(), v.rules(), &cfg.rules);
    if v.is_player()
        && let Some(bonus) = v.party_crit()
    {
        p = p.wrapping_add(bonus);
    }
    p
}

/// `IsCriticalAttack(defender, flags)`: never for a skill attack (and no draw), else
/// `x_random(100) < GetCriticalProb()`. Records in `info` that the party bonus was spent.
pub fn is_critical_attack(
    att: &dyn CombatantView,
    info: &mut AttackInfo<'_>,
    cfg: &CombatConfig,
    rng: &mut dyn GameRng,
) -> bool {
    if info.flags.is_skill_attack() {
        return false;
    }
    let prob = critical_prob(att, cfg);
    if att.is_player() && att.party_crit().is_some() {
        info.party_crit_used = true;
    }
    (x_random(rng, 100) as i32) < prob
}

/// `GetBlockFactor(attacker, info)` (`this` = the victim): `1.0` no block, `0.0` blocked, and for the vanilla
/// branch `0.1` / `0.2` partial blocks.
///
/// `features.improved_block_calculation` (the repack): only players block; `br = BLOCK_MELEE + BLOCK_RANGE`
/// capped at `max_block_value` (`max_block_value_gw` for PvP in the guild-war world, no lower clamp there);
/// the roll is `r = x_random(100)`, `br < r` no block, `br > r` block, equal no block. Without the switch the
/// vanilla DEX / level based rolls run (player `x_random(80)`, monster `x_random(100)`).
pub fn block_factor(
    def: &dyn CombatantView,
    att: &dyn CombatantView,
    info: &AttackInfo<'_>,
    cx: &CombatCx<'_>,
    rng: &mut dyn GameRng,
) -> f32 {
    let cfg = cx.cfg;
    if cfg.improved_block_calculation {
        if !def.is_player() {
            return 1.0;
        }
        let gw_pvp = att.is_player() && def.is_player() && def.in_guild_war_world();
        let br = stats::block_value(def.stat(), &cfg.rules, gw_pvp);
        let block = x_random(rng, 100) as i32;
        return if br < block {
            1.0
        } else if br > block {
            0.0
        } else {
            1.0
        };
    }
    // vanilla (M:883-935)
    let range_attack = info.flags.intersects(AtkFlags::RANGE);
    if def.is_player() {
        let r = x_random(rng, 80) as i32;
        if r <= 5 {
            return 1.0;
        }
        if r >= 75 {
            return 0.1;
        }
        let (lv, alv) = (def.level(), att.level());
        let block_a = lv as f32 / (lv.wrapping_add(alv) as f32 * 15.0_f32);
        let block_b = def.dex().wrapping_add(att.dex()).wrapping_add(2) as f32
            * (def.dex().wrapping_sub(att.dex()) as f32 / 800.0_f32);
        let block_b = if block_b > 10.0_f32 {
            10.0_f32
        } else {
            block_b
        };
        let mut add = block_a + block_b;
        if add < 0.0_f32 {
            add = 0.0_f32;
        }
        add += if range_attack {
            def.param(DST_BLOCK_RANGE, 0)
        } else {
            def.param(DST_BLOCK_MELEE, 0)
        } as f32;
        let mut br =
            c_int((def.dex() as f32 / 8.0_f32) * def.job_factor(JobFactor::Blocking) + add);
        if br < 0 {
            br = 0;
        }
        if br > r {
            return 0.0;
        }
    } else {
        let r = x_random(rng, 100) as i32;
        if cfg.dmgcorrect {
            if (range_attack && r <= 60) || (!range_attack && r <= 30) {
                return 1.0;
            }
        } else if r <= 5 {
            return 1.0;
        }
        if r >= 95 {
            return 0.1;
        }
        let mut br = c_int(def.parrying().wrapping_sub(def.level()) as f32 * 0.5_f32);
        if br < 0 {
            br = 0;
        }
        if br > r {
            return 0.2;
        }
    }
    1.0
}

/// The hit chance bounds `[MIN_HR, 96]` (Plan/17 §2.6), for the tests and the UI.
pub const HIT_CHANCE_BOUNDS: (i32, i32) = (MIN_HR, crate::consts::MAX_HR);
