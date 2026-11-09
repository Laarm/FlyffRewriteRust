//! `SimClip`: the validated owned copy of an `FMO1` pack (Plan/31 §13.1; Plan/30 §13.4; WP-SIM-12).

use super::quat::{Key, Mat43, Quat, mat43_from_44, mat43_is_finite};
use crate::anim::MotionAttr;
use crate::num::Vec3;
use crate::types::SimError;
use flyff_formats::pack::motion::MotionView;

/// One bone of a [`SimClip`]: either a key track (`TM_ANIMATION` per frame) or one fixed matrix.
#[derive(Clone, PartialEq, Debug)]
pub enum ClipBone {
    /// An animated bone: `keys.len() == frames`, parent from the clip's own hierarchy (MOTIONCORE).
    Animated {
        /// Parent bone, `-1` for a root.
        parent: i16,
        /// One `(rotation, translation)` key per frame.
        keys: Vec<Key>,
    },
    /// A static bone: one local matrix for every frame (may carry scale: 307 corpus matrices do).
    Static {
        /// Parent bone, `-1` for a root.
        parent: i16,
        /// The stored local matrix.
        local_tm: Mat43,
    },
}

impl ClipBone {
    /// The parent index (`-1` for a root).
    pub fn parent(&self) -> i16 {
        match self {
            ClipBone::Animated { parent, .. } | ClipBone::Static { parent, .. } => *parent,
        }
    }
}

/// A motion clip as the pose evaluator uses it (Plan/31 §13.1). The hierarchy used for animation is the
/// clip's own bone list, not the `.chr`'s (ref/18 §6.2).
#[derive(Clone, PartialEq, Debug)]
pub struct SimClip {
    /// Frame count `F` (`nFrame`, `> 0`).
    pub frames: u32,
    /// The file's `perSlerp`, raw (`trunc3` is applied by the motion clock exactly once).
    pub per_slerp_raw: f32,
    /// `true` for a node clip (`FMO1` flag bit 2): bone `i` is GMOBJECT `i` of an `.o3d`; the keys are
    /// evaluated by [`super::animate::animate_bones`] exactly like a skeleton clip (Plan/31 §13.6; the
    /// object semantics of the result belong to WP-SIM-24).
    pub nodes: bool,
    /// The bones in file order (parents precede children).
    pub bones: Vec<ClipBone>,
    /// The attributes with `flags != 0`, ascending array index (same shape as [`crate::anim::ClipMeta`]).
    pub attrs: Vec<MotionAttr>,
}

/// Most bones a clip may have in the sim (`parent` is an `i16`; real clips have 2..=75, node clips at most
/// a few dozen).
pub const MAX_CLIP_BONES: usize = i16::MAX as usize;

impl SimClip {
    /// The validated owned copy of an `FMO1` view (Plan/31 §13.1): per bone the parent and either the
    /// track's `F` keys or the static matrix; `frames = F`; the raw `per_slerp`; the `nodes` flag; the
    /// attribute entries with `flags != 0` at their array index.
    ///
    /// Errors (`SimError::BadClip`): more than [`MAX_CLIP_BONES`] bones, a non-finite `per_slerp`, key or
    /// matrix, or a key track shared by two bones (the copy would then be unbounded by the input size; real
    /// files give every animated bone its own track).
    pub fn from_view(v: &MotionView<'_>) -> Result<SimClip, SimError> {
        let nb = v.bone_count();
        if nb > MAX_CLIP_BONES {
            return Err(SimError::BadClip("too many bones"));
        }
        let frames = u32::try_from(v.frame_count())
            .ok()
            .filter(|&f| f > 0)
            .ok_or(SimError::BadClip("frame count out of range"))?;
        let per_slerp_raw = v.per_slerp();
        if !per_slerp_raw.is_finite() {
            return Err(SimError::BadClip("non-finite per_slerp"));
        }
        let mut used = vec![false; v.track_count()];
        let mut bones = Vec::with_capacity(nb);
        for i in 0..nb {
            let parent = v
                .parent(i)
                .and_then(|p| i16::try_from(p).ok())
                .ok_or(SimError::BadClip("bone parent out of range"))?;
            match v.track(i) {
                Some(t) => {
                    let t = usize::try_from(t).map_err(|_| SimError::BadClip("track index"))?;
                    match used.get_mut(t) {
                        Some(u) if !*u => *u = true,
                        Some(_) => return Err(SimError::BadClip("key track shared by two bones")),
                        None => return Err(SimError::BadClip("track index out of range")),
                    }
                    let mut keys: Vec<Key> = Vec::with_capacity(v.frame_count());
                    for k in v.keys(t) {
                        let q = Quat::from_array(k.rot);
                        let p = Vec3::new(k.pos[0], k.pos[1], k.pos[2]);
                        if !q.is_finite() || !p.is_finite() {
                            return Err(SimError::BadClip("non-finite key"));
                        }
                        keys.push((q, p));
                    }
                    if keys.len() != v.frame_count() {
                        return Err(SimError::BadClip("track does not hold one key per frame"));
                    }
                    bones.push(ClipBone::Animated { parent, keys });
                }
                None => {
                    let tm = v
                        .static_tm(i)
                        .ok_or(SimError::BadClip("missing static matrix"))?;
                    let local_tm = mat43_from_44(&tm);
                    if !mat43_is_finite(&local_tm) {
                        return Err(SimError::BadClip("non-finite matrix"));
                    }
                    bones.push(ClipBone::Static { parent, local_tm });
                }
            }
        }
        let attrs = v
            .attrs()
            .iter()
            .enumerate()
            .filter(|(_, a)| a.flags != 0)
            .filter_map(|(i, a)| {
                Some(MotionAttr {
                    index: u32::try_from(i).ok()?,
                    flags: a.flags,
                    sound: a.sound_id,
                    frame: a.frame,
                })
            })
            .collect();
        Ok(SimClip {
            frames,
            per_slerp_raw,
            nodes: v.is_node_clip(),
            bones,
            attrs,
        })
    }

    /// Number of bones.
    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }

    /// Approximate heap bytes (for `SimWorld.memory_report`).
    pub fn heap_bytes(&self) -> usize {
        let keys: usize = self
            .bones
            .iter()
            .map(|b| match b {
                ClipBone::Animated { keys, .. } => keys.capacity() * size_of::<Key>(),
                ClipBone::Static { .. } => 0,
            })
            .sum();
        self.bones.capacity() * size_of::<ClipBone>()
            + keys
            + self.attrs.capacity() * size_of::<MotionAttr>()
    }
}
