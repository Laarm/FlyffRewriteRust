//! `CMotion::AnimateBone` (`src:_Common/Bone.cpp:656-788`; Plan/31 §13.2; ref/18 §7.1; WP-SIM-12).
//!
//! Per bone: the key at `floor(frame)` and the key at the clock's next frame are slerped/lerped by the
//! fractional part; while a motion transition is running the pose of the OLD motion — frozen at
//! `old_frame`, the integer frame at the switch — is blended in with the weight `w` (the clock adds 0.05
//! per frame, 20 frames per transition); the local matrix is `R(q) · T(p)`, a static bone keeps its full
//! matrix (scale included) unless it is blended (the scale is then dropped); the model-space matrix is
//! `local · parent` (`m_mUpdateBone`).
//!
//! The functions are total: indices are clamped instead of read out of bounds (the original read past the
//! key arrays when an old frame did not fit), and a bone the old clip does not have is simply not blended.

use super::clip::{ClipBone, SimClip};
use super::quat::{
    Key, Mat44, Quat, decompose, lerp, mat_rt, mat44_from_43, mul_affine, slerp, transform_coord,
};
use super::skeleton::SimSkeleton;
use crate::anim::MotionClock;
use crate::mover::MotionState;
use crate::num::{Vec3, ftoi_x86};

/// The clock-dependent inputs of one evaluation (the trailing arguments of `AnimateBone`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sample {
    /// `fFrameCurrent`: integer frame plus the interpolation fraction.
    pub frame: f32,
    /// `nNextFrame`: the key interpolated towards ([`MotionClock::next_frame`]: wraps to 0 in `LOOP`).
    pub next: i32,
    /// `nFrameOld`: the frozen frame of the old motion (`MotionState::prev_frame`).
    pub old_frame: i32,
    /// `bMotionTrans`: blend with the old motion.
    pub blend: bool,
    /// `fBlendWeight`: 0 = old pose, 1 = new pose.
    pub weight: f32,
}

impl Sample {
    /// The sample of a mover's current motion (`CModelObject::UpdateMatrixBone`): the clock, the frozen old
    /// frame and the blend state.
    pub fn from_motion(m: &MotionState) -> Sample {
        Sample {
            frame: m.clock.frame_cur,
            next: m.clock.next_frame(),
            old_frame: m.prev_frame,
            blend: m.blend.active,
            weight: m.blend.weight,
        }
    }

    /// A sample at the clock's current frame without blending (server use, [`event_point`]).
    pub fn from_clock(c: &MotionClock) -> Sample {
        Sample {
            frame: c.frame_cur,
            next: c.next_frame(),
            old_frame: 0,
            blend: false,
            weight: 1.0,
        }
    }
}

/// The key at `idx`, clamped into the track (a negative index reads key 0, an index past the end the last
/// key); the identity for an empty track.
fn key_at(keys: &[Key], idx: i32) -> Key {
    let i = usize::try_from(idx).unwrap_or(0);
    keys.get(i)
        .or_else(|| keys.last())
        .copied()
        .unwrap_or((Quat::IDENTITY, Vec3::ZERO))
}

/// `local · parent` (or `local` for a root), with an overflow guard: a result that is not finite (only
/// possible for absurd, but finite, inputs) becomes the identity so that no NaN ever reaches a palette
/// (Plan/31 §2.1 rule 5; FIX, the original had no such guard).
fn finish(local: Mat44, parent: Option<&Mat44>) -> Mat44 {
    let m = match parent {
        Some(p) => mul_affine(&local, p),
        None => local,
    };
    if m.0.iter().all(|x| x.is_finite()) {
        m
    } else {
        Mat44::IDENTITY
    }
}

/// The per-evaluation constants (`nCurrFrame`, `fSlp`, ...) and the per-bone local pose.
struct Eval<'a> {
    old: &'a SimClip,
    f: i32,
    slp: f32,
    next: i32,
    old_frame: i32,
    blend: bool,
    w: f32,
}

impl<'a> Eval<'a> {
    fn new(m: &'a SimClip, old: Option<&'a SimClip>, s: &Sample) -> Eval<'a> {
        // FIX: a non-finite frame or weight (never produced by the clock) would poison every matrix
        let frame = if s.frame.is_finite() { s.frame } else { 0.0 };
        let w = if s.weight.is_finite() { s.weight } else { 0.0 };
        let f = ftoi_x86(frame);
        Eval {
            old: old.unwrap_or(m),
            f,
            slp: frame - f as f32,
            next: s.next,
            old_frame: s.old_frame,
            blend: s.blend,
            w,
        }
    }

    /// The old motion's pose of bone `i`: its key at the frozen frame, or the decomposed static matrix;
    /// `None` when the old clip has no such bone.
    fn old_key(&self, i: usize) -> Option<Key> {
        match self.old.bones.get(i)? {
            ClipBone::Animated { keys, .. } => Some(key_at(keys, self.old_frame)),
            ClipBone::Static { local_tm, .. } => Some(decompose(local_tm)),
        }
    }

    /// The local matrix of bone `i` (before the parent multiplication).
    fn local(&self, i: usize, bone: &ClipBone) -> Mat44 {
        match bone {
            ClipBone::Animated { keys, .. } => {
                let (q0, p0) = key_at(keys, self.f);
                let (q1, p1) = key_at(keys, self.next);
                let mut q = slerp(q0, q1, self.slp);
                let mut p = lerp(p0, p1, self.slp);
                if self.blend
                    && let Some((qo, po)) = self.old_key(i)
                {
                    q = slerp(qo, q, self.w);
                    p = lerp(po, p, self.w);
                }
                mat_rt(q, p)
            }
            ClipBone::Static { local_tm, .. } => {
                if self.blend
                    && let Some((qo, po)) = self.old_key(i)
                {
                    let (ql, pl) = decompose(local_tm);
                    mat_rt(slerp(qo, ql, self.w), lerp(po, pl, self.w))
                } else {
                    mat44_from_43(local_tm)
                }
            }
        }
    }
}

