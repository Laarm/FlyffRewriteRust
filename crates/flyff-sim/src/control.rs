//! Local player control, prediction and the outgoing state report (Plan/31 §15.2-§15.3; WP-SIM-15).
//!
//! The client runs the shared sim for its own player (`SimMover::is_active`): doc 43 maps keys to the
//! per-frame input record of `local_input` (Plan/31 §17.5), [`ActiveControl::control_frame`] applies it at the
//! start of every stepped frame (`CWndWorld::ProcessControl`, `WndWorldControlPlayer.cpp:424-564`) and
//! accumulates the [`LocalReport`] that doc 40 turns into one `PlayerMoveState` plus the destination request
//! (§15.3). Collision/drop corrections of the active mover (`MoverActEvent.cpp:1990-2090`) and the arrival
//! of a command ([`crate::cmd::on_arrival`]) are answered at the frame barrier by
//! [`ActiveControl::handle_events`].
//!
//! Prediction (PL-006): the report carries the CURRENT state at the start of the last stepped frame, there is
//! no `PresupposePos` 22/37-frame look-ahead; the server fast-forwards the stamp with the same sim.

use crate::act::{ActMsg, send_act_msg_cx};
use crate::cmd::{self, Arrival, CmdKind};
use crate::mover::{ActiveCmd, SimMover};
use crate::num::{Vec3, ftoi_x86, set_angle_quantized};
use crate::step::{Cx, SimEvent};
use crate::types::*;
use crate::wire::{self, input, report};
use flyff_common::ObjId;

/// Frames between two `PERIODIC` reports and the `ANGLE` throttle (500 ms, KQ-28).
pub const REPORT_PERIOD_FRAMES: u32 = 30;

/// The input of one stepped frame: held buttons plus the edges (`WALK_TOGGLE`, ...) latched since the last
/// frame, and the mouse movement accumulated since then.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FrameInput {
    /// Held buttons and consumed edges (`wire::input` bits).
    pub buttons: u32,
    /// Mouse movement in x (pixels).
    pub mouse_dx: i32,
    /// Mouse movement in y (pixels).
    pub mouse_dy: i32,
}

impl FrameInput {
    /// `true` when button `bit` is set.
    #[inline]
    pub const fn has(&self, bit: u32) -> bool {
        self.buttons & bit != 0
    }
}

/// The latched `local_input` record: held bits persist until the next record, edge bits and mouse movement
/// are consumed by the next stepped frame (Plan/31 §17.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputLatch {
    held: u32,
    edges: u32,
    dx: i32,
    dy: i32,
    /// The character level of the last record (`0` = unknown).
    pub level: i32,
    /// The `REQ_*` bits of the last record.
    pub req_flags: u32,
}

impl Default for InputLatch {
    fn default() -> Self {
        InputLatch {
            held: input::NEXT_SKILL_NONE,
            edges: 0,
            dx: 0,
            dy: 0,
            level: 0,
            req_flags: 0,
        }
    }
}

impl InputLatch {
    /// Applies a `local_input` record `[buttons, mouse_dx, mouse_dy, level, req_flags]`; missing trailing
    /// words are zero.
    pub fn update(&mut self, rec: &[i32]) {
        let g = |i: usize| rec.get(i).copied().unwrap_or(0);
        let buttons = g(input::S_BUTTONS) as u32; // BOUNDS: bit pattern
        self.held = buttons & !input::EDGE_MASK;
        self.edges |= buttons & input::EDGE_MASK;
        self.dx = self.dx.saturating_add(g(input::S_MOUSE_DX));
        self.dy = self.dy.saturating_add(g(input::S_MOUSE_DY));
        self.level = g(input::S_LEVEL);
        self.req_flags = g(input::S_REQ_FLAGS) as u32; // BOUNDS: bit pattern
    }

    /// The input of the next stepped frame; consumes the edges and the mouse movement.
    pub fn take(&mut self) -> FrameInput {
        let f = FrameInput {
            buttons: self.held | self.edges,
            mouse_dx: self.dx,
            mouse_dy: self.dy,
        };
        self.edges = 0;
        self.dx = 0;
        self.dy = 0;
        f
    }

