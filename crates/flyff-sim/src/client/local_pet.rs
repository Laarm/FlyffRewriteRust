//! The local system-pet controller: `CAIEgg::MoveProcessIdle` (`src:_Common/pet.cpp:497-570`, Plan/40 §7.9, Plan/18 §13;
//! WP-CLIENT-31).
//!
//! The server has no mover for a summoned system pet (an egg or a hatched pet is only an item): every client creates a
//! LOCAL mover for each player with an active system pet (`ADD_MOVER` with a local id, `SET_PARAMS { ai: AII_EGG, is_pet,
//! pet_owner }`, `game/pets/eggPet.ts`) and this module drives it. It runs inside [`ClientSim::step`](super::ClientSim::step),
//! once per frame before any mover steps, so the TypeScript side contains no movement code (rule B1) and needs no extra
//! boundary call.
//!
//! The rule, per egg (a mover with `params.ai == AII_EGG` and a `pet_owner` that is a world mover of this sim):
//!
//! * dead or in the knock-back flight (`OBJSTA_DMG_FLY_ALL`): nothing;
//! * the two candidate points are one metre to the left and to the right of the owner (the owner's facing turned by -90 and
//!   +90 degrees, `AngleToVector(yaw, -pitch, 1)`), on the terrain height under the owner (`y` of the owner's position is
//!   replaced by `GetLandHeight`);
//! * the candidate that is nearer to the pet (squared 3-D distance, `d1 < d2` else the right one) is the target;
//! * only when the squared XZ distance to the target exceeds `1` does the pet receive a new destination
//!   (`SetDestPos(target, forward = true)`, no network message); in between it keeps walking to the last one.
//!
//! The speed is the owner's (`CMover::GetSpeed`, client branch for `AII_EGG`: [`crate::kin::get_speed`]); the original reads
//! it from the owner unconditionally, `features.improved_pets` is not involved (it only lets a pet ignore scenery, which the
//! ground pass does). All inputs are the start-of-frame state of the owner, so the result does not depend on the insertion
//! order of the movers.

use super::ClientMover;
use crate::collision::world::CollisionWorld;
use crate::mover::NO_OBJ;
use crate::num::{Vec3, angle_to_vector, length_sq};
use crate::types::defs;
use flyff_common::ObjId;

/// The squared XZ distance to the chosen side point above which the pet gets a new destination (`fDistSq > 1.0f`).
pub const RETARGET_DIST_SQ: f32 = 1.0;
/// The distance of the two candidate points from the owner (`AngleToVector( ..., 1.0f )`).
pub const SIDE_OFFSET: f32 = 1.0;

/// `true` for a local system pet that follows `owner`.
#[inline]
fn is_egg(m: &ClientMover) -> bool {
    m.sim.params.ai == defs::AII_EGG && m.sim.params.pet_owner != NO_OBJ
}

/// The point a pet at `pet` follows: the nearer of the two side points of an owner at `owner` facing `yaw`/`pitch` (degrees),
/// `land_y` = the terrain height under the owner. `CAIEgg::MoveProcessIdle`.
pub fn follow_point(pet: Vec3, owner: Vec3, yaw: f32, pitch: f32, land_y: f32) -> Vec3 {
    let base = Vec3::new(owner.x, land_y, owner.z);
    let mut left_yaw = yaw - 90.0;
    if left_yaw < 0.0 {
        left_yaw += 360.0;
    }
    let mut right_yaw = yaw + 90.0;
    if right_yaw > 360.0 {
        right_yaw -= 360.0;
    }
    let p1 = angle_to_vector(left_yaw, -pitch, SIDE_OFFSET) + base;
    let p2 = angle_to_vector(right_yaw, -pitch, SIDE_OFFSET) + base;
    let d1 = f64::from(length_sq(p1 - pet));
    let d2 = f64::from(length_sq(p2 - pet));
    if d1 < d2 { p1 } else { p2 }
}

/// Does the pet at `pet` need a new destination for the target point `to` (`fDistSq > 1` on the XZ plane)?
pub fn needs_destination(pet: Vec3, to: Vec3) -> bool {
    let d = Vec3::new(to.x - pet.x, 0.0, to.z - pet.z);
    length_sq(d) > RETARGET_DIST_SQ
}

/// One pass over the world movers: every local pet looks at its owner and, when it has fallen behind, walks to the nearer
/// side point. Returns how many pets received a destination (tests, statistics).
pub(crate) fn drive(movers: &mut [ClientMover], world: &CollisionWorld) -> u32 {
    let mut retargeted = 0;
    for i in 0..movers.len() {
        let Some(owner_id) = movers
            .get(i)
            .filter(|m| is_egg(m))
            .map(|m| m.sim.params.pet_owner)
        else {
            continue;
        };
        let Some((owner_pos, yaw, pitch)) = owner_pose(movers, owner_id) else {
            continue;
        };
        let Some(m) = movers.get_mut(i) else { continue };
        let st = m.sim.act.state;
        if st.is_die() || st.dmg_fly_all() {
            continue;
        }
        let land_y = world.land_height(owner_pos.x, owner_pos.z);
        let to = follow_point(m.sim.pos, owner_pos, yaw, pitch, land_y);
        if needs_destination(m.sim.pos, to) {
            m.sim.set_dest_pos(to, true);
            retargeted += 1;
        }
    }
    retargeted
}

/// Position, yaw and pitch of the owner (`None` when it is not a world mover: the pet stands where it is).
fn owner_pose(movers: &[ClientMover], owner: ObjId) -> Option<(Vec3, f32, f32)> {
    movers
        .iter()
        .find(|m| m.sim.id == owner)
        .map(|m| (m.sim.pos, m.sim.angle, m.sim.angle_x))
}
