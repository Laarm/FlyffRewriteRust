//! Monster skills: the effect of a cast and the skill levels of `MonsterSkill.lua` (Plan/17 §6.15, §5.3 step 6;
//! WP-SKILL-13; `src:_AIInterface/AIMonster.cpp:2432-2566`).
//!
//! **Who does what.** The monster AI ([`crate::ai::monster_skill`], WP-AI-11) owns the *choice*: at the melee and
//! ranged hit frames of a monster it rolls `x_random(100)` over the entries of its `MonsterSkill.lua` block of
//! the attack method (probabilities accumulate, the first entry with `roll < sum` wins, ranged entries are skipped
//! when the method is melee and not accumulated), checks the skill and its level row (by **row id**
//! `sub_define + level - 1`, no `+5` quirk), refuses `SI_MAG_FIRE_HOTAIR`, compares the entry's hit index with
//! the swing's `motion_hit_count`, and collects the victims (the target, or every live player within the entry's
//! range of the monster by `IsRangeObj`) with their `sfx_obj[1]` / `sfx_obj[2]` effects. This module is the
//! *effect* on one victim: [`apply_cast`] runs for every [`MonsterCast`] the AI hands to
//! [`CombatHooks::apply_monster_skill`](crate::combat::CombatHooks) (its default queues a
//! [`SkillJob::Monster`](super::cast::SkillJob)).
//!
//! **The override view.** The original patched the shared props for the call (`dwReferTarget1/2 = NULL_ID`,
//! `dwSkillTime = entry duration`) and restored them. The cast applies an [`Applied`] view instead and the tables
//! are never touched. KEEP-QUIRK: a buff created through the view has `total = duration` (0 for every shipped
//! entry), but the buff engine decides whether a skill buff *may expire* from the real row
//! (`add.skill_time > 0`), so such a buff ends at the victim's next buff pass (at most 16 ticks later).
//!
//! The cast is `CMover::ApplySkill(caster, prop, add, ignore_prob)` on the victim: *not* `DoApplySkill`, so
//! there is no area resolution, no Dark Illusion bookkeeping, no secondary skill and no aggro call; the damage
//! the skill deals (`EXT_MELEEATK` / `EXT_MAGICATK`) goes through the damage pipeline as a skill hit of the
//! monster.
//!
//! A melee monster skill replaces the normal hit (the AI returns `true` from `apply`); the transformation of a
//! wounded monster (`MonsterTransform`, step 6 of the damage pipeline) is
//! [`ai::monster_skill::transform_check`](crate::ai::monster_skill::transform_check).

use super::apply::{Applied, ApplyArgs, apply_skill};
use crate::ai::monster_skill::MonsterCast;
use crate::ctx::{GameCtx, Services};
use flyff_data::GameData;
use flyff_data::types::MoverId;
use std::borrow::Cow;
use std::sync::Arc;

/// `CMonsterSkill::GetMonsterSkillLevel(mover, skill)`: the level of the first entry for `skill` in the
/// `MonsterSkill.lua` block of the monster `mover`; 0 when the monster has no block or no such entry. The damage
/// pipeline uses it for Asalraalaikum thrown by a monster (a player uses its own skill level).
pub fn monster_skill_level(data: &GameData, mover: MoverId, skill: u32) -> u32 {
    data.lua
        .monster_skills
        .iter()
        .find(|d| d.mover == mover)
        .and_then(|d| d.skills.iter().find(|e| e.skill == skill))
        .map_or(0, |e| u32::try_from(e.skill_level).unwrap_or(0))
}

/// The view of the cast: the skill row with both refer targets disabled and the level row (found by id,
/// `sub_define + level - 1`) with the entry's duration.
pub fn view<'a>(data: &'a GameData, cast: &MonsterCast) -> Option<Applied<'a>> {
    let prop = data.skill(cast.skill)?;
    let row = prop.sub_define.wrapping_add(cast.level).wrapping_sub(1);
    let add = data.add_skill_by_id(row)?;
    Some(
        Applied {
            prop: Cow::Borrowed(prop),
            add: Some(Cow::Borrowed(add)),
        }
        .without_refer_targets()
        .with_duration(cast.duration_ms),
    )
}

/// One cast on one victim (see the module documentation). Does nothing when the caster or the victim is gone or
/// the rows vanished.
pub fn apply_cast(svc: &mut Services, ctx: &mut GameCtx<'_>, cast: &MonsterCast) {
    if ctx.worlds.mover(cast.caster).is_none() || ctx.worlds.mover(cast.target).is_none() {
        return;
    }
    let data = Arc::clone(ctx.data);
    let Some(a) = view(&data, cast) else {
        return;
    };
    let args = ApplyArgs {
        ignore_prob: cast.ignore_prob,
        ..ApplyArgs::by(cast.caster)
    };
    apply_skill(svc, ctx, cast.target, &a, &args);
}
