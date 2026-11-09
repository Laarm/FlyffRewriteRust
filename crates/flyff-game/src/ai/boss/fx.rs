//! The effects of the boss AIs that leave the acting mover (Plan/18 §10.3, §10.4, §8.4; task boss-ai-b,
//! WP-AI-19 .. WP-AI-21): [`BossFx`], the announcements and skills a boss raises, the service-level runner
//! [`run`] and the frame-level [`damage_around_at`].
//!
//! **Who uses what.** The brains (the AI pass, `AiCx`) push [`AiCmd::DamageAround`](crate::ai::AiCmd),
//! [`AiCmd::Replace`](crate::ai::AiCmd), [`AiCmd::Boss`](crate::ai::AiCmd) and `AiCmd::Spawn`; the hit handlers
//! (the barrier, `ActionCx`) call `hits::on_damage_target` / `hits::on_damage_at` / [`damage_around_at`] at the hit
//! frame and queue every [`BossFx`] as `SkillJob::Boss(..)` in the original statement order. Skills and
//! announcements therefore land after the damage of the same hit or tick, at the end of the world phase, exactly
//! like `SkillJob::Monster` (documented deviation: the original applied them inline).
//!
//! | effect | original | audience |
//! |---|---|---|
//! | [`BossFx::Caption`] | `g_DPCoreClient.SendCaption(text, world)` | every online player (the client of that world shows it, instance layers included) |
//! | [`BossFx::Shout`] | `CUserMng::AddWorldShout(name, text, pos, world)` | players of the world, ANY layer, `d^2 < 255^2` (`AddBlock`) |
//! | [`BossFx::Sfx`] | `AddCreateSfxObj(anchor, sfx, x, y, z)` | the viewers of the anchor, the anchor included |
//! | [`BossFx::Quake`] | `AddCommonPlace(player, COMMONPLACE_QUAKE)` | the viewers of the player, the player included |
//! | [`BossFx::SkillAround`] | `ApplySkillAround(this, OBJTYPE_PLAYER, prop, add, false, FALSE, range)` | - |
//! | [`BossFx::Skill`] | `DoApplySkill(target, ..)` / `target->ApplySkill(this, ..)` | - |
//! | [`BossFx::BearGrow`] | `SetScale(scale * 1.3)` + `AddSetScale` while `scale.x < 10` | the viewers of the boss |

use super::hits::{act_item, on_damage_at, on_damage_target};
use crate::ai::{AiBrain, AiSlot};
use crate::chat::ingress::notice;
use crate::combat::ActionCx;
use crate::combat::action::target_valid;
use crate::combat::arbiter::{ApplyMask, AroundSpec, around_candidates, damage_around_hits};
use crate::ctx::{GameCtx, Services};
use crate::outbox::Audience;
use crate::skill::apply::{Applied, ApplyArgs, apply_skill};
use crate::skill::area::{ApplyType, apply_skill_around, do_apply_skill};
use crate::social::party::summon::format_s;
use crate::world::ctrl::COMMONPLACE_QUAKE;
use crate::world::fx::sfx_near;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{ObjId, WorldId};
use flyff_proto::pb;
use flyff_rules::combat::AtkFlags;
use smallvec::SmallVec;
use std::sync::Arc;

/// `AddWorldShout`'s range: players of the world with `d^2 < 255^2` (`User.cpp:5551-5570`, `AddBlock`).
pub const SHOUT_RANGE: f32 = 255.0;
/// The colour of a boss shout (`0xFFFF99CC`, the colour of an ordinary shout).
pub const SHOUT_COLOR: u32 = 0xFFFF_99CC;
/// `vScale *= 1.3f`: the Mushmoot grows by this factor with every ATK4.
pub const GROW_FACTOR: f32 = 1.3;
/// The Mushmoot stops growing when the next scale would reach this value (`vScale.x < 10.0f`).
pub const GROW_LIMIT: f32 = 10.0;

