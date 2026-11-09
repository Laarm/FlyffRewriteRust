//! NPC intents: the pending queue, stamp capture, the emitters, the spawn replay and the server-driven player
//! approach (Plan/18 §5.1, §5.2, §5.4, §5.6, §5.9; WP-AI-01, WP-AI-04).
//!
//! An emitter changes the mover's sim **now** and queues the message; `capture_pending` (called by the
//! pipeline right before the mover's next `step_frame(F)`) builds every queued message from one snapshot of
//! the sim, stamps it `F` and pushes it to its audience. So the stamp is always "the start of the frame the
//! intent was honoured in" (31 §2.4), whichever phase raised it.

use super::{IntentAudience, MoveStats, PendingEntry, PendingIntent};
use crate::mover::Mover;
use crate::world::visibility::SpawnSink;
use flyff_common::math::Vec3;
use flyff_common::{ObjId, SimFrame};
use flyff_proto::{EncodedMsg, pb};
use flyff_sim::SimMover;
use flyff_sim::num::angle_to_vector_xz;
use flyff_sim::types::{MTI_STAND, OBJSTA_DMG_FLY_ALL, OBJSTA_STUN};
use smallvec::SmallVec;

/// The wire `Vec3` of a position.
pub fn pb_vec(v: Vec3) -> pb::Vec3 {
    pb::Vec3 {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

/// The position of a wire `Vec3` (a missing one is the origin).
pub fn vec_of(v: Option<pb::Vec3>) -> Vec3 {
    v.map_or(Vec3::ZERO, |v| Vec3::new(v.x, v.y, v.z))
}

/// The `IntentStamp` of a mover at the start of `frame`: the position, yaw, state word, flags and pitch of
/// its sim (31 `sync::stamp_of`).
pub fn intent_stamp(sim: &SimMover, frame: SimFrame) -> pb::IntentStamp {
    let s = flyff_sim::sync::stamp_of(sim);
    pb::IntentStamp {
        server_frame: frame.0,
        start_pos: Some(pb_vec(s.pos)),
        start_angle: s.angle,
        start_state: s.state.bits(),
        start_state_flags: s.flags.bits(),
        start_angle_x: s.angle_x,
    }
}

/// `MoverStateNtf` of the sim's current state (the `CORR` of an NPC, a server-forced pose): position, delta,
/// yaw, state word, flags, the motion with its option, loop mode and frame, the flight fields when flying.
pub fn state_ntf(sim: &SimMover, reason: pb::MoveReason, frame: SimFrame) -> pb::MoverStateNtf {
    let fly = sim.act.flags.fly().then_some(pb::FlyState {
        angle_x: sim.angle_x,
        acc_power: sim.act.acc_power,
        turn_angle: sim.act.turn_angle,
    });
    pb::MoverStateNtf {
        reason: reason as i32,
        pos: Some(pb_vec(sim.pos)),
        delta: Some(pb_vec(sim.act.delta)),
        angle: sim.angle,
        state: sim.act.state.bits(),
        state_flags: sim.act.flags.bits(),
        motion: if sim.motion.id == u32::MAX {
            MTI_STAND
        } else {
            sim.motion.id
        },
        motion_ex: sim.act.motion_ex,
        r#loop: i32::try_from(sim.motion.clock.lp).unwrap_or(0),
        motion_option: sim.motion.option,
        motion_frame: sim.motion.clock.frame_cur,
        server_frame: frame.0,
        fly,
    }
}

// ---- queueing ----------------------------------------------------------------------------------------------------

/// Queues `intent` for `audience`. An NPC nobody observes is not processed (18 §7), so nothing is queued for
/// it: a viewer that arrives later gets the spawn replay (§5.4).
pub fn enqueue(m: &mut Mover, intent: PendingIntent, audience: IntentAudience) {
    if !m.is_player() && m.core.viewers.is_empty() {
        return;
    }
    m.sync.pending.push(PendingEntry { intent, audience });
}

/// `MoveToDst(pos)` (18 §8.8): walk to `v`. No-op when that is already the destination.
pub fn move_to_pos(m: &mut Mover, v: Vec3) {
    if m.sim.dest.pos == v {
        return;
    }
    m.sim.set_dest_pos(v, true);
    m.sync.corr_armed = true;
    enqueue(
        m,
        PendingIntent::DestPos {
            pos: v,
            forward: true,
        },
        IntentAudience::Viewers,
    );
}

/// `MoveToDst(id)` (18 §8.8): chase object `id` at the chase speed (factor 2.0, announced first). No-op when
/// that is already the destination object. (The legacy `SetTarget` of the enemy belongs to the AI.)
pub fn move_to_obj(m: &mut Mover, id: ObjId) {
    if m.sim.dest.obj == id {
        return;
    }
    set_speed_factor(m, 2.0);
    m.sim.set_dest_obj(id, 0.0);
    m.sync.corr_armed = true;
    enqueue(
        m,
        PendingIntent::DestObj {
            target: id,
            range: 0.0,
        },
        IntentAudience::Viewers,
    );
}

/// `SetDestObj(id, range)` for an NPC that melees (bosses, Meteonyker; `set_melee_attack_dest`, 18 §8.8):
/// no-op when already the destination object.
pub fn set_dest_obj(m: &mut Mover, id: ObjId, range: f32) {
    if m.sim.dest.obj == id {
        return;
    }
    m.sim.set_dest_obj(id, range);
    m.sync.corr_armed = true;
    enqueue(
        m,
        PendingIntent::DestObj { target: id, range },
        IntentAudience::Viewers,
    );
}

/// `SetSpeedFactor(f)` (18 §8.8): announces the change when it exceeds 0.000001.
pub fn set_speed_factor(m: &mut Mover, factor: f32) {
    if flyff_sim::sync::set_speed_factor(&mut m.sim, factor) {
        enqueue(
            m,
            PendingIntent::SpeedFactor(factor),
            IntentAudience::Viewers,
        );
    }
}

/// `SetDestAngle(angle, left)`: turn in place (the STAND return of 18 §9.5).
pub fn set_dest_angle(m: &mut Mover, angle: f32, left: bool) {
    m.sim.set_dest_angle(angle, left);
    enqueue(
        m,
        PendingIntent::DestAngle { angle, left },
        IntentAudience::Viewers,
    );
}

/// `SetMovePattern(pattern)` at the mover's current position and angles (flying monsters, 18 §9.4).
pub fn set_move_pattern(m: &mut Mover, pattern: i32) {
    let (pos, angle, angle_x) = (m.sim.pos, m.sim.angle, m.sim.angle_x);
    flyff_sim::sync::set_move_pattern(&mut m.sim, pattern, pos, angle, angle_x);
    enqueue(
        m,
        PendingIntent::MovePattern { pattern },
        IntentAudience::Viewers,
    );
}

/// `warp_npc` (18 §5.6): `CMover::SetPos` on the sim (destination and state unchanged) and the `SetPos`
/// message. The phase driver copies the position into the header and relinks after the mover's next frame.
pub fn warp_npc(m: &mut Mover, pos: Vec3) {
    flyff_sim::sync::set_pos(&mut m.sim, pos);
    enqueue(m, PendingIntent::SetPos, IntentAudience::Viewers);
}

/// `movement::push` (18 §5.7): always sets the external force `AngleToVectorXZ(push_angle, power)`; the
/// `PushPower` message only when the mover is neither in a knock-back flight (`DMG_FLY_ALL`) nor stunned
/// (`UserLux.cpp:298-305`).
pub fn push(m: &mut Mover, push_angle: f32, power: f32) {
    flyff_sim::sync::set_external_force(&mut m.sim, angle_to_vector_xz(push_angle, power));
    let st = m.sim.act.state.bits();
    if st & OBJSTA_DMG_FLY_ALL == 0 && st & OBJSTA_STUN == 0 {
        enqueue(
            m,
            PendingIntent::Push { push_angle, power },
            IntentAudience::Viewers,
        );
    }
}

/// `movement::broadcast_motion` (18 §4.4): the sit / stand / pickup motion `obj_msg` the mover just started,
/// to its viewers **and itself** (legacy `AddMotion`).
pub fn broadcast_motion(m: &mut Mover, obj_msg: u32) {
    enqueue(
        m,
        PendingIntent::Motion(obj_msg),
        IntentAudience::ViewersInclSelf,
    );
}

/// `movement::force_pose_broadcast` (18 §4.2): the mover's current state as a `BEHAVIOR` to its viewers and
/// itself (cheer, proposal kneel, final death pose).
pub fn force_pose_broadcast(m: &mut Mover) {
    enqueue(m, PendingIntent::ForcePose, IntentAudience::ViewersInclSelf);
}

/// `SendActMsg` broadcast of an AI or fuel event (18 §6: `MODE_TURBO_OFF`).
pub fn broadcast_act_msg(m: &mut Mover, obj_msg: i32, is_fly: bool, p: [i32; 3]) {
    enqueue(
        m,
        PendingIntent::ActMsg { obj_msg, is_fly, p },
        IntentAudience::Viewers,
    );
}

/// `movement::set_player_dest_obj` (18 §5.9): the server-driven approach of a player to `target` (17's queued
/// skills). Sets the destination object of the server copy (arming its arrival test) and announces
/// `SetDestObjNtf` to the observers **and the player itself**, stamped at the player's next pre-step point.
/// The caller sets `m.sim.cmd.act = OBJACT_USESKILL` afterwards (17's pending object action; `SetDestObj`
/// resets it).
pub fn set_player_dest_obj(m: &mut Mover, target: ObjId, range: f32) {
    m.sim.set_dest_obj(target, range);
    enqueue(
        m,
        PendingIntent::DestObj { target, range },
        IntentAudience::ViewersInclSelf,
    );
}

// ---- capture -----------------------------------------------------------------------------------------------------

/// The message body of one queued intent, built from the capture snapshot (`sim`) and `frame`.
fn body(e: &PendingEntry, sim: &SimMover, frame: SimFrame) -> pb::server_msg::Msg {
    let stamp = || intent_stamp(sim, frame);
    match e.intent {
        PendingIntent::DestPos { pos, forward } => pb::DestPosNtf {
            pos: Some(pb_vec(pos)),
            forward,
            stamp: Some(stamp()),
        }
        .into(),
        PendingIntent::DestObj { target, range } => pb::SetDestObjNtf {
            target_obj_id: target.0,
            range,
            stamp: Some(stamp()),
        }
        .into(),
        PendingIntent::DestAngle { angle, left } => pb::DestAngle {
            angle,
            left,
            stamp: Some(stamp()),
        }
        .into(),
        PendingIntent::MovePattern { pattern } => pb::SetMovePattern {
            pattern,
            pos: Some(pb_vec(sim.pos)),
            angle: sim.angle,
            angle_x: sim.angle_x,
            stamp: Some(stamp()),
        }
        .into(),
        PendingIntent::SpeedFactor(factor) => pb::SetSpeedFactor {
            factor,
            stamp: Some(stamp()),
        }
        .into(),
        PendingIntent::SetPos => pb::SetPos {
            pos: Some(pb_vec(sim.pos)),
            stamp: Some(stamp()),
        }
        .into(),
        PendingIntent::Corr => state_ntf(sim, pb::MoveReason::Corr, frame).into(),
        PendingIntent::ForcePose => state_ntf(sim, pb::MoveReason::Behavior, frame).into(),
        PendingIntent::Push { push_angle, power } => pb::PushPower {
            pos: Some(pb_vec(sim.pos)),
            angle: sim.angle,
            push_angle,
            power,
            server_frame: frame.0,
        }
        .into(),
        PendingIntent::ActMsg { obj_msg, is_fly, p } => pb::SendActMsg {
            obj_msg,
            is_fly,
            p1: p[0],
            p2: p[1],
            p3: p[2],
        }
        .into(),
        PendingIntent::Motion(obj_msg) => pb::MotionNtf {
            obj_msg,
            server_frame: frame.0,
        }
        .into(),
        PendingIntent::MotionArrive(obj_msg) => pb::MotionArrive { obj_msg }.into(),
    }
}

/// Pushes `msg` to the players of `aud` through the sink.
pub(crate) fn deliver(sink: &mut dyn SpawnSink, m: &Mover, aud: IntentAudience, msg: &EncodedMsg) {
    let id = m.core.id;
    match aud {
        IntentAudience::Viewer(v) => sink.relay(v, msg),
        IntentAudience::Viewers => {
            for &v in &m.core.viewers {
                if v != id {
                    sink.relay(v, msg);
                }
            }
        }
        IntentAudience::ViewersInclSelf => {
            for &v in &m.core.viewers {
                sink.relay(v, msg);
            }
            // a player that is not (yet) its own viewer still hears about itself
            if m.is_player() && !m.core.viewers.contains(&id) {
                sink.relay(id, msg);
            }
        }
    }
}

/// `true` when somebody would receive a message of `aud` about `m`.
fn has_audience(m: &Mover, aud: IntentAudience) -> bool {
    match aud {
        IntentAudience::Viewer(_) => true,
        IntentAudience::Viewers => m.core.viewers.iter().any(|v| *v != m.core.id),
        IntentAudience::ViewersInclSelf => m.is_player() || !m.core.viewers.is_empty(),
    }
}

/// `capture_pending(m, F)` (18 §5.2 rule 2): when the queue is not empty, builds every message from the
/// sim as it is right now (the start of frame `frame`, before `step_frame(frame)`), stamps it `frame`,
/// encodes it once and pushes it to its audience in queue order; then clears the queue.
pub(crate) fn capture_pending(
    m: &mut Mover,
    frame: SimFrame,
    sink: &mut dyn SpawnSink,
    stats: &mut MoveStats,
) {
    if m.sync.pending.is_empty() {
        return;
    }
    let mut pending = std::mem::take(&mut m.sync.pending);
    for e in pending.iter() {
        if let Some(n) = stats.intents.get_mut(e.intent.kind()) {
            *n += 1;
        }
        if matches!(e.intent, PendingIntent::Corr) {
            stats.corrections += 1;
        }
        if !has_audience(m, e.audience) {
            continue;
        }
        let enc = EncodedMsg::new(m.core.id.0, body(e, &m.sim, frame));
        deliver(sink, m, e.audience, &enc);
    }
    pending.clear();
    m.sync.pending = pending;
}

// ---- spawn replay ---------------------------------------------------------------------------------------------

/// The intents a viewer that has just been given the spawn of NPC `m` must hear to see what the NPC is doing
/// (18 §5.4, 31 §16.3 rule 8): `DestPosNtf` (destination position set), `SetDestObjNtf` (destination object),
/// `SetMovePattern` (pattern running), `DestAngle` (turn in place), each stamped `frame` with the NPC's
/// current state. The speed factor already travels in the spawn. Players are not replayed: their next relayed
/// state (<= 500 ms) carries everything.
pub fn replay_messages(m: &Mover, frame: SimFrame) -> SmallVec<[EncodedMsg; 4]> {
    let mut out = SmallVec::new();
    if m.is_player() {
        return out;
    }
    let sim = &m.sim;
    let mut push = |intent: PendingIntent| {
        let e = PendingEntry {
            intent,
            audience: IntentAudience::Viewers,
        };
        out.push(EncodedMsg::new(m.core.id.0, body(&e, sim, frame)));
    };
    if !sim.is_empty_dest_pos() {
        push(PendingIntent::DestPos {
            pos: sim.dest.pos,
            forward: sim.dest.forward,
        });
    }
    if !sim.is_empty_dest_obj() {
        push(PendingIntent::DestObj {
            target: sim.dest.obj,
            range: sim.dest.arrival_range,
        });
    }
    if sim.pattern.pattern != 0 {
        push(PendingIntent::MovePattern {
            pattern: sim.pattern.pattern,
        });
    }
    if sim.dest_angle > -0.001 {
        push(PendingIntent::DestAngle {
            angle: sim.dest_angle,
            left: sim.dest_left,
        });
    }
    out
}
