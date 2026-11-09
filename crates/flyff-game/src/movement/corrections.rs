//! Collision and drop corrections of NPCs (Plan/18 §5.5; the sim events `Collided` and `Dropped`).
//!
//! The original server asked a player's client for its true position after a collision; that resync family is
//! DROPPED (PL-027, 18 §4.6). An NPC, on the other hand, is simulated by the server alone, so when it bumps
//! into something on its way to an AI-issued destination its clients must be told where it really stopped:
//! the destination is cleared, the NPC stands, and a `MoverStateNtf{CORR}` is stamped at the next pre-step
//! point. Chasing (factor 2.0) and returning (2.66) monsters are never corrected: they slide along walls
//! (KEEP-QUIRK, KQ-13 family). During a knock-back flight (`DMG_FLY_ALL`) no correction is sent either
//! (`User.cpp:5012`).

use super::{IntentAudience, PendingIntent, intents};
use crate::ai::AiCx;
use crate::mover::Mover;
use flyff_sim::ActMsg;
use flyff_sim::types::{OBJMSG_STAND, OBJSTA_DMG_FLY_ALL};

/// Speed factors at or above this are never corrected (`< 1.9`, 18 §5.5).
pub const CORRECTION_MAX_FACTOR: f32 = 1.9;

fn correctable(m: &Mover) -> bool {
    m.sync.corr_armed && m.sim.params.speed_factor < CORRECTION_MAX_FACTOR && !m.sim.params.is_pet
}

fn in_knock_back(m: &Mover) -> bool {
    m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0
}

/// `OnActCollision` for an NPC (`MoverActEvent.cpp:2027-2056`): returns `true` when the AI must hear
/// `AIMSG_COLLISION` (only while the correction was armed; the one `Collision` from this source). Players are
/// never corrected (PL-027).
pub fn on_collided(m: &mut Mover, cx: &mut AiCx<'_>) -> bool {
    if m.is_player() || !correctable(m) {
        return false;
    }
    m.sim.clear_dest();
    cx.send_act(m, ActMsg::new(OBJMSG_STAND));
    if !in_knock_back(m) {
        intents::enqueue(m, PendingIntent::Corr, IntentAudience::Viewers);
    }
    m.sync.corr_armed = false;
    true
}

/// `OnActDrop` for an NPC (`MoverActEvent.cpp:1955-1983`): the destination position is cleared (the
/// destination object is kept), the NPC stands and a correction is queued; no AI message.
pub fn on_dropped(m: &mut Mover, cx: &mut AiCx<'_>) {
    if m.is_player() || in_knock_back(m) || !correctable(m) {
        return;
    }
    m.sim.clear_dest_pos();
    cx.send_act(m, ActMsg::new(OBJMSG_STAND));
    intents::enqueue(m, PendingIntent::Corr, IntentAudience::Viewers);
    m.sync.corr_armed = false;
}
