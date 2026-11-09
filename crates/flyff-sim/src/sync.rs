//! Synchronisation API: how the game (server) and the client host drive a mover from outside
//! (Plan/31 §16.2, §16.3; WP-SIM-04 types, WP-SIM-11 partly).
//!
//! The sim does not depend on `flyff-proto`: the handlers of docs 15/18 map their protocol messages to the
//! plain types below.

use crate::act::{ActMsg, action_force_set, action_force_set2, send_act_msg_cx};
use crate::anim::select::set_motion;
use crate::mover::{QueuedAct, QueuedMagic, RenderSmooth, SimMover, SimMoverParams, SkillCtx};
use crate::num::{Vec3, angle_to_vector_xz, set_angle_quantized};
use crate::step::{Cx, EventMode, SimEnv, SimEvents, SimRules, step_frame};
use crate::types::*;
use flyff_common::ObjId;
use flyff_common::SimFrame;
use flyff_common::rng::GameRng;

/// Maximum number of frames a received player state is fast-forwarded (`fidelity.remote_extrap_cap_frames`
/// default 37, PL-007).
pub const REMOTE_EXTRAP_CAP: u64 = 37;

/// Maximum number of frames an intent is fast-forwarded (`INTENT_FF_CAP`, 2 s, Plan/31 §3.1, 11 §9.3):
/// beyond it the mover is "lagging" until the next keyframe.
pub const INTENT_FF_CAP: u32 = 120;

/// The flight-only part of a forced state.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct FlyFields {
    /// Pitch.
    pub angle_x: f32,
    /// `m_fAccPower`.
    pub acc_power: f32,
    /// `m_fTurnAngle`.
    pub turn_angle: f32,
}

/// A complete mover state to force (`PlayerMoveState`, `MoverStateNtf`, server-forced poses).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StateUpdate {
    /// Position.
    pub pos: Vec3,
    /// Velocity (`m_vDelta`).
    pub delta: Vec3,
    /// Yaw (quantised on application).
    pub angle: f32,
    /// State word.
    pub state: ObjState,
    /// State flags.
    pub flags: StateFlags,
    /// Un-offset motion id.
    pub motion: u32,
    /// `m_nMotionEx`.
    pub motion_ex: i32,
    /// `ANILOOP_*` of the motion.
    pub loop_: u32,
    /// `MOP_*` of the motion.
    pub motion_option: u32,
    /// Phase-aligns the motion clock when given (D4 "frame").
    pub motion_frame: Option<f32>,
    /// Flight fields (flying movers).
    pub fly: Option<FlyFields>,
}

impl StateUpdate {
    /// A plain ground state at `pos` facing `angle`: standing, no velocity, `MTI_STAND`.
    pub fn standing(pos: Vec3, angle: f32) -> StateUpdate {
        StateUpdate {
            pos,
            delta: Vec3::ZERO,
            angle,
            state: ObjState(OBJSTA_STAND),
            flags: StateFlags(0),
            motion: MTI_STAND,
            motion_ex: -1,
            loop_: ANILOOP_LOOP,
            motion_option: 0,
            motion_frame: None,
            fly: None,
        }
    }
}

/// `ActionForceSet` or, for a flying state, `ActionForceSet2` (Plan/31 §6.5).
pub fn apply_state(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    s: &StateUpdate,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) {
    let mut cx = Cx::new(env, rules, rng, ev);
    if s.flags.fly() {
        action_force_set2(m, &mut cx, s);
    } else {
        action_force_set(m, &mut cx, s);
    }
}

/// Runs `step_frame` `frames` times for ONE mover with `EventMode::Silent` (cosmetic events suppressed;
/// state transitions and steering/arrival events kept). The frames stepped are
/// `env.frame - frames .. env.frame`, so afterwards the mover is at the start of `env.frame`. Other movers
/// are frozen (their snapshot in `env.lookup` is reused for every frame).
pub fn fast_forward(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    frames: u32,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) {
    let first = env.frame.0.saturating_sub(u64::from(frames));
    for f in first..first + u64::from(frames) {
        step_frame(m, &silent_env(env, f), rules, rng, ev);
    }
}

/// `env` for frame `frame` in [`EventMode::Silent`] (cosmetic events suppressed): what a catch-up uses.
fn silent_env<'a>(env: &SimEnv<'a>, frame: u64) -> SimEnv<'a> {
    SimEnv {
        frame: SimFrame(frame),
        role: env.role,
        flags: env.flags,
        world: env.world,
        dynamic: env.dynamic,
        motions: env.motions,
        lookup: env.lookup,
        mode: EventMode::Silent,
    }
}

