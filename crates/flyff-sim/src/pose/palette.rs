//! Skin palette and attachment outputs (Plan/31 §13.4, §17.7; ref/18 §7.2, §7.3, §11.3; WP-SIM-12).
//!
//! The render outputs are plain `f32` slices (a `Float32Array` view in the browser): per matrix 12 floats =
//! 3 vec4 rows of the transposed affine matrix in three.js space, so that the vertex shader computes
//! `p' = vec3(dot(row0, vec4(p, 1)), dot(row1, ..), dot(row2, ..))` (Plan/31 §13.4, doc 41 §12.6).
//! Everything is computed in the left-handed D3D space first and mapped with `S · M · S`
//! (`S = diag(1, 1, -1, 1)`, [`flyff_formats::space::mirror_mat`]) last.

use super::quat::{
    MAT43_IDENTITY, Mat43, Mat44, mat43_from_44, mat44_from_43, mul_affine, mul43, mul44,
    rotation_x_deg, rotation_z_deg, transform_coord,
};
use super::skeleton::SimSkeleton;
use crate::num::Vec3;

/// Floats per palette/attachment matrix (3 RGBA32F texels).
pub const MATRIX_FLOATS: usize = 12;
/// Floats per palette texture row: 512 matrices (1536 texels x 4), Plan/31 §17.7 size rule.
pub const PALETTE_ROW_FLOATS: usize = 6144;
/// Floats per attachment record: `RH, LH, SHIELD, KNUCKLE, WING` (5 x 12), 8 event points x 3, broom lift,
/// 3 padding (Plan/31 §17.7).
pub const ATTACH_FLOATS: usize = 88;

/// Texture rows needed for `palette_floats` floats: `ceil(palette_floats / 6144)` (0 when empty), the
/// `palette_rows` of the render-view header.
pub const fn palette_rows(palette_floats: usize) -> usize {
    palette_floats.div_ceil(PALETTE_ROW_FLOATS)
}

/// The first 12 floats of `out` receive `m` (left-handed, row-vector) as three.js space rows.
/// Returns `false` (nothing written) when `out` is too short.
pub fn write_render_matrix(m: &Mat43, out: &mut [f32]) -> bool {
    let Some(dst) = out.first_chunk_mut::<MATRIX_FLOATS>() else {
        return false;
    };
    *dst = render_rows(m);
    true
}

/// The 12 render floats of `m`: `S·M·S` (negate `_13 _23 _31 _32 _43`; the fourth column is dropped), then
/// the three rows of the transposed 3x4 matrix.
pub fn render_rows(m: &Mat43) -> [f32; MATRIX_FLOATS] {
    let [m11, m12, m13, m21, m22, m23, m31, m32, m33, m41, m42, m43] = *m;
    [
        m11, m21, -m31, m41, //
        m12, m22, -m32, m42, //
        -m13, -m23, m33, -m43,
    ]
}

/// A model-space position mapped to three.js space (`z` negated).
pub fn render_pos(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, -p.z)
}

/// The skin matrix of bone `b`: `inverse_tm[b] · bone[b]` (model space, row vectors; ref/18 §7.2).
/// `None` when `b` is outside the skeleton or the pose.
pub fn skin_matrix(skel: &SimSkeleton, bones: &[Mat44], b: usize) -> Option<Mat43> {
    let inv = skel.bones.get(b)?.inverse_tm;
    Some(mul43(&inv, &mat43_from_44(bones.get(b)?)))
}

/// Writes `inverse_tm[b] · bone[b]` for every bone of `skel` that `bones` has, as 12 floats each
/// ([`write_render_matrix`]) into `out`; returns the number of matrices written (limited by `out.len() /
/// 12`).
pub fn write_palette(skel: &SimSkeleton, bones: &[Mat44], out: &mut [f32]) -> usize {
    let (chunks, _) = out.as_chunks_mut::<MATRIX_FLOATS>();
    let mut n = 0;
    for ((bone, pose), dst) in skel.bones.iter().zip(bones).zip(chunks) {
        *dst = render_rows(&mul43(&bone.inverse_tm, &mat43_from_44(pose)));
        n += 1;
    }
    n
}

/// Where an attached model hangs (`AttachPoint` of the decor entries, Plan/31 §13.7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttachPoint {
    /// The model root: identity.
    Root,
    /// `local_rh · bone[r_hand]`: right-hand weapon.
    RightHand,
    /// `local_lh · bone[l_hand]`: left-hand weapon.
    LeftHand,
    /// `local_shield · bone[l_arm]`.
    Shield,
    /// `local_knuckle · bone[r_arm]`.
    Knuckle,
    /// [`wing_matrix`].
    Wing,
}

impl AttachPoint {
    /// The point of the wire code (0 ROOT, 1 RH, 2 LH, 3 SHIELD, 4 KNUCKLE, 5 WING, Plan/31 §17.4 op 0x1D).
    pub const fn from_code(code: u8) -> Option<AttachPoint> {
        Some(match code {
            0 => AttachPoint::Root,
            1 => AttachPoint::RightHand,
            2 => AttachPoint::LeftHand,
            3 => AttachPoint::Shield,
            4 => AttachPoint::Knuckle,
            5 => AttachPoint::Wing,
            _ => return None,
        })
    }
}

