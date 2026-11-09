//! `CMonsterSkill` (Plan/18 §9.11, ref/11 §6.11, `AIMonster.cpp:2355-2568`; WP-AI-11): the skills a monster casts
//! at its hit frames (`MonsterSkill.lua`) and the transformation of a wounded monster (`propMoverEx`
//! `Transform`). Doc 17 calls both from its hit and damage paths:
//!
//! * [`apply`] at every NPC melee hit frame (`ATK_MELEE`; a `true` result **replaces** the normal damage of
//!   that hit) and ranged hit frame (`ATK_RANGE`; the shot still leaves);
//! * [`transform_check`] from step 6 of the damage pipeline (a `true` result ends the damage call with 0).
//!
//! The effect of a cast is the skill engine's (WP-SKILL): [`apply`] decides **which** skill, **on whom**, with
//! which duration and effects, and hands every cast to the [`CombatHooks::apply_monster_skill`](
//! crate::combat::CombatHooks::apply_monster_skill) seam as a [`MonsterCast`]. The original patched the shared
//! skill props for the duration of the call (`dwReferTarget1/2 = NULL_ID`, `dwSkillTime`); the cast carries those
//! overrides instead and nothing shared is mutated.

use crate::ai::rng::x_random;
use crate::ai::spawn::MonsterSpawn;
use crate::combat::context::ActionCx;
use crate::combat::env::ArbiterCx;
use crate::combat::hooks::MonsterSkillTrigger;
use crate::combat::state::{is_live, motion_hit_count};
use crate::world::grid::LinkSet;
use flyff_common::ObjId;
use flyff_common::defs::obj::XI_SKILL_MAG_EARTH_LOOTING01;
use flyff_common::defs::skill::SI_MAG_FIRE_HOTAIR;
use flyff_common::math::Vec3;
use flyff_data::lua_config::{ATK_MELEE, ATK_RANGE, MonsterSkillEntry};
use flyff_rules::combat::mul_div;
use flyff_rules::consts::NULL_ID;
use flyff_sim::bounds::{is_range_obj, radius};

/// One skill cast of a monster on one target (`pTarget->ApplySkill(pAttacker, skillProp, addProp, ignore)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonsterCast {
    /// The monster.
    pub caster: ObjId,
    /// The player (or mover) the skill is applied to.
    pub target: ObjId,
    /// The skill (`SI_*`).
    pub skill: u32,
    /// The skill level (`dwSkillLv`).
    pub level: u32,
    /// `bIgnoreSkillProb`: apply without rolling the skill's own probability.
    pub ignore_prob: bool,
    /// `dwSkillTime` of the Lua entry in milliseconds: replaces the duration of the add-skill row for this cast.
    pub duration_ms: u32,
}

/// The skill of the entries of one monster that a roll `p` (0..100) picks for the attack method `method`: the
/// entries of that method are walked in order, their probabilities accumulate, and the first with `p < sum`
/// wins (so probabilities that sum to less than 100 leave room for "no skill").
pub fn pick(entries: &[MonsterSkillEntry], method: i32, p: i32) -> Option<&MonsterSkillEntry> {
    let mut total = 0_i32;
    for e in entries.iter().filter(|e| e.atk_method == method) {
        total = total.saturating_add(e.prob);
        if p < total {
            return Some(e);
        }
    }
    None
}

