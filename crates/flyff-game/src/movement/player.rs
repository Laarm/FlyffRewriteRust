//! The handlers of the client's movement messages (Plan/18 §4; WP-AI-02, WP-AI-03).
//!
//! Player movement is client-authoritative (D1, D4): the client sends timestamped states, the server adopts
//! them, re-simulates its own copy to the present and relays them to the observers. The **only** plausibility
//! check is the original 1 km jump guard (VR-080, KQ-28); there is no speed, collision or line-of-sight
//! validation (§4.8). All handlers run in phase 1 of tick `n`, where `now = 4n`: no frame of the tick has been
//! simulated yet, so a mover that was fast-forwarded to `now` stands at "the start of frame `4n`" and the AI
//! pass and the frame loop that follow continue from there.

use super::{
    FrameLookup, IgnoreReason, IntentAudience, PLAYER_STAMP_MAX_AGE, PendingIntent, dist_sq,
    intents,
};
use crate::ctx::{GameCtx, PlayerRef};
use crate::outbox::Audience;
use flyff_common::math::Vec3;
use flyff_common::{ObjId, SessionId, SimFrame};
use flyff_proto::pb;
use flyff_sim::ActMsg;
use flyff_sim::act::half_force_set;
use flyff_sim::step::{EventMode, NoRules, SimEnv, SimRole};
use flyff_sim::sync::{
    FlyFields, IntentOp, IntentStampData, StateUpdate, apply_intent, apply_player_pose,
    apply_player_state, fast_forward,
};
use flyff_sim::types::{
    ACT_OK, OBJMSG_SITDOWN, OBJMSG_STANDUP, OBJSTA_ATK_ALL, OBJSTAF_FLY, ObjState, StateFlags,
};

/// The bit test of the `BEHAVIOR` rule: `state & 0x000F0000 != 0` (`CASTING1|CASTING2|MAGIC1|RANGE1|
/// MELEESKILL|RANGESKILL` OR to this value, so ATK1..4 and every other sub-state `0x1..0xF` hit it, SP_ATK
/// does not; ref/07a §11.2).
pub const ATK_BUSY_MASK: u32 = 0x000F_0000;

fn finite3(v: Option<pb::Vec3>) -> Option<Vec3> {
    let v = v.map_or(Vec3::ZERO, |v| Vec3::new(v.x, v.y, v.z));
    (v.x.is_finite() && v.y.is_finite() && v.z.is_finite()).then_some(v)
}

/// The frame a client stamp stands for: `client_frame + offset` clamped into `[now - 37, now]`
/// (`PLAYER_STAMP_MAX_AGE`, KQ-28).
pub fn convert_stamp(client_frame: u32, offset: i64, now: SimFrame) -> SimFrame {
    let lo = now.0.saturating_sub(PLAYER_STAMP_MAX_AGE);
    let raw = i64::from(client_frame).saturating_add(offset);
    let lo_i = i64::try_from(lo).unwrap_or(i64::MAX);
    let hi_i = i64::try_from(now.0).unwrap_or(i64::MAX);
    SimFrame(u64::try_from(raw.clamp(lo_i, hi_i)).unwrap_or(lo))
}

/// What the matrix of 18 §4.1 decides for an accepted `PlayerMoveState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovePlan {
    /// `action_force_set`, guarded; `corr` also clears the destination.
    Ground {
        /// `reason == CORR`.
        corr: bool,
    },
    /// `action_force_set2`, no guard; `corr` also clears the destination.
    Fly {
        /// `reason == CORR`.
        corr: bool,
    },
    /// `half_force_set` (flying steering).
    Angle,
}

