//! The action slot (skill queue) and the action points (Plan/17 §6.13; WP-SKILL-09;
//! `src:_Interface/UserTaskBar.cpp:180-240`, `src:_Common/MoverMsg.cpp:172-423`, `MoverMove.cpp:285-360`,
//! `Mover.cpp:3494-3530`).
//!
//! The queue is the five `TaskbarDoc.queue` slots. The client starts a chain with
//! `UseSkillReq{slot of queue[0], SUT_QUEUESTART}`; the cast time then covers the whole queue
//! ([`super::use_skill::queue_casting_ticks`], 70 %). Every time a skill state ends, [`queue_next`]
//! (`CUserTaskBar::SetNextSkill`) advances the cursor, charges the action points of the **next** slot
//! (6 / 8 / 11 / 30 for slots 1..=4, free under `SM_ACTPOINT`), and either ends the chain (`EndSkillQueueNtf`) or
//! orders the next skill with [`cmd_set_use_skill`]: the server walks the caster to the target
//! (`SetDestObj(target, attack range)`) and, on arrival, tests the line of sight against **objects** at +0.5 m
//! and starts the skill as `SUT_QUEUEING` ([`on_arrival`]).
//!
//! * A blocked line **stalls** the chain: the player is told `TID_GAME_BLOCKTARGETING`, the command is dropped and
//!   the cursor stays where it is. The chain is then neither ended nor advanced until some other skill state
//!   ends and calls [`queue_next`] again (KEEP-QUIRK).
//! * Blinkpool (and Hero Return without `rm_hero_skill`) cannot be ordered by the server: the arrival drops them.
//! * An action-slot skill the sim refuses ends the chain.
//!
//! Action points (0..=100): +1 per landed player ATK1 hit and per wand shot with a charge of 1 or more (done by
//! the action layer), +2 per 3 s ([`process_action_point`], strictly more than 3000 ms, as the source tests), set by
//! the client's `SendToServerAp` clamped to `0..=100` (VR-004; the V15 client never sends it). The points live in
//! `SkillRuntime.action_points` and are saved in `TaskbarDoc.action_points`.

use super::SkillRuntime;
use super::use_skill::{SkillUseType, do_use_skill, effective_level, props};
use crate::combat::context::ActionCx;
use crate::combat::hit_type::hit_type;
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::sm::sm_active;
use crate::taskbar::{QueueStep, next_queue_step};
use flyff_common::defs::define::{
    EXT_AROUNDATK, EXT_MAGICATK, EXT_MAGICATKSHOT, EXT_MAGICSHOT, EXT_SELFCHGPARAMET, WUI_NOW,
    WUI_TARGETINGOBJ,
};
use flyff_common::defs::skill::{
    SI_ASS_HEAL_RESURRECTION, SI_MAG_MAG_BLINKPOOL, SI_RIG_HERO_RETURN,
};
use flyff_common::defs::text::TID_GAME_BLOCKTARGETING;
use flyff_common::{GameMs, ObjId};
use flyff_proto::pb;
use flyff_rules::combat::HitType;
use flyff_rules::consts::dst::DST_HAWKEYE_RATE;
use flyff_rules::consts::{NULL_ID, SM_ACTPOINT, STATE_BASEMOTION_MODE};
use flyff_sim::collision::client_query::{RAY_CTRL, RAY_STATIC};
use flyff_sim::types::{OBJACT_NONE, OBJACT_USESKILL, OBJSTA_DMG_FLY_ALL};

/// The most action points (`m_nActionPoint` is capped at 100).
pub const AP_MAX: i32 = 100;

/// The passive gain period: more than 3000 ms since the last gain (`(int)(g_tmCurrent - m_tmActionPoint) > 3000`).
pub const AP_PERIOD_MS: u64 = 3000;

/// The action points a landed hit adds (`AP += 1` below the cap): used by the action layer and the tests.
pub fn queue_ap_gain(rt: &mut SkillRuntime, n: i32) {
    rt.action_points = rt.action_points.saturating_add(n).clamp(0, AP_MAX);
}

/// What the order of a queued skill remembers until the player arrives (`m_oaCmd` and its three parameters):
/// the slot of the skill, the target and how it is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSkill {
    /// The job-skill slot (`nSkillIdx`).
    pub slot: usize,
    /// The target (`idTarget`).
    pub target: ObjId,
    /// `sutType`.
    pub sut: SkillUseType,
}

// ---- action points ------------------------------------------------------------------------------------------------

