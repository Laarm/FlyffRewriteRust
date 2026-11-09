//! Motion selection and the per-frame clock step of a mover (Plan/31 §12.3, §12.4, §12.7; WP-SIM-06/09).
//!
//! [`set_motion`] ports `CMover::SetMotion` (weapon-stance offsets, same-motion rule, missing-clip quirk,
//! blending rule); [`frame_move_mover`] runs the footstep window, the attribute sounds, the clock advance
//! and the blend weight progression of one frame.

use crate::anim::attr::attrs_firing;
use crate::anim::motion_table::ModelKey;
use crate::mover::SimMover;
use crate::num::{ftoi_x86, trunc3};
use crate::step::{Cx, SimEvent};
use crate::types::*;

/// `m_bMotionBlending` setter (`SetMotionBlending`): changing the flag resets the weight to 0; setting it
/// to its current value does nothing (KEEP-QUIRK: a running blend is not restarted).
pub fn set_blending(m: &mut SimMover, on: bool) {
    if on == m.motion.blend.active {
        return;
    }
    m.motion.blend.active = on;
    m.motion.blend.weight = 0.0;
}

/// The weapon-stance offset added to `mti` in combat stance (`CMover::SetMotion`, Plan/31 §12.3): `None`
/// when the mover has no active-hand item (the client returned FALSE, the server dereferenced NULL).
fn stance_offset(m: &SimMover, mti: u32) -> Option<u32> {
    let p = &m.params;
    let idx = p.mover_index;
    let is_master_index = matches!(
        idx,
        defs::MI_CROWNIBLIS | defs::MI_CROWNSHADE | defs::MI_CROWNBUBBLE | defs::MI_DWARPETMAS
    );
    let st = p.stance?;
    let applies = p.kind != MoverKind::Player
        || is_master_index
        || st.master_model
        || idx == defs::MI_MALE
        || idx == defs::MI_FEMALE;
    if !applies {
        return Some(mti);
    }
    let master = st.master_model || is_master_index;
    // DWORD arithmetic: the motion id of a `MOTION` message is client-controlled and may be anything
    let add = |base: u32, n: u32| base.wrapping_add(n);
    let mut out = mti;
    if st.dual && !master {
        out = add(out, 900);
    } else {
        match st.kind3 {
            defs::IK3_YOYO => out = add(out, if master { 200 } else { MTI_STAND_14 }),
            defs::IK3_BOW => out = add(out, if master { 200 } else { MTI_STAND_13 }),
            defs::IK3_SWD => out = add(out, if st.two_handed { 800 } else { 200 }),
            defs::IK3_AXE => out = add(out, if st.two_handed { 800 } else { 500 }),
            defs::IK3_WAND => {
                out = add(
                    out,
                    if st.psy_job {
                        MTI_STAND_11
                    } else {
                        MTI_STAND_02
                    },
                )
            }
            defs::IK3_CHEERSTICK => out = add(out, 400),
            defs::IK3_STAFF => out = add(out, MTI_STAND_05),
            defs::IK3_KNUCKLEHAMMER => out = add(out, 700),
            _ => {
                if p.kind == MoverKind::Player {
                    out = add(out, 200);
                }
            }
        }
    }
    Some(out)
}

