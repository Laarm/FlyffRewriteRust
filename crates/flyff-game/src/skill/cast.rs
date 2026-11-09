//! Skill states, hit handlers and shot skills (Plan/17 §6.5, §6.6, §4.4, §4.5; WP-SKILL-05;
//! `src:_Common/MoverActEvent.cpp:72-164, 475-705, 2108-2132`).
//!
//! The states themselves are the sim's (`flyff_sim::act`, WP-SIM-19): `MELEESKILL` ends at 65 % of the motion
//! (Silent Strike ends at the end frame, Yo-yo Counter never), `MAGICCASTING` runs `CASTING1`, `CASTING2` for
//! `cast * 4` frames and `MAGICSKILL`, bursts repeat on the frames with `SimFrame % 4 == 0`. This module is the
//! game side of the hit frames:
//!
//! * [`SkillHits`] is the [`SkillHitHooks`] the action layer calls at the barrier of a frame:
//!   - a **shot skill** (`EXT_MAGICATKSHOT` / `EXT_MAGICSHOT`) spawns its projectile right there
//!     (`OnAttackSFX`, payload `{skill level, skill, AF_*SKILL, dmg_shift * 0.2, control}`); the cost and the
//!     cool-down follow in the same frame;
//!   - any other skill needs the buff engine (`DoApplySkill`), which needs the services: the hit is queued as a
//!     [`SkillJob`] and [`process_pending`] runs it right after the world phase (like the deaths, at most three
//!     frames late); the cost and the cool-down follow its result;
//!   - a **repeated shot** of a burst is the same handler with `hit_index = 1` (the original calls
//!     `OnMeleeSkill(0)` with the default `nCount = 1`: the cool-down is not set again; the magic variant sets it
//!     on every successful call, as the server code does);
//!   - the impact of a skill projectile (`AttackBySFX`) queues a [`SkillJob::Impact`].
//! * [`on_skill_end`] is `OnActEndMeleeSkill` / `OnActEndMagicSkill`: the act parameters and `MVRF_SKILL` are
//!   cleared and a running action-slot chain asks for its next skill ([`super::queue::queue_next`]).
//!
//! Magic costs are paid **at the hit**: `mp -= req_mp / max(1, skill_count)` and `fp -= req_fp` unless the
//! requirement is the "use all" sentinel (already paid at the start). A magic skill whose target died before the
//! hit (Healing) or whose application failed (`n == 0`) pays nothing and sets no cool-down.

use super::REQ_ALL;
use super::use_skill::{effective_level, props, slot_of};
use crate::combat::ActionCx;
use crate::combat::projectile::{HitPayload, ShotRequest, spawn_shot};
use crate::combat::projectile_kind::kind_for_sfx;
use crate::combat::skillhit::{SkillHit, SkillHitHooks, SkillImpact, SkillShot};
use crate::combat::state::{ActParams, is_die};
use crate::ctx::{GameCtx, Services};
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::skill::apply::{Applied, announce, do_apply_skill};
use crate::stats::StatsCore;
use crate::stats::out::Out;
use crate::stats::pool::{inc_fp, inc_mp};
use flyff_common::defs::attribute::{KT_MAGIC, KT_SKILL};
use flyff_common::defs::define::{EXT_MAGICATKSHOT, EXT_MAGICSHOT};
use flyff_common::defs::skill::{
    SI_ASS_CHEER_ACCURACY, SI_ASS_CHEER_BEEFUP, SI_ASS_CHEER_CANNONBALL, SI_ASS_CHEER_CATSREFLEX,
    SI_ASS_CHEER_HASTE, SI_ASS_CHEER_HEAPUP, SI_ASS_CHEER_MENTALSIGN, SI_ASS_CHEER_QUICKSTEP,
    SI_ASS_HEAL_HEALING, SI_ASS_HEAL_PATIENCE, SI_KNT_HERO_DRAWING, SI_RIG_HERO_RETURN,
    SI_RIG_MASTER_BLESSING, SI_RIN_HEAL_HEALRAIN, SI_RIN_SQU_GEBURAHTIPHRETH, SI_RIN_SUP_HOLYGUARD,
    SI_RIN_SUP_PROTECT, SI_RIN_SUP_SPIRITUREFORTUNE,
};
use flyff_common::{GameMs, LayerId, ObjId, SimFrame};
use flyff_data::props::item::ItemProp;
use flyff_data::props::skill_add::AddSkillProp;
use flyff_rules::combat::AtkFlags;
use flyff_rules::consts::NULL_ID;
use flyff_rules::skill::{req_fp, req_mp};
use flyff_sim::types::{MVRF_SKILL, OBJACT_NONE};
use std::sync::Arc;