    /// The held buttons (diagnostics).
    pub fn held(&self) -> u32 {
        self.held
    }
}

/// What the ground control of one frame did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ControlOutcome {
    /// A key-driven state change (`MOVED`).
    pub moved: bool,
    /// A discrete action (`BEHAVIOR`).
    pub behavior: bool,
    /// Casting must be cancelled (movement or jump).
    pub cast_cancel: bool,
}

/// `SendActMsg(msg) == 1` (the message executed and changed the state).
fn act(m: &mut SimMover, cx: &mut Cx<'_>, msg: ObjMsg) -> bool {
    send_act_msg_cx(m, cx, ActMsg::new(msg)) == 1
}

/// `control::ground` (Plan/31 §15.2, `WndWorldControlPlayer.cpp:385-564`): the order per frame is
/// FORWARD/BACKWARD or (without a destination, not jumping and not sitting) STAND; turn or STOP_TURN; jump;
/// the walk/run toggle; any `1` result clears the destination. Messages are only counted when they returned
/// exactly `1` (state changed).
pub fn ground(m: &mut SimMover, cx: &mut Cx<'_>, i: &FrameInput) -> ControlOutcome {
    let mut o = ControlOutcome::default();
    if i.has(input::BLOCKED) {
        return o;
    }
    let (up, down, left, right, jump) = (
        i.has(input::UP),
        i.has(input::DOWN),
        i.has(input::LEFT),
        i.has(input::RIGHT),
        i.has(input::JUMP),
    );
    if up || down || left || right || jump {
        // any movement key cancels the pending command (the auto attack is doc 43's)
        m.cmd = ActiveCmd::default();
    }
    if up {
        if act(m, cx, OBJMSG_FORWARD) {
            o.moved = true;
            o.cast_cancel = true;
        }
    } else if down {
        if act(m, cx, OBJMSG_BACKWARD) {
            o.moved = true;
            o.cast_cancel = true;
        }
    } else if m.is_empty_dest()
        && !m.act.state.is_act_jump()
        && !m.act.flags.sit()
        && act(m, cx, OBJMSG_STAND)
    {
        o.moved = true;
    }
    if left {
        o.moved |= act(m, cx, OBJMSG_LTURN);
    } else if right {
        o.moved |= act(m, cx, OBJMSG_RTURN);
    } else {
        o.moved |= act(m, cx, OBJMSG_STOP_TURN);
    }
    if jump && act(m, cx, OBJMSG_JUMP) {
        o.behavior = true;
        o.cast_cancel = true;
    }
    if i.has(input::WALK_TOGGLE) {
        if m.act.flags.is_walk() {
            if act(m, cx, OBJMSG_MODE_RUN) {
                o.behavior = true;
                cx.emit(SimEvent::Notice {
                    tid: flyff_common::defs::text::TID_GAME_RUN,
                });
            }
        } else if act(m, cx, OBJMSG_MODE_WALK) {
            o.behavior = true;
            cx.emit(SimEvent::Notice {
                tid: flyff_common::defs::text::TID_GAME_WALK,
            });
        }
    }
    if o.moved || o.behavior {
        m.clear_dest();
    }
    if o.cast_cancel && m.params.base_motion_mode {
        cx.emit(SimEvent::CastCancel);
    }
    o
}

/// The visual roll limit of the flight camera in degrees (`m_fRollAng`, `+-45`).
pub const ROLL_LIMIT: f32 = 45.0;
/// The turn rate (degrees per frame) used when the ride is unknown (`fTurnAngle = 0.6f`).
pub const DEFAULT_FLIGHT_TURN: f32 = 0.6;
/// The pitch limit of mouse steering in degrees.
pub const MOUSE_PITCH_LIMIT: f32 = 45.0;

/// What the flying control of one frame did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FlyOutcome {
    /// A key-driven state change (`MOVED`).
    pub moved: bool,
    /// The mouse changed the yaw or the pitch (`PostPlayerAngle`, throttled to one `ANGLE` per 30 frames).
    pub angle: bool,
}