/// Whose skill code applies a skill (`DoApplySkill` of the caster, or `ApplySkill` of the victim).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillHow {
    /// `caster->DoApplySkill(target, prop, add)`: the area rules and the secondary skill apply.
    Do,
    /// `target->ApplySkill(caster, prop, add)`: the plain application on the victim.
    Receive,
}

/// An effect of a boss that the acting mover cannot run itself (see the module docs). Only ids and numbers, so
/// that `SkillJob` stays `Copy`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BossFx {
    /// `SendCaption(text, world)`: `tid` with `%s` filled by the name of `name_of` (`""` when it is gone).
    Caption {
        /// The world of the caption (the display filter of the client).
        world: WorldId,
        /// The text id.
        tid: u32,
        /// The mover whose name fills the `%s`.
        name_of: Option<ObjId>,
    },
    /// `AddWorldShout(speaker name, text, center, world)`.
    Shout {
        /// The boss that speaks (its prop name is the sender; a gone speaker skips the shout).
        speaker: ObjId,
        /// The world.
        world: WorldId,
        /// The centre of the 255 m circle.
        center: Vec3,
        /// The text id.
        tid: u32,
        /// The mover whose name fills the `%s`.
        name_of: Option<ObjId>,
    },
    /// `AddCreateSfxObj(anchor, sfx, pos)`: a zero `pos` attaches the effect to the anchor.
    Sfx {
        /// The mover the effect is announced around.
        anchor: ObjId,
        /// The effect id.
        sfx: u32,
        /// The world position (zero = on the anchor).
        pos: Vec3,
    },
    /// `AddCommonPlace(anchor, COMMONPLACE_QUAKE)`; the anchor is a PLAYER.
    Quake {
        /// The player that shakes (and the players that see it).
        anchor: ObjId,
    },
    /// `caster->ApplySkillAround(caster, OBJTYPE_PLAYER, prop, add, false, FALSE, range)`: `range` 0 = the skill's own.
    SkillAround {
        /// The boss.
        caster: ObjId,
        /// The skill id.
        skill: u32,
        /// The level.
        level: u32,
        /// `fRangeCustom` in metres (0 = the level row's range).
        range: f32,
    },
    /// One skill on one mover.
    Skill {
        /// The caster.
        caster: ObjId,
        /// The victim.
        target: ObjId,
        /// The skill id.
        skill: u32,
        /// The level.
        level: u32,
        /// `DoApplySkill` or `ApplySkill`.
        how: SkillHow,
    },
    /// The Mushmoot grows (`ATK4`): `scale *= 1.3` while the product stays below 10.
    BearGrow {
        /// The boss.
        boss: ObjId,
    },
}

/// Marks the key `(skill, level)` (a skill row) or `(tid, u32::MAX)` (a text) as logged for the mover that owns
/// this slot; `true` the first time. A mover without a boss or Meteonyker brain logs every time.
impl AiSlot {
    pub(crate) fn note_missing(&mut self, key: (u32, u32)) -> bool {
        let seen = match &mut self.brain {
            AiBrain::Boss(b) => &mut b.missing_logged,
            AiBrain::Meteonyker(b) => &mut b.missing_logged,
            _ => return true,
        };
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        true
    }
}

/// Logs a missing skill row once per (skill, level) and caster (`GetSkillProp` failing logs an `Error` at every call
/// in the original; the rows of the shipped data all exist).
fn warn_missing_skill(ctx: &mut GameCtx<'_>, caster: ObjId, skill: u32, level: u32) {
    let first = ctx
        .worlds
        .mover_mut(caster)
        .and_then(|m| m.monster_mut())
        .is_none_or(|e| e.ai.note_missing((skill, level)));
    if first {
        tracing::warn!(skill, level, "boss effect: the skill row does not exist");
    }
}

