//! Skill points: spending (VR-086), the legend skill-up and the skill messages
//! (Plan/17 §6.2; Plan/16 §12; WP-SKILL-10; `CDPSrvr::OnDoUseSkillPoint` `DPSrvr.cpp:3299-3362`,
//! `OnLegendSkillStart` `DPSrvr.cpp:10506-10580`, `CMover::SetMasterSkillPointUp` `MoverSkill.cpp:60-90`,
//! `CMover::InitSkillExp` `Mover.cpp:1904-1949`).
//!
//! * [`on_do_use_skill_point`] answers `DoUseSkillPointReq{45 x (skill, level)}`. The rule is **not** restated
//!   here: it is [`flyff_rules::skill_points::validate_skill_plan`] over the server's own table (the same
//!   function the browser's skill tree runs, TQ-16-05). `NothingToDo` is ignored; any other refusal is
//!   `DefinedText{TID_RESKILLPOINT_ERROR}` and nothing changes; an accepted plan deducts exactly the returned
//!   cost, sets the levels, plays `XI_SYS_EXCHAN01` for the viewers and answers `DoUseSkillPointNtf` built from
//!   the **server's** table (the original echoed the request).
//! * [`on_legend_skill_up_start`] is the hero skill-up with five gems: a hero only, refused with
//!   `LegendSkillUpResult{-1}` when a hero skill is already above level 4, the five gems must be the Diamond,
//!   Emerald, Sapphire, Ruby and Topaz, in this order, each usable in the inventory (else silence); one of
//!   each is consumed in any case, and the roll `x_random(1000) > 766` (23.3 %) raises every hero skill by one.
//! * The master growth (`SetMasterSkillPointUp`), the skill reset (`InitSkillExp`) and the GM level setter live in
//!   `stats::skills` and `gm::cmd::progress`, which the level-up, the reset scroll and `/skilllevel` call; this
//!   module does not repeat them (B6 integration: the parallel copies were removed).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{RemoveReason, remove_item};
use crate::mover::Mover;
use crate::player::JobSkill;
use flyff_common::WorldId;
use flyff_common::defs::item::{
    II_GEN_MAT_DIAMOND, II_GEN_MAT_EMERALD, II_GEN_MAT_RUBY, II_GEN_MAT_SAPPHIRE, II_GEN_MAT_TOPAZ,
};
use flyff_common::defs::obj::XI_SYS_EXCHAN01;
use flyff_common::defs::text::TID_RESKILLPOINT_ERROR;
use flyff_proto::pb;
use flyff_rules::consts::job::JTYPE_HERO;
use flyff_rules::jobs::{EMPTY_SKILLS, JobSkill as RulesSkill, SkillTable};
use flyff_rules::skill_points::{
    LegendUpRefusal, SkillPlanError, legend_up_check, legend_up_roll, validate_skill_plan,
};
use serde_json::json;

/// The gems of the legend skill-up, in the order the request must list them.
pub const LEGEND_GEMS: [u32; 5] = [
    II_GEN_MAT_DIAMOND,
    II_GEN_MAT_EMERALD,
    II_GEN_MAT_SAPPHIRE,
    II_GEN_MAT_RUBY,
    II_GEN_MAT_TOPAZ,
];

/// The 45 slots of a player as the rules see them.
pub fn skill_table(m: &Mover) -> SkillTable {
    let mut t = EMPTY_SKILLS;
    if let Some(p) = m.player() {
        for (dst, src) in t.iter_mut().zip(p.progression.job_skills.iter()) {
            *dst = RulesSkill {
                skill_id: src.skill_id,
                level: src.level,
            };
        }
    }
    t
}

/// Writes a rules table back into the player's slots.
fn store_table(m: &mut Mover, t: &SkillTable) {
    if let Some(p) = m.player_mut() {
        for (dst, src) in p.progression.job_skills.iter_mut().zip(t.iter()) {
            *dst = JobSkill {
                skill_id: src.skill_id,
                level: src.level,
            };
        }
    }
}