/// `CMotion::AnimateBone`: writes the model-space matrix of bone `i` into `out[i]` for
/// `i < min(out.len(), clip.bones.len())` (Plan/31 §13.2). `old` is the previous motion (`None` = the clip
/// itself, as in the original when there is none); bones beyond the shorter of clip and `out` are not
/// touched (the caller keeps them at the bind pose, [`super::PoseState`]).
///
/// The hierarchy is the clip's own (`ClipBone::parent`, MOTIONCORE), a parent index that does not precede
/// its bone (impossible for a validated pack) makes the bone a root.
pub fn animate_bones(out: &mut [Mat44], m: &SimClip, old: Option<&SimClip>, s: &Sample) {
    let ev = Eval::new(m, old, s);
    let n = out.len().min(m.bones.len());
    for (i, bone) in m.bones.iter().take(n).enumerate() {
        let local = ev.local(i, bone);
        let (done, rest) = out.split_at_mut(i);
        let Some(slot) = rest.first_mut() else { break };
        *slot = finish(
            local,
            usize::try_from(bone.parent())
                .ok()
                .and_then(|p| done.get(p)),
        );
    }
}

/// The model-space matrix of bone `b` at the clock's current frame, without blending (Plan/31 §13.5):
/// evaluates only the ancestor chain of `b`, with the same arithmetic as [`animate_bones`] (bit-identical to
/// its entry `b`). A bone the clip does not have (a clip with fewer bones than the skeleton) is at the bind
/// pose; `None` when `b` is outside the skeleton.
pub fn try_bone_matrix(
    skel: &SimSkeleton,
    clip: &SimClip,
    clock: &MotionClock,
    b: usize,
) -> Option<Mat44> {
    if b >= skel.bones.len() {
        return None;
    }
    if b >= clip.bones.len() {
        return skel.bind_matrix(b);
    }
    let ev = Eval::new(clip, None, &Sample::from_clock(clock));
    let mut chain: Vec<(usize, &ClipBone)> = Vec::new();
    let mut cur = b;
    loop {
        let bone = clip.bones.get(cur)?;
        chain.push((cur, bone));
        match usize::try_from(bone.parent()) {
            Ok(p) if p < cur => cur = p,
            _ => break,
        }
    }
    let mut acc: Option<Mat44> = None;
    for (i, bone) in chain.into_iter().rev() {
        let local = ev.local(i, bone);
        acc = Some(finish(local, acc.as_ref()));
    }
    acc
}

/// [`try_bone_matrix`], or the identity for a bone outside the skeleton (`bone_matrix` of Plan/31 §13.5).
pub fn bone_matrix(skel: &SimSkeleton, clip: &SimClip, clock: &MotionClock, b: usize) -> Mat44 {
    try_bone_matrix(skel, clip, clock, b).unwrap_or(flyff_common::math::D3dMatrix::IDENTITY)
}

/// The model-space position of event point `idx` (`CModelObject::GetEventPos`, ref/18 §7.7): the `.chr`
/// event position transformed by its parent bone's matrix at the clock's current frame, then by `world`
/// (`TransformCoord(event, bone[parent]) · world`, Plan/31 §13.5). `None` for `idx >= 8`, an unused event
/// slot (parent `-1`, the original read `bone[-1]`) or a parent bone the pose does not have.
pub fn try_event_point(
    skel: &SimSkeleton,
    clip: &SimClip,
    clock: &MotionClock,
    idx: usize,
    world: &Mat44,
) -> Option<Vec3> {
    let (pos, parent) = *skel.events.get(idx)?;
    let parent = usize::try_from(parent).ok()?;
    let bone = try_bone_matrix(skel, clip, clock, parent)?;
    Some(transform_coord(world, transform_coord(&bone, pos)))
}

/// [`try_event_point`], or the model origin (`world` applied to `(0, 0, 0)`) for an invalid event — an
/// attack from the boss's own position is the sane fallback of the server callers (docs 17/18).
pub fn event_point(
    skel: &SimSkeleton,
    clip: &SimClip,
    clock: &MotionClock,
    idx: usize,
    world: &Mat44,
) -> Vec3 {
    try_event_point(skel, clip, clock, idx, world)
        .unwrap_or_else(|| transform_coord(world, Vec3::ZERO))
}
