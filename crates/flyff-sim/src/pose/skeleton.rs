//! `SimSkeleton`: the validated owned copy of an `FSK1` pack (Plan/31 §13.1; Plan/30 §13.3; WP-SIM-12).

use super::quat::{
    MAT43_IDENTITY, Mat43, Mat44, mat43_from_44, mat43_is_finite, mat44_from_43, mul43,
};
use crate::num::Vec3;
use crate::types::SimError;
use flyff_common::math::D3dMatrix;
use flyff_formats::pack::skeleton::SkeletonView;

/// Most events a `.chr` carries (`MAX_MDL_EVENT`).
pub const MAX_EVENTS: usize = 8;

/// A weapon grip with an element beyond this magnitude is uninitialised data, not a transform: 25 monster
/// `.chr` files carry such a `localKnuckle` (values up to 7e26 and denormals; nothing ever attaches to it).
/// [`SimSkeleton::from_view`] replaces it by the identity so that no attachment record holds `inf`/`NaN`.
pub const GRIP_SANE_LIMIT: f32 = 1.0e6;

/// One bone of a [`SimSkeleton`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SkelBone {
    /// The `.chr` `inverseTM` (skinning: `inverse_tm[b] · bone[b]`).
    pub inverse_tm: Mat43,
    /// The local bind matrix (bind-pose fallback).
    pub local_tm: Mat43,
    /// Parent index, `-1` for the root; always smaller than the bone's own index.
    pub parent: i16,
}

/// The skeleton the sim needs (Plan/31 §13.1): skinning inverses, the special bones with their weapon
/// grips, the event points and the bind pose. Built from an `FSK1` pack by [`SimSkeleton::from_view`].
///
/// The `tm` (bind model matrix) of the pack is not stored: the bind pose is rebuilt from `local_tm`
/// (`bind[i] = local_tm[i] · bind[parent]`, [`SimSkeleton::bind_pose`]), which is what the
/// bind-pose fallback of Plan/31 §13.1 specifies (16 corpus `.chr` carry a `tm` that disagrees with it).
#[derive(Clone, PartialEq, Debug)]
pub struct SimSkeleton {
    /// The bones in file order (parents precede children).
    pub bones: Vec<SkelBone>,
    /// `RHandIdx`.
    pub r_hand: u16,
    /// `LHandIdx`.
    pub l_hand: u16,
    /// `LArmIdx` (the L ForeArm bone carries the shield).
    pub l_arm: u16,
    /// `RArmIdx` (the R ForeArm bone carries the knuckle weapon).
    pub r_arm: u16,
    /// Right-hand weapon grip, relative to the R Hand bone.
    pub local_rh: Mat43,
    /// Left-hand weapon grip (identity when the `.chr` has none).
    pub local_lh: Mat43,
    /// Shield grip, relative to the L ForeArm bone.
    pub local_shield: Mat43,
    /// Knuckle-weapon grip, relative to the R ForeArm bone.
    pub local_knuckle: Mat43,
    /// Event points `(position in the parent bone's space, parent bone)`; unused slots have parent `-1`.
    pub events: [(Vec3, i16); MAX_EVENTS],
}

fn m43(m: &D3dMatrix) -> Mat43 {
    mat43_from_44(m)
}