/// `AddDoUseSkillPoint(&m_aJobSkill[0], m_nSkillPoint)`: the table the server holds and the unspent points.
pub fn do_use_skill_point_ntf(m: &Mover) -> pb::DoUseSkillPointNtf {
    let (skills, sp) = match m.player() {
        Some(p) => (
            p.progression
                .job_skills
                .iter()
                .map(|s| pb::Skill {
                    skill_id: s.skill_id,
                    level: s.level,
                })
                .collect(),
            p.progression.skill_points,
        ),
        None => (Vec::new(), 0),
    };
    pb::DoUseSkillPointNtf {
        skills,
        skill_point: sp,
    }
}

/// `CreateSfxObj(XI_SYS_EXCHAN01)` at the player (everybody around, the player included).
fn exchange_sfx(m: &Mover) -> pb::CreateSfxObj {
    pb::CreateSfxObj {
        sfx_id: XI_SYS_EXCHAN01,
        x: m.core.pos.x,
        y: m.core.pos.y,
        z: m.core.pos.z,
        flag: false,
    }
}

/// `DoUseSkillPointReq` (`OnDoUseSkillPoint`): see the module documentation. A request that does not carry
/// exactly 45 entries is dropped.
pub fn on_do_use_skill_point(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::DoUseSkillPointReq,
) {
    let _ = use_skill_points(s, ctx, who, &req);
}

/// What [`use_skill_points`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointsOutcome {
    /// The plan was applied: the points it cost.
    Applied(i32),
    /// The rule refused the plan (`DefinedText{TID_RESKILLPOINT_ERROR}` unless it was `NothingToDo`).
    Refused(SkillPlanError),
    /// The request did not have 45 entries, or the player is gone.
    Dropped,
}

/// [`on_do_use_skill_point`] with the outcome (tests, GM tools).
pub fn use_skill_points(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: &pb::DoUseSkillPointReq,
) -> PointsOutcome {
    let me = who.0;
    if req.skills.len() != crate::skill::SLOTS {
        return PointsOutcome::Dropped;
    }
    let core = s.stats.handle();
    let Some(m) = ctx.worlds.player(me) else {
        return PointsOutcome::Dropped;
    };
    let cur = skill_table(m);
    let mut plan = EMPTY_SKILLS;
    for (slot, sk) in plan.iter_mut().zip(req.skills.iter()) {
        *slot = RulesSkill {
            skill_id: sk.skill_id,
            level: sk.level,
        };
    }
    let (level, job, sp) = (
        m.mv.level,
        m.mv.job,
        m.player().map_or(0, |p| p.progression.skill_points),
    );
    match validate_skill_plan(&cur, &plan, level, job, sp, &core.data.skills, &core.rules) {
        Err(SkillPlanError::NothingToDo) => PointsOutcome::Refused(SkillPlanError::NothingToDo),
        Err(e) => {
            let mut fx = Fx::new();
            fx.defined(TID_RESKILLPOINT_ERROR, Vec::new());
            fx.flush_for(s, ctx, me);
            PointsOutcome::Refused(e)
        }
        Ok(cost) => {
            let Some(m) = ctx.worlds.player_mut(me) else {
                return PointsOutcome::Dropped;
            };
            // the levels of the plan; the skill ids are the server's (the plan was checked slot by slot)
            let mut next = cur;
            for (slot, want) in next.iter_mut().zip(plan.iter()) {
                if !slot.is_empty() {
                    slot.level = want.level;
                }
            }
            store_table(m, &next);
            if let Some(p) = m.player_mut() {
                p.progression.skill_points = p.progression.skill_points.wrapping_sub(cost);
            }
            let mut fx = Fx::new();
            fx.out.near(exchange_sfx(m));
            fx.out.to_self(do_use_skill_point_ntf(m));
            fx.audit(
                flyff_db::audit::ev::SKILL_POINTS_SPEND,
                json!({ "cost": cost, "points_left": m.player().map_or(0, |p| p.progression.skill_points) }),
            );
            fx.flush_for(s, ctx, me);
            PointsOutcome::Applied(cost)
        }
    }
}