/// The display name of mover `id`: a player's name, a monster's prop name, `""` when it is gone.
fn name_of(ctx: &GameCtx<'_>, id: Option<ObjId>) -> String {
    let Some(m) = id.and_then(|id| ctx.worlds.mover(id)) else {
        return String::new();
    };
    match m.player() {
        Some(p) => p.ident.name.clone(),
        None => ctx
            .data
            .mover(m.prop_id())
            .map(|p| p.name.clone())
            .unwrap_or_default(),
    }
}

/// `prj.GetText(tid)` with the `%s` of the template filled by the name of `who`; the second value is `true` when the
/// text does not exist (`GETTEXT` answers `"error"` out of range and `""` for a hole).
fn text(ctx: &GameCtx<'_>, tid: u32, who: Option<ObjId>) -> (String, bool) {
    let template = ctx.data.texts.get(tid);
    let missing = template.is_empty() || template == "error";
    if who.is_some() {
        (format_s(template, &[&name_of(ctx, who)]), missing)
    } else {
        (template.to_owned(), missing)
    }
}

/// Logs a missing text once per speaker and text (a caption has no speaker: it logs every time, captions are rare).
fn warn_missing_text(ctx: &mut GameCtx<'_>, speaker: Option<ObjId>, tid: u32) {
    let first = speaker
        .and_then(|id| ctx.worlds.mover_mut(id))
        .and_then(|m| m.monster_mut())
        .is_none_or(|e| e.ai.note_missing((tid, u32::MAX)));
    if first {
        tracing::warn!(tid, "boss effect: the text does not exist");
    }
}

/// Runs one effect at the end of the world phase (`SkillJob::Boss`, the path of `SkillJob::Monster`).
pub fn run(s: &mut Services, ctx: &mut GameCtx<'_>, fx: BossFx) {
    match fx {
        BossFx::Caption {
            world,
            tid,
            name_of: who,
        } => {
            let (line, missing) = text(ctx, tid, who);
            if missing {
                warn_missing_text(ctx, None, tid);
            }
            notice::caption(s, ctx, Audience::AllOnline, false, world, &line);
        }
        BossFx::Shout {
            speaker,
            world,
            center,
            tid,
            name_of: who,
        } => {
            // a speaker that is gone says nothing
            let Some(name) = ctx
                .worlds
                .mover(speaker)
                .and_then(|m| ctx.data.mover(m.prop_id()))
                .map(|p| p.name.clone())
            else {
                return;
            };
            let (line, missing) = text(ctx, tid, who);
            if missing {
                warn_missing_text(ctx, Some(speaker), tid);
            }
            ctx.broadcast(
                Some(&*s),
                ObjId::NULL,
                Audience::Radius {
                    world,
                    center,
                    range: SHOUT_RANGE,
                },
                pb::Shout {
                    obj_id: 0,
                    name,
                    text: line,
                    color: SHOUT_COLOR,
                },
            );
        }
        BossFx::Sfx { anchor, sfx, pos } => sfx_near(ctx, anchor, sfx, pos, false),
        BossFx::Quake { anchor } => ctx.broadcast(
            None,
            anchor,
            Audience::NearInclSelf,
            pb::CommonPlace {
                kind: COMMONPLACE_QUAKE,
            },
        ),
        BossFx::SkillAround {
            caster,
            skill,
            level,
            range,
        } => {
            let data = Arc::clone(ctx.data);
            let Some(a) = Applied::skill(&data, skill, level) else {
                warn_missing_skill(ctx, caster, skill, level);
                return;
            };
            if ctx.worlds.mover(caster).is_none() {
                return;
            }
            apply_skill_around(
                s,
                ctx,
                caster,
                caster,
                ApplyType::PLAYER,
                &a,
                false,
                false,
                range,
                false,
            );
        }
        BossFx::Skill {
            caster,
            target,
            skill,
            level,
            how,
        } => {
            let data = Arc::clone(ctx.data);
            let Some(a) = Applied::skill(&data, skill, level) else {
                warn_missing_skill(ctx, caster, skill, level);
                return;
            };
            if ctx.worlds.mover(caster).is_none() || ctx.worlds.mover(target).is_none() {
                return;
            }
            match how {
                SkillHow::Do => {
                    do_apply_skill(s, ctx, caster, target, &a, false, 0, false, false);
                }
                SkillHow::Receive => {
                    apply_skill(s, ctx, target, &a, &ApplyArgs::by(caster));
                }
            }
        }
        BossFx::BearGrow { boss } => {
            // computed here, not when the hit was handled: two ATK4 hits of one tick compound correctly
            let Some(m) = ctx.worlds.mover_mut(boss) else {
                return;
            };
            let grown = m.core.scale * GROW_FACTOR;
            if grown < GROW_LIMIT {
                m.core.scale = grown;
                m.sim.params.scale = grown;
                let factor = grown / crate::mover::model_scale(ctx.data, m.prop_id());
                ctx.broadcast(None, boss, Audience::NearInclSelf, pb::SetScale { factor });
            }
        }
    }
}

