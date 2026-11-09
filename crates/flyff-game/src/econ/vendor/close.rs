//! Closing a shop and leaving one (Plan/19 §9.2 `PVendorCloseReq`, "also triggered by logout/disconnect, death and
//! teleport"; src:WORLDSERVER/DPSrvr.cpp:9034-9110 `ClosePVendor`).
//!
//! * [`close_vendor`]: the shop of a player ends. Reservations and prices are released, the title and the slots go,
//!   the visitors lose their link, the near set gets `PVendorCloseNtf{closed: true}` and the room hook runs
//!   ([`VendorRoomHooks::on_vendor_close`], exactly once per shop). Idempotent: closing a shop that is not open and
//!   has no goods does nothing, so the request, the logout step and the per-tick pass can all call it.
//! * [`leave_browse`]: a visitor closes the window or is taken away from it.
//! * [`process`]: the per-tick pass for the paths no handler sees (death, teleport, disconnect, a vanished avatar).
//!
//! [`VendorRoomHooks::on_vendor_close`]: super::VendorRoomHooks::on_vendor_close

use super::{live_player, sync_vending};
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::TradePartner;
use crate::econ::trade::send;
use crate::outbox::Audience;
use flyff_common::ObjId;
use flyff_proto::pb;

/// Ends the shop of `vendor`. Returns `false` when there was nothing to close.
pub fn close_vendor(svc: &mut Services, ctx: &mut GameCtx<'_>, vendor: ObjId) -> bool {
    let registered = svc.econ.vendor.active.remove(&vendor);
    let mut was_vendor = false;
    if let Some(p) = ctx.worlds.player_mut(vendor).and_then(|m| m.player_mut()) {
        was_vendor = p.econ.vendor.is_open() || p.econ.vendor.is_vendor();
        let state = std::mem::take(&mut p.econ.vendor);
        for (_, good) in state.goods() {
            if let Some(it) = p.items.inventory.get_at_id_mut(good.obj_id)
                && it.serial == good.serial
            {
                it.reserved = 0;
                it.vendor_price = 0;
            }
        }
        p.econ.interrupted = false;
        sync_vending(p);
    }
    if !registered && !was_vendor {
        return false;
    }
    svc.econ.vendor.stats.closed += 1;
    // the visitors lose their link; the ones the near broadcast does not reach are told one by one
    let browsers = svc.econ.vendor.browsers.remove(&vendor).unwrap_or_default();
    let near: Vec<ObjId> = ctx
        .worlds
        .mover(vendor)
        .map(|m| m.core.viewers.iter().copied().collect())
        .unwrap_or_default();
    for b in browsers {
        if let Some(p) = ctx.worlds.player_mut(b).and_then(|m| m.player_mut())
            && p.econ.partner == (TradePartner::VendorBrowse { vendor })
        {
            p.econ.partner = TradePartner::None;
            p.econ.interrupted = false;
        }
        if !near.contains(&b) {
            send(ctx, b, vendor, pb::PVendorCloseNtf { closed: false });
        }
    }
    if ctx.worlds.mover(vendor).is_some() {
        ctx.broadcast(
            Some(&*svc),
            vendor,
            Audience::NearInclSelf,
            pb::PVendorCloseNtf { closed: true },
        );
    }
    svc.econ
        .vendor
        .rooms
        .on_vendor_close(ctx, PlayerRef(vendor));
    true
}

/// A visitor `who` leaves the vendor window (`ClosePVendor` of a non-owner, also a teleport, a death or a logout of
/// the visitor): the link is cleared, the visitor gets `PVendorCloseNtf{closed: false}` (header object = `header`,
/// default the vendor it visited) and leaves the room. Returns `false` when the player visited nobody (nothing is
/// sent; the room is still left, a member may have no link). Only a **visit** is ended, never another partner.
pub fn leave_browse(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: ObjId,
    header: Option<ObjId>,
) -> bool {
    let visited = ctx
        .worlds
        .player(who)
        .and_then(|m| m.player())
        .and_then(|p| match p.econ.partner {
            TradePartner::VendorBrowse { vendor } => Some(vendor),
            _ => None,
        });
    let Some(vendor) = visited else {
        // no visit; a stale `interrupted` flag has no owner any more
        if let Some(p) = ctx.worlds.player_mut(who).and_then(|m| m.player_mut())
            && !p.econ.vending
            && !matches!(p.econ.partner, TradePartner::Player { .. })
        {
            p.econ.interrupted = false;
        }
        return false;
    };
    if let Some(p) = ctx.worlds.player_mut(who).and_then(|m| m.player_mut()) {
        p.econ.partner = TradePartner::None;
        p.econ.interrupted = false;
    }
    if let Some(set) = svc.econ.vendor.browsers.get_mut(&vendor) {
        set.remove(&who);
        if set.is_empty() {
            svc.econ.vendor.browsers.remove(&vendor);
        }
    }
    send(
        ctx,
        who,
        header.unwrap_or(vendor),
        pb::PVendorCloseNtf { closed: false },
    );
    svc.econ.vendor.rooms.on_member_leave(ctx, PlayerRef(who));
    true
}

/// Why a shop must close now, if it must.
fn must_close(ctx: &GameCtx<'_>, vendor: ObjId) -> bool {
    let Some(m) = live_player(ctx, vendor) else {
        return true;
    };
    let Some(p) = m.player() else { return true };
    ctx.session_of(PlayerRef(vendor)).is_none() || is_die(m) || p.econ.interrupted
}

/// The per-tick pass (after the worlds): shops whose owner died, teleported, lost the socket or vanished close;
/// visitors who teleported, died, lost the socket, vanished or whose shop is gone leave; the room hook drops
/// members that vanished.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let vendors: Vec<ObjId> = svc.econ.vendor.active().collect();
    for v in vendors {
        if must_close(ctx, v) {
            close_vendor(svc, ctx, v);
        }
    }
    let visits: Vec<(ObjId, ObjId)> = svc
        .econ
        .vendor
        .browsers
        .iter()
        .flat_map(|(v, set)| set.iter().map(move |b| (*v, *b)))
        .collect();
    for (vendor, b) in visits {
        let leave = match live_player(ctx, b) {
            None => true,
            Some(m) => {
                m.player().is_none_or(|p| {
                    p.econ.interrupted || p.econ.partner != (TradePartner::VendorBrowse { vendor })
                }) || ctx.session_of(PlayerRef(b)).is_none()
                    || is_die(m)
                    || !svc.econ.vendor.is_active(vendor)
            }
        };
        if leave && !(live_player(ctx, b).is_some() && leave_browse(svc, ctx, b, None)) {
            // the avatar is gone or has no link any more: only the book-keeping and the room are left
            if let Some(set) = svc.econ.vendor.browsers.get_mut(&vendor) {
                set.remove(&b);
                if set.is_empty() {
                    svc.econ.vendor.browsers.remove(&vendor);
                }
            }
            svc.econ.vendor.rooms.on_member_leave(ctx, PlayerRef(b));
        }
    }
    svc.econ.vendor.rooms.on_tick(ctx);
}