/// The acceptance matrix of 18 §4.1 (VR-081): which variant may be applied given the message's `reason`, its
/// fly flag and the state of the server copy; `Err` names the ignore reason. `atk_busy` (the server copy has
/// an attack or cast in progress) ignores a `Behavior` report; `Moved` / `Periodic` / `Corr` are accepted but
/// applied as a pose only ([`apply_player_pose`], B7).
pub fn plan_for(
    reason: pb::MoveReason,
    variant_fly: bool,
    server_fly: bool,
    atk_busy: bool,
) -> Result<MovePlan, IgnoreReason> {
    use pb::MoveReason as R;
    let corr = reason == R::Corr;
    let ground_or_fly = |corr: bool| {
        if variant_fly {
            if server_fly {
                Ok(MovePlan::Fly { corr })
            } else {
                Err(IgnoreReason::FlyMismatch)
            }
        } else if !server_fly {
            Ok(MovePlan::Ground { corr })
        } else {
            Err(IgnoreReason::FlyMismatch)
        }
    };
    match reason {
        R::Unspecified => Err(IgnoreReason::Unspecified),
        R::Moved | R::Periodic | R::Corr => ground_or_fly(corr),
        R::Behavior => match ground_or_fly(false)? {
            MovePlan::Ground { .. } if atk_busy => Err(IgnoreReason::AttackBusy),
            plan => Ok(plan),
        },
        R::Angle => {
            if server_fly && variant_fly {
                Ok(MovePlan::Angle)
            } else {
                Err(IgnoreReason::FlyMismatch)
            }
        }
    }
}

/// The wire state as the sim's [`StateUpdate`].
fn state_update(m: &pb::PlayerMoveState, pos: Vec3, delta: Vec3) -> StateUpdate {
    StateUpdate {
        pos,
        delta,
        angle: m.angle,
        state: ObjState(m.state),
        flags: StateFlags(m.state_flags),
        motion: m.motion,
        motion_ex: m.motion_ex,
        loop_: u32::try_from(m.r#loop).unwrap_or(0),
        motion_option: m.motion_option,
        motion_frame: Some(m.motion_frame),
        fly: m.fly.map(|f| FlyFields {
            angle_x: f.angle_x,
            acc_power: f.acc_power,
            turn_angle: f.turn_angle,
        }),
    }
}

/// `PlayerMoveState` (18 §4.2): the acceptance matrix, the 1 km guard on the ground variants, the stamp
/// conversion, the forced set of the state at `S`, the fast-forward to `now` and the relay of the reported
/// state to the observers (`NearExclSelf`) with `server_frame = S`.
pub fn on_player_move_state(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    session: SessionId,
    msg: pb::PlayerMoveState,
) {
    let id = who.0;
    let now = ctx.clock.frame0;
    let reason = pb::MoveReason::try_from(msg.reason).unwrap_or(pb::MoveReason::Unspecified);
    let shared = ctx.worlds.movement.shared.clone();
    let offset = ctx.sessions.get(id).map_or(0, |e| e.clock_offset_frames);
    let Some(statics) = ctx.worlds.statics_of(id).cloned() else {
        return;
    };
    let ignore = |ctx: &mut GameCtx<'_>, why: IgnoreReason| {
        ctx.worlds.movement.stats.note_ignored(why);
    };

    // 1. preconditions, in the order of 18 §4.1: dead, finite, stale
    let Some(m) = ctx.worlds.player(id) else {
        return;
    };
    if m.sim.act.state.is_die() {
        return ignore(ctx, IgnoreReason::Dead);
    }
    let (Some(pos), Some(delta)) = (finite3(msg.pos), finite3(msg.delta)) else {
        return ignore(ctx, IgnoreReason::NonFinite);
    };
    let fly_ok = msg.fly.is_none_or(|f| {
        f.angle_x.is_finite() && f.acc_power.is_finite() && f.turn_angle.is_finite()
    });
    if !msg.angle.is_finite() || !msg.motion_frame.is_finite() || !fly_ok {
        return ignore(ctx, IgnoreReason::NonFinite);
    }
    if let Some((s, last)) = m.sync.last_seq
        && s == session.0
        && msg.seq <= last
    {
        return ignore(ctx, IgnoreReason::Stale);
    }

    // 2. the matrix
    let variant_fly = msg.state_flags & OBJSTAF_FLY != 0
        || (reason == pb::MoveReason::Angle && msg.fly.is_some());
    let server_fly = m.sim.act.flags.fly();
    let atk_busy = m.sim.act.state.bits() & ATK_BUSY_MASK != 0;
    let plan = match plan_for(reason, variant_fly, server_fly, atk_busy) {
        Ok(p) => p,
        Err(why) => return ignore(ctx, why),
    };
    // the 1 km guard (VR-080): ground variants only, against the server copy's position before the message
    if matches!(plan, MovePlan::Ground { .. }) && dist_sq(m.sim.pos, pos) > shared.guard_dist_sq {
        return ignore(ctx, IgnoreReason::Guard);
    }

    // 3. the stamp, the forced set and the fast-forward
    let stamp = convert_stamp(msg.client_frame, offset, now);
    let upd = state_update(&msg, pos, delta);
    let dest = m.sim.dest.obj;
    let world = m.core.world;
    let dynamic = ctx.worlds.dynamic_of(world, m.core.layer);
    let lookup = FrameLookup::for_objects(ctx.worlds.store(), &[dest], &shared.flags);
    {
        let rng = ctx.rng.world(world);
        let Some(m) = ctx.worlds.player_mut(id) else {
            return;
        };
        let env = SimEnv {
            frame: now,
            role: SimRole::Server,
            flags: shared.flags,
            world: &statics.collision,
            dynamic: &dynamic,
            motions: &shared.motions,
            lookup: &lookup,
            mode: EventMode::Normal,
        };
        let mut rules = NoRules;
        match plan {
            // A ground report over a server copy with an attack or a cast in progress (the `BEHAVIOR` rule of
            // `plan_for`, applied to `Moved` / `Periodic` / `Corr` the way `apply_dest_request` applies it):
            // the client authors the position and the yaw, the attack group, the motion and the velocity stay
            // the server's, nothing is replayed. A stale `Moved` of the browser (its report of the command is
            // sent a frame after the `UseSkillReq`) used to replace the casting state by `STAND`.
            MovePlan::Ground { corr } if atk_busy => apply_player_pose(&mut m.sim, &upd, corr),
            MovePlan::Ground { corr } | MovePlan::Fly { corr } => apply_player_state(
                &mut m.sim,
                &env,
                &upd,
                corr,
                stamp,
                &mut rules,
                rng,
                &mut m.sync.events,
            ),
            MovePlan::Angle => {
                half_force_set(&mut m.sim, &upd);
                let k = u32::try_from(now.0.saturating_sub(stamp.0)).unwrap_or(0);
                fast_forward(&mut m.sim, &env, k, &mut rules, rng, &mut m.sync.events);
            }
        }
        m.sync.last_seq = Some((session.0, msg.seq));
    }
    if let Some(n) = ctx.worlds.movement.stats.states.get_mut(reason as usize) {
        *n += 1;
    }

    // 4. the relay: the REPORTED values, `server_frame = S`
    let ntf = pb::MoverStateNtf {
        reason: msg.reason,
        pos: msg.pos,
        delta: msg.delta,
        angle: msg.angle,
        state: msg.state,
        state_flags: msg.state_flags,
        motion: msg.motion,
        motion_ex: msg.motion_ex,
        r#loop: msg.r#loop,
        motion_option: msg.motion_option,
        motion_frame: msg.motion_frame,
        server_frame: stamp.0,
        fly: msg.fly,
    };
    ctx.broadcast(None, id, Audience::NearExclSelf, ntf);
}

