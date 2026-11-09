//! Per-mover pose state and the skeleton/clip library (Plan/31 §13.1, §13.2, §17.3; WP-SIM-12).
//!
//! [`PoseState`] is the output side of one mover's model: the model-space bone matrices
//! (`m_mUpdateBone`) kept between evaluations. The input side — the clock, the previous clip with its
//! frozen frame and the blend state — is the mover's [`MotionState`] (`SimMover::motion`), which the
//! stepper advances once per frame; [`PoseState::update_motion`] evaluates it. [`PoseLibrary`] owns the
//! skeletons and clips by id (the `add_skeleton`/`add_clip` of `SimWorld`, ids are the `skel_id` and
//! the `MotionTable` `ClipId`).

use super::animate::{Sample, animate_bones};
use super::clip::SimClip;
use super::palette::{self, AttachPoint};
use super::quat::Mat44;
use super::skeleton::SimSkeleton;
use crate::mover::MotionState;
use crate::types::SimError;
use flyff_formats::error::{ErrorKind, FormatError};
use flyff_formats::pack::motion::MotionView;
use flyff_formats::pack::skeleton::SkeletonView;
use std::collections::BTreeMap;

/// The model-space bone matrices of one mover (`m_mUpdateBone`) and whether they came from a clip.
///
/// With no clip (missing, or its keys not fetched yet) [`PoseState::update`] keeps the previous pose — the
/// render stays frozen (KEEP-QUIRK, Plan/31 §13.2); before the first evaluation the pose is the bind pose
/// and [`PoseState::has_pose`] is `false` (the `HAS_POSE` flag of the render view).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PoseState {
    bones: Vec<Mat44>,
    has_pose: bool,
}

impl PoseState {
    /// An empty state (no bones until [`PoseState::reset`] or the first [`PoseState::update`]).
    pub fn new() -> PoseState {
        PoseState::default()
    }

    /// The model-space bone matrices.
    pub fn bones(&self) -> &[Mat44] {
        &self.bones
    }

    /// `true` once a clip has been evaluated since the last [`PoseState::reset`].
    pub fn has_pose(&self) -> bool {
        self.has_pose
    }

    /// Sets the bind pose of `skel` (`has_pose = false`); used when a mover gets a new skeleton.
    pub fn reset(&mut self, skel: &SimSkeleton) {
        self.bones.clear();
        self.bones.resize(skel.bones.len(), Mat44::IDENTITY);
        skel.bind_pose(&mut self.bones);
        self.has_pose = false;
    }

    /// Evaluates `clip` (with `old` as the frozen previous motion; `None` = the clip itself) at `s` into
    /// the pose. Returns `false` and keeps the previous pose when `clip` is `None`.
    ///
    /// A clip with fewer bones than the skeleton animates the first bones and leaves the others at the bind
    /// pose; extra clip bones are ignored (FIX-minor: the original only logged the mismatch, the corpus has
    /// `mvr_infopeng_getup.ani`).
    pub fn update(
        &mut self,
        skel: &SimSkeleton,
        clip: Option<&SimClip>,
        old: Option<&SimClip>,
        s: &Sample,
    ) -> bool {
        let Some(clip) = clip else {
            if self.bones.len() != skel.bones.len() {
                self.reset(skel);
            }
            return false;
        };
        if self.bones.len() != skel.bones.len() {
            self.bones.clear();
            self.bones.resize(skel.bones.len(), Mat44::IDENTITY);
        }
        animate_bones(&mut self.bones, clip, old, s);
        if clip.bones.len() < self.bones.len() {
            // bones the clip does not have: the bind pose of the skeleton
            let mut bind = vec![Mat44::IDENTITY; self.bones.len()];
            skel.bind_pose(&mut bind);
            for (dst, src) in self.bones.iter_mut().zip(bind).skip(clip.bones.len()) {
                *dst = src;
            }
        }
        self.has_pose = true;
        true
    }

    /// [`PoseState::update`] for a mover's motion: clip, previous clip, clock and blend from `motion`, the
    /// clips from `lib`.
    ///
    /// Returns `false` (pose kept) when the mover has no clip or the library does not have it yet. A
    /// previous clip the library does not have disables the blend for this evaluation (FIX-minor); a mover
    /// that never had a previous clip blends with the current clip frozen at `prev_frame` like the original.
    pub fn update_motion(
        &mut self,
        skel: &SimSkeleton,
        lib: &PoseLibrary,
        motion: &MotionState,
    ) -> bool {
        let clip = motion.clip.and_then(|id| lib.clip(id));
        let mut sample = Sample::from_motion(motion);
        let old = match motion.prev_clip {
            Some(id) => {
                let old = lib.clip(id);
                if old.is_none() {
                    sample.blend = false;
                }
                old
            }
            None => None,
        };
        self.update(skel, clip, old, &sample)
    }

