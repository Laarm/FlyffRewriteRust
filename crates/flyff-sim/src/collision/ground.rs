//! Per-frame collision: applies `delta_accu` to the position (Plan/31 §10.8, §10.9; WP-SIM-06).
//!
//! `ProcessCollisionGround` handles walking movers (terrain following, attribute undo, slope slide,
//! ledge drops, head bump) and `ProcessCollisionFly` flying ones (flying players and flying NPCs). Both
//! run once per 1/60 s frame on both sides (PL-002; the server's `/4`-`*4` `__X15` scaling is gone).

use super::terrain::QueryCtx;
use crate::mover::SimMover;
use crate::num::{Vec3, length};
use crate::step::{Cx, SimEvent};
use crate::types::*;

/// `COLL_TEST_DIR`: the horizontal probe looks 6.5 frames of travel ahead.
pub const COLL_TEST_DIR: f32 = 6.5;

/// `CActionMover::ProcessCollision` (Plan/31 §10.8/10.9): applies `delta_accu`, stores `pos_last` and emits
/// `Collided` when the result was non-zero. Pending-land touches become `ChunkMissing` events.
pub fn process(m: &mut SimMover, cx: &mut Cx<'_>) {
    let mut q = QueryCtx::new(m.pos.y);
    let collided = if m.act.flags.fly() || m.params.flying_npc {
        process_fly(m, cx, &mut q)
    } else {
        process_ground(m, cx, &mut q)
    };
    m.act.pos_last = m.pos;
    for (lx, lz) in q.missing.drain(..) {
        cx.emit(SimEvent::ChunkMissing { lx, lz });
    }
    if collided {
        cx.emit(SimEvent::Collided);
    }
}

/// Moves along x then z with the heightmap attribute checks; returns `(blocked, died)`.
fn step_attr_ground(m: &mut SimMover, cx: &mut Cx<'_>, probe: Vec3) -> bool {
    let w = cx.env.world;
    let mut coll = false;
    m.pos.x += probe.x;
    m.pos.x = w.clip_x(m.pos.x);
    let a = w.height_attr(m.pos.x, m.pos.z);
    if a == HATTR_NOWALK || a == HATTR_NOMOVE {
        m.pos.x -= probe.x;
        coll = true;
    }
    m.pos.z += probe.z;
    m.pos.z = w.clip_z(m.pos.z);
    let a = w.height_attr(m.pos.x, m.pos.z);
    if a == HATTR_NOWALK || a == HATTR_NOMOVE {
        m.pos.z -= probe.z;
        coll = true;
    } else if a == HATTR_DIE {
        cx.emit(SimEvent::EnteredDeathZone);
    }
    coll
}

/// `ProcessCollisionGround` (Plan/31 §10.8). Returns whether the collision result was non-zero.
fn process_ground(m: &mut SimMover, cx: &mut Cx<'_>, q: &mut QueryCtx) -> bool {
    let env = cx.env;
    let world = env.world;
    let d = m.act.delta_accu;
    let mut result: u8 = 0;

    // A. horizontal movement with the 6.5-frame look-ahead probe
    if m.act.moving {
        let knee = m.pos + Vec3::new(0.0, 0.4, 0.0);
        let mut probe = Vec3::new(d.x, 0.0, d.z) * COLL_TEST_DIR;
        let pet = env.flags.improved_pets && matches!(m.params.ai, defs::AII_PET | defs::AII_EGG);
        if !pet && length(probe) != 0.0 {
            let (r, out) = world.process_collision(env.dynamic, knee, probe, 0);
            result = r;
            probe = out;
        }
        probe = Vec3::new(
            probe.x / COLL_TEST_DIR,
            probe.y / COLL_TEST_DIR,
            probe.z / COLL_TEST_DIR,
        );
        if result != 0 && length(probe) > 0.15 {
            // __FIX_COLLISION: keep the slide as the new velocity
            probe.y = 0.0;
            m.act.delta_accu.y = 0.0;
            m.act.delta.y = 0.0;
            m.act.delta.x = probe.x;
            m.act.delta.z = probe.z;
        }
        if step_attr_ground(m, cx, probe) {
            cx.emit(SimEvent::AttrBlocked);
        }
    }
    // B. the corpse is fading: no terrain test
    if m.act.state.dmg() == OBJSTA_DISAPPEAR {
        return result != 0;
    }
    // C. falling / landing
    if d.y < 0.0 {
        if m.act.ground == 1 {
            return result != 0;
        }
        let under = Vec3::new(m.pos.x, m.pos.y + 1.0, m.pos.z);
        let (h, slide) = world.process_under_collision(env.dynamic, under, q);
        m.act.current_height = h;
        m.pos.y += d.y;
        let landing = if m.act.state.jump() == OBJSTA_SJUMP3 {
            m.pos.y < h
        } else if m.pos.y - 0.25 > h {
            if d.y < -0.1 {
                if m.act.state.bits() & OBJSTA_COLLECT == 0
                    && m.is_player()
                    && !m.act.state.dmg_fly_all()
                    // FIX (PL-172, deviation from the original): a cast or a swing in progress keeps its state and
                    // motion while the mover drops. The original entered the fall again on the frame after
                    // `MAGICCASTING` had cleared the state word, and `ProcessState(SJUMP3)` replaced the cast
                    // motion with `MTI_JUMP3` on every frame, so a spell started right after a step down a slope
                    // ended without a hit and without its MP cost. The fall itself is unchanged (gravity, landing).
                    && !m.act.state.is_act_attack()
                {
                    m.set_jump_state(cx, OBJSTA_SJUMP3); // V15: only players enter the fall state
                }
                cx.emit(SimEvent::Dropped);
            }
            false
        } else {
            true
        };
        if landing {
            let returning_npc = !m.is_player() && m.params.speed_factor > 1.9;
            if slide.y != 0.0 && !returning_npc {
                m.pos = slide;
                m.act.ground = 2;
                cx.emit(SimEvent::AttrBlocked);
            } else {
                m.pos.y = h;
                m.act.ground = 1;
            }
            m.act.delta.y = 0.0;
        }
    } else if d.y > 0.0 {
        // D. rising: head bump (height 1.5)
        let top = Vec3::new(m.pos.x, m.pos.y + 1.5, m.pos.z);
        let h = if m.is_player() {
            world.over_height_for_player(top)
        } else {
            world.over_height(top)
        };
        m.pos.y += d.y;
        if m.pos.y + 1.5 > h {
            m.pos.y = h - 1.5;
            m.act.delta.y = -0.01;
            result = 2;
        }
    }
    result != 0
}