/// `CMover::ProcessActionPoint` for a live player (`Mover.cpp:3494-3530`): every time more than 3000 ms passed
/// since the last gain, `AP += (int)(elapsed / 3000.0 * 2.0)` up to 100 and the clock restarts. The first call
/// only starts the clock (the legacy constructor stamps the join time). Returns `true` when points were added.
pub fn process_action_point(m: &mut Mover, now: GameMs) -> bool {
    let Some(p) = m.player_mut() else {
        return false;
    };
    let rt = &mut p.skills_rt;
    let Some(last) = rt.ap_last else {
        rt.ap_last = Some(now);
        return false;
    };
    let elapsed = now.0.saturating_sub(last.0);
    if elapsed <= AP_PERIOD_MS {
        return false;
    }
    let t = (elapsed as f32 / 1000.0_f32) / 3.0_f32;
    rt.action_points = rt
        .action_points
        .saturating_add((t * 2.0_f32) as i32)
        .min(AP_MAX);
    rt.ap_last = Some(now);
    true
}

/// `SendToServerAp{ap}` (`OnActionPoint`, `DPSrvr.cpp:1627-1640`): negative is ignored; VR-004 caps the value at
/// 100 (the original stored any number).
pub fn on_send_to_server_ap(ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::SendToServerAp) {
    if m.action_point < 0 {
        return;
    }
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(Mover::player_mut) {
        p.skills_rt.action_points = m.action_point.min(AP_MAX);
    }
}

// ---- the chain ----------------------------------------------------------------------------------------------------

/// `CUserTaskBar::OnEndSkillQueue`: the cursor is reset, the player is told, the command is dropped.
pub fn end_queue(cx: &mut ActionCx<'_>, id: ObjId) {
    if let Some(m) = cx.mover_mut(id) {
        if let Some(p) = m.player_mut() {
            p.skills_rt.queue_cursor = None;
            p.skills_rt.pending = None;
        }
        // `ClearCmd()`
        m.sim.cmd.act = OBJACT_NONE;
    }
    cx.arb.fx.to_self(id, pb::EndSkillQueueNtf {});
}

/// `EndSkillQueueReq` (`OnEndSkillQueue`, `DPSrvr.cpp:7201`): always ends the chain and answers.
pub fn on_end_skill_queue_req(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let _ = crate::combat::with_action(s, ctx, who.0, |cx| end_queue(cx, who.0));
}

/// `CUserTaskBar::SetNextSkill(user)` (`UserTaskBar.cpp:200-235`). Returns `true` when the chain goes on (a skill
/// was ordered) and `false` when it ended.
pub fn queue_next(cx: &mut ActionCx<'_>, id: ObjId) -> bool {
    loop {
        let (step, ap_before, index) = {
            let Some(m) = cx.mover_mut(id) else {
                return false;
            };
            let sm = sm_active(m, SM_ACTPOINT);
            let Some(p) = m.player_mut() else {
                return false;
            };
            let next = p.skills_rt.queue_cursor.map_or(0, |c| usize::from(c) + 1);
            p.skills_rt.queue_cursor = u8::try_from(next).ok();
            let ap = p.skills_rt.action_points;
            (next_queue_step(&p.taskbar, next, ap, sm), ap, next)
        };
        let _ = (ap_before, index);
        match step {
            QueueStep::End => {
                end_queue(cx, id);
                return false;
            }
            QueueStep::Use { skill_id, ap_after } => {
                if let Some(p) = cx.mover_mut(id).and_then(Mover::player_mut) {
                    p.skills_rt.action_points = ap_after;
                }
                cx.arb.fx.to_self(
                    id,
                    pb::SetActionPoint {
                        action_point: ap_after,
                    },
                );
                let target = cx
                    .mover(id)
                    .and_then(|m| m.combat.id_set_target)
                    .unwrap_or(ObjId::NONE);
                if cmd_set_use_skill(cx, id, target, skill_id as usize, SkillUseType::Queueing) {
                    return true;
                }
                // the order failed: on to the next slot
            }
        }
    }
}