/// `control::fly` (Plan/31 §15.2, `WndWorldControlPlayer.cpp:671-952`, `ControlFlying`): while `ACC` the
/// client keeps sending `FORWARD`, else `STAND`; the acceleration key toggles `ACC_START`/`ACC_STOP`, the
/// turbo key `MODE_TURBO_ON`/`OFF`; `W` looks down (only above the terrain), `S` up; `A`/`D` turn with the
/// ride's `lr_angle` (hundredths of a degree in `p1`) and drive the visual `roll` (`+-1` per frame to `+-45`,
/// back to 0 by 2 per frame without overshoot); the mouse steers directly: yaw `-= dx / 2`, pitch
/// `+= dy / 4` clamped to `+-45`. Messages count only when they returned exactly `1`.
///
/// The auto-tracking toggle (`Z`, `TAB`) and the attack inputs are doc 43's (`SET_TRACKING`, `local_act`).
pub fn fly(m: &mut SimMover, cx: &mut Cx<'_>, i: &FrameInput, roll: &mut f32) -> FlyOutcome {
    let mut o = FlyOutcome::default();
    if i.has(input::BLOCKED) {
        return o;
    }
    if m.act.flags.has(OBJSTAF_ACC) {
        o.moved |= act(m, cx, OBJMSG_FORWARD);
    } else {
        o.moved |= act(m, cx, OBJMSG_STAND);
    }
    if i.has(input::ACC_TOGGLE) {
        if m.act.flags.has(OBJSTAF_ACC) {
            act(m, cx, OBJMSG_ACC_STOP);
            if m.act.state.is_act_turn() {
                o.moved = true;
            }
        } else if send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_ACC_START)) == ACT_IGNORED {
            cx.emit(SimEvent::Notice {
                tid: flyff_common::defs::text::TID_GAME_AIRFUELEMPTY,
            });
        } else {
            o.moved |= act(m, cx, OBJMSG_FORWARD);
        }
    }
    if i.has(input::TURBO_TOGGLE) {
        let msg = if m.act.flags.has(OBJSTAF_TURBO) {
            OBJMSG_MODE_TURBO_OFF
        } else {
            OBJMSG_MODE_TURBO_ON
        };
        o.moved |= act(m, cx, msg);
    }
    let turn = m.params.ride.map_or(DEFAULT_FLIGHT_TURN, |r| r.lr_angle);
    let turn_p1 = ftoi_x86(turn * 100.0);
    if i.has(input::UP) {
        if cx.env.world.full_height(m.pos) < m.pos.y {
            o.moved |= act(m, cx, OBJMSG_LOOKDOWN);
        }
    } else if i.has(input::DOWN) {
        o.moved |= act(m, cx, OBJMSG_LOOKUP);
    } else {
        o.moved |= act(m, cx, OBJMSG_STOP_LOOK);
    }
    if i.has(input::LEFT) {
        *roll = (*roll - 1.0).max(-ROLL_LIMIT);
        o.moved |= send_act_msg_cx(m, cx, ActMsg::with(OBJMSG_LTURN, [turn_p1, 0, 0, 0, 0])) == 1;
    } else if i.has(input::RIGHT) {
        *roll = (*roll + 1.0).min(ROLL_LIMIT);
        o.moved |= send_act_msg_cx(m, cx, ActMsg::with(OBJMSG_RTURN, [turn_p1, 0, 0, 0, 0])) == 1;
    } else {
        if *roll < 0.0 {
            *roll += 2.0;
            if *roll > 0.0 {
                *roll = 0.0;
            }
        } else if *roll > 0.0 {
            *roll -= 2.0;
            if *roll < 0.0 {
                *roll = 0.0;
            }
        }
        o.moved |= act(m, cx, OBJMSG_STOP_TURN);
    }
    if i.mouse_dx != 0 || i.mouse_dy != 0 {
        let add = i.mouse_dx as f32 / 2.0; // BOUNDS: pixel deltas are small
        m.angle = set_angle_quantized(m.angle - add);
        let add_x = i.mouse_dy as f32 / 4.0; // BOUNDS: pixel deltas are small
        let mut ax = m.angle_x + add_x;
        if add_x > 0.0 && ax > MOUSE_PITCH_LIMIT {
            ax = MOUSE_PITCH_LIMIT;
        } else if add_x < 0.0 && ax < -MOUSE_PITCH_LIMIT {
            ax = -MOUSE_PITCH_LIMIT;
        }
        m.angle_x = ax;
        o.angle = add != 0.0 || add_x != 0.0;
    }
    o
}