    /// The matrix of bone `b`.
    pub fn bone(&self, b: usize) -> Option<&Mat44> {
        self.bones.get(b)
    }

    /// Writes the skin palette (`inverse_tm · bone`, three.js rows, 12 floats per bone); returns the number
    /// of matrices written.
    pub fn write_palette(&self, skel: &SimSkeleton, out: &mut [f32]) -> usize {
        palette::write_palette(skel, &self.bones, out)
    }

    /// Writes the 88-float attachment record ([`palette::write_attachments`]).
    pub fn write_attachments(&self, skel: &SimSkeleton, out: &mut [f32]) -> bool {
        palette::write_attachments(skel, &self.bones, out)
    }

    /// The model-space matrix of an attachment point.
    pub fn attach_matrix(&self, skel: &SimSkeleton, point: AttachPoint) -> Option<Mat44> {
        palette::attach_matrix(skel, &self.bones, point)
    }
}

/// The reason of a pack validation failure as a static string (the `SimError` payloads are `&'static str`).
fn pack_reason(e: &FormatError) -> &'static str {
    match &e.kind {
        ErrorKind::UnexpectedEof { .. } => "truncated pack",
        ErrorKind::BadMagic { .. } => "bad magic",
        ErrorKind::UnsupportedVersion { .. } => "unsupported pack version",
        ErrorKind::CountOutOfRange { .. } => "count out of range",
        ErrorKind::InvalidValue { .. } => "invalid field value",
        ErrorKind::Inconsistent { what } => what,
        _ => "invalid pack",
    }
}

/// Skeletons and clips by id (`SimWorld.add_skeleton` / `add_clip`; clip ids are `MotionTable` `ClipId`s).
/// A `BTreeMap` keeps iteration order independent of insertion history (Plan/31 §2.2 D-1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PoseLibrary {
    skeletons: BTreeMap<u32, SimSkeleton>,
    clips: BTreeMap<u32, SimClip>,
}

impl PoseLibrary {
    /// An empty library.
    pub fn new() -> PoseLibrary {
        PoseLibrary::default()
    }

    /// Validates one `FSK1` pack and stores the skeleton under `id` (an existing id is replaced).
    pub fn add_skeleton(&mut self, id: u32, fsk1: &[u8]) -> Result<(), SimError> {
        let view = SkeletonView::parse(fsk1).map_err(|e| SimError::BadSkeleton(pack_reason(&e)))?;
        let skel = SimSkeleton::from_view(&view)?;
        self.skeletons.insert(id, skel);
        Ok(())
    }

    /// Validates one `FMO1` pack and stores the clip under `id` (an existing id is replaced).
    pub fn add_clip(&mut self, id: u32, fmo1: &[u8]) -> Result<(), SimError> {
        let view = MotionView::parse(fmo1).map_err(|e| SimError::BadClip(pack_reason(&e)))?;
        let clip = SimClip::from_view(&view)?;
        self.clips.insert(id, clip);
        Ok(())
    }

    /// Stores an already built skeleton (server rigs built natively, tests).
    pub fn insert_skeleton(&mut self, id: u32, skel: SimSkeleton) {
        self.skeletons.insert(id, skel);
    }

    /// Stores an already built clip.
    pub fn insert_clip(&mut self, id: u32, clip: SimClip) {
        self.clips.insert(id, clip);
    }

    /// The skeleton `id`.
    pub fn skeleton(&self, id: u32) -> Option<&SimSkeleton> {
        self.skeletons.get(&id)
    }

    /// The clip `id`.
    pub fn clip(&self, id: u32) -> Option<&SimClip> {
        self.clips.get(&id)
    }

    /// Removes a skeleton; returns it.
    pub fn remove_skeleton(&mut self, id: u32) -> Option<SimSkeleton> {
        self.skeletons.remove(&id)
    }

    /// Removes a clip; returns it.
    pub fn remove_clip(&mut self, id: u32) -> Option<SimClip> {
        self.clips.remove(&id)
    }

    /// Number of skeletons.
    pub fn skeleton_count(&self) -> usize {
        self.skeletons.len()
    }

    /// Number of clips.
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Approximate heap bytes of all clips (for `memory_report`).
    pub fn clip_bytes(&self) -> usize {
        self.clips.values().map(SimClip::heap_bytes).sum()
    }

    /// Approximate heap bytes of all skeletons.
    pub fn skeleton_bytes(&self) -> usize {
        self.skeletons.values().map(SimSkeleton::heap_bytes).sum()
    }
}
