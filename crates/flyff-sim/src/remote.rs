//! Remote movers on the client (Plan/31 §15.4-§15.5; WP-SIM-16): applying timestamped states and intents
//! with silent fast-forward, the keyframe rule with its 128-frame position history, and the original `/8`
//! render smoothing (`CMover::Interpolate`, `src:_Common/Mover.cpp:4253-4335`; PL-007, PL-008, PL-009).
//!
//! Every function takes the client's "now" through `env.frame` and the mover's time lag in frames (`now -
//! F` of the message stamp; `0` when the stamp lies in the future, clock skew). The fast-forward is silent
//! (cosmetic events suppressed) and records the logical position of every frame into the mover's
//! [`PosHistory`], which the `MoverKeyframe` rule compares against.

use crate::act::{ActMsg, send_act_msg_cx};
use crate::mover::{MagicKind, QueuedAct, QueuedMagic, SimMover};
use crate::num::{Vec3, length, set_angle_quantized};
use crate::step::{Cx, EventMode, SimEnv, SimRules, step_frame};
use crate::sync::{self, INTENT_FF_CAP, IntentOp, IntentOutcome, IntentStampData, StateUpdate};
use crate::types::*;
use flyff_common::ObjId;
use flyff_common::SimFrame;
use flyff_common::rng::GameRng;

/// `KEYFRAME_SNAP_DIST`: a keyframe whose position differs from the history by more than this snaps
/// (Plan/31 §3.1, 11 §8.2).
pub const KEYFRAME_SNAP_DIST: f32 = 0.5;
/// Length of the position history (2.13 s).
pub const HISTORY_LEN: usize = 128;
/// The cap of the silent catch-up of a keyframe snap (same as the intent cap, 2 s).
pub const KEYFRAME_FF_CAP: u32 = INTENT_FF_CAP;

/// The logical position of a mover at the START of each of the last [`HISTORY_LEN`] frames.
#[derive(Clone, Debug)]
pub struct PosHistory {
    slots: Vec<(u64, Vec3)>,
}

impl Default for PosHistory {
    fn default() -> Self {
        PosHistory::new()
    }
}

impl PosHistory {
    /// An empty history.
    pub fn new() -> PosHistory {
        PosHistory {
            slots: vec![(u64::MAX, Vec3::ZERO); HISTORY_LEN],
        }
    }

    fn slot(frame: u64) -> usize {
        (frame % HISTORY_LEN as u64) as usize // BOUNDS: remainder is below 128
    }

    /// Records that the mover was at `pos` at the start of `frame`.
    pub fn record(&mut self, frame: u64, pos: Vec3) {
        if let Some(s) = self.slots.get_mut(Self::slot(frame)) {
            *s = (frame, pos);
        }
    }

    /// The position at the start of `frame`, when it is still in the ring.
    pub fn get(&self, frame: u64) -> Option<Vec3> {
        self.slots
            .get(Self::slot(frame))
            .filter(|(f, _)| *f == frame)
            .map(|(_, p)| *p)
    }

    /// Forgets everything (the mover was re-based by a forced state).
    pub fn clear(&mut self) {
        self.slots.fill((u64::MAX, Vec3::ZERO));
    }

    /// Shifts every recorded position by `d` (the keyframe's logical correction).
    pub fn shift(&mut self, d: Vec3) {
        for (f, p) in &mut self.slots {
            if *f != u64::MAX {
                *p += d;
            }
        }
    }
}

/// `env` for frame `frame` in [`EventMode::Silent`].
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

/// `env` at frame `frame` with the mode of `env`.
fn env_at<'a>(env: &SimEnv<'a>, frame: u64) -> SimEnv<'a> {
    SimEnv {
        frame: SimFrame(frame),
        role: env.role,
        flags: env.flags,
        world: env.world,
        dynamic: env.dynamic,
        motions: env.motions,
        lookup: env.lookup,
        mode: env.mode,
    }
}