/// The skill work that waits for the service context (see the module documentation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkillJob {
    /// The hit of a non-shot skill: `OnMeleeSkill` / `OnMagicSkill` past the shot test.
    Hit {
        /// The caster.
        caster: ObjId,
        /// The target of the state.
        target: ObjId,
        /// The skill id.
        skill: u32,
        /// The level of the act parameters.
        level: u32,
        /// `bControl`.
        control: bool,
        /// `OnMeleeSkill`'s `nCount` / `OnMagicSkill`'s `nCount`: 0 for the first hit, 1 for a repeated shot.
        count: i32,
        /// The frame the hit fired in.
        frame: SimFrame,
    },
    /// A skill projectile arrived (`AttackBySFX` with `IsSkillAttack`).
    Impact {
        /// The shooter.
        attacker: ObjId,
        /// The target.
        target: ObjId,
        /// The skill id.
        skill: u32,
        /// The damage parameter of the info (`MakeParamDamage(magic_power, skill, 0)`).
        param: u32,
        /// `bControl`.
        control: bool,
    },
    /// A proc: `DoActiveSkill(skill, level, target, ignore_prob)` of `caster` ([`super::procs::run_active`]).
    Active(crate::combat::ActiveSkill),
    /// The counter-attack skill of a victim (`SI_GEN_ATK_COUNTER`, [`super::procs::run_counter`]).
    Counter {
        /// The victim that counters.
        caster: ObjId,
        /// `SI_GEN_ATK_COUNTER`.
        skill: u32,
        /// Its level.
        level: u32,
        /// The attacker.
        target: ObjId,
    },
    /// One cast of a monster skill on one victim ([`super::monster::apply_cast`]).
    Monster(crate::ai::monster_skill::MonsterCast),
    // ---- boss effects (doc 18 §10.3, task boss-ai-b) ----
    /// An announcement or a skill of a boss AI ([`crate::ai::boss::fx::run`]).
    Boss(crate::ai::boss::fx::BossFx),
    /// `RemoveInvisible()` of a mover ([`super::procs::remove_invisible`]).
    RemoveInvisible {
        /// The mover.
        mover: ObjId,
    },
    /// `RemoveBuff(BUFF_SKILL, skill)` of a mover.
    RemoveSkillBuff {
        /// The mover.
        mover: ObjId,
        /// The skill.
        skill: u32,
    },
    /// `RemoveChrStateBuffs(bits)` of a mover.
    RemoveChrState {
        /// The mover.
        mover: ObjId,
        /// The `CHS_*` bits.
        bits: u32,
    },
}

/// The skill hit handlers (see the module documentation); installed by default in
/// [`CombatWorld`](crate::combat::CombatWorld).
#[derive(Clone, Copy, Debug, Default)]
pub struct SkillHits;

impl SkillHitHooks for SkillHits {
    fn on_skill_hit(&mut self, cx: &mut ActionCx<'_>, hit: &SkillHit) {
        let count = i32::from(hit.hit_index != 0);
        on_hit(cx, hit.attacker, count, hit.frame, 0);
    }

    fn on_repeat_shot(&mut self, cx: &mut ActionCx<'_>, shot: &SkillShot) {
        // the burst index: the events are handled at the barrier, after the sim counted this shot down, so the
        // shots still to come are `shoot_cnt` and this one is number `skill_count - 1 - shoot_cnt`
        let skill_count = cx
            .mover(shot.attacker)
            .map_or(0, |m| m.sim.skill.skill_count);
        let left = cx.mover(shot.attacker).map_or(0, |m| m.sim.act.shoot_cnt);
        let index = u32::try_from(skill_count.wrapping_sub(1).wrapping_sub(left)).unwrap_or(0);
        on_hit(cx, shot.attacker, 1, shot.frame, index);
    }