/// `CMover::CMD_SetUseSkill(target, slot, sut)` (`MoverMsg.cpp:172-423`, server branch): orders the skill of
/// `slot`: refused (`false`) while flying, attacking, jumping, in a knock-back flight, dead, in the base-motion
/// state or without a usable skill / target; otherwise the target is chosen by `use_chance` and `exe_target`, the
/// caster walks toward it (`SetDestObj(target, attack range)`, announced to the viewers **and the caster**) and the
/// command waits for the arrival ([`on_arrival`]).
pub fn cmd_set_use_skill(
    cx: &mut ActionCx<'_>,
    id: ObjId,
    target_in: ObjId,
    slot: usize,
    sut: SkillUseType,
) -> bool {
    let core = cx.arb.env.core;
    let Some(m) = cx.mover(id) else { return false };
    // `m_oaCmd = OBJACT_NONE`
    let refused = m.sim.act.flags.fly()
        || m.sim.act.state.is_act_attack()
        || m.sim.act.state.is_act_jump()
        || m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0
        || is_die(m);
    if let Some(m) = cx.mover_mut(id) {
        m.sim.cmd.act = OBJACT_NONE;
    }
    if refused {
        return false;
    }
    let Some(m) = cx.mover(id) else { return false };
    let Some(s) = m
        .player()
        .and_then(|p| p.progression.job_skills.get(slot))
        .filter(|s| !s.is_empty())
    else {
        return false;
    };
    let Some(prop) = core.data.skill(s.skill_id) else {
        return false;
    };
    if m.player()
        .is_some_and(|_| m.mv.state_mode & STATE_BASEMOTION_MODE != 0)
    {
        return false;
    }
    let hawkeye = m.mv.dst.table.param(DST_HAWKEYE_RATE, 0);
    let range = flyff_rules::combat::attack_range(prop.attack_range, hawkeye);
    let mut target = target_in;
    match prop.use_chance {
        WUI_NOW => target = id,
        WUI_TARGETINGOBJ => target = m.combat.id_set_target.unwrap_or(ObjId::NONE),
        _ => {}
    }
    // somebody else: hostile skills need an attackable target, a revival a dead player, everything else a live one
    if target != id
        && let Some(t) = cx.mover(target)
    {
        if prop.evildoing < 0 {
            let able = m.mv.disguise.is_none()
                && hit_type(m, t, cx.arb.env, &*cx.arb.hooks, true, 0) != HitType::Fail;
            if !able {
                return false;
            }
        }
        if prop.id == SI_ASS_HEAL_RESURRECTION {
            if !t.is_player() || !is_die(t) {
                return false;
            }
        } else if is_die(t) {
            return false;
        }
    }
    match prop.exe_target {
        EXT_SELFCHGPARAMET | EXT_AROUNDATK => target = id,
        EXT_MAGICATKSHOT | EXT_MAGICATK | EXT_MAGICSHOT => {
            if target == ObjId::NONE {
                return false;
            }
        }
        flyff_common::defs::define::EXT_ANOTHERWITH => {
            if target == ObjId::NONE {
                target = id;
            }
        }
        _ => {
            if target == ObjId::NONE {
                return false;
            }
        }
    }
    if let Some(m) = cx.mover_mut(id) {
        crate::movement::intents::set_player_dest_obj(m, target, range);
        // `ClearActParam()`; `SetCmd(OBJACT_USESKILL, slot, target, sut)`
        super::cast::clear_act(m);
        m.sim.cmd.act = OBJACT_USESKILL;
        m.sim.cmd.params = [slot as i32, target.0 as i32, sut.wire()];
        if let Some(p) = m.player_mut() {
            p.skills_rt.pending = Some(PendingSkill { slot, target, sut });
        }
    }
    true
}

/// `ProcessMoveArrival` of a queued skill (`MoverMove.cpp:296-346`, server branch): the player reached the
/// target of its `OBJACT_USESKILL` command. Not for a target that is no mover; Blinkpool (and Hero Return
/// without `rm_hero_skill`) are dropped; a line of sight blocked by an **object** (no terrain) at +0.5 m tells
/// `TID_GAME_BLOCKTARGETING` and stalls the chain; otherwise the skill starts as `SUT_QUEUEING` and a refusal ends
/// the chain.
pub fn on_arrival(cx: &mut ActionCx<'_>, id: ObjId, reached: ObjId) {
    let act = cx.act;
    let Some(pending) = cx
        .mover_mut(id)
        .and_then(Mover::player_mut)
        .and_then(|p| p.skills_rt.pending.take())
    else {
        return;
    };
    let Some(target_pos) = cx.mover(reached).map(|t| t.sim.pos) else {
        return;
    };
    let Some(m) = cx.mover(id) else { return };
    let Some(skill_id) = m
        .player()
        .and_then(|p| p.progression.job_skills.get(pending.slot))
        .map(|s| s.skill_id)
    else {
        return;
    };
    if skill_id == SI_MAG_MAG_BLINKPOOL || (!act.rm_hero_skill && skill_id == SI_RIG_HERO_RETURN) {
        return;
    }
    // the obstacle test between the caster and the target (objects only)
    let mut from = m.sim.pos;
    from.y += 0.5;
    let mut to = target_pos;
    to.y += 0.5;
    let d = to - from;
    let len = flyff_sim::num::length(d);
    if len > 0.0 {
        let dir = flyff_sim::num::normalize_or_zero(d);
        let blocked = cx
            .sc
            .collision
            .ray_cast(cx.sc.dynamic, from, dir, len, RAY_STATIC | RAY_CTRL)
            .is_some();
        if blocked {
            cx.arb.fx.to_self(
                id,
                crate::chat::ingress::notice::defined_text(TID_GAME_BLOCKTARGETING, Vec::new()),
            );
            return;
        }
    }
    if do_use_skill(cx, id, pending.slot, pending.target, pending.sut, false).is_err() {
        end_queue(cx, id);
    }
    let _ = (effective_level, props, NULL_ID);
}