/// The outcome of `LegendSkillUpStart` (the result code goes to the client when there is one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendOutcome {
    /// Silent refusal: not a hero, a gem missing or unusable.
    Ignored,
    /// `LegendSkillUpResult{-1}`: a hero skill is already above level 4.
    AtMaximum,
    /// The gems were consumed and the roll failed (`LegendSkillUpResult{0}`).
    Failed,
    /// The gems were consumed and every hero skill rose by one (`LegendSkillUpResult{1}`).
    Succeeded,
}

/// `LegendSkillUpStart` (`OnLegendSkillStart`): see the module documentation.
pub fn on_legend_skill_up_start(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::LegendSkillUpStart,
) {
    let _ = legend_skill_up(s, ctx, who, &req);
}

/// [`on_legend_skill_up_start`] with the outcome.
pub fn legend_skill_up(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: &pb::LegendSkillUpStart,
) -> LegendOutcome {
    let me = who.0;
    let core = s.stats.handle();
    if req.item_obj_ids.len() != LEGEND_GEMS.len() {
        return LegendOutcome::Ignored;
    }
    let Some(m) = ctx.worlds.player(me) else {
        return LegendOutcome::Ignored;
    };
    let cur = skill_table(m);
    match legend_up_check(&core.rules, m.mv.job, &cur, &core.data.skills) {
        Err(LegendUpRefusal::NotHero) => return LegendOutcome::Ignored,
        Err(LegendUpRefusal::AtMaximum) => {
            let mut fx = Fx::new();
            fx.out.to_self(pb::LegendSkillUpResult { result: -1 });
            fx.flush_for(s, ctx, me);
            return LegendOutcome::AtMaximum;
        }
        Ok(()) => {}
    }
    // the five gems, in order, owned and usable
    let mut ids = [0 as StorageId; 5];
    for (i, (slot, wire)) in ids.iter_mut().zip(req.item_obj_ids.iter()).enumerate() {
        let Ok(id) = StorageId::try_from(*wire) else {
            return LegendOutcome::Ignored;
        };
        let ok = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(id))
            .is_some_and(|it| it.is_usable() && it.item_id == LEGEND_GEMS[i]);
        if !ok {
            return LegendOutcome::Ignored;
        }
        *slot = id;
    }
    // all gems match: one of each is used up, whatever the roll says
    for id in ids {
        remove_item(s, ctx, who, id, 1, RemoveReason::Other("legend_skill"));
    }
    let world: WorldId = ctx.worlds.core(me).map(|c| c.world).unwrap_or_default();
    let success = legend_up_roll(ctx.rng.world(world));
    let mut fx = Fx::new();
    if success {
        let Some(m) = ctx.worlds.player_mut(me) else {
            return LegendOutcome::Ignored;
        };
        let mut raised = 0_u32;
        let mut next = cur;
        for slot in next.iter_mut().filter(|sl| !sl.is_empty()) {
            if core
                .data
                .skill(slot.skill_id)
                .is_some_and(|p| p.item_kind1 == JTYPE_HERO)
            {
                slot.level += 1;
                raised += 1;
            }
        }
        store_table(m, &next);
        fx.out.near(exchange_sfx(m));
        fx.out.to_self(do_use_skill_point_ntf(m));
        fx.audit(
            flyff_db::audit::ev::SKILL_POINTS_SPEND,
            json!({ "legend_skill_up": true, "skills": raised }),
        );
    }
    fx.out.to_self(pb::LegendSkillUpResult {
        result: i32::from(success),
    });
    fx.flush_for(s, ctx, me);
    if success {
        LegendOutcome::Succeeded
    } else {
        LegendOutcome::Failed
    }
}