/// The common start of the destination requests: the dead and non-finite checks of `start`, the guard on the
/// start position, the stamp. Returns the stamp data of the client's start state and its frame.
fn start_stamp(
    ctx: &mut GameCtx<'_>,
    id: ObjId,
    start: Option<pb::ClientStamp>,
    dest: Option<Vec3>,
    guard_dist_sq: f32,
) -> Result<(IntentStampData, SimFrame), IgnoreReason> {
    let Some(m) = ctx.worlds.player(id) else {
        return Err(IgnoreReason::Dead);
    };
    if m.sim.act.state.is_die() {
        return Err(IgnoreReason::Dead);
    }
    let Some(start) = start else {
        return Err(IgnoreReason::NonFinite);
    };
    let Some(start_pos) = finite3(start.pos) else {
        return Err(IgnoreReason::NonFinite);
    };
    if !start.angle.is_finite() {
        return Err(IgnoreReason::NonFinite);
    }
    // both guards of the original `DESTPOS` handler and 11 §9.3: the destination and the start state
    if let Some(d) = dest
        && dist_sq(m.sim.pos, d) > guard_dist_sq
    {
        return Err(IgnoreReason::Guard);
    }
    if dist_sq(m.sim.pos, start_pos) > guard_dist_sq {
        return Err(IgnoreReason::Guard);
    }
    let offset = ctx.sessions.get(id).map_or(0, |e| e.clock_offset_frames);
    let stamp = convert_stamp(start.client_frame, offset, ctx.clock.frame0);
    Ok((
        IntentStampData {
            pos: start_pos,
            angle: start.angle,
            state: ObjState(start.state),
            flags: StateFlags(start.state_flags),
            angle_x: 0.0,
        },
        stamp,
    ))
}

