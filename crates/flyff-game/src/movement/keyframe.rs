//! `MoverKeyframe`: the drift bound of moving NPCs (Plan/18 §5.3, PL-008; WP-AI-04).
//!
//! At the first frame `4n` of a tick, a processed non-player mover with viewers that is **moving** gets a
//! keyframe `{server_frame: 4n, pos, angle, state}` when `tick % K == obj_id % K` (`K = max(1, round(15 /
//! net.npc_keyframe_hz))`, so a continuously moving NPC is keyed once per second at its own phase), plus a
//! **settle keyframe** the first tick it stops moving, whatever the phase. With `debug.dev_surfaces` (the
//! `--dev` ghost overlay of `/dev/sim`) a debug build sends one keyframe per tick for every moving NPC with
//! viewers; a release build never does, whatever the configuration.

use super::{IntentAudience, MoveShared, MoveStats, intents};
use crate::mover::Mover;
use crate::world::visibility::SpawnSink;
use flyff_common::{SimFrame, Tick};
use flyff_proto::{EncodedMsg, pb};
use flyff_sim::SimMover;
use flyff_sim::types::{OBJSTA_DMG_FLY_ALL, OBJSTA_STAND};

/// The "moving" predicate at the pre-step point of frame `4n` (18 §5.3): a destination (position or object)
/// is set, a move pattern runs, the move group is neither `NONE` nor `STAND`, a jump is in progress, the
/// mover is in a knock-back flight (`DMG_FLY_ALL` bit test) or an external force acts on it.
pub fn is_moving(sim: &SimMover) -> bool {
    let st = sim.act.state;
    !sim.is_empty_dest()
        || sim.pattern.pattern != 0
        || (st.move_() != 0 && st.move_() != OBJSTA_STAND)
        || st.jump() != 0
        || st.bits() & OBJSTA_DMG_FLY_ALL != 0
        || sim.act.delta_e != flyff_common::math::Vec3::ZERO
}

/// What the keyframe step decided (for the counters and tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyframeKind {
    /// `tick % K == obj_id % K` while moving.
    Periodic,
    /// The first tick after the mover stopped moving.
    Settle,
    /// The `--dev` ghost keyframe of every tick (debug builds only).
    Dev,
}

/// The keyframe step of one processed mover at the pre-step point of frame `4n` (18 §3.3 step 6b): updates the
/// settle detector and sends the keyframe when due. Players are never keyed.
pub(crate) fn maybe_emit(
    m: &mut Mover,
    tick: Tick,
    frame: SimFrame,
    shared: &MoveShared,
    sink: &mut dyn SpawnSink,
    stats: &mut MoveStats,
) -> Option<KeyframeKind> {
    if m.is_player() {
        return None;
    }
    let moving = is_moving(&m.sim);
    let was = std::mem::replace(&mut m.sync.kf_was_moving, moving);
    if m.core.viewers.is_empty() {
        return None;
    }
    let k = shared.keyframe_period;
    let kind = if cfg!(debug_assertions) && shared.dev_keyframes && moving {
        Some(KeyframeKind::Dev)
    } else if k == 0 {
        None
    } else if moving && tick.0 % k == u64::from(m.core.id.0) % k {
        Some(KeyframeKind::Periodic)
    } else if was && !moving {
        Some(KeyframeKind::Settle)
    } else {
        None
    }?;
    let enc = EncodedMsg::new(
        m.core.id.0,
        pb::MoverKeyframe {
            server_frame: frame.0,
            pos: Some(intents::pb_vec(m.sim.pos)),
            angle: m.sim.angle,
            state: m.sim.act.state.bits(),
        },
    );
    intents::deliver(sink, m, IntentAudience::Viewers, &enc);
    let slot = match kind {
        KeyframeKind::Periodic => 0,
        KeyframeKind::Settle => 1,
        KeyframeKind::Dev => 2,
    };
    if let Some(n) = stats.keyframes.get_mut(slot) {
        *n += 1;
    }
    Some(kind)
}
