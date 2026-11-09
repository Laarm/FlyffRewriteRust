//! The per-tick buff pass (Plan/17 §7.3; `CMover::Process` -> `m_buffs.Process()`): every awake mover with
//! buffs is processed once per 16 ticks (about 1.07 s), its phase being the mover's id. Unobserved NPCs are
//! asleep and are not processed (KEEP-QUIRK of the sleep rule): a debuff on a monster nobody sees waits.
//!
//! The pass needs the services (the expiry of `SI_GEN_BURN` hurts, `BUFF_TOGIFT` creates an item), so it
//! runs in the timers phase next to the other per-tick jobs instead of inside the world phase.

use super::api::with_mover;
use super::core::process;
use super::status;
use crate::ctx::{GameCtx, Services};
use crate::mover::Mover;
use flyff_common::ObjId;

/// The processing period in ticks (`(m_nCount & 15) == 0`).
pub const PERIOD: u64 = 16;

/// The sleep rule: players, and movers somebody sees.
fn is_awake(m: &Mover) -> bool {
    m.is_player() || !m.core.viewers.is_empty()
}

/// The movers to process in tick `tick`.
pub fn due(ctx: &GameCtx<'_>) -> Vec<ObjId> {
    let tick = ctx.clock.tick.0;
    ctx.worlds
        .store()
        .movers
        .iter()
        .filter(|(_, m)| {
            !m.buffs.is_empty()
                && !m.core.deleted
                && is_awake(m)
                && (tick + u64::from(m.id().0)).is_multiple_of(PERIOD)
        })
        .map(|(_, m)| m.id())
        .collect()
}

/// The pass (called by `Game::phase_timers`).
pub fn process_all(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    for id in due(ctx) {
        with_mover(svc, ctx, id, process);
    }
}

/// `CMover::ProcessAbnormalState` of every awake mover that has a state to count down (WP-SKILL-07, every tick):
/// stun, poison, bleeding, darkness, the dark cover and the heal over time (called by `Game::phase_timers`).
/// The forced hits of the poison and the bleeding run as follow-ups, so a death they cause is handled at once.
pub fn process_abnormal_all(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let ids: Vec<ObjId> = ctx
        .worlds
        .store()
        .movers
        .values()
        .filter(|m| !m.core.deleted && is_awake(m) && status::needs_pass(m))
        .map(Mover::id)
        .collect();
    for id in ids {
        with_mover(svc, ctx, id, status::process_abnormal_state);
    }
}