/// The local report of one `step` call (Plan/31 §15.3, §17.7 `report` area).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LocalReport {
    /// `report::*` bits, OR-accumulated over the call.
    pub bits: u32,
    /// The last stepped frame whose control step ran.
    pub client_frame: u64,
    /// Position at the start of that frame (after the control messages).
    pub pos: Vec3,
    /// Per-frame velocity.
    pub delta: Vec3,
    /// Yaw.
    pub angle: f32,
    /// State word.
    pub state: u32,
    /// State flags.
    pub state_flags: u32,
    /// Un-offset motion id.
    pub motion: u32,
    /// `m_nMotionEx`.
    pub motion_ex: i32,
    /// `ANILOOP_*`.
    pub loop_: i32,
    /// `MOP_*`.
    pub motion_option: u32,
    /// Current motion frame.
    pub motion_frame: f32,
    /// Flight pitch.
    pub angle_x: f32,
    /// Flight acceleration power.
    pub acc_power: f32,
    /// Flight turn angle.
    pub turn_angle: f32,
    /// The chosen destination position (valid when `dest_obj` is none).
    pub dest_pos: Vec3,
    /// `forward` of the destination position.
    pub dest_forward: bool,
    /// The chosen destination object (`ObjId::NONE` = none).
    pub dest_obj: ObjId,
    /// Arrival range of the destination object.
    pub dest_range: f32,
}

impl Default for LocalReport {
    fn default() -> Self {
        LocalReport {
            bits: 0,
            client_frame: 0,
            pos: Vec3::ZERO,
            delta: Vec3::ZERO,
            angle: 0.0,
            state: 0,
            state_flags: 0,
            motion: 0,
            motion_ex: 0,
            loop_: 0,
            motion_option: 0,
            motion_frame: 0.0,
            angle_x: 0.0,
            acc_power: 0.0,
            turn_angle: 0.0,
            dest_pos: Vec3::ZERO,
            dest_forward: false,
            dest_obj: ObjId::NONE,
            dest_range: 0.0,
        }
    }
}

impl LocalReport {
    /// `step(0)`: an all-zero report (the destination object reads as `NONE` only once a destination was
    /// chosen; an empty report has every slot zero).
    pub fn empty() -> LocalReport {
        LocalReport {
            dest_obj: ObjId(0),
            ..LocalReport::default()
        }
    }

    /// Writes the 32-float `report` area of the render view (ids and bit fields as `u32` patterns).
    pub fn write_floats(&self, out: &mut [f32]) {
        out.iter_mut()
            .take(wire::view::REPORT_FLOATS)
            .for_each(|s| *s = 0.0);
        let mut set = |i: usize, v: f32| {
            if let Some(s) = out.get_mut(i) {
                *s = v;
            }
        };
        let bits = |v: u32| f32::from_bits(v);
        set(report::S_BITS, bits(self.bits));
        // BOUNDS: the frame is exposed as its low 32 bits (a 60 Hz counter wraps after 2.2 years)
        set(report::S_CLIENT_FRAME, bits(self.client_frame as u32));
        set(report::S_POS, self.pos.x);
        set(report::S_POS + 1, self.pos.y);
        set(report::S_POS + 2, self.pos.z);
        set(report::S_DELTA, self.delta.x);
        set(report::S_DELTA + 1, self.delta.y);
        set(report::S_DELTA + 2, self.delta.z);
        set(report::S_ANGLE, self.angle);
        set(report::S_STATE, bits(self.state));
        set(report::S_STATE_FLAGS, bits(self.state_flags));
        set(report::S_MOTION, bits(self.motion));
        set(report::S_MOTION_EX, bits(self.motion_ex as u32)); // BOUNDS: bit pattern
        set(report::S_LOOP, bits(self.loop_ as u32)); // BOUNDS: bit pattern
        set(report::S_MOTION_OPTION, bits(self.motion_option));
        set(report::S_MOTION_FRAME, self.motion_frame);
        set(report::S_ANGLE_X, self.angle_x);
        set(report::S_ACC_POWER, self.acc_power);
        set(report::S_TURN_ANGLE, self.turn_angle);
        set(report::S_DEST_POS, self.dest_pos.x);
        set(report::S_DEST_POS + 1, self.dest_pos.y);
        set(report::S_DEST_POS + 2, self.dest_pos.z);
        set(
            report::S_DEST_FORWARD,
            if self.dest_forward { 1.0 } else { 0.0 },
        );
        set(report::S_DEST_OBJ, bits(self.dest_obj.0));
        set(report::S_DEST_RANGE, self.dest_range);
    }

