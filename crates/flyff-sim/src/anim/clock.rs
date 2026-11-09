//! The motion frame clock (`CModel::FrameMove`, `src:_Common/Model.cpp:13-55`; Plan/31 §12.4; WP-SIM-05).
//!
//! One `frame_move` per 1/60 s frame advances `frame_cur` by `per_slerp * speed` (0.5 frames per frame at
//! speed 1.0, 30 animation frames per second). `LOOP` wraps, `1PLAY` and `CONT` clamp to the last frame
//! and set `end_frame` once `frame_cur` exceeds `frame_max - 1`.

use crate::num::ftoi_x86;
use crate::types::{ANILOOP_1PLAY, ANILOOP_LOOP};

/// The animation clock of one model (`CModel::m_fFrameCurrent` and friends).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MotionClock {
    /// `m_fFrameCurrent`: integer frame plus `slp`.
    pub frame_cur: f32,
    /// `m_fFrameOld`: the value before the last advance (`-1` after `init_animate`; may be negative after
    /// a wrap in `LOOP` mode).
    pub frame_old: f32,
    /// `m_nFrameMax`: frame count of the loaded clip (`0` = no clip, nothing advances).
    pub frame_max: i32,
    /// `m_fPerSlerp` (already `trunc3`-ed).
    pub per_slerp: f32,
    /// `m_fSlp`: the fractional part carried across the advance.
    pub slp: f32,
    /// `ANILOOP_*` bits.
    pub lp: u32,
    /// `m_bEndFrame`: set when `frame_cur` passes `frame_max - 1`; reset only by `set_motion`/`init_animate`.
    pub end_frame: bool,
    /// `m_nPause`: hit-stop frames; while non-zero `frame_move` does nothing (client only).
    pub pause: i32,
    /// `m_bSlow`: advance by `0.1` per frame (only the sword-trail pre-simulation uses it).
    pub slow: bool,
}

impl Default for MotionClock {
    fn default() -> Self {
        MotionClock::new()
    }
}

impl MotionClock {
    /// The state `CModel::InitAnimate` leaves: frame 0, `frame_old = -1`, `frame_max = 0`, loop mode `LOOP`,
    /// `per_slerp = 0.5`.
    pub const fn new() -> MotionClock {
        MotionClock {
            frame_cur: 0.0,
            frame_old: -1.0,
            frame_max: 0,
            per_slerp: 0.5,
            slp: 0.0,
            lp: ANILOOP_LOOP,
            end_frame: false,
            pause: 0,
            slow: false,
        }
    }

    /// `CModel::InitAnimate`.
    pub fn init_animate(&mut self) {
        *self = MotionClock::new();
    }

    /// `CModel::FrameMove(speed)`: one frame of advance (the caller passes `ani_speed * walk_factor`).
    pub fn frame_move(&mut self, speed: f32) {
        if self.pause != 0 {
            return;
        }
        if self.frame_max <= 0 {
            return;
        }
        self.frame_old = self.frame_cur;
        let max_f = self.frame_max as f32;
        if self.frame_old > (self.frame_max - 1) as f32 {
            self.frame_old -= max_f;
        }
        if self.slow {
            self.frame_cur += 0.1;
        } else {
            self.frame_cur += self.per_slerp * speed;
        }
        let mut f = ftoi_x86(self.frame_cur);
        self.slp = self.frame_cur - f as f32;
        if self.lp & ANILOOP_LOOP != 0 {
            if self.frame_cur > (self.frame_max - 1) as f32 {
                self.end_frame = true;
                f = f.checked_rem(self.frame_max).unwrap_or(0);
            }
        } else if self.frame_cur > (self.frame_max - 1) as f32 {
            self.end_frame = true;
            self.slp = 0.0;
            f = self.frame_max - 1;
        }
        self.frame_cur = f as f32 + self.slp;
    }

    /// `GetNextFrame`: the frame after `frame_cur`, wrapping to `0` in `LOOP` mode and holding the last
    /// frame otherwise.
    pub fn next_frame(&self) -> i32 {
        let n = ftoi_x86(self.frame_cur).saturating_add(1);
        if n >= self.frame_max {
            if self.lp & ANILOOP_LOOP != 0 {
                0
            } else {
                (self.frame_max - 1).max(0)
            }
        } else {
            n
        }
    }

    /// `MotionClock::set_frame(f)` (forced-set phase alignment, Plan/31 §6.5): `frame_cur = frame_old =
    /// clamp(f, 0, frame_max - 1)` and `end_frame = false`.
    pub fn set_frame(&mut self, f: f32) {
        let hi = (self.frame_max - 1).max(0) as f32;
        let v = f.max(0.0).min(hi);
        self.frame_cur = v;
        self.frame_old = v;
        self.end_frame = false;
    }

    /// `true` when the clip plays once (`1PLAY`).
    pub fn is_one_play(&self) -> bool {
        self.lp & ANILOOP_1PLAY != 0
    }
}

/// `ObjClock`: the clock of an `.o3d` built-in animation (ctrls and animated decor, Plan/31 §12.8). The
/// attribute rule for `.o3d` built-ins is the exact-equality rule of [`super::attr::is_attr_o3d`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ObjClock {
    /// The frame clock (`per_slerp` = `trunc3(raw)`).
    pub clock: MotionClock,
}

impl ObjClock {
    /// A clock for a built-in clip of `frames` frames and the file's raw `perSlerp`, playing in `lp` mode
    /// (`trunc3` is applied here, once).
    pub fn new(frames: u32, per_slerp_raw: f32, lp: u32) -> ObjClock {
        let mut clock = MotionClock::new();
        clock.frame_max = i32::try_from(frames).unwrap_or(i32::MAX);
        clock.per_slerp = crate::num::trunc3(per_slerp_raw);
        clock.lp = lp;
        ObjClock { clock }
    }

    /// One frame of advance at `speed`.
    pub fn frame_move(&mut self, speed: f32) {
        self.clock.frame_move(speed);
    }
}
