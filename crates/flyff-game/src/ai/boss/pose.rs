//! Pose data of the boss hits (Plan/18 §10.4 "Pose data", Plan/24 §10.9, Plan/31 §13.5; task boss-ai-core,
//! WP-AI-18): the event points and bones of a boss's rig at the frame its hit saw.
//!
//! Only the ClockWorks needs positions of its body on the server: the damage of its Slash is centred on event
//! point 0 and its laser runs from event point 2 along bone 6 (`OnAttackMelee_ClockWorks`,
//! `OnAttackMeleeContinue`). The original updated the model's bone matrices (`UpdateMatrixBone`) at the hit; the
//! server of the rewrite evaluates the ancestor chain of the one bone it needs
//! ([`flyff_sim::pose::try_event_point`], [`flyff_sim::pose::try_bone_matrix`]) on the rig the asset pipeline
//! materialised at start-up (`ServerDerived.rigs`), reached through the [`BossPoseSource`] seam
//! (`Handlers.boss_poses` -> `MoveShared.boss_poses`; `flyff-game` may not depend on `flyff-assets`, so the
//! server wires `BossRigs` in through a newtype).
//!
//! **Which frame.** A hit is detected in the state processing of frame `F`, before that step's clock advance
//! (`step_frame`: `process_action`, then `frame_move_mover`); the consumers run at the barrier of `F`, after the
//! advance. The frame the hit saw is therefore [`MotionClock::frame_old`] of the barrier's clock
//! ([`hit_clock`] puts it back into `frame_cur`; `MotionClock::frame_move` subtracts `frame_max` from it when a
//! looped clip had passed its end, which is undone).

use crate::mover::Mover;
use flyff_common::math::{D3dMatrix, Vec3};
use flyff_sim::anim::MotionClock;
use flyff_sim::client::view::world_matrix_lh;
use flyff_sim::pose::quat::{mat44_from_43, transform_coord};
use flyff_sim::pose::{SimClip, SimSkeleton, try_bone_matrix, try_event_point};

/// The rigs of the bosses (`ServerDerived.rigs`): the skeleton of a boss mover and the clip it plays for a motion
/// id. `mover` is the `MI_*` id (`m_dwIndex`), `mti` the un-offset motion id of the mover
/// (`SimMover.motion.id`); the implementation resolves the motion table's rules (a motion at or above
/// `motion_max` plays `MTI_STAND`'s clip).
pub trait BossPoseSource: Send + Sync {
    /// The skeleton of boss `mover`, `None` when it has no rig or its `.chr` failed to load.
    fn skeleton(&self, mover: u32) -> Option<&SimSkeleton>;

    /// The clip boss `mover` plays for motion `mti`.
    fn clip(&self, mover: u32, mti: u32) -> Option<&SimClip>;
}

/// The clock of the frame a hit or continue saw, from the clock the barrier sees (see the module docs).
pub fn hit_clock(clock: &MotionClock) -> MotionClock {
    let frame = if clock.frame_old <= -1.0 {
        // `-1` is the state of a clock that never advanced (`InitAnimate`): it is still where it is
        clock.frame_cur
    } else if clock.frame_old < 0.0 && clock.frame_max > 0 {
        // `frame_move` reduced `frame_old` by `frame_max` (a looped clip past its last frame, which is a
        // `frame_cur` in `(max - 1, max)`, so `frame_old` in `(-1, 0)`): undo it
        clock.frame_old + clock.frame_max as f32 // BOUNDS: a frame count
    } else {
        clock.frame_old
    };
    MotionClock {
        frame_cur: frame,
        ..*clock
    }
}

/// `GetMatrixWorldPtr()` of the server copy of `m` (`CObj::UpdateMatrix`: scale, yaw, pitch, position).
pub fn world_matrix(m: &Mover) -> D3dMatrix {
    mat44_from_43(&world_matrix_lh(
        m.sim.pos,
        m.sim.angle,
        m.sim.angle_x,
        m.sim.params.scale,
    ))
}

/// `GetEventPos(event)` of boss `mover` in world space: the `.chr` event point transformed by its parent bone at
/// the frame the hit saw (`clock` is the clock the barrier sees) and by `world`. `None` without a rig, skeleton or
/// clip, or for an event slot or parent bone the rig does not have.
pub fn event_pos(
    rig: Option<&dyn BossPoseSource>,
    mover: u32,
    mti: u32,
    clock: &MotionClock,
    event: usize,
    world: &D3dMatrix,
) -> Option<Vec3> {
    let rig = rig?;
    let skel = rig.skeleton(mover)?;
    let clip = rig.clip(mover, mti)?;
    try_event_point(skel, clip, &hit_clock(clock), event, world)
}

/// `TransformCoord(local, GetMatrixBone(bone))` then `TransformCoord(.., world)`: the model-space point `local` of
/// bone `bone` in world space, at the frame the hit saw. `None` as [`event_pos`].
pub fn bone_point(
    rig: Option<&dyn BossPoseSource>,
    mover: u32,
    mti: u32,
    clock: &MotionClock,
    bone: usize,
    local: Vec3,
    world: &D3dMatrix,
) -> Option<Vec3> {
    let rig = rig?;
    let skel = rig.skeleton(mover)?;
    let clip = rig.clip(mover, mti)?;
    let m = try_bone_matrix(skel, clip, &hit_clock(clock), bone)?;
    Some(transform_coord(world, transform_coord(&m, local)))
}