/// Applies the intent `op` of a click-to-move request: the client's start state at `stamp`, the operation, the
/// fast-forward to `now` (`sync::apply_intent`), then relays the wire message built by `relay` to the observers.
fn apply_dest_request(
    ctx: &mut GameCtx<'_>,
    id: ObjId,
    s: IntentStampData,
    stamp: SimFrame,
    op: IntentOp,
) -> bool {
    let now = ctx.clock.frame0;
    let shared = ctx.worlds.movement.shared.clone();
    let Some(statics) = ctx.worlds.statics_of(id).cloned() else {
        return false;
    };
    let target = match op {
        IntentOp::DestObj { target, .. } => target,
        _ => ObjId::NONE,
    };
    let (world, layer) = ctx
        .worlds
        .core(id)
        .map(|c| (c.world, c.layer))
        .unwrap_or_default();
    let dynamic = ctx.worlds.dynamic_of(world, layer);
    let lookup = FrameLookup::for_objects(ctx.worlds.store(), &[target], &shared.flags);
    let rng = ctx.rng.world(world);
    let Some(m) = ctx.worlds.player_mut(id) else {
        return false;
    };
    let env = SimEnv {
        frame: now,
        role: SimRole::Server,
        flags: shared.flags,
        world: &statics.collision,
        dynamic: &dynamic,
        motions: &shared.motions,
        lookup: &lookup,
        mode: EventMode::Normal,
    };
    // The stamp is the CLIENT's start state, taken before the server handled anything else of this tick. A cast or a swing the server
    // copy started meanwhile (the browser sends `UseSkillReq` at once and the movement report of the same command a frame later, B6
    // integration: Fire Strike never hit because this request replaced the casting state by STAND) belongs to the server: its attack
    // group stays, and the intent is applied without the replay of the stamped frames (a fast-forward would run the attack in the past).
    let (s, lag) = if m.sim.act.state.bits() & ATK_BUSY_MASK != 0 {
        let keep = m.sim.act.state.bits() & OBJSTA_ATK_ALL;
        let mut kept = s;
        kept.state = ObjState((s.state.bits() & !OBJSTA_ATK_ALL) | keep);
        (kept, 0)
    } else {
        (s, u32::try_from(now.0.saturating_sub(stamp.0)).unwrap_or(0))
    };
    apply_intent(
        &mut m.sim,
        &env,
        &s,
        op,
        lag,
        &mut NoRules,
        rng,
        &mut m.sync.events,
    );
    true
}

fn intent_stamp_of(s: &IntentStampData, stamp: SimFrame) -> pb::IntentStamp {
    pb::IntentStamp {
        server_frame: stamp.0,
        start_pos: Some(intents::pb_vec(s.pos)),
        start_angle: s.angle,
        start_state: s.state.bits(),
        start_state_flags: s.flags.bits(),
        start_angle_x: 0.0,
    }
}

/// `DestPosReq` (18 §4.3): click-to-move. Both 1 km guards (the destination and the start state), the stamp,
/// the client's start state at `S`, `set_dest_pos`, the fast-forward, then `DestPosNtf` to the observers with
/// the `IntentStamp` of the client's start state.
pub fn on_dest_pos_req(ctx: &mut GameCtx<'_>, who: PlayerRef, req: pb::DestPosReq) {
    let id = who.0;
    let guard = ctx.worlds.movement.shared.guard_dist_sq;
    let Some(pos) = finite3(req.pos) else {
        return ctx
            .worlds
            .movement
            .stats
            .note_ignored(IgnoreReason::NonFinite);
    };
    let (s, stamp) = match start_stamp(ctx, id, req.start, Some(pos), guard) {
        Ok(v) => v,
        Err(why) => return ctx.worlds.movement.stats.note_ignored(why),
    };
    if !apply_dest_request(
        ctx,
        id,
        s,
        stamp,
        IntentOp::DestPos {
            pos,
            forward: req.forward,
        },
    ) {
        return;
    }
    ctx.worlds.movement.stats.dest_pos_reqs += 1;
    let ntf = pb::DestPosNtf {
        pos: req.pos,
        forward: req.forward,
        stamp: Some(intent_stamp_of(&s, stamp)),
    };
    ctx.broadcast(None, id, Audience::NearExclSelf, ntf);
}

