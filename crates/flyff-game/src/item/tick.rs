//! The per-tick job of the item system (Plan/19 §5.2; the player process of `CUser::Process`): fires the use of
//! an item whose cast time is over, runs the keep-time pass, removes the expired instant banks, forwards the
//! premium-mode changes, and gives the items that were created without a serial (the pickup in the
//! world phase has no serial source) their serial before the snapshot pass of the same tick.

use super::ops::CtxSerials;
use super::usage::{cast, run_use};
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::ObjId;

/// The job body (registered by `Game::new`, `PerTick` group, every tick).
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let avatars: Vec<ObjId> = ctx.sessions.iter().map(|e| e.player).collect();
    for id in avatars {
        let who = PlayerRef(id);
        assign_serials(svc, ctx, who);
        fire_due_cast(svc, ctx, who);
    }
    // the keep-time pass of `CheckFiniteItem` (one second per player, phase = the avatar id; ITEM-08)
    super::expiry::process(svc, ctx);
    // the ticket pass (`CUser::Process`, every 16 ticks per player; ITEM-24): after the keep-time pass, so an item flagged
    // expired in this tick is seen by it
    super::ticket::tick(svc, ctx);
    // `ProcessPet`: the summoned system pets' bonus, energy and life (every 16 ticks per player; ITEM-16)
    super::pets::system::tick(svc, ctx);
    // the eat pets: the pet left behind, the re-summon, the expired charms of a vis pet (every 16 ticks; ITEM-18)
    super::pets::eat::tick(svc, ctx);
    // the instant-bank NPCs whose 180 s are up (`ProcInstantBanker`; ITEM-09)
    for id in svc.items.take_due_instant_banks(ctx.clock.game_ms) {
        ctx.worlds.delete(id);
    }
    // the pocket expiry pass of the non-permanent mode (`ProcessExpiring`, every 16 ticks; ITEM-15)
    super::pockets::process_expiring_all(svc, ctx);
    // collecting: the interruption check every tick, the battery and the item every 16 ticks (ITEM-19)
    super::collect::tick(svc, ctx);
    // the feed pocket "in use" mark follows its buff (ITEM-20)
    super::buffitems::tick(svc, ctx);
    // the premium-mode changes the 1 s timer of the tick tail queued (party skill modes; MOVER-16)
    crate::stats::sm::drain_events(svc, ctx);
}

/// Gives every inventory item with serial 0 a fresh serial (they only exist for the part of a tick between the
/// world phase and this job).
fn assign_serials(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let pending = ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player())
        .is_some_and(|p| p.items.serial_pending);
    if !pending {
        return;
    }
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    let Some(p) = m.player_mut() else { return };
    let mut serials = CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
    let mut all_done = true;
    for (_, it) in p.items.inventory.iter_mut() {
        if it.serial.0 == 0 {
            match super::container::SerialSource::next_serial(&mut serials) {
                Ok(s) => it.serial = flyff_common::ItemSerial(s),
                Err(e) => {
                    tracing::warn!(error = %e, "items: no serial for a pending item");
                    all_done = false;
                    break;
                }
            }
        }
    }
    if all_done {
        p.items.serial_pending = false;
    }
}

/// Re-issues the use of an item whose cast is over (`CUser::Process`); a vanished item cancels the cast with
/// `TID_PK_BLINK_LIMIT`. A virtual use (the escape wing) is re-issued without an item.
fn fire_due_cast(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let now = ctx.clock.game_ms;
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    let Some(due) = cast::take_due(m, now) else {
        return;
    };
    let id = match due {
        cast::Due::Virtual(prop_id) => {
            super::usage::blink::run_virtual(svc, ctx, who, prop_id);
            return;
        }
        cast::Due::Item(id) => id,
    };
    let exists = m
        .player()
        .is_some_and(|p| p.items.inventory.get_at_id(id).is_some());
    if exists {
        run_use(svc, ctx, who, id, -1);
    } else {
        let mut fx = super::fx::Fx::new();
        if let Some(m) = ctx.worlds.player_mut(who.0) {
            cast::fail_due(m, &mut fx);
        }
        fx.flush(svc, ctx, who);
    }
}
