//! Frame attribute detection (`IsAttrHit`/`IsAttrSound`/`IsAttrQuake`, `src:_Common/Bone.h:162-185`;
//! KQ-01, config `fidelity.hit_attr_rule`; Plan/31 §12.5; WP-SIM-05).
//!
//! * [`HitAttrRule::Legacy`] (default, KEEP-QUIRK): only the attribute stored at `(int)frame_cur` is
//!   examined; it fires when `frame_old < a.frame <= frame_cur`. The rule skips attributes whenever
//!   `per_slerp * speed > 1` and never sees a frame-0 hit after a loop wrap.
//! * [`HitAttrRule::Window`]: every attribute with `frame_old < a.frame <= frame_cur` (across the wrap in
//!   `LOOP` mode) fires once, in time order.
//! * `.o3d` built-ins (ctrls): exact float equality `attr[(int)f].frame == f` ([`is_attr_o3d`]).

use super::clock::MotionClock;
use super::motion_table::{ClipMeta, MotionAttr};
use crate::num::ftoi_x86;
use flyff_common::config::HitAttrRule;

/// The attributes of `clip` matching `mask` that fire for the clock's last advance (`frame_old` ->
/// `frame_cur`) under `rule`; empty for a missing clip (`None`: the original `LoadMotion` failed, hits
/// never fire).
pub fn attrs_firing<'a>(
    clock: &MotionClock,
    clip: Option<&'a ClipMeta>,
    mask: u32,
    rule: HitAttrRule,
) -> Vec<&'a MotionAttr> {
    let Some(clip) = clip else {
        return Vec::new();
    };
    let (old, cur) = (clock.frame_old, clock.frame_cur);
    match rule {
        HitAttrRule::Legacy => {
            let idx = ftoi_x86(cur);
            let Ok(idx) = u32::try_from(idx) else {
                return Vec::new();
            };
            match clip.attr_at(idx) {
                Some(a) if a.flags & mask != 0 && old < a.frame && a.frame <= cur => vec![a],
                _ => Vec::new(),
            }
        }
        HitAttrRule::Window => {
            let mut out = Vec::new();
            if cur >= old {
                for a in &clip.attrs {
                    if a.flags & mask != 0 && old < a.frame && a.frame <= cur {
                        out.push(a);
                    }
                }
            } else {
                // looped: the tail (old, frame_max) first, then [0, cur]
                let max_f = clock.frame_max as f32;
                for a in &clip.attrs {
                    if a.flags & mask != 0 && old < a.frame && a.frame < max_f {
                        out.push(a);
                    }
                }
                for a in &clip.attrs {
                    if a.flags & mask != 0 && a.frame >= 0.0 && a.frame <= cur {
                        out.push(a);
                    }
                }
            }
            out
        }
    }
}

/// `IsAttrHit()`: does a `MA_HIT` attribute fire for the last advance? (`Legacy` fires at most one;
/// `Window` may fire two at the 2.0 speed cap.)
pub fn is_attr_hit(clock: &MotionClock, clip: Option<&ClipMeta>, rule: HitAttrRule) -> bool {
    !attrs_firing(clock, clip, crate::types::MA_HIT, rule).is_empty()
}

/// The first attribute of `mask` that fires (`IsAttrSound` returns the attribute itself).
pub fn is_attr<'a>(
    clock: &MotionClock,
    clip: Option<&'a ClipMeta>,
    mask: u32,
    rule: HitAttrRule,
) -> Option<&'a MotionAttr> {
    attrs_firing(clock, clip, mask, rule).into_iter().next()
}

/// The `.o3d` built-in rule (`Object3D.h:319-345`): the attribute at `(int)f` fires only when its stored
/// `frame` equals `f` exactly (ctrl chests/traps advance in whole frames).
pub fn is_attr_o3d(attrs: &[MotionAttr], f: f32, mask: u32) -> Option<&MotionAttr> {
    let idx = u32::try_from(ftoi_x86(f)).ok()?;
    let a = attrs
        .binary_search_by_key(&idx, |a| a.index)
        .ok()
        .and_then(|i| attrs.get(i))?;
    (a.flags & mask != 0 && a.frame == f).then_some(a)
}