/// `PlayerMoveState` application (Plan/31 §16.3 rule 2, 11 §9.1 rule 4; the eight parameters are the
/// signature of Plan/31 §16.2): the stamp is clamped to `[now - 37, now]`, the state is forced, `corr`
/// clears the destination, and the mover is fast-forwarded by `now - stamp` frames, where `now` is
/// `env.frame` (the mover ends "at the start of `now`", Plan/31 §2.4).
#[allow(clippy::too_many_arguments)]
pub fn apply_player_state(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    s: &StateUpdate,
    corr: bool,
    stamp: SimFrame,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) {
    apply_player_state_capped(m, env, s, corr, stamp, REMOTE_EXTRAP_CAP, rules, rng, ev);
}

/// [`apply_player_state`] with an explicit extrapolation cap in frames (the client's
/// `fidelity.remote_extrap_cap_frames`, PL-007; the server always uses [`REMOTE_EXTRAP_CAP`]).
#[allow(clippy::too_many_arguments)]
pub fn apply_player_state_capped(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    s: &StateUpdate,
    corr: bool,
    stamp: SimFrame,
    cap: u64,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) {
    let now = env.frame.0;
    let lo = now.saturating_sub(cap);
    let stamp = stamp.0.clamp(lo, now);
    apply_state(m, env, s, rules, rng, ev);
    if corr {
        m.clear_dest();
    }
    let k = u32::try_from(now - stamp).unwrap_or(u32::MAX); // `now - stamp <= cap`
    fast_forward(m, env, k, rules, rng, ev);
}

/// `PlayerMoveState` over a server copy that is attacking or casting (flyff-game's `ATK_BUSY_MASK` rule, the same
/// rule as the destination requests; B7 `sim-ledger`): the client authors the position and the yaw, so those
/// are adopted (and the destination position and angle cleared like `ActionForceSet` does, `corr` also clears
/// the destination object), but the state word, the flags, the velocity and the motion of the copy stay: the
/// attack or the cast belongs to the server, a stale report (the client had not started the spell yet, or
/// finished it earlier) must not replace it, and the stamped frames are not replayed (a fast-forward would run
/// the attack in the past). The caller does not call [`apply_player_state`] in that case.
pub fn apply_player_pose(m: &mut SimMover, s: &StateUpdate, corr: bool) {
    m.clear_dest_pos();
    m.clear_dest_angle();
    m.pos = s.pos;
    m.angle = set_angle_quantized(s.angle);
    if corr {
        m.clear_dest();
    }
}

/// The state needed by `IntentStamp` (Plan/11 §7.1): start state of an intent.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct IntentStampData {
    /// Position.
    pub pos: Vec3,
    /// Yaw.
    pub angle: f32,
    /// State word.
    pub state: ObjState,
    /// State flags.
    pub flags: StateFlags,
    /// Pitch.
    pub angle_x: f32,
}

/// The `IntentStamp` data of a mover's current state.
pub fn stamp_of(m: &SimMover) -> IntentStampData {
    IntentStampData {
        pos: m.pos,
        angle: m.angle,
        state: m.act.state,
        flags: m.act.flags,
        angle_x: m.angle_x,
    }
}

/// `SetPos`: moves the mover without any side effect (spawn placement, server-side corrections whose
/// message is stamped by the caller). The client's `OnSetPos` teleport is [`teleport`].
pub fn set_pos(m: &mut SimMover, pos: Vec3) {
    m.pos = pos;
}

/// The client's `OnSetPos` for a mover (ref/09a §6.4; `SetPos` intents, Plan/31 §15.4): the position is set,
/// then the destination is cleared and `ACC_STOP`, `STOP_TURN` and `STAND` are sent; the render smoothing
/// restarts at the new position (`InitInterpolation`). Ground movers keep their state otherwise.
pub fn teleport(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    pos: Vec3,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) {
    let mut cx = Cx::new(env, rules, rng, ev);
    m.pos = pos;
    m.clear_dest();
    send_act_msg_cx(m, &mut cx, ActMsg::new(OBJMSG_ACC_STOP));
    send_act_msg_cx(m, &mut cx, ActMsg::new(OBJMSG_STOP_TURN));
    send_act_msg_cx(m, &mut cx, ActMsg::new(OBJMSG_STAND));
    reset_render_smoothing(m);
}

