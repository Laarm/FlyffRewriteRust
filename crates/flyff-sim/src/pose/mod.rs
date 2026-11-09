//! Pose evaluation (Plan/31 §13; WP-SIM-12): the runtime skeleton and clip structures built from the
//! `FSK1`/`FMO1` packs of Plan/30, `CMotion::AnimateBone` ([`animate_bones`]), per-mover pose state with
//! motion blending, skin palettes, attachment matrices and event/force points.
//!
//! The data flow is one-way: pack bytes (the browser gets them unchanged from the decode workers, the
//! server builds the same bytes natively with `flyff_formats::pack`, [`native`]) go through the validated
//! views `SkeletonView`/`MotionView` into [`SimSkeleton`]/[`SimClip`]; [`animate_bones`] turns a clip and a
//! clock [`Sample`] into model-space bone matrices; [`PoseState`] keeps them per mover; [`palette`] writes
//! the render outputs (`Float32Array`-friendly `f32` slices in three.js space).
//!
//! | module | content |
//! |---|---|
//! | [`quat`] | `Quat`, `slerp`/`lerp`, `R(q)`, decompose, 4x4/4x3 products, `inverse4x4` |
//! | [`skeleton`], [`clip`] | `SimSkeleton::from_view`, `SimClip::from_view` (validated owned copies) |
//! | [`animate`] | `animate_bones`, `Sample`, `bone_matrix`, `event_point` |
//! | [`palette`] | skin palette, attachment record, wing matrix, render-space matrices |
//! | [`force`] | grips, force points, the sword-trail path (`sample_force_path`) |
//! | [`rigid`] | `animate_objects`: rigid node animation of ctrl objects (`Object3D::Animate`) |
//! | [`state`] | `PoseState`, `PoseLibrary` |
//! | [`native`] | server-side builders from parsed `.chr`/`.ani`/`.o3d` through the same packs |
//!
//! Everything is `f32`, row-vector, left-handed and panic-free; see `NOTES.md` in this directory.

pub mod animate;
pub mod clip;
pub mod force;
pub mod native;
pub mod palette;
pub mod quat;
pub mod rigid;
pub mod skeleton;
pub mod state;

pub use animate::{
    Sample, animate_bones, bone_matrix, event_point, try_bone_matrix, try_event_point,
};
pub use clip::{ClipBone, MAX_CLIP_BONES, SimClip};
pub use force::{
    ForceParams, Grip, MAX_FORCE_POINTS, force_point, force_point_world, grip_matrix,
    hand_point_world, sample_force_path,
};
pub use native::PoseBuildError;
pub use palette::{
    ATTACH_FLOATS, AttachPoint, MATRIX_FLOATS, PALETTE_ROW_FLOATS, attach_matrix, palette_rows,
    skin_matrix, weapon_matrix, wing_matrix, write_attachments, write_palette, write_render_matrix,
};
pub use quat::{
    Key, MAT43_IDENTITY, Mat43, Mat44, Quat, decompose, inverse4x4, lerp, mat_from_quat, mat_rt,
    mul_affine, mul43, mul44, quat_from_mat43, slerp,
};
pub use rigid::animate_objects;
pub use skeleton::{GRIP_SANE_LIMIT, MAX_EVENTS, SimSkeleton, SkelBone};
pub use state::{PoseLibrary, PoseState};