/// Steps `m` through the `frames` frames before `env.frame` silently (`sync::fast_forward`), recording the
/// position at the start of each frame into `hist`; afterwards the mover is at the start of `env.frame`
/// (and recorded as such).
#[allow(clippy::too_many_arguments)]
pub fn catch_up(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    frames: u32,
    hist: &mut PosHistory,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut crate::step::SimEvents,
) {
    let first = env.frame.0.saturating_sub(u64::from(frames));
    for f in first..first + u64::from(frames) {
        hist.record(f, m.pos);
        step_frame(m, &silent_env(env, f), rules, rng, ev);
    }
    hist.record(env.frame.0, m.pos);
}

/// `MoverStateNtf` and the other timestamped states (Plan/31 §15.4): the state is forced at its frame
/// (`ActionForceSet`, or `ActionForceSet2` for FLY), `corr` also clears the destination, then the mover is
/// extrapolated `min(lag, cap)` frames silently (`lag = now - F`, `0` for a stamp in the future).
#[allow(clippy::too_many_arguments)]
pub fn apply_state(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    s: &StateUpdate,
    corr: bool,
    lag: u64,
    cap: u32,
    hist: &mut PosHistory,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut crate::step::SimEvents,
) {
    sync::apply_state(m, env, s, rules, rng, ev);
    if corr {
        m.clear_dest();
    }
    let k = u32::try_from(lag.min(u64::from(cap))).unwrap_or(cap);
    hist.clear();
    catch_up(m, env, k, hist, rules, rng, ev);
}

/// NPC intents and relayed player destinations (`sync::apply_intent` + the history): the stamp state at
/// `F`, the operation, then `min(lag, 120)` frames of silent catch-up; an older intent marks the mover
/// `lagging` until the next keyframe.
#[allow(clippy::too_many_arguments)]
pub fn apply_intent(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    stamp: &IntentStampData,
    op: IntentOp,
    lag: u64,
    hist: &mut PosHistory,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut crate::step::SimEvents,
) -> IntentOutcome {
    // steps 1-2 of the intent without any catch-up; the catch-up below records the history
    sync::apply_intent(m, env, stamp, op, 0, rules, rng, ev);
    let stepped = u32::try_from(lag.min(u64::from(INTENT_FF_CAP))).unwrap_or(INTENT_FF_CAP);
    let lagging = lag > u64::from(INTENT_FF_CAP);
    if lagging {
        m.lagging = true;
    }
    hist.clear();
    catch_up(m, env, stepped, hist, rules, rng, ev);
    IntentOutcome { stepped, lagging }
}

/// A `MoverKeyframe` (`server_frame` already converted to the local frame `F`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Keyframe {
    /// The local frame the keyframe is valid at (start of the frame).
    pub frame: u64,
    /// The server's logical position.
    pub pos: Vec3,
    /// The server's yaw.
    pub angle: f32,
    /// The server's state word.
    pub state: u32,
}

/// What a keyframe did.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KeyframeResult {
    /// The position agreed within [`KEYFRAME_SNAP_DIST`]: the logical position moved by the error.
    Shifted {
        /// `keyframe.pos - history(F)`.
        error: Vec3,
    },
    /// The mover snapped to the keyframe and was caught up with its current intent.
    Snapped,
}

/// The keyframe rule (Plan/31 §15.4): `err = |hist(F) - kf.pos|`; `err > 0.5` or `F` not in the history
/// snaps (position, yaw and state word) and fast-forwards `min(now - F, 120)` frames with the current
/// intent; otherwise `pos += kf.pos - hist(F)` (the `/8` filter hides it). Either way the mover is no longer
/// `lagging`.
///
/// A keyframe for the frame that is about to run (`F == now`, the case of a keyframe that was held until its frame:
/// the command queue drains before the step records the history of the frame) is compared with the mover's
/// logical position, which IS the history entry of `F` once it is recorded. Without that, every keyframe stamped
/// at the client's own frame would find an empty slot and snap, whatever its error (found by the bot's K1 on
/// loopback: 389 of 389 keyframes counted as re-bases).
#[allow(clippy::too_many_arguments)]
pub fn apply_keyframe(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    kf: &Keyframe,
    hist: &mut PosHistory,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut crate::step::SimEvents,
) -> KeyframeResult {
    m.lagging = false;
    let now = env.frame.0;
    let at_f = if kf.frame == now {
        Some(m.pos)
    } else {
        hist.get(kf.frame)
    };
    let near = at_f.filter(|h| length(*h - kf.pos) <= KEYFRAME_SNAP_DIST);
    if let Some(h) = near {
        let d = kf.pos - h;
        m.pos += d;
        hist.shift(d);
        return KeyframeResult::Shifted { error: d };
    }
    m.pos = kf.pos;
    m.angle = set_angle_quantized(kf.angle);
    m.act.state = ObjState(kf.state);
    let lag = now.saturating_sub(kf.frame);
    let k = u32::try_from(lag.min(u64::from(KEYFRAME_FF_CAP))).unwrap_or(KEYFRAME_FF_CAP);
    hist.clear();
    catch_up(m, env, k, hist, rules, rng, ev);
    KeyframeResult::Snapped
}

