//! Client decor entries: animated statics, attached models and the render pose of ctrls (Plan/31 §13.7,
//! §17.4 ops `DECOR_*` / `CTRL_*`, §17.7 decor area; WP-SIM-24).
//!
//! Pose and node evaluation stays in the sim (rule B1): the renderer registers an animated object once
//! (`DECOR_ADD`), toggles it with `DECOR_VISIBLE`, and reads its node matrices / palette and world matrix from
//! the `decor` area of the render view. Three kinds:
//!
//! | kind | world matrix | clock |
//! |---|---|---|
//! | `Static` | `local` | the entry's own clock, one `frame_move` per stepped frame |
//! | `Attached` | `local · attach_matrix(parent, point) · parent world` | the entry's own clock |
//! | `Ctrl` | the `CTRL_ADD` matrix | [`CtrlClock`], driven by `CTRL_STATE` |
//!
//! * **Rigid** entries (`skel == 0`) evaluate a *node clip* (`FMO1` with flag bit 2, Plan/31 §13.6) with
//!   [`animate_bones`] and write one model-space matrix per node (the `update[i]` of `Object3D::Animate`);
//!   **skinned** entries (own `.chr` + clips: wings, ride attachments) write the skin palette
//!   `inverse_tm · bone`. A pose is only written for `VISIBLE` entries whose clip (and skeleton) are loaded;
//!   otherwise `HAS_POSE` stays clear and the renderer draws the bind pose.
//! * **Clocks** advance once per stepped frame at speed 1 whether the entry is visible or not. A clip whose
//!   metadata is not known yet (the motion table was not loaded when the entry arrived) starts its clock when
//!   the table is there.
//! * **Sounds**: an entry with `SOUNDS` raises `AttrSound` events (object = the entry id, decor bit set, the
//!   position of its world matrix); built-in `.o3d` clips use the exact-equality rule of `Object3D.h:319-345`.
//! * **Ctrl entries** exist for ctrls whose model animates (a `FLYK` model with keys, or a built-in clip in the
//!   motion table). They follow `CTRL_STATE`: the clock plays (`CONT`, or `LOOP`), pauses at half for
//!   `CK3_HALF`, re-arms for `CK2_KEEP`, and the collider of the ctrl is re-posed whenever the frame
//!   changes (Plan/31 §10.10 rule 1).

use super::commands::{CtrlAdd, DecorAdd};
use super::events::ExportEvent;
use crate::anim::attr::{attrs_firing, is_attr_o3d};
use crate::anim::motion_table::{ClipId, ClipSource, ModelKey, MotionAttr, MotionTable};
use crate::anim::{CtrlClock, MotionClock};
use crate::collision::dynamic::{AddCollider, AddOutcome, DynamicColliders};
use crate::num::Vec3;
use crate::pose::palette::render_rows;
use crate::pose::quat::{Mat43, mat43_from_44, mat44_from_43};
use crate::pose::{
    AttachPoint, MATRIX_FLOATS, Mat44, PoseLibrary, PoseState, Sample, animate_bones,
};
use crate::types::{ANILOOP_1PLAY, ANILOOP_LOOP, MA_SOUND, SimError};
use crate::wire::{self, decor as dc};
use flyff_common::ObjId;
use flyff_common::config::HitAttrRule;
use std::collections::BTreeMap;

/// The most `DECOR_ADD` entries kept (a hostile stream cannot grow the set without bound).
pub const MAX_DECOR: usize = 16_384;
/// The most frames a late `CTRL_STATE` is caught up (2 s, like an intent).
pub const CTRL_FF_CAP: u64 = 120;

/// Kind of a decor entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorKind {
    /// A static animated object (`wire::decor::KIND_STATIC`).
    Static,
    /// An entry attached to a mover.
    Attached,
}

