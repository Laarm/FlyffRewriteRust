//! Weapon grips, force points and the sword-trail path (Plan/31 §13.4, §13.8; ref/18 §7.7, §7.8;
//! `CModelObject::GetForcePos`/`GetHandPos`, `MakeSWDForce`; WP-SIM-12).

use super::animate::{Sample, animate_bones};
use super::clip::SimClip;
use super::palette::{AttachPoint, attach_matrix};
use super::quat::{Mat44, transform_coord};
use super::skeleton::SimSkeleton;
use crate::anim::attr::is_attr_hit;
use crate::anim::motion_table::{ClipMeta, ClipSource};
use crate::mover::MotionState;
use crate::num::Vec3;
use crate::types::ANILOOP_1PLAY;
use flyff_common::config::HitAttrRule;

/// `MAX_SF_SWDFORCE`: most edge-point pairs of one sword-trail path.
pub const MAX_FORCE_POINTS: usize = 512;

/// The weapon grip a force path follows (`grip` of `SimWorld.sample_force_path`: 0 RH, 1 LH, 2 SHIELD,
/// 3 KNUCKLE).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grip {
    /// `local_rh · bone[r_hand]`.
    RightHand,
    /// `local_lh · bone[l_hand]`.
    LeftHand,
    /// `local_shield · bone[l_arm]`.
    Shield,
    /// `local_knuckle · bone[r_arm]`.
    Knuckle,
}

impl Grip {
    /// The wire code (0 RH, 1 LH, 2 SHIELD, 3 KNUCKLE).
    pub const fn from_code(code: u32) -> Option<Grip> {
        Some(match code {
            0 => Grip::RightHand,
            1 => Grip::LeftHand,
            2 => Grip::Shield,
            3 => Grip::Knuckle,
            _ => return None,
        })
    }

    /// The attachment point of the grip.
    pub const fn attach(self) -> AttachPoint {
        match self {
            Grip::RightHand => AttachPoint::RightHand,
            Grip::LeftHand => AttachPoint::LeftHand,
            Grip::Shield => AttachPoint::Shield,
            Grip::Knuckle => AttachPoint::Knuckle,
        }
    }
}

/// The model-space grip matrix `grip_local · bone[grip_bone]` at the given pose (`None` when the pose lacks
/// the bone).
pub fn grip_matrix(skel: &SimSkeleton, bones: &[Mat44], grip: Grip) -> Option<Mat44> {
    attach_matrix(skel, bones, grip.attach())
}

/// A weapon force point in model space: `TransformCoord(force, grip_matrix)` (the `force1/force2` of the
/// weapon `.o3d`, object space, left-handed). Multiply with the mover's world matrix for world space
/// (`GetForcePos`).
pub fn force_point(grip: &Mat44, force: Vec3) -> Vec3 {
    transform_coord(grip, force)
}

/// `GetForcePos`: [`force_point`] then the mover's `world` matrix.
pub fn force_point_world(grip: &Mat44, force: Vec3, world: &Mat44) -> Vec3 {
    transform_coord(world, transform_coord(grip, force))
}

/// `GetHandPos`: the translation of `grip_matrix`, then `world`.
pub fn hand_point_world(grip: &Mat44, world: &Mat44) -> Vec3 {
    transform_coord(world, Vec3::new(grip.0[12], grip.0[13], grip.0[14]))
}

/// The parameters of [`sample_force_path`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ForceParams {
    /// Which grip the weapon hangs on.
    pub grip: Grip,
    /// `MOP_HITSLOW`: after the hit frame the clock advances by 0.1 per step.
    pub hit_slow: bool,
    /// The weapon's `force1` (object space, left-handed).
    pub f1: Vec3,
    /// The weapon's `force2`.
    pub f2: Vec3,
    /// The mover's `ani_speed` (`m_fAniSpeed`).
    pub ani_speed: f32,
    /// The hit-attribute rule (`fidelity.hit_attr_rule`).
    pub rule: HitAttrRule,
}

/// `CModelObject::MakeSWDForce` on demand (Plan/31 §13.8): pre-plays the whole clip in `1PLAY` mode on a
/// **copy** of the mover's clock (the live clock is not touched; the original's rewind side effect is
/// dropped) and records the blade edge points `(f1, f2) · grip · bone[parent]` per step in model space.
/// The blend weight is not advanced. At most [`MAX_FORCE_POINTS`] pairs are returned (extra points are
/// dropped); the walk stops at `end_frame` or at the cap. No clip gives an empty path.
pub fn sample_force_path(
    skel: &SimSkeleton,
    clip: Option<&SimClip>,
    prev: Option<&SimClip>,
    motion: &MotionState,
    p: &ForceParams,
) -> Vec<(Vec3, Vec3)> {
    let Some(clip) = clip else {
        return Vec::new();
    };
    let meta = ClipMeta {
        file: String::new(),
        source: ClipSource::Ani,
        frames: clip.frames,
        per_slerp_raw: clip.per_slerp_raw,
        bones: u16::try_from(clip.bones.len()).unwrap_or(u16::MAX),
        attrs: clip.attrs.clone(),
    };
    let mut c = motion.clock;
    c.frame_cur = 0.0;
    c.pause = 0;
    c.end_frame = false;
    c.lp = ANILOOP_1PLAY;
    c.slow = false;
    let mut bones = vec![Mat44::IDENTITY; skel.bones.len()];
    skel.bind_pose(&mut bones);
    let mut out = Vec::new();
    // one pair per step: at most MAX_FORCE_POINTS steps, which also bounds a clock that cannot advance
    for _ in 0..MAX_FORCE_POINTS {
        let sample = Sample {
            frame: c.frame_cur,
            next: c.next_frame(),
            old_frame: motion.prev_frame,
            blend: motion.blend.active,
            weight: motion.blend.weight,
        };
        animate_bones(&mut bones, clip, prev, &sample);
        if let Some(g) = grip_matrix(skel, &bones, p.grip) {
            out.push((force_point(&g, p.f1), force_point(&g, p.f2)));
        }
        c.frame_move(p.ani_speed);
        if p.hit_slow && is_attr_hit(&c, Some(&meta), p.rule) {
            c.slow = true;
        }
        if c.end_frame {
            break;
        }
    }
    out
}
