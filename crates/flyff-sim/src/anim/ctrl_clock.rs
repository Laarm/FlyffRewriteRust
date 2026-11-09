//! The animation state machine of a common ctrl (`CCommonCtrl::Process` animation part, `src:_Common/
//! CommonCtrl.cpp:373-443`; Plan/15 §14.5, Plan/31 §12.8; WP-SIM-24).
//!
//! The server (`flyff-game::world::ctrl`) and the client decor entry of a ctrl (`client::decor`) run this
//! one implementation, so a door opens, holds and closes in the same frames on both sides:
//!
//! ```text
//! step():   clock.frame_move()                               (CModel::FrameMove, o3d object mode)
//!           playing ? frozen = frame : frame = frozen         (m_bAniPlay / m_fFrameCount)
//!           CK2_KEEP && end_frame: Init() (playing = false, frozen = 0, no pending resume),
//!                                   end_frame = false, frame = 0
//!           CK3_HALF: playing && frame == frame_max / 2 (exact)   -> pause, resume 20 s later
//!                     !playing && resume_at < now                  -> resume
//! ```
//!
//! `CK2_FADE` (blend factor, delete) and the trap are the caller's: FADE and KEEP are alternatives of one
//! `if / else if` in the original, so a FADE ctrl never reaches the KEEP branch here. Times are sim frames
//! (`SimFrame`): the original compared `g_tmCurrent` in milliseconds against `now + SEC(20)`, here the
//! delay is [`CTRL_HALF_DELAY_FRAMES`] frames (1 200 = 20 s) and the resume test has frame granularity.

use super::clock::MotionClock;
use crate::num::{ftoi_x86, trunc3};
use crate::types::{ANILOOP_CONT, ANILOOP_LOOP};

/// `SEC(20)` in 1/60 s frames: the `CK3_HALF` pause.
pub const CTRL_HALF_DELAY_FRAMES: u64 = 1_200;

/// What one [`CtrlClock::step`] did that the caller may care about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CtrlStep {
    /// `CK2_KEEP` re-armed the ctrl (`Init()`): the caller resets the rest of its state (trap, blend, ...).
    pub rearmed: bool,
}

/// The clock of one ctrl.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CtrlClock {
    /// The frame clock (`frame_max`, `trunc3(per_slerp)`, loop mode, `end_frame`).
    pub clock: MotionClock,
    /// `m_bAniPlay`.
    pub playing: bool,
    /// `m_fFrameCount`: the frame held while not playing.
    pub frozen: f32,
    /// `m_dwAniDelay`: the frame at which a `CK3_HALF` pause ends (`None` = `0xFFFFFFFF`).
    pub resume_at: Option<u64>,
    /// `CK2_KEEP`.
    pub keep: bool,
    /// `CK3_HALF`.
    pub half: bool,
    /// Guild-house furniture with a built-in animation starts playing again after a re-arm.
    pub autoplay: bool,
}

impl CtrlClock {
    /// A stopped clock at frame 0 for a model of `frames` frames and the raw `perSlerp` of its `.o3d`
    /// (`trunc3` is applied here, once). The loop mode is the model default (`LOOP`) until
    /// [`CtrlClock::set_action_play`].
    pub fn new(frames: u32, per_slerp_raw: f32, keep: bool, half: bool) -> CtrlClock {
        let mut clock = MotionClock::new();
        clock.frame_max = i32::try_from(frames).unwrap_or(i32::MAX);
        clock.per_slerp = trunc3(per_slerp_raw);
        clock.lp = ANILOOP_LOOP;
        CtrlClock {
            clock,
            playing: false,
            frozen: 0.0,
            resume_at: None,
            keep,
            half,
            autoplay: false,
        }
    }

    /// `CCommonCtrl::SetActionPlay` (clock part): `SetLoop(ANILOOP_CONT)`, `m_bAniPlay = TRUE`.
    pub fn set_action_play(&mut self) {
        self.clock.lp = ANILOOP_CONT;
        self.playing = true;
    }

    /// The `CTRL_STATE` / forced form: play (`CONT`, or `LOOP`), optionally from `frame` (`None` keeps the
    /// frame the ctrl holds).
    pub fn play_from(&mut self, frame: Option<f32>, looping: bool) {
        if let Some(f) = frame {
            let hi = (self.clock.frame_max - 1).max(0) as f32;
            let v = if f.is_finite() { f.clamp(0.0, hi) } else { 0.0 };
            self.clock.frame_cur = v;
            self.clock.frame_old = v;
            self.clock.end_frame = false;
            self.frozen = v;
        }
        self.clock.lp = if looping { ANILOOP_LOOP } else { ANILOOP_CONT };
        self.playing = true;
        self.resume_at = None;
    }

    /// `Init()` for the clock: stopped at frame 0 (a guild-house furniture model with frames plays on).
    pub fn rearm(&mut self) {
        self.playing = self.autoplay && self.clock.frame_max > 0;
        self.frozen = 0.0;
        self.resume_at = None;
    }

    /// One sim frame (`now` = the frame number, for the `CK3_HALF` resume).
    pub fn step(&mut self, now: u64) -> CtrlStep {
        let mut out = CtrlStep::default();
        self.clock.frame_move(1.0);
        if self.playing {
            self.frozen = self.clock.frame_cur;
        } else {
            self.clock.frame_cur = self.frozen;
        }
        if self.keep && self.clock.end_frame {
            self.rearm();
            self.clock.end_frame = false;
            self.clock.frame_cur = 0.0;
            out.rearmed = true;
        }
        if self.half {
            let mid = (self.clock.frame_max / 2) as f32; // BOUNDS: integer division like the original
            if self.playing && self.clock.frame_cur == mid {
                self.playing = false;
                self.resume_at = Some(now.saturating_add(CTRL_HALF_DELAY_FRAMES));
            }
            if !self.playing && self.resume_at.is_some_and(|t| t < now) {
                self.resume_at = None;
                self.playing = true;
            }
        }
        out
    }

    /// The frame to show and to pose the collider with.
    pub fn frame(&self) -> f32 {
        self.clock.frame_cur
    }

    /// `GetNextFrame` of the clock.
    pub fn next_frame(&self) -> i32 {
        self.clock.next_frame()
    }

    /// `true` once the clock reached the end of a non-looping play (`IsEndFrame`).
    pub fn end_frame(&self) -> bool {
        self.clock.end_frame
    }

    /// `true` when a non-looping play reached its end (a looping clock latches `end_frame` at its first wrap
    /// but never ends).
    pub fn ended(&self) -> bool {
        self.clock.end_frame && self.clock.lp & ANILOOP_LOOP == 0
    }

    /// The array slot of the current frame (`(int)frame`).
    pub fn slot(&self) -> i32 {
        ftoi_x86(self.clock.frame_cur)
    }
}