/// `CWorld::SendDamageAround(&center, flags, attacker, OBJTYPE_PLAYER, item, radius)`: every live player strictly
/// closer than `radius` metres to `center` takes a hit of `item` (the damage source; an unknown item hits nobody).
/// The caller is a hit handler (`ActionCx`) or the effect phase of an AI command.
pub fn damage_around_at(
    cx: &mut ActionCx<'_>,
    attacker: ObjId,
    center: Vec3,
    flags: AtkFlags,
    item: u32,
    radius: f32,
) {
    let spec = AroundSpec::at_point(center, flags, attacker, ApplyMask::PLAYER, item, radius);
    let hits = around_candidates(cx.grid, cx.arb.w.store, &spec);
    damage_around_hits(&mut cx.arb, &spec, &hits);
}

// ---- the hit effects of a boss swing (pure description + executor) ----------------------------------------------

/// One effect of a boss hit frame (the output of the pure `hit_effects` functions of the bosses): the damage calls
/// run at once, in list order, the [`BossFx`] jobs are queued behind them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitFx {
    /// `OnDamage(item, state, target)` with a hit object ([`on_damage_target`]): an `SRO_AROUND` item hits the
    /// players around the target, any other item hits the target once. `item` `None` = the item of the swing.
    Damage {
        /// The attack item (`None` = `GetActParam(1)`).
        item: Option<u32>,
        /// The hit object.
        target: ObjId,
    },
    /// `OnDamage(item, state, NULL, &pos)` ([`on_damage_at`], `AF_GENERIC`).
    DamageAt {
        /// The attack item (`None` = `GetActParam(1)`).
        item: Option<u32>,
        /// The centre.
        pos: Vec3,
    },
    /// `CWorld::SendDamageAround(&center, flags, this, OBJTYPE_PLAYER, item, radius)` ([`damage_around_at`]).
    Around {
        /// The centre.
        center: Vec3,
        /// The `AF_*` of every hit.
        flags: AtkFlags,
        /// The damage source.
        item: u32,
        /// Metres (strict).
        radius: f32,
    },
    /// An announcement or a skill ([`BossFx`]).
    Job(BossFx),
}

/// The effects of one hit frame; the Mushmoot's ATK4 (ten skills and the growth) is the longest list.
pub type HitList = SmallVec<[HitFx; 12]>;

/// What the pure `hit_effects` functions may look at besides their arguments.
pub struct HitEnv<'a> {
    /// The boss (the caster of every [`BossFx`] it raises).
    pub boss: ObjId,
    /// The prop's `dwAtk1 .. dwAtk4`.
    pub atk: [u32; 4],
    /// `GetSkillProp(skill, level)` succeeds (the Meteonyker draws only when the row exists).
    pub skill_exists: &'a dyn Fn(u32, u32) -> bool,
    /// BigMuscle: the function-static position of hit 0 of ATK4 (Q18), kept per boss in [`super::BossAi`].
    pub atk4_pos: Option<&'a mut Option<Vec3>>,
}