    fn on_skill_impact(&mut self, cx: &mut ActionCx<'_>, impact: &SkillImpact) -> bool {
        // `AttackBySFX`: the attacker must still know the skill; its current level applies
        let core = cx.arb.env.core;
        let Some(m) = cx.mover(impact.attacker) else {
            return false;
        };
        let Some(slot) = slot_of(m, impact.skill) else {
            return false;
        };
        let level = effective_level(m, core, slot);
        if props(core, impact.skill, level).is_none() {
            return false;
        }
        // `MakeParamDamage(magic_power, skill, 0)`
        let param = flyff_rules::combat::DamageParam::make(impact.magic_power, impact.skill, 0).0;
        cx.q.skill_jobs.push(SkillJob::Impact {
            attacker: impact.attacker,
            target: impact.target,
            skill: impact.skill,
            param,
            control: impact.control,
        });
        true
    }
}

/// `OnMeleeSkill(0, count)` / `OnMagicSkill(0, count)` at the barrier: validates the act, spawns the projectile
/// of a shot skill (cost and cool-down in the same frame) or queues the hit of any other skill for the service
/// context.
fn on_hit(cx: &mut ActionCx<'_>, attacker: ObjId, count: i32, frame: u64, shot_index: u32) {
    let core = cx.arb.env.core;
    let (skill, target, level, control, angle) = {
        let Some(m) = cx.mover(attacker) else { return };
        // `m_oaAct != OBJACT_USESKILL`: the action changed before the hit frame
        if m.combat.act.act != crate::combat::ObjAct::UseSkill {
            return;
        }
        let a = m.combat.act.p;
        (
            u32::try_from(a[0]).unwrap_or(0),
            ObjId(u32::try_from(a[1]).unwrap_or(NULL_ID)),
            u32::try_from(a[3]).unwrap_or(0),
            a[4] != 0,
            m.sim.angle,
        )
    };
    let Some((prop, add)) = props(core, skill, level) else {
        return;
    };
    // the target must still be a valid mover
    let Some(t) = cx.mover(target) else { return };
    if prop.skill_type == KT_MAGIC && prop.id == SI_ASS_HEAL_HEALING && is_die(t) {
        return;
    }
    let shoot = matches!(prop.exe_target, EXT_MAGICATKSHOT | EXT_MAGICSHOT);
    if !shoot {
        cx.q.skill_jobs.push(SkillJob::Hit {
            caster: attacker,
            target,
            skill,
            level,
            control,
            count,
            frame: SimFrame(frame),
        });
        return;
    }
    // a shot skill: the projectile (the damage is done where it arrives)
    let sfx = prop.sfx_obj[1];
    let hit_sfx = prop.sfx_obj[2];
    if let Some(kind) = kind_for_sfx(sfx) {
        spawn_shot(
            cx,
            ShotRequest {
                attacker,
                target,
                kind,
                sfx_id: sfx,
                hit_sfx_id: if hit_sfx == NULL_ID { 0 } else { hit_sfx },
                projectile_id: None,
                payload: HitPayload {
                    magic_power: add.skill_lvl,
                    skill_id: prop.id,
                    dmg_cnt: 0,
                    dmg_angle: angle,
                    dmg_power: if prop.dmg_shift != NULL_ID {
                        prop.dmg_shift as i32 as f32 * 0.2
                    } else {
                        0.0
                    },
                    atk_flags: if prop.skill_type == KT_SKILL {
                        AtkFlags::MELEESKILL
                    } else {
                        AtkFlags::MAGICSKILL
                    },
                    control,
                },
                shot_index,
            },
        );
    }
    // `ShootSkill` answers 1 on the server: the shot counts as a success
    let now = cx.arb.w.now;
    if prop.skill_type == KT_SKILL {
        if count == 0 {
            set_cooldown(cx.arb.w.store.mover_mut(attacker), prop, add, now);
        }
        return;
    }
    let mut out = Out::new();
    if let Some(m) = cx.arb.w.store.mover_mut(attacker) {
        pay_magic(m, core, add, &mut out);
        set_cooldown(Some(m), prop, add, now);
    }
    cx.arb.fx.extend_out(attacker, out);
}

/// `SetCoolTime(add)`: the slot of the skill is ready again `add.cooldown` ms from `now`; nothing when the level
/// row has no cool-down (`NULL_ID`) or the mover is not a player.
fn set_cooldown(m: Option<&mut Mover>, prop: &ItemProp, add: &AddSkillProp, now: GameMs) {
    if add.cooldown == NULL_ID {
        return;
    }
    let Some(m) = m else { return };
    let Some(slot) = slot_of(m, prop.id) else {
        return;
    };
    if let Some(p) = m.player_mut() {
        p.skills_rt.set_cool_time(slot, now, add.cooldown);
    }
}