/// `SetDestObjReq` (18 §4.3): walk to an object. The target must be a live object; an identical consecutive
/// target is ignored without a relay (`__TRAFIC_1222`, F-117); then as [`on_dest_pos_req`] with
/// `SetDestObjNtf`.
pub fn on_set_dest_obj_req(ctx: &mut GameCtx<'_>, who: PlayerRef, req: pb::SetDestObjReq) {
    let id = who.0;
    let guard = ctx.worlds.movement.shared.guard_dist_sq;
    let target = ObjId(req.target_obj_id);
    if !req.range.is_finite() {
        return ctx
            .worlds
            .movement
            .stats
            .note_ignored(IgnoreReason::NonFinite);
    }
    if ctx.worlds.core(target).is_none_or(|c| c.deleted) {
        return ctx
            .worlds
            .movement
            .stats
            .note_ignored(IgnoreReason::NoTarget);
    }
    if ctx
        .worlds
        .player(id)
        .is_some_and(|m| m.sim.dest.obj == target)
    {
        return ctx
            .worlds
            .movement
            .stats
            .note_ignored(IgnoreReason::Duplicate);
    }
    let (s, stamp) = match start_stamp(ctx, id, req.start, None, guard) {
        Ok(v) => v,
        Err(why) => return ctx.worlds.movement.stats.note_ignored(why),
    };
    if !apply_dest_request(
        ctx,
        id,
        s,
        stamp,
        IntentOp::DestObj {
            target,
            range: req.range,
        },
    ) {
        return;
    }
    ctx.worlds.movement.stats.dest_obj_reqs += 1;
    let ntf = pb::SetDestObjNtf {
        target_obj_id: req.target_obj_id,
        range: req.range,
        stamp: Some(intent_stamp_of(&s, stamp)),
    };
    ctx.broadcast(None, id, Audience::NearExclSelf, ntf);
}

/// `MotionReq` (18 §4.4): sit down or stand up. Only `OBJMSG_SITDOWN` (4) and `OBJMSG_STANDUP` (5) are
/// accepted (VR-004); anything else, and a request the state machine refuses, is answered with
/// `MotionNtf{obj_msg: 0}` to the player alone (legacy `AddMotionError`). An accepted request clears the
/// destination and queues `MotionNtf` for the observers **and the player**, stamped at the player's next
/// pre-step point.
pub fn on_motion_req(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    session: SessionId,
    req: pb::MotionReq,
) {
    let id = who.0;
    let now = ctx.clock.frame0;
    let reject = |ctx: &mut GameCtx<'_>| {
        ctx.worlds.movement.stats.motion_reqs[1] += 1;
        ctx.outbox.to_session(
            session,
            id.0,
            pb::MotionNtf {
                obj_msg: 0,
                server_frame: now.0,
            },
        );
    };
    if req.obj_msg != OBJMSG_SITDOWN && req.obj_msg != OBJMSG_STANDUP {
        return reject(ctx);
    }
    let shared = ctx.worlds.movement.shared.clone();
    let Some(statics) = ctx.worlds.statics_of(id).cloned() else {
        return;
    };
    let (world, layer) = ctx
        .worlds
        .core(id)
        .map(|c| (c.world, c.layer))
        .unwrap_or_default();
    let dynamic = ctx.worlds.dynamic_of(world, layer);
    let lookup = FrameLookup::default();
    let rng = ctx.rng.world(world);
    let Some(m) = ctx.worlds.player_mut(id) else {
        return;
    };
    let env = SimEnv {
        frame: now,
        role: SimRole::Server,
        flags: shared.flags,
        world: &statics.collision,
        dynamic: &dynamic,
        motions: &shared.motions,
        lookup: &lookup,
        mode: EventMode::Normal,
    };
    let r = flyff_sim::send_act_msg(
        &mut m.sim,
        &env,
        &mut NoRules,
        rng,
        ActMsg::new(req.obj_msg),
        &mut m.sync.events,
    );
    if r == ACT_OK {
        m.sim.clear_dest();
        m.sim.clear_dest_angle();
        intents::enqueue(
            m,
            PendingIntent::Motion(req.obj_msg),
            IntentAudience::ViewersInclSelf,
        );
        ctx.worlds.movement.stats.motion_reqs[0] += 1;
    } else {
        reject(ctx);
    }
}