    /// Reads the area back (tests and the TS contract goldens).
    pub fn from_floats(f: &[f32]) -> LocalReport {
        let g = |i: usize| f.get(i).copied().unwrap_or(0.0);
        let u = |i: usize| g(i).to_bits();
        LocalReport {
            bits: u(report::S_BITS),
            client_frame: u64::from(u(report::S_CLIENT_FRAME)),
            pos: Vec3::new(g(report::S_POS), g(report::S_POS + 1), g(report::S_POS + 2)),
            delta: Vec3::new(
                g(report::S_DELTA),
                g(report::S_DELTA + 1),
                g(report::S_DELTA + 2),
            ),
            angle: g(report::S_ANGLE),
            state: u(report::S_STATE),
            state_flags: u(report::S_STATE_FLAGS),
            motion: u(report::S_MOTION),
            motion_ex: u(report::S_MOTION_EX) as i32, // BOUNDS: bit pattern
            loop_: u(report::S_LOOP) as i32,          // BOUNDS: bit pattern
            motion_option: u(report::S_MOTION_OPTION),
            motion_frame: g(report::S_MOTION_FRAME),
            angle_x: g(report::S_ANGLE_X),
            acc_power: g(report::S_ACC_POWER),
            turn_angle: g(report::S_TURN_ANGLE),
            dest_pos: Vec3::new(
                g(report::S_DEST_POS),
                g(report::S_DEST_POS + 1),
                g(report::S_DEST_POS + 2),
            ),
            dest_forward: g(report::S_DEST_FORWARD) != 0.0,
            dest_obj: ObjId(u(report::S_DEST_OBJ)),
            dest_range: g(report::S_DEST_RANGE),
        }
    }
}

/// A local command (`LOCAL_*` ops 0x19-0x1C), applied at the start of the next stepped frame before the
/// control step.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LocalOp {
    /// `LOCAL_CMD`.
    Cmd {
        /// `OBJACT_*` (0 = plain `SetDestObj`).
        act: u32,
        /// The target.
        target: ObjId,
        /// `>= 0` explicit arrival range, `< 0` derive with [`cmd::approach_range`].
        range: f32,
        /// `AR_*` of the hand item (attacks) or of the skill (skills).
        ar: u32,
        /// `SetCmd` parameters.
        p: [i32; 3],
        /// `wire::LOCAL_CMD_BEHAVIOR_FIRST`.
        flags: u8,
    },
    /// `LOCAL_DEST_POS`.
    DestPos {
        /// The clicked position.
        pos: Vec3,
        /// Forced to `true` (`__BS_ADJUST_SYNC`).
        forward: bool,
    },
    /// `LOCAL_CLEAR` (`wire::CLEAR_*` bits).
    Clear(u8),
    /// `LOCAL_FACE`.
    Face(f32),
}