/// `ProcessCollisionFly` (Plan/31 §10.9). Returns whether the result was non-zero.
fn process_fly(m: &mut SimMover, cx: &mut Cx<'_>, q: &mut QueryCtx) -> bool {
    let env = cx.env;
    let world = env.world;
    if m.act.acc_power == 0.0 && m.act.delta_accu == Vec3::ZERO {
        return false;
    }
    let start = m.pos + Vec3::new(0.0, 0.5, 0.0);
    let mut d = m.act.delta_accu;
    let mut result = false;
    // CanCollisionFly: the server rule on both sides (Plan/31 §22 Q7)
    if !m.is_player() || m.is_empty_dest() {
        let mut dir = m.act.delta_accu;
        let l = length(dir);
        if l > 0.001 {
            let r = if l < 0.65 { 0.65 / l } else { 1.0 };
            dir = Vec3::new(dir.x * r, dir.y * r, dir.z * r);
            if let Some(o) = world.process_collision_reflection(start, dir, 0) {
                let v = Vec3::new(o.x / r, o.y / r, o.z / r);
                d = Vec3::new(v.x * 0.7, v.y * 0.7, v.z * 0.7);
                m.act.delta = d;
                result = true;
            }
        }
    }
    m.pos.y += d.y;
    m.pos.x += d.x;
    m.pos.x = world.clip_x(m.pos.x);
    if world.height_attr(m.pos.x, m.pos.z) == HATTR_NOMOVE {
        m.pos.x -= d.x;
    }
    m.pos.z += d.z;
    m.pos.z = world.clip_z(m.pos.z);
    let attr = if world.fly_allowed {
        world.height_attr(m.pos.x, m.pos.z)
    } else {
        HATTR_NOFLY
    };
    match attr {
        HATTR_NOMOVE => m.pos.z -= d.z,
        HATTR_DIE => cx.emit(SimEvent::EnteredDeathZone),
        HATTR_NOFLY => {
            m.pos.x -= d.x;
            m.pos.z -= d.z;
            m.pos.y -= d.y;
            cx.emit(SimEvent::EnteredNoFly);
        }
        _ => {}
    }
    m.act.current_height = world.full_height_q(Vec3::new(m.pos.x, m.pos.y + 1.0, m.pos.z), q);
    if m.pos.y > world.max_height {
        m.pos.y = world.max_height;
    } else if world
        .water_at(m.pos)
        .is_some_and(|(_, tex)| tex == WTYPE_CLOUD)
        && m.act.current_height == 0.0
    {
        if m.pos.y < world.min_height {
            m.pos.y = world.min_height;
        }
    } else if m.pos.y < m.act.current_height {
        m.pos.y = m.act.current_height;
        result = true;
    }
    result
}