/// `InitInterpolation`: the smoothed position and angles restart at the simulated ones (hidden-tab return,
/// `SetPos`, PL-009/PL-014).
pub fn reset_render_smoothing(m: &mut SimMover) {
    m.render = RenderSmooth {
        scr_pos: m.pos,
        scr_angle: m.angle,
        scr_angle_x: m.angle_x,
        prev_pos: m.pos,
        prev_angle: m.angle,
    };
}

/// `SetSpeedFactor` (`m_fSpeedFactor`, Plan/31 §5.4): returns `true` when the value changed by more than
/// `0.000001`, so the caller broadcasts a `SetSpeedFactor` intent (doc 18).
pub fn set_speed_factor(m: &mut SimMover, factor: f32) -> bool {
    let changed = (m.params.speed_factor - factor).abs() > 0.000_001;
    m.params.speed_factor = factor;
    changed
}

/// `SetExternalForce`: `m_vDeltaE = v` (`PushPower`, knock-back helpers).
pub fn set_external_force(m: &mut SimMover, v: Vec3) {
    m.act.delta_e = v;
}

/// `MotionArrive{obj_msg}`: the message sent when the destination position is reached
/// (`m_dwMotionArrive`, default `STAND`).
pub fn set_motion_arrive(m: &mut SimMover, msg: ObjMsg) {
    m.motion_arrive = msg;
}

/// `__BS_FIXED_EQUIPMOTION` (Plan/31 §12.3): when a weapon or shield part (`PARTS_RWEAPON`,
/// `PARTS_LWEAPON`, `PARTS_SHIELD`) is equipped or removed the mover restarts its stand motion
/// (`MTI_STAND`, `1PLAY`, `NO_TRANS` when equipped, `FIXED` when removed). The original did this on the
/// client only; the port does it on both roles so the motion clocks stay aligned (FIX-minor).
pub fn on_equip_change(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    equipped: bool,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) {
    let mut cx = Cx::new(env, rules, rng, ev);
    let opt = if equipped { MOP_NO_TRANS } else { MOP_FIXED };
    set_motion(m, &mut cx, MTI_STAND, ANILOOP_1PLAY, opt);
}

/// Replaces the owner-written inputs.
pub fn set_params(m: &mut SimMover, p: SimMoverParams) {
    m.params = p;
}

/// Writes the skill context (before MELEESKILL/MAGICSKILL/MAGICCASTING).
pub fn set_skill(m: &mut SimMover, s: SkillCtx) {
    m.skill = s;
}

/// The operation of a server-originated movement intent (Plan/11 §9.3; one variant per message).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum IntentOp {
    /// `DestPosNtf`: walk to `pos`.
    DestPos {
        /// Destination.
        pos: Vec3,
        /// `m_bForward`.
        forward: bool,
    },
    /// `SetDestObjNtf`: chase `target` until within `range`.
    DestObj {
        /// The destination object.
        target: ObjId,
        /// `m_fArrivalRange`.
        range: f32,
    },
    /// `DestAngle`: turn to `angle`.
    DestAngle {
        /// Wanted yaw.
        angle: f32,
        /// Turn left (`m_bLeft`).
        left: bool,
    },
    /// `SetMovePattern`: start (or stop with `0`) a scripted pattern.
    SetMovePattern {
        /// Pattern id.
        pattern: i32,
        /// Position the pattern starts at.
        pos: Vec3,
        /// Yaw.
        angle: f32,
        /// Pitch.
        angle_x: f32,
    },
    /// `SetSpeedFactor`.
    SpeedFactor {
        /// The new `m_fSpeedFactor`.
        factor: f32,
    },
    /// `SetPos`: a teleport ([`teleport`]).
    SetPos {
        /// The new position.
        pos: Vec3,
    },
}

/// What [`apply_intent`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct IntentOutcome {
    /// Frames the mover was stepped (`min(lag, INTENT_FF_CAP)`).
    pub stepped: u32,
    /// The intent was older than `INTENT_FF_CAP`: the mover is flagged `lagging` until the next keyframe.
    pub lagging: bool,
}