/// The control state of the active mover: the input latch, the pending correction, the call's report and
/// the cadence counters.
#[derive(Clone, Debug)]
pub struct ActiveControl {
    /// The latched `local_input` record.
    pub latch: InputLatch,
    /// `m_nCorr`: `-1` after a click-to-move or follow destination (the next collision or drop is a
    /// correction), `0` otherwise; the predicted `+22/+37` values are dropped with the prediction (PL-006).
    pub corr_pending: i32,
    /// The report of the current `step` call.
    pub report: LocalReport,
    /// The visual flight roll in degrees (`m_fRollAng`, `-45..=45`): the steering key rolls it, the render view
    /// carries it for the active mover (slot `roll`); 0 on the ground.
    pub roll: f32,
    /// `AR_*` of the pending command's hand item (range check of a bow attack on arrival).
    pub cmd_ar: u32,
    since_report: u32,
    /// The move group of the previous control step was not at rest (see the arrival report in `control_frame`).
    was_moving: bool,
    last_angle_frame: Option<u64>,
    ops: Vec<LocalOp>,
}

impl Default for ActiveControl {
    fn default() -> Self {
        ActiveControl {
            latch: InputLatch::default(),
            corr_pending: 0,
            report: LocalReport::empty(),
            roll: 0.0,
            cmd_ar: 0,
            since_report: 0,
            was_moving: false,
            last_angle_frame: None,
            ops: Vec::new(),
        }
    }
}

impl ActiveControl {
    /// A fresh control state (a new active mover).
    pub fn new() -> ActiveControl {
        ActiveControl::default()
    }

    /// Start of a `step` call: the report is cleared (`step(0)` leaves an all-zero report).
    pub fn begin_call(&mut self) {
        self.report = LocalReport::empty();
    }

    /// Queues a local command for the next stepped frame.
    pub fn queue(&mut self, op: LocalOp) {
        self.ops.push(op);
    }

    /// Number of queued local commands.
    pub fn pending_ops(&self) -> usize {
        self.ops.len()
    }

    /// Steps 1-2 of the client frame (Plan/31 §3.5): the queued local commands, then the control step of
    /// frame `cx.frame()` (ground or, with the FLY flag, flight), then the report capture. Returns the
    /// frame's report bits.
    pub fn control_frame(&mut self, m: &mut SimMover, cx: &mut Cx<'_>) -> u32 {
        let ops = std::mem::take(&mut self.ops);
        for op in ops {
            self.apply_op(m, cx, op);
        }
        let i = self.latch.take();
        m.input.lmb_held = i.has(input::LMB_HELD);
        m.input.auto_attack = i.has(input::AUTO_ATTACK);
        m.input.next_skill_none = i.has(input::NEXT_SKILL_NONE);
        m.req_flags = self.latch.req_flags;
        if self.latch.level > 0 {
            m.params.level = self.latch.level as u32; // BOUNDS: checked positive
        }
        let o = if m.act.flags.fly() {
            let f = fly(m, cx, &i, &mut self.roll);
            if f.angle {
                self.note_angle(cx.frame());
            }
            ControlOutcome {
                moved: f.moved,
                ..ControlOutcome::default()
            }
        } else {
            self.roll = 0.0; // MODE_GROUND resets the camera roll
            ground(m, cx, &i)
        };
        let mut bits = 0;
        if o.moved {
            bits |= report::MOVED;
        }
        if o.behavior {
            bits |= report::BEHAVIOR;
        }
        // PERIODIC: every 30 frames after the last report while moving or heading for a destination
        let moving = m.act.state.is_move()
            || matches!(
                m.act.state.move_(),
                OBJSTA_LMOVE | OBJSTA_RMOVE | OBJSTA_STOP_RUN
            );
        // A mover that came to rest on its own (arrival at the clicked destination, `on_arrive_at_pos`) is a state
        // change like a key release, but `act(STAND)` above finds it already standing, so no bit was raised. The
        // server copy keeps walking on the last reported delta (every state report clears its destination
        // position, `action_force_set`), so the stop must be reported: `MOVED` (found by the e2e bots: without
        // it the server's player walked on 20 m past the destination and a relog saved that position).
        if bits == 0 && self.was_moving && !moving {
            bits |= report::MOVED;
        }
        self.was_moving = moving;
        if bits != 0 {
            self.since_report = 0;
        } else if moving || !m.is_empty_dest() {
            self.since_report += 1;
            if self.since_report >= REPORT_PERIOD_FRAMES {
                bits |= report::PERIODIC;
                self.since_report = 0;
            }
        } else {
            self.since_report = 0;
        }
        self.report.bits |= bits;
        self.capture(m, cx.frame());
        bits
    }