/// A `DECOR_ADD` entry.
#[derive(Clone, Debug)]
pub struct DecorEntry {
    /// The decor id.
    pub id: u32,
    /// Static or attached.
    pub kind: DecorKind,
    /// `wire::decor` flag bits (`VISIBLE` is kept in `visible`).
    pub flags: u8,
    /// Whether the renderer wants the pose.
    pub visible: bool,
    /// `(OT_*, index)` of the model.
    pub model: ModelKey,
    /// The explicit clip id (`None` = the built-in clip of the model).
    pub clip: Option<ClipId>,
    /// Skeleton id (`0` = rigid node animation).
    pub skel: u32,
    /// The parent mover (attached entries).
    pub parent: ObjId,
    /// The attachment point.
    pub attach: AttachPoint,
    /// The LH `Mat43` of the command.
    pub local: Mat43,
    start_frame: f32,
    clock: MotionClock,
    clock_ready: bool,
    nodes: Vec<Mat44>,
    skinned: PoseState,
    has_pose: bool,
}

/// The render pose entry of one ctrl.
#[derive(Clone, Debug)]
pub struct CtrlEntry {
    /// The ctrl's `ObjId`.
    pub id: ObjId,
    /// `mdlDyna` ctrl index.
    pub model_key: u32,
    /// The LH world matrix.
    pub world: Mat44,
    /// The animation state machine.
    pub clock: CtrlClock,
    /// Whether the renderer wants the pose.
    pub visible: bool,
    nodes: Vec<Mat44>,
    has_pose: bool,
}

/// All decor and ctrl entries of the world.
#[derive(Clone, Debug, Default)]
pub struct DecorSet {
    entries: BTreeMap<u32, DecorEntry>,
    ctrls: BTreeMap<ObjId, CtrlEntry>,
}

fn model_key_of(m: (u32, u32)) -> ModelKey {
    ModelKey {
        ot: m.0,
        index: m.1,
    }
}

/// `(frames, per_slerp_raw)` of the clip a decor entry plays, when the motion table knows it.
fn clip_meta(
    motions: &MotionTable,
    clip: Option<ClipId>,
    model: ModelKey,
) -> Option<(ClipId, u32, f32)> {
    let id = clip.or_else(|| motions.builtin(model))?;
    let meta = motions.clip(id)?;
    Some((id, meta.frames, meta.per_slerp_raw))
}

/// A decor sound event: object = the entry id, the decor bit set.
fn sound_event(id: u32, frame: u64, a: &MotionAttr, pos: Vec3, positional: bool) -> ExportEvent {
    ExportEvent {
        kind: wire::event_kind("AttrSound").unwrap_or(0),
        obj: id,
        frame: frame as u32, // BOUNDS: low word
        w: [
            a.sound as u32, // BOUNDS: bit pattern of an i32
            pos.x.to_bits(),
            pos.y.to_bits(),
            pos.z.to_bits(),
            wire::SOUND_DECOR
                | if positional {
                    wire::SOUND_POSITIONAL
                } else {
                    0
                },
        ],
    }
}

impl DecorEntry {
    fn new(a: &DecorAdd, motions: &MotionTable) -> DecorEntry {
        let model = model_key_of(a.model);
        let clip = (a.clip_id != dc::CLIP_BUILTIN).then_some(a.clip_id);
        let mut e = DecorEntry {
            id: a.id,
            kind: if a.kind == dc::KIND_ATTACHED {
                DecorKind::Attached
            } else {
                DecorKind::Static
            },
            flags: a.flags & !dc::VISIBLE,
            visible: a.flags & dc::VISIBLE != 0,
            model,
            clip,
            skel: a.skel_id,
            parent: a.parent_mover,
            attach: AttachPoint::from_code(a.attach).unwrap_or(AttachPoint::Root),
            local: a.local,
            start_frame: a.start_frame,
            clock: MotionClock::new(),
            clock_ready: false,
            nodes: Vec::new(),
            skinned: PoseState::new(),
            has_pose: false,
        };
        e.ready_clock(motions);
        e
    }

    fn looping(&self) -> bool {
        self.flags & dc::LOOP != 0
    }