/// The payment of a magic hit (`OnMagicSkill`): `mp -= req_mp / skill_count` (a burst pays its share per shot) and
/// `fp -= req_fp`, both reduced by the `DST_MP_DEC_RATE` / `DST_FP_DEC_RATE`; nothing when the MP requirement is
/// the "use all" sentinel (it emptied the pools at the start). A requirement of `NULL_ID` FP is skipped.
fn pay_magic(m: &mut Mover, core: &StatsCore, add: &AddSkillProp, out: &mut Out) {
    let dst = m.mv.dst.table.clone();
    let mut mp = req_mp(&dst, add.req_mp);
    if mp == REQ_ALL {
        return;
    }
    if add.skill_count > 1 {
        mp /= add.skill_count;
    }
    inc_mp(m, core, mp.wrapping_neg(), out);
    if add.req_fp != -1 {
        let fp = req_fp(&dst, add.req_fp);
        inc_fp(m, core, fp.wrapping_neg(), out);
    }
}

// ---- the end of a skill state ---------------------------------------------------------------------------------------

/// `OnActEndMeleeSkill` / `OnActEndMagicSkill` (`MoverActEvent.cpp:2108-2132`): the act parameters are cleared,
/// `MVRF_SKILL` goes, and a player whose action-slot chain is running asks for the next skill.
pub fn on_skill_end(cx: &mut ActionCx<'_>, id: ObjId) {
    let queued = {
        let Some(m) = cx.mover_mut(id) else { return };
        clear_act(m);
        m.sim.mvrf.remove(MVRF_SKILL);
        m.player()
            .is_some_and(|p| p.skills_rt.queue_cursor.is_some())
    };
    if queued {
        super::queue::queue_next(cx, id);
    }
}

/// `ClearActParam()` for the skill act: the game's act parameters and the sim's skill act.
pub fn clear_act(m: &mut Mover) {
    m.combat.act = ActParams::default();
    m.sim.skill.act = OBJACT_NONE;
}

/// A cast was cancelled (`ClearState` of Hero Drawing by a damage motion or a released target): the act is
/// forgotten like at the end of a state, but no chain continues.
pub fn on_cast_cancelled(cx: &mut ActionCx<'_>, id: ObjId) {
    if let Some(m) = cx.mover_mut(id) {
        if m.combat.act.act == crate::combat::ObjAct::UseSkill {
            clear_act(m);
        }
        m.sim.mvrf.remove(MVRF_SKILL);
    }
}

// ---- the service-context half ---------------------------------------------------------------------------------------

/// The most generations of jobs one drain runs (a proc that procs again is the next generation).
const MAX_ROUNDS: u32 = 16;

/// Runs the skill jobs the world phase queued (called by [`crate::combat::tick::finish`]): their effects start
/// at the first frame of the next tick, like the deaths'. The jobs run **a generation at a time**: what a job
/// queues (the proc of a proc) is the next generation, run in the next round, up to `MAX_ROUNDS` rounds (a
/// self-referencing skill chain cannot loop: the rest waits for the next drain). While the jobs run, a nested
/// [`drain_jobs`] does nothing - the calls a job makes (damage, buffs) queue behind it - so the work is a loop,
/// never a recursion.
pub fn process_pending(s: &mut Services, ctx: &mut GameCtx<'_>) {
    for _ in 0..MAX_ROUNDS {
        let jobs = std::mem::take(&mut ctx.worlds.movement.combat.queues.skill_jobs);
        if jobs.is_empty() {
            break;
        }
        ctx.worlds.movement.combat.queues.skill_depth += 1;
        for job in jobs {
            run_job(s, ctx, job);
        }
        ctx.worlds.movement.combat.queues.skill_depth -= 1;
    }
}

/// Runs the jobs a combat call queued right after the call (a client handler, a GM command, a skill
/// application): the procs, buff removals and casts of the damage pipeline happen inside the original call, so
/// they happen before the caller goes on. Nothing happens while a drain is already running (see
/// [`process_pending`]).
pub fn drain_jobs(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let q = &ctx.worlds.movement.combat.queues;
    if q.skill_depth == 0 && !q.skill_jobs.is_empty() {
        process_pending(s, ctx);
    }
}