/// `MotionNtf`, `SendActMsg`, the attack relays and `UseSkillNtf` (Plan/31 §15.4): the message is sent at
/// its frame `F` (`F = now - lag`; remote movers enqueue attack messages that returned `0` when
/// `queue_if_busy`), then the mover is extrapolated `min(lag, cap)` frames silently so the swing is in
/// phase. Returns the message result.
#[allow(clippy::too_many_arguments)]
pub fn apply_act(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    msg: ActMsg,
    queue_if_busy: bool,
    lag: u64,
    cap: u32,
    hist: &mut PosHistory,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut crate::step::SimEvents,
) -> ActResult {
    let k = u32::try_from(lag.min(u64::from(cap))).unwrap_or(cap);
    let at = env.frame.0.saturating_sub(u64::from(k));
    let r = {
        let e = env_at(env, at);
        let mut cx = Cx::new(&e, rules, rng, ev);
        let r = send_act_msg_cx(m, &mut cx, msg);
        if r == ACT_IGNORED && queue_if_busy {
            enqueue_attack(m, &msg);
        }
        r
    };
    // the message happened at `at`; the frames `at..now` are stepped silently (history from `at`)
    let e = env_at(env, env.frame.0);
    catch_up(m, &e, k, hist, rules, rng, ev);
    r
}

/// Queues a refused attack message like the server handlers do (Plan/31 §6.6 rule 1): `ATK1..4` and
/// `SP_ATK1/2` into the melee queue, `ATK_MAGIC1` and `ATK_RANGE1` into the magic queue.
pub fn enqueue_attack(m: &mut SimMover, am: &ActMsg) {
    match am.msg {
        OBJMSG_ATK1 | OBJMSG_ATK2 | OBJMSG_ATK3 | OBJMSG_ATK4 | OBJMSG_SP_ATK1 | OBJMSG_SP_ATK2 => {
            sync::enqueue_melee(
                m,
                QueuedAct {
                    msg: am.msg,
                    p1: am.p[0],
                    p2: am.p[1],
                    p3: am.p[2],
                },
            );
        }
        OBJMSG_ATK_MAGIC1 => {
            sync::enqueue_magic(
                m,
                QueuedMagic {
                    target: ObjId(am.p[0] as u32), // BOUNDS: ids travel as bit patterns
                    kind: MagicKind::Magic { power: am.p[1] },
                    sfx_hit: am.p[2] as u32, // BOUNDS: bit pattern
                },
            );
        }
        OBJMSG_ATK_RANGE1 => {
            sync::enqueue_magic(
                m,
                QueuedMagic {
                    target: ObjId(am.p[0] as u32), // BOUNDS: ids travel as bit patterns
                    kind: MagicKind::Range { item: am.p[1] },
                    sfx_hit: am.p[2] as u32, // BOUNDS: bit pattern
                },
            );
        }
        _ => {}
    }
}