    /// Builds the clock from the clip metadata once the motion table knows the clip.
    fn ready_clock(&mut self, motions: &MotionTable) {
        if self.clock_ready {
            return;
        }
        let Some((_, frames, per_slerp_raw)) = clip_meta(motions, self.clip, self.model) else {
            return;
        };
        let mut c = MotionClock::new();
        c.frame_max = i32::try_from(frames).unwrap_or(i32::MAX);
        c.per_slerp = crate::num::trunc3(per_slerp_raw);
        c.lp = if self.looping() {
            ANILOOP_LOOP
        } else {
            ANILOOP_1PLAY
        };
        c.set_frame(self.start_frame);
        self.clock = c;
        self.clock_ready = true;
    }

    /// The world position of the entry (sound position): the translation of `local` (static) or of the
    /// attached offset.
    fn position(&self) -> Vec3 {
        Vec3::new(
            self.local.get(9).copied().unwrap_or(0.0),
            self.local.get(10).copied().unwrap_or(0.0),
            self.local.get(11).copied().unwrap_or(0.0),
        )
    }

    /// The clip id, when the motion table resolves one.
    fn clip_id(&self, motions: &MotionTable) -> Option<ClipId> {
        self.clip.or_else(|| motions.builtin(self.model))
    }

    /// `true` once a non-looping clock passed its last frame (a looping clock never ends; its raw
    /// `end_frame` latch is set by the first wrap).
    pub fn ended(&self) -> bool {
        self.clock.end_frame && self.clock.lp & ANILOOP_LOOP == 0
    }

    /// `frame_cur` of the clock.
    pub fn frame(&self) -> f32 {
        self.clock.frame_cur
    }

    /// `true` when the pose was evaluated at the last `render_view`.
    pub fn has_pose(&self) -> bool {
        self.has_pose
    }
}

impl DecorSet {
    /// An empty set.
    pub fn new() -> DecorSet {
        DecorSet::default()
    }

    /// Number of `DECOR_ADD` entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Number of ctrl entries.
    pub fn ctrl_len(&self) -> usize {
        self.ctrls.len()
    }