/// The skeleton of every boss `on_hit`: reads the hit count, the positions and the props at the barrier, lets `f`
/// (the pure `hit_effects` of the boss) decide, writes BigMuscle's remembered position back and executes the list.
/// The `m_nMotionHitCount++` is the dispatcher's ([`super::hits::bump_motion_hit_count`]), never done here.
pub fn on_hit_with(
    cx: &mut ActionCx<'_>,
    boss: ObjId,
    target: ObjId,
    f: impl FnOnce(i32, Vec3, Option<(ObjId, Vec3)>, &mut HitEnv<'_>, &mut dyn GameRng) -> HitList,
) {
    let data = Arc::clone(&cx.arb.env.core.data);
    let Some(m) = cx.mover(boss) else { return };
    let hit = m.sim.act.motion_hit_count;
    let me = m.sim.pos;
    let atk = data
        .mover(m.prop_id())
        .map_or([0; 4], |p| [p.atk1, p.atk2, p.atk3, p.atk4]);
    let mut atk4 = m
        .monster()
        .and_then(|e| e.ai.boss())
        .and_then(|b| b.atk4_pos);
    let item = act_item(cx, boss);
    let t = if target_valid(cx, boss, target) {
        cx.mover(target).map(|t| (target, t.core.pos))
    } else {
        None
    };
    let exists = |skill: u32, level: u32| Applied::skill(&data, skill, level).is_some();
    let list = {
        let mut env = HitEnv {
            boss,
            atk,
            skill_exists: &exists,
            atk4_pos: Some(&mut atk4),
        };
        f(hit, me, t, &mut env, &mut *cx.arb.w.rng)
    };
    if let Some(b) = cx
        .mover_mut(boss)
        .and_then(|m| m.monster_mut())
        .and_then(|e| e.ai.boss_mut())
    {
        b.atk4_pos = atk4;
    }
    exec(cx, boss, item, &list);
}

/// Executes a hit list in order: damage calls at once, [`BossFx`] jobs queued for the end of the phase.
pub fn exec(cx: &mut ActionCx<'_>, boss: ObjId, act: u32, list: &[HitFx]) {
    for e in list {
        match *e {
            HitFx::Damage { item, target } => {
                let damage = on_damage_target(cx, boss, item.unwrap_or(act), target);
                gather_reflex(cx, target, damage);
            }
            HitFx::DamageAt { item, pos } => {
                on_damage_at(cx, boss, item.unwrap_or(act), pos, AtkFlags::GENERIC);
            }
            HitFx::Around {
                center,
                flags,
                item,
                radius,
            } => damage_around_at(cx, boss, center, flags, item, radius),
            HitFx::Job(fx) => cx
                .arb
                .fx
                .skill_jobs
                .push(crate::skill::cast::SkillJob::Boss(fx)),
        }
    }
}

/// The tail of `CMover::OnDamage`: a victim in the middle of Reflex Hit gathers the damage of a single hit.
fn gather_reflex(cx: &mut ActionCx<'_>, target: ObjId, damage: i32) {
    use flyff_common::defs::skill::SI_MER_ONE_REFLEXHIT;
    use flyff_sim::types::OBJSTA_ATK_MELEESKILL;
    if damage <= 0 {
        return;
    }
    if let Some(t) = cx.mover_mut(target)
        && t.sim.act.state.bits() & OBJSTA_ATK_MELEESKILL != 0
        && t.sim.skill.skill == SI_MER_ONE_REFLEXHIT
    {
        t.combat.reflex_dmg = t.combat.reflex_dmg.wrapping_add(damage);
    }
}
