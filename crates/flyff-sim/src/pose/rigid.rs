//! Rigid node animation of an `.o3d` (`CObject3D::Animate`, `src:_Common/Object3D.cpp:2582-2656`; Plan/31
//! §13.6; WP-SIM-24).
//!
//! For every LOD-0 object `i` (parents precede children):
//!
//! ```text
//! node keys present:  update[i] = R(slerp(k[f].q, k[next].q, s)) · T(lerp(k[f].p, k[next].p, s)) · parent
//! otherwise:          update[i] = local_tm · parent                    (a frozen object always takes this branch)
//! ```
//!
//! `f = (int)frame`, `s = frame - f`, `parent` is `update[parent_index]` or the identity for a root; the
//! `localTM` of an animated object is ignored (its scale is lost, ref/18 §4.8). Used by the dynamic ctrl
//! colliders ([`crate::collision::dynamic`]) over the objects of a `CtrlModel`. The renderer's twin is the
//! node clip (`FMO1` flag bit 2) evaluated by [`super::animate_bones`], which gives the same matrices
//! (Plan/31 §13.6).
//!
//! The original never animated on the world server (`#if !defined(__WORLDSERVER)`, `_mUpdate` stayed the
//! identity); both roles of the port evaluate the same per-instance pose at the ctrl's own frame (FIX,
//! Plan/31 §10.10 rule 4, PL-019).

use super::quat::{Mat44, lerp, mat_rt, mul_affine, slerp};
use crate::collision::ctrl_models::RigidObject;
use crate::num::ftoi_x86;

/// The `update` matrices of `objects` at `frame` (`next` = the clock's `GetNextFrame`), written to `out`
/// (cleared first; one matrix per object).
///
/// The key indices are clamped into the key array (a clock that is ahead of the clip reads the last key,
/// the original read out of bounds); a non-finite `frame` counts as `0`. Total: never panics.
pub fn animate_objects(objects: &[RigidObject], frame: f32, next: i32, out: &mut Vec<Mat44>) {
    out.clear();
    let f = ftoi_x86(if frame.is_finite() { frame } else { 0.0 });
    let slp = if frame.is_finite() {
        frame - f as f32 // BOUNDS: f is a small frame index
    } else {
        0.0
    };
    for obj in objects {
        let local = match obj.frames.len() {
            0 => obj.local_tm,
            n => {
                let last = n - 1;
                let at = |i: i32| usize::try_from(i).unwrap_or(0).min(last);
                match (obj.frames.get(at(f)), obj.frames.get(at(next))) {
                    (Some(a), Some(b)) => {
                        let q = slerp(a.0, b.0, slp);
                        let p = lerp(a.1, b.1, slp);
                        mat_rt(q, p)
                    }
                    _ => obj.local_tm,
                }
            }
        };
        let m = match usize::try_from(obj.parent).ok().and_then(|p| out.get(p)) {
            Some(parent) => mul_affine(&local, parent),
            None => local,
        };
        out.push(m);
    }
}