fn run_job(s: &mut Services, ctx: &mut GameCtx<'_>, job: SkillJob) {
    match job {
        SkillJob::Hit {
            caster,
            target,
            skill,
            level,
            control,
            count,
            ..
        } => run_hit(s, ctx, caster, target, skill, level, control, count),
        SkillJob::Impact {
            attacker,
            target,
            skill,
            param,
            control,
        } => run_impact(s, ctx, attacker, target, skill, param, control),
        SkillJob::Active(a) => super::procs::run_active(s, ctx, &a),
        SkillJob::Counter {
            caster,
            skill,
            level,
            target,
        } => super::procs::run_counter(s, ctx, caster, skill, level, target),
        SkillJob::Monster(cast) => super::monster::apply_cast(s, ctx, &cast),
        SkillJob::Boss(fx) => crate::ai::boss::fx::run(s, ctx, fx),
        SkillJob::RemoveInvisible { mover } => super::procs::remove_invisible(s, ctx, mover),
        SkillJob::RemoveSkillBuff { mover, skill } => {
            crate::buff::api::remove_skill(s, ctx, mover, skill);
        }
        SkillJob::RemoveChrState { mover, bits } => {
            crate::buff::api::remove_flags(s, ctx, mover, crate::buff::RemoveFlags::CHRSTATE, bits);
        }
    }
}

/// `DoApplySkill(target, prop, add, bIgnoreProb = false, nParam, bOnlyDmg = false, bControl)` of a skill hit:
/// the area regions of [`super::area`] with the caster's control flag.
fn apply_hit(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    caster: ObjId,
    target: ObjId,
    a: &Applied<'_>,
    param: u32,
    control: bool,
) -> i32 {
    super::area::do_apply_skill(s, ctx, caster, target, a, false, param, false, control)
}

/// `AttackBySFX` of a skill projectile: the attacker's **current** level of the skill is applied
/// (`DoApplySkill`); an attacker that no longer has the skill, or whose target is gone, does nothing.
fn run_impact(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    attacker: ObjId,
    target: ObjId,
    skill: u32,
    param: u32,
    control: bool,
) {
    let core = s.stats.handle();
    let Some(m) = ctx.worlds.mover(attacker) else {
        return;
    };
    if ctx.worlds.mover(target).is_none() {
        return;
    }
    let Some(slot) = slot_of(m, skill) else {
        return;
    };
    let level = effective_level(m, &core, slot);
    let data = Arc::clone(ctx.data);
    let Some(a) = Applied::skill(&data, skill, level) else {
        return;
    };
    apply_hit(s, ctx, attacker, target, &a, param, control);
}

/// `OnMeleeSkill` / `OnMagicSkill` after the shot test, in the service context: apply, then pay and cool down.
#[allow(clippy::too_many_arguments)]
fn run_hit(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    caster: ObjId,
    target: ObjId,
    skill: u32,
    level: u32,
    control: bool,
    count: i32,
) {
    let core = s.stats.handle();
    if ctx.worlds.mover(caster).is_none() || ctx.worlds.mover(target).is_none() {
        return;
    }
    let data = Arc::clone(ctx.data);
    let Some(a) = Applied::skill(&data, skill, level) else {
        return;
    };
    let (prop, add) = (&*a.prop, a.add.as_deref());
    let Some(add) = add else { return };
    let n = apply_hit(s, ctx, caster, target, &a, 0, control);
    let now = ctx.clock.game_ms;
    if prop.skill_type == KT_SKILL {
        // a melee skill pays at the start; its first hit starts the cool-down
        if n != 0 && count == 0 {
            set_cooldown(ctx.worlds.mover_mut(caster), prop, add, now);
        }
        return;
    }
    // a magic skill: nothing is paid when the application failed
    if n == 0 {
        return;
    }
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.mover_mut(caster) {
        pay_magic(m, &core, add, &mut fx.out);
    }
    fx.flush_for(s, ctx, caster);
    match prop.id {
        SI_RIG_HERO_RETURN => hero_return(s, ctx, caster, target),
        SI_KNT_HERO_DRAWING => hero_drawing(ctx, caster, target),
        _ => {}
    }
    set_cooldown(ctx.worlds.mover_mut(caster), prop, add, now);
}