impl SimSkeleton {
    /// The validated owned copy of an `FSK1` view (Plan/31 §13.1): the four grips, the per-bone
    /// `inverse_tm`/`local_tm`/`parent`, the events (E = 0, 4 or 8, the other slots unused) and the special
    /// bone indices.
    ///
    /// Errors (`SimError::BadSkeleton`): a special bone index outside the skeleton, a non-finite matrix or
    /// event position. The view has already validated the hierarchy (one root, parents precede children).
    pub fn from_view(v: &SkeletonView<'_>) -> Result<SimSkeleton, SimError> {
        let n = v.bone_count();
        if n == 0 || n > 255 {
            return Err(SimError::BadSkeleton("bone count outside 1..=255"));
        }
        let special = v.special_bones();
        let idx = |i: i32| -> Result<u16, SimError> {
            u16::try_from(i)
                .ok()
                .filter(|&u| usize::from(u) < n)
                .ok_or(SimError::BadSkeleton("special bone index out of range"))
        };
        let (r_hand, l_hand, l_arm, r_arm) = (
            idx(special.r_hand)?,
            idx(special.l_hand)?,
            idx(special.l_arm)?,
            idx(special.r_arm)?,
        );
        let finite = |m: Mat43| -> Result<Mat43, SimError> {
            if mat43_is_finite(&m) {
                Ok(m)
            } else {
                Err(SimError::BadSkeleton("non-finite matrix"))
            }
        };
        // a grip: finite, and uninitialised garbage (|element| > GRIP_SANE_LIMIT) becomes the identity
        let grip = |m: Mat43| -> Result<Mat43, SimError> {
            let m = finite(m)?;
            Ok(if m.iter().any(|x| x.abs() > GRIP_SANE_LIMIT) {
                MAT43_IDENTITY
            } else {
                m
            })
        };
        let mut bones = Vec::with_capacity(n);
        for i in 0..n {
            let parent = v
                .parent(i)
                .and_then(|p| i16::try_from(p).ok())
                .ok_or(SimError::BadSkeleton("bone parent out of range"))?;
            let inverse_tm = v
                .inverse_tm(i)
                .ok_or(SimError::BadSkeleton("missing inverse matrix"))?;
            let local_tm = v
                .local_tm(i)
                .ok_or(SimError::BadSkeleton("missing local matrix"))?;
            bones.push(SkelBone {
                inverse_tm: finite(m43(&inverse_tm))?,
                local_tm: finite(m43(&local_tm))?,
                parent,
            });
        }
        let mut events = [(Vec3::ZERO, -1_i16); MAX_EVENTS];
        for (k, slot) in events.iter_mut().enumerate().take(v.event_count()) {
            let Some(e) = v.event(k) else { break };
            if !e.pos.iter().all(|c| c.is_finite()) {
                return Err(SimError::BadSkeleton("non-finite event position"));
            }
            let parent = i16::try_from(e.parent)
                .ok()
                .filter(|&p| p == -1 || usize::try_from(p).is_ok_and(|p| p < n))
                .ok_or(SimError::BadSkeleton("event parent out of range"))?;
            *slot = (Vec3::new(e.pos[0], e.pos[1], e.pos[2]), parent);
        }
        Ok(SimSkeleton {
            bones,
            r_hand,
            l_hand,
            l_arm,
            r_arm,
            local_rh: grip(m43(&v.local_rh()))?,
            local_lh: grip(m43(&v.local_lh()))?,
            local_shield: grip(m43(&v.local_shield()))?,
            local_knuckle: grip(m43(&v.local_knuckle()))?,
            events,
        })
    }

    /// Number of bones.
    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }

    /// Writes the bind pose (`local_tm[i] · bind[parent]`, model space) into `out`; at most
    /// `min(out.len(), bones.len())` entries are written.
    pub fn bind_pose(&self, out: &mut [Mat44]) {
        let n = out.len().min(self.bones.len());
        let mut done: Vec<Mat43> = Vec::with_capacity(n);
        for (bone, slot) in self.bones.iter().zip(out.iter_mut()).take(n) {
            let m = match usize::try_from(bone.parent).ok().and_then(|p| done.get(p)) {
                Some(pm) => mul43(&bone.local_tm, pm),
                None => bone.local_tm,
            };
            done.push(m);
            *slot = mat44_from_43(&m);
        }
    }

    /// The bind-pose matrix of bone `b` (`None` when `b` is out of range): the same chain
    /// [`SimSkeleton::bind_pose`] computes, evaluated from the root down (bit-identical to its entry `b`).
    pub fn bind_matrix(&self, b: usize) -> Option<Mat44> {
        let mut chain: Vec<&Mat43> = Vec::new();
        let mut cur = b;
        loop {
            let bone = self.bones.get(cur)?;
            chain.push(&bone.local_tm);
            match usize::try_from(bone.parent) {
                // parents precede children: the walk strictly decreases
                Ok(p) if p < cur => cur = p,
                _ => break,
            }
        }
        let mut acc: Option<Mat43> = None;
        for local in chain.into_iter().rev() {
            acc = Some(match acc {
                Some(parent) => mul43(local, &parent),
                None => *local,
            });
        }
        acc.map(|m| mat44_from_43(&m))
    }

    /// Approximate heap bytes (for `SimWorld.memory_report`).
    pub fn heap_bytes(&self) -> usize {
        self.bones.capacity() * size_of::<SkelBone>()
    }
}

impl Default for SimSkeleton {
    /// A one-bone identity skeleton (useful as a placeholder in tests).
    fn default() -> Self {
        SimSkeleton {
            bones: vec![SkelBone {
                inverse_tm: MAT43_IDENTITY,
                local_tm: MAT43_IDENTITY,
                parent: -1,
            }],
            r_hand: 0,
            l_hand: 0,
            l_arm: 0,
            r_arm: 0,
            local_rh: MAT43_IDENTITY,
            local_lh: MAT43_IDENTITY,
            local_shield: MAT43_IDENTITY,
            local_knuckle: MAT43_IDENTITY,
            events: [(Vec3::ZERO, -1); MAX_EVENTS],
        }
    }
}
