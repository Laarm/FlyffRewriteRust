//! Preview movers (Plan/31 §15.1; F-317 char select/create, `/dev/models`, portraits): local, non-networked
//! movers that exist with or without a world. They have no collision, gravity or steering, are not part of
//! the frame snapshot, never the active mover, emit no events, and play motions set by `ACT` records with
//! `msg = OBJMSG_MOTION` (`p1 = mti`, `p2 = loop`, `p3 = option`). A motion record with `queue_if_busy` is
//! queued (at most [`MAX_PREVIEW_QUEUE`]) behind a running `1PLAY` motion and started when it ends
//! (`MTI_GETUP` 1PLAY then `MTI_RUN` LOOP).

use super::ClientSim;
use crate::act::ActMsg;
use crate::anim::select::{set_blending, set_motion};
use crate::mover::SimMover;
use crate::pose::PoseState;
use crate::step::{Cx, NoRules, SimEnv, SimEvents};
use crate::types::*;
use flyff_common::ObjId;
use flyff_common::rng::GameRng;
use std::collections::VecDeque;

/// The queue length of a preview mover (Plan/31 §15.1).
pub const MAX_PREVIEW_QUEUE: usize = 4;

/// A queued motion: `(mti, loop, option)`.
pub type PreviewMotion = (u32, u32, u32);

/// A preview mover.
#[derive(Clone, Debug)]
pub struct PreviewMover {
    /// The simulation state (only the motion machinery is used).
    pub sim: SimMover,
    /// The model-space bones of the last evaluation.
    pub pose: PoseState,
    /// Skeleton id (`0` = none).
    pub skel_id: u32,
    /// Previews are visible and want their attachments until told otherwise.
    pub visible: bool,
    /// The attachment record is written.
    pub want_attach: bool,
    /// Motions waiting for the running `1PLAY` motion.
    pub queue: VecDeque<PreviewMotion>,
}

impl PreviewMover {
    /// A preview mover of `sim` with skeleton `skel_id`.
    pub fn new(sim: SimMover, skel_id: u32) -> PreviewMover {
        PreviewMover {
            sim,
            pose: PoseState::new(),
            skel_id,
            visible: true,
            want_attach: true,
            queue: VecDeque::new(),
        }
    }

    /// `true` while a `1PLAY` motion has not reached its end frame (a motion without a clip never runs, so it
    /// is never busy and cannot hold the queue back).
    pub fn busy(&self) -> bool {
        let c = &self.sim.motion.clock;
        c.lp & ANILOOP_1PLAY != 0 && !c.end_frame && c.frame_max > 0
    }

    /// `preview_step`: starts the next queued motion when the current one ended, then advances the motion
    /// clock at speed 1.0 and the blend weight (Plan/31 §12.4); no events, no state machine.
    pub fn step(&mut self, env: &SimEnv<'_>, rng: &mut dyn GameRng) {
        if !self.busy()
            && let Some((mti, lp, opt)) = self.queue.pop_front()
        {
            self.start(env, rng, mti, lp, opt);
        }
        let m = &mut self.sim;
        m.motion.clock.frame_move(1.0);
        if m.motion.blend.active {
            m.motion.blend.weight += 0.05;
            if m.motion.blend.weight >= 1.0 {
                set_blending(m, false);
            }
        }
    }

    fn start(&mut self, env: &SimEnv<'_>, rng: &mut dyn GameRng, mti: u32, lp: u32, opt: u32) {
        let mut ev = SimEvents::new();
        let mut rules = NoRules;
        let mut cx = Cx::new(env, &mut rules, rng, &mut ev);
        set_motion(&mut self.sim, &mut cx, mti, lp, opt);
    }
}

impl ClientSim {
    /// `ACT` on a preview mover: only `OBJMSG_MOTION` is understood; the record's frame is ignored.
    pub(super) fn preview_act(
        &mut self,
        id: ObjId,
        msg: &ActMsg,
        queue_if_busy: bool,
    ) -> Result<bool, SimError> {
        if msg.msg != OBJMSG_MOTION {
            return Ok(false);
        }
        let snap = self.snapshot();
        let now = self.frame;
        let ClientSim {
            flags,
            world,
            dynamic,
            motions,
            previews,
            rng,
            ..
        } = self;
        let env = super::make_env(
            *flags,
            world,
            dynamic,
            motions,
            &snap,
            now,
            crate::step::EventMode::Silent,
        );
        let mut done = false;
        if let Some(p) = previews.iter_mut().find(|p| p.sim.id == id) {
            let motion = (msg.p[0] as u32, msg.p[1] as u32, msg.p[2] as u32); // BOUNDS: bit patterns
            if queue_if_busy && p.busy() {
                if p.queue.len() < MAX_PREVIEW_QUEUE {
                    p.queue.push_back(motion);
                }
            } else {
                p.queue.clear();
                p.start(&env, rng, motion.0, motion.1, motion.2);
            }
            done = true;
        }
        self.snap = Some(snap);
        Ok(done)
    }
}