    /// The `ANGLE` report (flying steering and tracking): at most once per 30 frames (`PostPlayerAngle`).
    pub fn note_angle(&mut self, frame: u64) -> bool {
        let due = self
            .last_angle_frame
            .is_none_or(|f| frame >= f + u64::from(REPORT_PERIOD_FRAMES));
        if due {
            self.last_angle_frame = Some(frame);
            self.report.bits |= report::ANGLE;
        }
        due
    }

    /// Records the state fields of the report (the state at the start of `frame`).
    pub fn capture(&mut self, m: &SimMover, frame: u64) {
        let r = &mut self.report;
        r.client_frame = frame;
        r.pos = m.pos;
        r.delta = m.act.delta;
        r.angle = m.angle;
        r.state = m.act.state.bits();
        r.state_flags = m.act.flags.bits();
        r.motion = m.motion.id;
        r.motion_ex = m.act.motion_ex;
        r.loop_ = m.motion.clock.lp as i32; // BOUNDS: ANILOOP_* are small
        r.motion_option = m.motion.option;
        r.motion_frame = m.motion.clock.frame_cur;
        r.angle_x = m.angle_x;
        r.acc_power = m.act.acc_power;
        r.turn_angle = m.act.turn_angle;
    }

    fn choose_dest_pos(&mut self, pos: Vec3, forward: bool) {
        let r = &mut self.report;
        r.bits |= report::DEST_POS;
        r.dest_pos = pos;
        r.dest_forward = forward;
        r.dest_obj = ObjId::NONE;
        r.dest_range = 0.0;
    }

    fn choose_dest_obj(&mut self, target: ObjId, range: f32) {
        let r = &mut self.report;
        r.bits |= report::DEST_OBJ;
        r.dest_pos = Vec3::ZERO;
        r.dest_forward = false;
        r.dest_obj = target;
        r.dest_range = range;
    }

    /// Applies one local command (`control::{local_cmd, local_dest_pos, local_clear, local_face}`).
    pub fn apply_op(&mut self, m: &mut SimMover, cx: &mut Cx<'_>, op: LocalOp) {
        match op {
            LocalOp::Cmd {
                act,
                target,
                range,
                ar,
                p,
                flags,
            } => self.local_cmd(m, cx, act, target, range, ar, p, flags),
            LocalOp::DestPos { pos, forward } => self.local_dest_pos(m, cx, pos, forward),
            LocalOp::Clear(what) => self.local_clear(m, cx, what),
            LocalOp::Face(a) => local_face(m, a),
        }
    }

    /// `local_cmd(act, target, range, ar, p[3], flags)` (Plan/31 §15.3): a non-empty `act` clears the act
    /// parameters; a NEW target (`__TRAFIC_1222`: an unchanged one reports nothing) optionally reports
    /// `BEHAVIOR` first (`BEHAVIOR_FIRST`, `__BS_ADJUST_SYNC`), sets the destination object with the explicit
    /// or derived range, `corr_pending = -1` and reports `DEST_OBJ`; then the command is `{act, p}`.
    #[allow(clippy::too_many_arguments)]
    pub fn local_cmd(
        &mut self,
        m: &mut SimMover,
        cx: &mut Cx<'_>,
        act: u32,
        target: ObjId,
        range: f32,
        ar: u32,
        p: [i32; 3],
        flags: u8,
    ) {
        if act != OBJACT_NONE {
            m.skill.act = OBJACT_NONE; // ClearActParam
        }
        if target != m.dest.obj {
            if flags & wire::LOCAL_CMD_BEHAVIOR_FIRST != 0 {
                self.report.bits |= report::BEHAVIOR;
            }
            let r = if range >= 0.0 {
                range
            } else {
                cmd::approach_range(
                    CmdKind::from_act(act),
                    m.params.stance.as_ref(),
                    ar,
                    &cx.env.flags,
                    m.params.hawkeye_rate,
                    m.act.flags.fly(),
                )
            };
            m.set_dest_obj(target, r);
            self.corr_pending = -1;
            self.choose_dest_obj(target, r);
        }
        m.cmd = ActiveCmd { act, params: p };
        self.cmd_ar = ar;
    }