    /// `true` when there is no entry at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.ctrls.is_empty()
    }

    /// The decor entry `id`.
    pub fn get(&self, id: u32) -> Option<&DecorEntry> {
        self.entries.get(&id)
    }

    /// The ctrl entry of ctrl `id`.
    pub fn ctrl(&self, id: ObjId) -> Option<&CtrlEntry> {
        self.ctrls.get(&id)
    }

    /// The entries in id order.
    pub fn iter(&self) -> impl Iterator<Item = &DecorEntry> {
        self.entries.values()
    }

    /// The ctrl entries in id order.
    pub fn ctrls(&self) -> impl Iterator<Item = &CtrlEntry> {
        self.ctrls.values()
    }

    /// `DECOR_ADD`: adds an entry, or replaces the one of the same id; the clock of a replaced entry continues
    /// iff clip, skeleton, model and the LOOP flag are unchanged, else it restarts at `start_frame`.
    pub fn add(&mut self, a: &DecorAdd, motions: &MotionTable) -> bool {
        if !self.entries.contains_key(&a.id) && self.entries.len() >= MAX_DECOR {
            return false;
        }
        let mut fresh = DecorEntry::new(a, motions);
        if let Some(old) = self.entries.get(&a.id)
            && old.clip == fresh.clip
            && old.skel == fresh.skel
            && old.model == fresh.model
            && old.looping() == fresh.looping()
        {
            fresh.clock = old.clock;
            fresh.clock_ready = old.clock_ready;
            fresh.start_frame = old.start_frame;
        }
        self.entries.insert(a.id, fresh);
        true
    }

    /// `DECOR_REMOVE`.
    pub fn remove(&mut self, id: u32) -> bool {
        self.entries.remove(&id).is_some()
    }

    /// `DECOR_VISIBLE`.
    pub fn set_visible(&mut self, id: u32, visible: bool) -> bool {
        match self.entries.get_mut(&id) {
            Some(e) => {
                e.visible = visible;
                true
            }
            None => false,
        }
    }

    /// `SET_VISIBLE` of a ctrl.
    pub fn set_ctrl_visible(&mut self, id: ObjId, visible: bool) -> bool {
        match self.ctrls.get_mut(&id) {
            Some(e) => {
                e.visible = visible;
                true
            }
            None => false,
        }
    }

    /// Drops the entries attached to mover `id` (they follow their mover).
    pub fn remove_attached_to(&mut self, id: ObjId) {
        self.entries
            .retain(|_, e| !(e.kind == DecorKind::Attached && e.parent == id));
    }

    /// `set_world`: everything goes except the entries attached to the preview movers `previews`.
    pub fn clear_world(&mut self, previews: &[ObjId]) {
        self.entries
            .retain(|_, e| e.kind == DecorKind::Attached && previews.contains(&e.parent));
        self.ctrls.clear();
    }

    /// `CTRL_ADD`: registers the collider and, when the ctrl animates, its render entry. Returns whether
    /// anything was registered.
    pub fn ctrl_add(
        &mut self,
        c: &CtrlAdd,
        motions: &MotionTable,
        dynamic: &mut DynamicColliders,
    ) -> bool {
        let world = mat44_from_43(&c.world);
        // an all-zero box means the box of the `FLYK` model (the server's own rule)
        let aabb = wire::ctrl_aabb(c.aabb);
        // frames and per_slerp: the FLYK model first (the collider and the clock agree by construction),
        // else the built-in clip of the motion table
        let (frames, per_slerp, animated) = match dynamic.models().get(c.model_key) {
            Some(m) => (m.max_frame, m.per_slerp_raw, m.animated()),
            None => {
                let key = ModelKey {
                    ot: crate::types::defs::OT_CTRL,
                    index: c.model_key,
                };
                match clip_meta(motions, None, key) {
                    Some((_, f, p)) => (f, p, true),
                    None => (0, 0.5, false),
                }
            }
        };
        let clock = CtrlClock::new(frames, per_slerp, false, false);
        let outcome = dynamic.add(AddCollider {
            id: c.id,
            model: c.model_key,
            world,
            aabb,
            seq: u64::from(c.seq),
            frame: 0.0,
            next: clock.next_frame(),
        });
        let mut registered = matches!(outcome, AddOutcome::Added { .. });
        self.ctrls.remove(&c.id);
        if animated {
            self.ctrls.insert(
                c.id,
                CtrlEntry {
                    id: c.id,
                    model_key: c.model_key,
                    world,
                    clock,
                    visible: false,
                    nodes: Vec::new(),
                    has_pose: false,
                },
            );
            registered = true;
        }
        registered
    }

    /// `CTRL_STATE`: the ctrl plays from `anim_frame` (`< 0` = where it holds) in the mode of `loop_bits`;
    /// `lag` frames of the clock are caught up (at most [`CTRL_FF_CAP`]) and the collider is re-posed.
    pub fn ctrl_state(
        &mut self,
        id: ObjId,
        anim_frame: f32,
        loop_bits: u32,
        lag: u64,
        now: u64,
        dynamic: &mut DynamicColliders,
    ) -> bool {
        let Some(e) = self.ctrls.get_mut(&id) else {
            return false;
        };
        e.clock.keep = loop_bits & dc::STATE_KEEP != 0;
        e.clock.half = loop_bits & dc::STATE_HALF != 0;
        e.clock.play_from(
            (anim_frame >= 0.0).then_some(anim_frame),
            loop_bits & dc::STATE_LOOP != 0,
        );
        let lag = lag.min(CTRL_FF_CAP);
        for k in 0..lag {
            e.clock.step(now.saturating_sub(lag).saturating_add(k));
        }
        dynamic.set_pose(id, e.clock.frame(), e.clock.next_frame());
        true
    }

    /// `CTRL_REMOVE`: drops the render entry (the caller removes the collider).
    pub fn ctrl_remove(&mut self, id: ObjId) -> bool {
        self.ctrls.remove(&id).is_some()
    }

    /// Step 7 of Plan/31 §3.5 for the decor set: one frame of every clock (whether visible or not), the
    /// sound events, and the re-pose of the ctrl colliders whose frame moved. `frame` is the local frame being
    /// completed.
    pub fn step(
        &mut self,
        frame: u64,
        motions: &MotionTable,
        dynamic: &mut DynamicColliders,
        out: &mut Vec<ExportEvent>,
    ) {
        for e in self.entries.values_mut() {
            e.ready_clock(motions);
            if !e.clock_ready {
                continue;
            }
            e.clock.frame_move(1.0);
            if e.flags & dc::SOUNDS == 0 {
                continue;
            }
            let Some(id) = e.clip_id(motions) else {
                continue;
            };
            let Some(meta) = motions.clip(id) else {
                continue;
            };
            let positional = e.flags & dc::SOUND_POSITIONAL != 0;
            let pos = e.position();
            if meta.source == ClipSource::O3dBuiltin {
                if let Some(a) = is_attr_o3d(&meta.attrs, e.clock.frame_cur, MA_SOUND) {
                    out.push(sound_event(e.id, frame, a, pos, positional));
                }
            } else {
                for a in attrs_firing(&e.clock, Some(meta), MA_SOUND, HitAttrRule::Legacy) {
                    out.push(sound_event(e.id, frame, a, pos, positional));
                }
            }
        }
        for e in self.ctrls.values_mut() {
            let before = e.clock.frame();
            e.clock.step(frame);
            let after = e.clock.frame();
            if after != before {
                dynamic.set_pose(e.id, after, e.clock.next_frame());
                let attrs: Option<&[MotionAttr]> = match dynamic.models().get(e.model_key) {
                    Some(m) => Some(m.attrs.as_slice()),
                    None => {
                        let key = ModelKey {
                            ot: crate::types::defs::OT_CTRL,
                            index: e.model_key,
                        };
                        motions
                            .builtin(key)
                            .and_then(|id| motions.clip(id))
                            .map(|m| m.attrs.as_slice())
                    }
                };
                if let Some(attrs) = attrs
                    && let Some(a) = is_attr_o3d(attrs, after, MA_SOUND)
                {
                    let w = &e.world.0;
                    let pos = Vec3::new(
                        w.get(12).copied().unwrap_or(0.0),
                        w.get(13).copied().unwrap_or(0.0),
                        w.get(14).copied().unwrap_or(0.0),
                    );
                    out.push(sound_event(e.id.0, frame, a, pos, false));
                }
            }
        }
    }

    /// The matrix count each entry would write at the next `render_view` (rigid: nodes of the node clip,
    /// skinned: bones of the skeleton), `None` when the entry has no pose to write. Order: decor entries by
    /// id, then ctrl entries by id.
    pub fn plan(&self, poses: &PoseLibrary, motions: &MotionTable) -> Vec<Option<usize>> {
        let mut out = Vec::with_capacity(self.entries.len() + self.ctrls.len());
        for e in self.entries.values() {
            let n = if !e.visible {
                None
            } else if e.skel != 0 {
                poses.skeleton(e.skel).and_then(|s| {
                    e.clip_id(motions)
                        .and_then(|c| poses.clip(c))
                        .map(|_| s.bones.len())
                })
            } else {
                e.clip_id(motions)
                    .and_then(|c| poses.clip(c))
                    .map(|c| c.bones.len())
            };
            out.push(n);
        }
        for e in self.ctrls.values() {
            let key = ModelKey {
                ot: crate::types::defs::OT_CTRL,
                index: e.model_key,
            };
            out.push(if e.visible {
                motions
                    .builtin(key)
                    .and_then(|c| poses.clip(c))
                    .map(|c| c.bones.len())
            } else {
                None
            });
        }
        out
    }

    /// The entries and the ctrl entries in render order, mutable (the view evaluates their poses).
    pub(super) fn parts_mut(
        &mut self,
    ) -> (
        impl Iterator<Item = &mut DecorEntry>,
        impl Iterator<Item = &mut CtrlEntry>,
    ) {
        (self.entries.values_mut(), self.ctrls.values_mut())
    }
}