/// `CMover::Render` wing placement (`MoverRender.cpp:734-760`): `Rz(-90°) · Rx(180°) · bone[4]` with the
/// translation replaced by `bone[5]`'s; feather wings add `(0, -0.02, +0.1)` to that translation.
///
/// The product order is that of the C++ (`matEvent = Rz × (Rx × I)`, then `× bone[4]`); Plan/31 §13.4 and
/// ref/18 §11.3 print the rotations in the opposite order, which gives the negated xy block (see
/// `NOTES.md`). `None` when the pose has no bone 4 or 5.
pub fn wing_matrix(bones: &[Mat44], feather: bool) -> Option<Mat44> {
    let spine1 = bones.get(4)?;
    let spine2 = bones.get(5)?;
    let rot = mul44(&rotation_z_deg(-90.0), &rotation_x_deg(180.0));
    let mut m = mul44(&rot, spine1);
    m.0[12] = spine2.0[12];
    m.0[13] = spine2.0[13] - if feather { 0.02 } else { 0.0 };
    m.0[14] = spine2.0[14] + if feather { 0.1 } else { 0.0 };
    Some(m)
}

/// The model-space matrix of an attachment point at the current pose (`None` when the pose lacks the bone).
/// Weapon grips: `gmLocalTM · localRH · bone[parent]` in the original, so the weapon's own `localTM` is the
/// renderer's to compose in front (ref/18 §7.3).
pub fn attach_matrix(skel: &SimSkeleton, bones: &[Mat44], point: AttachPoint) -> Option<Mat44> {
    let grip = |local: &Mat43, bone: u16| -> Option<Mat44> {
        let b = bones.get(usize::from(bone))?;
        Some(mul_affine(&mat44_from_43(local), b))
    };
    match point {
        AttachPoint::Root => Some(Mat44::IDENTITY),
        AttachPoint::RightHand => grip(&skel.local_rh, skel.r_hand),
        AttachPoint::LeftHand => grip(&skel.local_lh, skel.l_hand),
        AttachPoint::Shield => grip(&skel.local_shield, skel.l_arm),
        AttachPoint::Knuckle => grip(&skel.local_knuckle, skel.r_arm),
        AttachPoint::Wing => wing_matrix(bones, false),
    }
}

/// The world matrix of a weapon (or shield) model hanging on `point` (`CModelObject::Render`, ref/18 §7.3):
/// `gm_local · grip_local · bone[parent] · World`, associated like the original
/// (`m = bone · World; m = grip_local · m; m = gm_local · m`). `gm_local` is the weapon object's own
/// `localTM`; `world` the mover's world matrix. `Root` hangs the model on the mover itself
/// (`gm_local · World`); `Wing` uses [`wing_matrix`] in place of `bone · World`'s bone. `None` when the pose
/// lacks the bone.
pub fn weapon_matrix(
    skel: &SimSkeleton,
    bones: &[Mat44],
    point: AttachPoint,
    gm_local: &Mat44,
    world: &Mat44,
) -> Option<Mat44> {
    let on = |local: &Mat43, bone: u16| -> Option<Mat44> {
        let b = bones.get(usize::from(bone))?;
        let m = mul44(b, world);
        Some(mul44(gm_local, &mul44(&mat44_from_43(local), &m)))
    };
    match point {
        AttachPoint::Root => Some(mul44(gm_local, world)),
        AttachPoint::RightHand => on(&skel.local_rh, skel.r_hand),
        AttachPoint::LeftHand => on(&skel.local_lh, skel.l_hand),
        AttachPoint::Shield => on(&skel.local_shield, skel.l_arm),
        AttachPoint::Knuckle => on(&skel.local_knuckle, skel.r_arm),
        AttachPoint::Wing => Some(mul44(gm_local, &mul44(&wing_matrix(bones, false)?, world))),
    }
}

/// Writes the 88-float attachment record of Plan/31 §17.7 into `out`: `RH, LH, SHIELD, KNUCKLE, WING` as
/// render matrices (identity for a point the pose lacks), 8 event points x 3 in three.js space (zeros for an
/// unused slot), the broom lift `bone[0]._42` and 3 zero floats. Returns `false` (nothing written) when
/// `out` has fewer than [`ATTACH_FLOATS`] floats.
pub fn write_attachments(skel: &SimSkeleton, bones: &[Mat44], out: &mut [f32]) -> bool {
    let Some(rec) = out.first_chunk_mut::<ATTACH_FLOATS>() else {
        return false;
    };
    rec.fill(0.0);
    let points = [
        AttachPoint::RightHand,
        AttachPoint::LeftHand,
        AttachPoint::Shield,
        AttachPoint::Knuckle,
        AttachPoint::Wing,
    ];
    let (chunks, _) = rec.as_chunks_mut::<MATRIX_FLOATS>();
    for (point, dst) in points.iter().zip(chunks) {
        let m = attach_matrix(skel, bones, *point).map_or(MAT43_IDENTITY, |m| mat43_from_44(&m));
        *dst = render_rows(&m);
    }
    let events = rec.get_mut(5 * MATRIX_FLOATS..5 * MATRIX_FLOATS + 24);
    if let Some(events) = events {
        let (chunks, _) = events.as_chunks_mut::<3>();
        for ((pos, parent), dst) in skel.events.iter().zip(chunks) {
            let bone = usize::try_from(*parent).ok().and_then(|p| bones.get(p));
            if let Some(bone) = bone {
                let p = render_pos(transform_coord(bone, *pos));
                *dst = [p.x, p.y, p.z];
            }
        }
    }
    if let Some(lift) = rec.get_mut(5 * MATRIX_FLOATS + 24)
        && let Some(root) = bones.first()
    {
        *lift = root.0[13];
    }
    true
}