/// The client closure of `sync::drain_magic` (Plan/31 §6.6): `ATK_MAGIC1` / `ATK_RANGE1` with the queued
/// target; the result is mapped to the drain protocol (`1` executed, `0` busy, `-1` dropped).
pub fn drain_magic_queue(m: &mut SimMover, cx: &mut Cx<'_>) -> bool {
    // the closure needs the context while `drain_magic` borrows the mover: take the queue entry by entry
    while let Some(q) = m.act.queues.magic.head() {
        let am = match q.kind {
            MagicKind::Magic { power } => ActMsg::with(
                OBJMSG_ATK_MAGIC1,
                [q.target.0 as i32, power, q.sfx_hit as i32, 0, 0], // BOUNDS: bit patterns
            ),
            MagicKind::Range { item } => ActMsg::with(
                OBJMSG_ATK_RANGE1,
                [q.target.0 as i32, item, q.sfx_hit as i32, 0, 0], // BOUNDS: bit patterns
            ),
        };
        let r = send_act_msg_cx(m, cx, am);
        if r > 0 {
            m.act.queues.magic.pop();
            return true;
        } else if r == 0 {
            return false;
        }
        m.act.queues.magic.pop();
    }
    false
}

// ---------------------------------------------------------------------------------------------------
// render smoothing
// ---------------------------------------------------------------------------------------------------

/// `fabs(a - b) > 0.001` evaluated in `double` like the original (`fabs(float)` promoted for the compare).
fn differs(a: f32, b: f32) -> bool {
    f64::from((a - b).abs()) > 0.001
}

/// `CMover::Interpolate` (Plan/31 §15.5), run once per frame after stepping for every world mover:
/// `render.prev = render.cur` first, then the active mover tracks its logical state exactly and every
/// other mover approaches it by 1/8 per frame (KEEP-QUIRK: within 4 degrees the yaw snaps AND still takes
/// the `d / 8` step, which overshoots and then decays by 1/8 per frame).
pub fn smooth(m: &mut SimMover) {
    m.render.prev_pos = m.render.scr_pos;
    m.render.prev_angle = m.render.scr_angle;
    let (pos, angle, angle_x) = (m.pos, m.angle, m.angle_x);
    let r = &mut m.render;
    if m.is_active {
        if differs(pos.x, r.scr_pos.x) || differs(pos.y, r.scr_pos.y) || differs(pos.z, r.scr_pos.z)
        {
            r.scr_pos = pos;
        }
        if differs(angle, r.scr_angle) || differs(angle_x, r.scr_angle_x) {
            r.scr_angle = angle;
            r.scr_angle_x = angle_x;
        }
        return;
    }
    if differs(pos.x, r.scr_pos.x) || differs(pos.y, r.scr_pos.y) || differs(pos.z, r.scr_pos.z) {
        let v = pos - r.scr_pos;
        let d = v.x * v.x + v.y * v.y + v.z * v.z;
        if f64::from(d) > 0.001 {
            // `v / 8` is `v * (1 / 8)`: exact
            r.scr_pos += Vec3::new(v.x * 0.125, v.y * 0.125, v.z * 0.125);
        } else {
            r.scr_pos = pos;
        }
    }
    if differs(angle, r.scr_angle) || differs(angle_x, r.scr_angle_x) {
        let mut d = angle - r.scr_angle;
        if f64::from(d.abs()) < 4.0 {
            r.scr_angle = angle;
        }
        if d < -180.0 {
            d += 360.0;
        } else if d > 180.0 {
            d -= 360.0;
        }
        r.scr_angle += d / 8.0;
        if r.scr_angle > 360.0 {
            r.scr_angle -= 360.0;
        } else if r.scr_angle < 0.0 {
            r.scr_angle += 360.0;
        }
        r.scr_angle_x = angle_x.clamp(-45.0, 45.0);
    }
}

/// `reset_render_smoothing()` for one mover (hidden-tab return, own `SetPos`, PL-009/PL-014).
pub fn reset_smoothing(m: &mut SimMover) {
    sync::reset_render_smoothing(m);
}

/// The interpolated render position and yaw between the previous and the current smoothed values
/// (`render_view(alpha)`, PL-009): `lerp` of the position, shortest arc of the yaw.
pub fn interpolate(m: &SimMover, alpha: f32) -> (Vec3, f32) {
    let r = &m.render;
    let a = alpha.clamp(0.0, 1.0);
    let pos = r.prev_pos + (r.scr_pos - r.prev_pos) * a;
    let mut d = r.scr_angle - r.prev_angle;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    let mut yaw = r.prev_angle + d * a;
    if yaw < 0.0 {
        yaw += 360.0;
    } else if yaw >= 360.0 {
        yaw -= 360.0;
    }
    (pos, yaw)
}