/// The result of evaluating one entry for the view.
pub(super) struct Written {
    /// The pose was written.
    pub has_pose: bool,
    /// Matrices written to the palette.
    pub count: usize,
}

/// Evaluates the rigid node matrices of `clip` at `frame` into `nodes` and writes them (three.js rows) to
/// `out`; returns the node count.
fn eval_nodes(
    poses: &PoseLibrary,
    clip: ClipId,
    frame: f32,
    next: i32,
    nodes: &mut Vec<Mat44>,
    out: &mut [f32],
) -> Option<usize> {
    let c = poses.clip(clip)?;
    nodes.resize(c.bones.len(), Mat44::IDENTITY);
    animate_bones(
        nodes,
        c,
        None,
        &Sample {
            frame,
            next,
            old_frame: 0,
            blend: false,
            weight: 1.0,
        },
    );
    let (chunks, _) = out.as_chunks_mut::<MATRIX_FLOATS>();
    for (m, dst) in nodes.iter().zip(chunks) {
        *dst = render_rows(&mat43_from_44(m));
    }
    Some(nodes.len())
}

impl DecorEntry {
    /// Evaluates the pose of a visible entry into `out` (the palette slice starting at its first matrix).
    pub(super) fn write_pose(
        &mut self,
        poses: &PoseLibrary,
        motions: &MotionTable,
        out: &mut [f32],
    ) -> Written {
        self.has_pose = false;
        let none = Written {
            has_pose: false,
            count: 0,
        };
        if !self.visible {
            return none;
        }
        let Some(clip_id) = self.clip_id(motions) else {
            return none;
        };
        let frame = self.clock.frame_cur;
        let next = self.clock.next_frame();
        if self.skel == 0 {
            return match eval_nodes(poses, clip_id, frame, next, &mut self.nodes, out) {
                Some(count) => {
                    self.has_pose = true;
                    Written {
                        has_pose: true,
                        count,
                    }
                }
                None => none,
            };
        }
        let (Some(skel), Some(clip)) = (poses.skeleton(self.skel), poses.clip(clip_id)) else {
            return none;
        };
        let sample = Sample {
            frame,
            next,
            old_frame: 0,
            blend: false,
            weight: 1.0,
        };
        self.skinned.update(skel, Some(clip), None, &sample);
        let count = self.skinned.write_palette(skel, out);
        self.has_pose = true;
        Written {
            has_pose: true,
            count,
        }
    }