    /// `local_dest_pos(pos, forward)`: click-to-move. `forward` is forced (`__BS_ADJUST_SYNC`).
    pub fn local_dest_pos(&mut self, m: &mut SimMover, cx: &mut Cx<'_>, pos: Vec3, _forward: bool) {
        m.set_dest_pos(pos, true);
        self.corr_pending = -1;
        self.choose_dest_pos(pos, true);
        cx.emit(SimEvent::DestPosChosen { pos, forward: true });
    }

    /// `local_clear(what)`.
    pub fn local_clear(&mut self, m: &mut SimMover, cx: &mut Cx<'_>, what: u8) {
        if what & wire::CLEAR_CMD != 0 {
            m.cmd = ActiveCmd::default();
        }
        if what & wire::CLEAR_DEST != 0 {
            m.clear_dest();
        }
        if what & wire::CLEAR_DEST_OBJ != 0 {
            m.clear_dest_obj();
        }
        if what & wire::CLEAR_CASTING != 0 {
            m.skill.casting_end_frame = 0;
            m.clear_state(cx);
        }
        if what & wire::CLEAR_SKILL_FLAG != 0 {
            m.mvrf.remove(MVRF_SKILL);
        }
    }

    /// The frame barrier of the active mover: answers the collision/drop corrections
    /// (`OnActCollision`/`OnActDrop` client branch, Plan/31 §15.2) and command arrivals. `events` are the
    /// mover's events of the frame; events produced here go to `out`.
    pub fn handle_events(
        &mut self,
        m: &mut SimMover,
        cx: &mut Cx<'_>,
        events: &[SimEvent],
        out: &mut Vec<SimEvent>,
    ) {
        for e in events {
            match e {
                SimEvent::Collided | SimEvent::Dropped if self.corr_pending != 0 => {
                    let collided = matches!(e, SimEvent::Collided);
                    let better = cx.env.flags.better_movement;
                    if self.corr_pending < 0 && !better {
                        m.clear_dest();
                        send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
                        if collided {
                            out.push(SimEvent::LmbRelease);
                        }
                    }
                    self.report.bits |= report::CORR;
                    if better && !m.is_empty_dest() {
                        self.report.bits |= report::KEPT_DEST;
                        if m.is_empty_dest_pos() {
                            self.choose_kept_obj(m);
                        } else {
                            self.report.dest_pos = m.dest.pos;
                            self.report.dest_forward = m.dest.forward;
                            self.report.dest_obj = ObjId::NONE;
                            self.report.dest_range = 0.0;
                        }
                    }
                    self.corr_pending = 0;
                    // the correction describes the state AFTER the collision: start of the next frame
                    self.capture(m, cx.frame() + 1);
                }
                SimEvent::FlyAngle => {
                    // the auto-tracking steered the mover: the report carries the new angles
                    if self.note_angle(cx.frame()) {
                        self.capture(m, cx.frame() + 1);
                    }
                }
                SimEvent::FlyMoved => {
                    self.report.bits |= report::MOVED;
                    self.capture(m, cx.frame() + 1);
                }
                SimEvent::CmdArrived { target, .. } => {
                    match cmd::on_arrival(m, cx, *target, self.cmd_ar) {
                        Arrival::Arrived { cmd, target } => {
                            out.push(SimEvent::CmdArrived { cmd, target });
                        }
                        Arrival::Aborted => out.push(SimEvent::CmdAborted),
                        Arrival::Quiet => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn choose_kept_obj(&mut self, m: &SimMover) {
        self.report.dest_pos = Vec3::ZERO;
        self.report.dest_forward = false;
        self.report.dest_obj = m.dest.obj;
        self.report.dest_range = m.dest.arrival_range;
    }
}

/// `local_face(angle)`: the cursor-point facing of a skill (no report bit of its own).
pub fn local_face(m: &mut SimMover, angle: f32) {
    m.angle = set_angle_quantized(angle);
}