/// `sync::apply_intent` (Plan/31 §15.4, 11 §9.3; the client side of every NPC intent and of the relayed
/// player destinations; the server applies the operations directly):
///
/// 1. the `IntentStamp` start state at frame `F`: position, yaw (quantised) and pitch are set; the state
///    word and flags only when the FLY and SIT flags agree with the current ones (the forced-set rule);
///    `delta.y` is kept;
/// 2. the operation: `set_dest_pos(pos, forward)` (without the active-mover report), `set_dest_obj`,
///    `set_dest_angle`, the pattern reset, the speed factor or the teleport;
/// 3. `fast_forward(min(lag, INTENT_FF_CAP))` in [`EventMode::Silent`], where `lag = now - F` is given by
///    the caller (`env.frame` is "now"; `0` when `F` is in the future). A larger lag marks the mover
///    `lagging` (cleared by the next keyframe, WP-SIM-16).
#[allow(clippy::too_many_arguments)]
pub fn apply_intent(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    stamp: &IntentStampData,
    op: IntentOp,
    lag: u32,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) -> IntentOutcome {
    // 1. the start state at F
    m.pos = stamp.pos;
    m.angle = set_angle_quantized(stamp.angle);
    m.angle_x = stamp.angle_x;
    if m.act.flags.fly() == stamp.flags.fly() && m.act.flags.sit() == stamp.flags.sit() {
        m.act.state = stamp.state;
        m.act.flags = stamp.flags;
    }
    // 2. the operation
    match op {
        IntentOp::DestPos { pos, forward } => m.set_dest_pos(pos, forward),
        IntentOp::DestObj { target, range } => m.set_dest_obj(target, range),
        IntentOp::DestAngle { angle, left } => m.set_dest_angle(angle, left),
        IntentOp::SetMovePattern {
            pattern,
            pos,
            angle,
            angle_x,
        } => set_move_pattern(m, pattern, pos, angle, angle_x),
        IntentOp::SpeedFactor { factor } => {
            set_speed_factor(m, factor);
        }
        IntentOp::SetPos { pos } => {
            teleport(m, &silent_env(env, env.frame.0), pos, rules, rng, ev);
        }
    }
    // 3. catch up with the present
    let stepped = lag.min(INTENT_FF_CAP);
    let lagging = lag > INTENT_FF_CAP;
    if lagging {
        m.lagging = true;
    }
    fast_forward(m, env, stepped, rules, rng, ev);
    IntentOutcome { stepped, lagging }
}

/// `SetDestPos`.
pub fn set_dest_pos(m: &mut SimMover, v: Vec3, forward: bool) {
    m.set_dest_pos(v, forward);
}

/// `SetDestObj`.
pub fn set_dest_obj(m: &mut SimMover, id: ObjId, range: f32) {
    m.set_dest_obj(id, range);
}

/// `ClearDest`.
pub fn clear_dest(m: &mut SimMover) {
    m.clear_dest();
}

/// `SetDestAngle`.
pub fn set_dest_angle(m: &mut SimMover, a: f32, left: bool) {
    m.set_dest_angle(a, left);
}

/// `SetMovePattern{pattern, pos, angle, angle_x}`: sets the pattern, `event = 0`, `cnt = 0`, position and
/// angles.
pub fn set_move_pattern(m: &mut SimMover, pattern: i32, pos: Vec3, angle: f32, angle_x: f32) {
    m.pattern.pattern = pattern;
    m.pattern.event = 0;
    m.pattern.cnt = 0;
    m.pos = pos;
    m.angle = set_angle_quantized(angle);
    m.angle_x = angle_x;
}

/// `PushPower` (ref/09b): the yaw is set from `angle`, the external force is
/// `angle_to_vector_xz(push_angle, power)`; the position is NOT snapped. Suppressed by the caller when the
/// mover is in a knock-back state or stunned (doc 17 checks).
pub fn push_power(m: &mut SimMover, angle: f32, push_angle: f32, power: f32) {
    m.angle = set_angle_quantized(angle);
    m.act.delta_e = angle_to_vector_xz(push_angle, power);
}

/// Queues a melee attack message (`m_qMeleeAtkMsg.AddTail`); `false` when the queue is full.
pub fn enqueue_melee(m: &mut SimMover, a: QueuedAct) -> bool {
    m.act.queues.melee.push(a)
}

/// Queues a magic/range attack (`m_qMagicAtkMsg.AddTail`); `false` when the queue is full.
pub fn enqueue_magic(m: &mut SimMover, q: QueuedMagic) -> bool {
    m.act.queues.magic.push(q)
}

/// `CMagicAtkMsgq::Process` with the owner's closure: `1` removes the head and stops (an attack started), `0`
/// stops and keeps the head, any other value removes the head and continues (**FIX**: values other than
/// `1`/`0`/`-1` looped forever in the original). Returns whether an attack started.
pub fn drain_magic(m: &mut SimMover, f: &mut dyn FnMut(&QueuedMagic) -> i32) -> bool {
    while let Some(q) = m.act.queues.magic.head() {
        match f(&q) {
            1 => {
                m.act.queues.magic.pop();
                return true;
            }
            0 => return false,
            _ => {
                m.act.queues.magic.pop();
            }
        }
    }
    false
}