    /// The attachment matrix of the entry's point on its parent, when attached and the parent pose has it.
    pub(super) fn attach_on(
        &self,
        poses: &PoseLibrary,
        parent_skel: u32,
        parent_bones: &PoseState,
    ) -> Option<Mat44> {
        let skel = poses.skeleton(parent_skel)?;
        parent_bones.attach_matrix(skel, self.attach)
    }

    /// `true` for a skinned entry.
    pub fn skinned(&self) -> bool {
        self.skel != 0
    }
}

impl CtrlEntry {
    /// Evaluates the node matrices of a visible ctrl into `out`.
    pub(super) fn write_pose(
        &mut self,
        poses: &PoseLibrary,
        motions: &MotionTable,
        out: &mut [f32],
    ) -> Written {
        self.has_pose = false;
        let none = Written {
            has_pose: false,
            count: 0,
        };
        if !self.visible {
            return none;
        }
        let key = ModelKey {
            ot: crate::types::defs::OT_CTRL,
            index: self.model_key,
        };
        let Some(clip) = motions.builtin(key) else {
            return none;
        };
        match eval_nodes(
            poses,
            clip,
            self.clock.frame(),
            self.clock.next_frame(),
            &mut self.nodes,
            out,
        ) {
            Some(count) => {
                self.has_pose = true;
                Written {
                    has_pose: true,
                    count,
                }
            }
            None => none,
        }
    }

    /// `true` when the pose was evaluated at the last `render_view`.
    pub fn has_pose(&self) -> bool {
        self.has_pose
    }
}

/// Validates the part of a `DECOR_ADD` that needs the world (`Err` leaves the set unchanged).
pub fn check_add(a: &DecorAdd) -> Result<(), SimError> {
    if a.kind == dc::KIND_ATTACHED && !a.parent_mover.is_valid() {
        return Err(SimError::BadCommand(
            "an attached DECOR_ADD needs a parent mover",
        ));
    }
    Ok(())
}
