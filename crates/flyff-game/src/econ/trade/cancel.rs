//! Ending a trade (Plan/19 §8.2 `TradeCancelReq`, §8.4 cancellation triggers, VR-017).
//!
//! A session ends by [`cancel_session`]: the reservations on both sides are released, the partner links are
//! cleared and both players get `TradeCancelNtf`. The penya never left the wallets (E5), so there is nothing to
//! refund. Who ends it decides the message:
//!
//! * [`Cancel::By`]: a player cancelled (`TradeCancelReq`) or a trigger ended the trade on its behalf (death,
//!   teleport, logout, ...): header object = that player, `id_player` = its character, `mode` as sent;
//! * [`Cancel::Error`]: a commit that failed validation: header `NULL_ID`, `id_player` 0, mode 0 (the legacy
//!   error form).
//!
//! [`process`] is the per-tick pass that finds the triggers of §8.4 that no handler sees: it runs in the tick's
//! timers phase after the worlds, so a pickup, a death or a teleport of the world phase ends the trade in the same
//! tick, before the next input can touch it. The handlers re-check the same facts on every call, so a trade that
//! is about to be cancelled never advances.

use super::{TradeId, TradeSession, TradeSide, send};
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::TradePartner;
use flyff_common::{CharacterId, ObjId};
use flyff_proto::pb;

/// How a trade ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancel {
    /// A participant cancelled, or a trigger ended the trade on its behalf.
    By {
        /// The affected player.
        player: ObjId,
        /// `nMode` of `TradeCancelReq` (0 for a trigger).
        mode: i32,
    },
    /// A failed commit: nobody is named.
    Error,
}

/// Releases what `side` held on the avatar `obj`: the reservations of its offers and the partner link.
fn release(ctx: &mut GameCtx<'_>, obj: ObjId, side: &TradeSide) {
    let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) else {
        return;
    };
    for (_, o) in side.offers() {
        if let Some(it) = p.items.inventory.get_at_id_mut(o.obj_id)
            && it.serial == o.serial
            && it.item_id == o.item_id
        {
            it.reserved = 0;
        }
    }
    if matches!(p.econ.partner, TradePartner::Player { .. }) {
        p.econ.partner = TradePartner::None;
    }
    p.econ.interrupted = false;
}

/// Ends the session `id` (a no-op for an unknown id): reservations released, links cleared, `TradeCancelNtf` to
/// both players.
pub fn cancel_session(svc: &mut Services, ctx: &mut GameCtx<'_>, id: TradeId, how: Cancel) {
    let Some(s) = svc.econ.trade.remove(id) else {
        return;
    };
    svc.econ.trade.stats.cancelled += 1;
    if how == Cancel::Error {
        svc.econ.trade.stats.errors += 1;
    }
    for i in 0..2 {
        release(ctx, s.obj(i), &s.side[i]);
    }
    let (header, id_player, mode) = match how {
        Cancel::By { player, mode } => (player, character_of(&s, player), mode),
        Cancel::Error => (ObjId::NULL, CharacterId(0), 0),
    };
    for to in [s.a, s.b] {
        send(
            ctx,
            to,
            header,
            pb::TradeCancelNtf {
                id_player: id_player.0,
                mode,
            },
        );
    }
}

/// The character of a participant (kept in the session, so an avatar that is already gone still has one).
fn character_of(s: &TradeSession, who: ObjId) -> CharacterId {
    s.index_of(who).map_or(CharacterId(0), |i| s.chars[i])
}

/// Ends the trade `player` is in, if any, as a cancel of that player (`mode` as sent).
pub fn cancel_for_player(svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId, mode: i32) {
    if let Some(id) = svc.econ.trade.session_id_of(player) {
        cancel_session(svc, ctx, id, Cancel::By { player, mode });
    }
}

/// The participant of `s` that breaks one of the §8.4 rules now, if any (the first, requester before accepter).
fn violation(ctx: &GameCtx<'_>, s: &TradeSession) -> Option<ObjId> {
    for i in 0..2 {
        let obj = s.obj(i);
        let other = s.obj(1 - i);
        let Some(m) = ctx.worlds.player(obj) else {
            return Some(obj);
        };
        let Some(p) = m.player() else {
            return Some(obj);
        };
        // logout, disconnect and kick leave the avatar without a socket (or gone)
        if ctx.session_of(PlayerRef(obj)).is_none() || m.core.deleted {
            return Some(obj);
        }
        if is_die(m) || p.econ.interrupted {
            return Some(obj);
        }
        if m.core.world != s.world || m.core.layer != s.layer {
            return Some(obj);
        }
        if p.econ.partner != (TradePartner::Player { other }) {
            return Some(obj);
        }
        if p.econ.bank_open || p.social.guild_bank_open || p.econ.vending {
            return Some(obj);
        }
        if p.items.content_rev() != s.revs[i] {
            return Some(obj);
        }
    }
    None
}

/// The per-tick pass: expired or orphaned requests are dropped, every running trade is checked against the
/// triggers of §8.4 and ended when one holds.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    {
        let ctx_ref = &*ctx;
        svc.econ.trade.retain_requests(now, |r| {
            [r.from, r.to].into_iter().all(|o| {
                ctx_ref.worlds.player(o).is_some() && ctx_ref.session_of(PlayerRef(o)).is_some()
            })
        });
    }
    for id in svc.econ.trade.ids() {
        let Some(s) = svc.econ.trade.session(id) else {
            continue;
        };
        if let Some(player) = violation(ctx, s) {
            cancel_session(svc, ctx, id, Cancel::By { player, mode: 0 });
        }
    }
}