/// `CMover::SetMotion(mti0, lp, opt)`: returns whether the motion changed.
///
/// In combat stance without `MOP_FIXED` the id gets the weapon offset; asking for the motion that is
/// already playing (same un-offset id and option) is ignored while looping or still playing, and for an
/// ended `CONT`. A missing clip keeps the stale clock (KEEP-QUIRK) and never fires hits.
pub fn set_motion(m: &mut SimMover, cx: &mut Cx<'_>, mti0: u32, lp: u32, opt: u32) -> bool {
    let mut mti = mti0;
    if m.act.flags.combat() && opt & MOP_FIXED == 0 {
        match stance_offset(m, mti0) {
            Some(v) => mti = v,
            None => return false,
        }
    }
    if m.motion.id == mti0 && m.motion.option == opt {
        if lp == ANILOOP_LOOP {
            return false;
        }
        if !m.motion.clock.end_frame {
            return false;
        }
        if lp == ANILOOP_CONT {
            return false;
        }
    }
    // LoadMotion
    m.motion.prev_clip = m.motion.clip;
    m.motion.prev_frame = ftoi_x86(m.motion.clock.frame_cur);
    let clip = cx.env.motions.resolve(m.params.model_key, mti);
    m.motion.clip = clip;
    if let Some(c) = clip {
        if m.motion.prev_clip.is_none() {
            m.motion.prev_clip = Some(c);
        }
        m.motion.clock.init_animate();
        if let Some(meta) = cx.env.motions.clip(c) {
            m.motion.clock.frame_max = i32::try_from(meta.frames).unwrap_or(i32::MAX);
            m.motion.clock.per_slerp = trunc3(meta.per_slerp_raw);
        }
    }
    m.motion.id = mti0;
    m.motion.resolved = mti;
    m.motion.option = opt;
    m.motion.clock.end_frame = false;
    m.motion.clock.lp = lp;
    let opt2 = if matches!(
        m.params.mover_index,
        defs::MI_NPC_SNOWMAN01 | defs::MI_NPC_SNOWMAN02
    ) {
        opt | MOP_NO_TRANS
    } else {
        opt
    };
    set_blending(m, opt2 & MOP_NO_TRANS == 0);
    if cx.client() {
        if opt & MOP_SWDFORCE != 0 {
            if m.is_player() {
                cx.emit(SimEvent::SwordTrail {
                    hit_slow: opt & MOP_HITSLOW != 0,
                });
            }
        } else {
            cx.emit(SimEvent::SwordTrailClear);
        }
    }
    true
}

/// The `ProcessAniSpeed` walk factor right before `frame_move` (`Mover.cpp:3911-3962`, Plan/31 §12.7).
pub fn walk_factor(m: &SimMover, cx: &Cx<'_>) -> f32 {
    let st = m.act.state;
    if (m.is_player() && m.act.flags.fly())
        || !(st.is_state(OBJSTA_FMOVE) || st.is_state(OBJSTA_TURN_ALL))
    {
        return 1.0;
    }
    let spd = crate::kin::get_speed(m, cx.env);
    let mf = m.motion.clock.frame_max as f32;
    let mut f = if m.params.kind != MoverKind::Player {
        if matches!(
            m.params.ai,
            defs::AII_MONSTER | defs::AII_PET | defs::AII_EGG
        ) {
            (spd * mf) / 1.1
        } else {
            1.0
        }
    } else {
        (spd * mf) / 2.2
    };
    f = if (m.params.frame_factor - 0.0).abs() < 0.000_001 || m.params.prop_flying == 1 {
        1.0
    } else {
        f * m.params.frame_factor
    };
    if (f - 0.0).abs() < 0.000_001 { 1.0 } else { f }
}

/// Step 6 of `step_frame` (Plan/31 §3.2): footstep window, attribute sounds, the clock advance and the
/// blend weight progression (which runs even while the clock is paused).
pub fn frame_move_mover(m: &mut SimMover, cx: &mut Cx<'_>) {
    let env = cx.env;
    let clip = m.motion.clip.and_then(|c| env.motions.clip(c));
    let rule = env.flags.hit_attr_rule;
    if cx.client() && m.motion.clock.pause == 0 {
        // Footstep (CMover::ProcessWalkSound): evaluated on the window of the previous advance
        if m.is_player()
            && m.act.state.is_state(OBJSTA_FMOVE)
            && !attrs_firing(&m.motion.clock, clip, MA_HIT, rule).is_empty()
        {
            cx.emit(SimEvent::Footstep {
                obj: m.id,
                x: m.pos.x,
                y: m.pos.y,
                z: m.pos.z,
            });
        }
        // attribute sounds and quakes of the previous advance
        for a in attrs_firing(&m.motion.clock, clip, MA_SOUND, rule) {
            if a.sound > 0 {
                cx.emit(SimEvent::AttrSound {
                    obj: m.id,
                    sound: a.sound,
                    x: m.pos.x,
                    y: m.pos.y,
                    z: m.pos.z,
                    positional: true,
                });
            }
        }
        if !attrs_firing(&m.motion.clock, clip, MA_QUAKE, rule).is_empty() {
            cx.emit(SimEvent::AttrQuake { obj: m.id });
        }
    }
    let speed = m.ani_speed * walk_factor(m, cx);
    m.motion.clock.frame_move(speed);
    if m.motion.blend.active {
        m.motion.blend.weight += 0.05;
        if m.motion.blend.weight >= 1.0 {
            set_blending(m, false);
        }
    }
}

/// The model key a mover's motions are resolved with.
pub fn model_key(m: &SimMover) -> ModelKey {
    m.params.model_key
}