/// The buffs Hero Return casts on its target at their **maximum level** (`__RM_HERO_SKILL`), in this order.
pub const HERO_RETURN_BUFFS: [u32; 16] = [
    SI_ASS_HEAL_HEALING,
    SI_ASS_HEAL_PATIENCE,
    SI_ASS_CHEER_HEAPUP,
    SI_ASS_CHEER_CANNONBALL,
    SI_ASS_CHEER_MENTALSIGN,
    SI_ASS_CHEER_BEEFUP,
    SI_ASS_CHEER_QUICKSTEP,
    SI_ASS_CHEER_CATSREFLEX,
    SI_ASS_CHEER_ACCURACY,
    SI_ASS_CHEER_HASTE,
    SI_RIN_HEAL_HEALRAIN,
    SI_RIN_SUP_PROTECT,
    SI_RIN_SUP_HOLYGUARD,
    SI_RIN_SUP_SPIRITUREFORTUNE,
    SI_RIN_SQU_GEBURAHTIPHRETH,
    SI_RIG_MASTER_BLESSING,
];

/// `SI_RIG_HERO_RETURN` after a successful hit. With `features.rm_hero_skill`: a player target gets the sixteen
/// buffs of [`HERO_RETURN_BUFFS`] at their maximum level (`add = GetAddSkillProp(sub_define + expert_max - 1)`,
/// by row id), each announced with `DoApplyUseSkill`. Without it the caster teleports to its revival point.
fn hero_return(s: &mut Services, ctx: &mut GameCtx<'_>, caster: ObjId, target: ObjId) {
    if ctx.cfg.rules.features.rm_hero_skill {
        if !ctx.worlds.mover(target).is_some_and(Mover::is_player) {
            return;
        }
        let data = Arc::clone(ctx.data);
        for id in HERO_RETURN_BUFFS {
            let Some(prop) = data.skill(id) else { continue };
            let row = prop
                .sub_define
                .wrapping_add(prop.expert_max)
                .wrapping_sub(1);
            let Some(add) = data.add_skill_by_id(row) else {
                continue;
            };
            let a = Applied {
                prop: std::borrow::Cow::Borrowed(prop),
                add: Some(std::borrow::Cow::Borrowed(add)),
            };
            do_apply_skill(s, ctx, caster, target, &a, false, 0, false);
            announce(s, ctx, caster, target, id, prop.expert_max);
        }
        return;
    }
    // the vanilla skill: a teleport to the (chaotic) revival point, else back to the same place
    let Some(m) = ctx.worlds.mover(caster) else {
        return;
    };
    let (world, pos, layer, chaotic) = (
        m.core.world,
        m.core.pos,
        m.core.layer,
        crate::combat::is_chaotic(m),
    );
    let settings = ctx.worlds.world(world).map(|w| w.statics.settings.clone());
    let dest = settings
        .and_then(|st| ctx.worlds.revival.select(world, &st, pos, chaotic))
        .map(|p| (p.world, p.pos, LayerId(0)))
        .unwrap_or((world, pos, layer));
    let _ = ctx.worlds.replace(
        caster,
        dest.0,
        dest.1,
        dest.2,
        crate::world::ReplaceKind::Normal,
    );
}

/// `SI_KNT_HERO_DRAWING` after a successful hit: the target is set at the caster's position and pushed 0.2 along
/// the caster-to-target angle (`SetPos`, `AddSetPos`, `AddPushPower`, `ClearDestObj`).
fn hero_drawing(ctx: &mut GameCtx<'_>, caster: ObjId, target: ObjId) {
    let (Some(c), Some(t)) = (ctx.worlds.mover(caster), ctx.worlds.mover(target)) else {
        return;
    };
    let (cpos, tpos) = (c.sim.pos, t.sim.pos);
    let angle = flyff_sim::num::get_degree(tpos, cpos);
    let (world, layer, is_player) = (t.core.world, t.core.layer, t.is_player());
    if is_player {
        let _: Result<(), _> = ctx.worlds.replace(
            target,
            world,
            cpos,
            layer,
            crate::world::ReplaceKind::Normal,
        );
    } else if let Some(m) = ctx.worlds.mover_mut(target) {
        crate::movement::intents::warp_npc(m, cpos);
    }
    if let Some(m) = ctx.worlds.mover_mut(target) {
        crate::movement::intents::push(m, angle, 0.2);
        m.sim.clear_dest_obj();
    }
}