/// `CMonsterSkill::ApplySkill(attacker, target, method)` at a hit frame (see the module docs). `true` when a
/// skill was chosen and applied.
///
/// Draws: nothing when the monster has no entry in `MonsterSkill.lua`; otherwise one `x_random(100)`, before any
/// validity test. The chosen skill must exist with its add-skill row for the level, must not be
/// `SI_MAG_FIRE_HOTAIR`, and its hit index (the script stores `nHitCount - 1`) must equal the attacker's current
/// `motion_hit_count`. Range 0 applies to the target, else to every live player but the attacker inside the
/// legacy player window of `range` link units whose `IsRangeObj(attacker, range)` holds (window order). Each
/// victim gets the skill's `dwSfxObj2` (when set) and `dwSfxObj3`.
pub fn apply(
    cx: &mut ActionCx<'_>,
    attacker: ObjId,
    target: ObjId,
    trigger: MonsterSkillTrigger,
) -> bool {
    let method = match trigger {
        MonsterSkillTrigger::Melee => ATK_MELEE,
        MonsterSkillTrigger::Range => ATK_RANGE,
    };
    let Some(a) = cx.mover(attacker) else {
        return false;
    };
    if cx.mover(target).is_none() {
        return false;
    }
    let (mover_id, hit_count, pos, a_radius) = (
        a.prop_id(),
        motion_hit_count(a),
        a.core.pos,
        radius(&a.sim.params.bounds, a.sim.params.scale),
    );
    let core = cx.arb.env.core;
    let data = &core.data;
    let Some(def) = data.lua.monster_skills.iter().find(|d| d.mover == mover_id) else {
        return false;
    };
    let p = x_random(&mut *cx.arb.w.rng, 100);
    let Some(e) = pick(&def.skills, method, i32::try_from(p).unwrap_or(i32::MAX)) else {
        return false;
    };
    let Some(skill) = data.skill(e.skill) else {
        return false;
    };
    let Ok(level) = u32::try_from(e.skill_level) else {
        return false;
    };
    let level_row = (skill.sub_define).wrapping_add(level).wrapping_sub(1);
    if data.add_skill_by_id(level_row).is_none() || skill.id == SI_MAG_FIRE_HOTAIR {
        return false;
    }
    if e.hit_count != hit_count {
        return false;
    }
    let (sfx2, sfx3) = (skill.sfx_obj[1], skill.sfx_obj[2]);
    let cast = |target: ObjId| MonsterCast {
        caster: attacker,
        target,
        skill: e.skill,
        level,
        ignore_prob: e.ignore,
        duration_ms: e.skill_time_ms,
    };
    let victims: Vec<ObjId> = if e.range == 0 {
        vec![target]
    } else {
        // every live player in the window (link units = the script's range), within `IsRangeObj` of the monster
        let range = e.range as f32;
        let mut out = Vec::new();
        cx.grid
            .for_each_legacy(pos, e.range, LinkSet::Players, |id| {
                if id == attacker {
                    return;
                }
                let Some(m) = cx.arb.w.store.mover(id).filter(|m| !m.core.deleted) else {
                    return;
                };
                if m.is_player()
                    && is_live(m)
                    && is_range_obj(
                        m.core.pos,
                        radius(&m.sim.params.bounds, m.sim.params.scale),
                        pos,
                        a_radius,
                        range,
                    )
                {
                    out.push(id);
                }
            });
        out
    };
    for v in victims {
        let c = cast(v);
        cx.arb
            .hooks
            .apply_monster_skill(&mut cx.arb.w, &mut cx.arb.fx, &c);
        if sfx2 != NULL_ID {
            cx.arb
                .fx
                .near(v, crate::world::fx::create_sfx_msg(sfx2, Vec3::ZERO, false));
        }
        if sfx3 != NULL_ID {
            cx.arb
                .fx
                .near(v, crate::world::fx::create_sfx_msg(sfx3, Vec3::ZERO, false));
        }
    }
    true
}

/// `CMonsterSkill::MonsterTransform(victim, hp_after)` (`__VER >= 14`), step 6 of the damage pipeline: a monster
/// with a `Transform(hp_rate, mover)` block whose HP percent after the hit, `MulDiv(hp_after, 100, max_hp)`,
/// is **at or below** `hp_rate` is replaced by the target mover (aggressive, at the same position, in the same
/// layer; a sound effect `XI_SKILL_MAG_EARTH_LOOTING01` on the old one) and deleted. `true` ends the damage call
/// with 0 damage. Players, monsters without the block or whose target mover is unknown are not transformed.
pub fn transform_check(cx: &mut ArbiterCx<'_>, victim: ObjId, hp_after: i32) -> bool {
    let core = cx.env.core;
    let data = &core.data;
    let Some(v) = cx.w.store.mover(victim).filter(|m| !m.core.deleted) else {
        return false;
    };
    // a player, or a monster that an earlier hit of this barrier already replaced (`IsValidObj` is false then)
    if v.is_player() || v.combat.transformed {
        return false;
    }
    let Some(tr) = data
        .mover(v.prop_id())
        .and_then(|p| p.ex.transform.as_ref())
    else {
        return false;
    };
    let max = crate::stats::env::max_hp(v, core);
    let pct = if max == 0 {
        0
    } else {
        mul_div(hp_after, 100, max)
    };
    if tr.hp_rate < pct as f32 {
        return false;
    }
    if data.mover(tr.mover).is_none() {
        return false;
    }
    let spawn = MonsterSpawn::aggressive(tr.mover, v.core.world, v.core.layer, v.core.pos);
    let live = is_live(v);
    if let Some(v) = cx.w.store.mover_mut(victim) {
        v.combat.transformed = true;
    }
    cx.fx.spawns.push(spawn);
    cx.fx.near(
        victim,
        crate::world::fx::create_sfx_msg(XI_SKILL_MAG_EARTH_LOOTING01, Vec3::ZERO, false),
    );
    if live {
        cx.fx.deletes.push(victim);
    }
    true
}
