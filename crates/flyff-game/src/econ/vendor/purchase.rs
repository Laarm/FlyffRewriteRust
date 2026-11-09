//! `BuyPVendorItem` (Plan/19 §9.4, VR-018, VR-019, PL-064; ref/07c §5.6; src:_Common/MoverItem.cpp:637-704): one
//! atomic purchase between a buyer and a vendor. Like the trade commit it validates without mutating, simulates
//! on clones of the two inventories and then installs them in one block with one
//! `AtomicBatch{VendorPurchase, [snap(buyer), snap(seller)]}`.
//!
//! Rules, in the original's order: the slot is `< 30` and `num > 0`; both players are online in the same world;
//! the shop is open; the slot's good still exists (same storage entry, serial and prop; `reserved > 0`) and its
//! prop id equals the request's (the anti-race check); `num` is clamped to what is for sale; the buyer can pay
//! (`TID_GAME_LACKMONEY`); **FIX (PL-064, VR-019):** the seller's wallet must hold the payment
//! (`TID_GAME_VENDOR_MAX_ALL_GOLD`, nothing is transferred; the original clamped and destroyed the excess); the
//! buyer must have room (`TID_GAME_LACKSPACE`). A whole stack keeps its serial, a part gets a fresh one (PL-050,
//! the original copied the serial). There is no tax on vendor sales. A buyer that is in a player trade is turned
//! away (**FIX**: the trade's penya reservation must not interleave with a purchase).

use super::{MAX_VENDITEM, live_player, sync_vending};
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::TradePartner;
use crate::econ::trade::moves::{
    Arrived, MoveError, PoolSerials, Taken, create_msg, put_stack, serials_needed, take_stack,
    update_msg,
};
use crate::econ::trade::snap::{atomic_snapshot, can_snapshot};
use crate::item::container::{ItemContainer, SerialSource, StorageId};
use crate::item::fx::Fx;
use crate::item::gold::GOLD_MAX;
use crate::item::ops::CtxSerials;
use crate::outbox::Audience;
use flyff_common::defs::text::{
    TID_GAME_LACKMONEY, TID_GAME_LACKSPACE, TID_GAME_VENDOR_MAX_ALL_GOLD,
};
use flyff_common::{CharacterId, ObjId};
use flyff_db::AtomicBatch;
use flyff_db::audit::econ::vendor_sale;
use flyff_proto::pb;

/// What a purchase decided before anything changed.
enum Verdict {
    /// Ignored without a message.
    Ignore,
    /// Refused with a text for the buyer.
    Text(u32),
    /// Go.
    Buy(Box<Facts>),
}

/// The state the commit starts from.
struct Facts {
    slot: usize,
    obj_id: StorageId,
    /// The count for sale before the purchase.
    reserved: u16,
    num: u16,
    total: i64,
    inv: [ItemContainer; 2],
    gold: [i32; 2],
}

fn judge(
    svc: &Services,
    ctx: &GameCtx<'_>,
    buyer: ObjId,
    vendor: ObjId,
    msg: &pb::BuyPVendorItem,
) -> Verdict {
    let slot = msg.slot as usize;
    let core = svc.items.handle();
    let (Some(bm), Some(sm)) = (live_player(ctx, buyer), live_player(ctx, vendor)) else {
        return Verdict::Ignore;
    };
    let (Some(bp), Some(sp)) = (bm.player(), sm.player()) else {
        return Verdict::Ignore;
    };
    if bm.core.world != sm.core.world
        || bm.core.layer != sm.core.layer
        || is_die(bm)
        || matches!(bp.econ.partner, TradePartner::Player { .. })
        || !sp.econ.vendor.is_open()
        || !can_snapshot(ctx, buyer)
        || !can_snapshot(ctx, vendor)
    {
        return Verdict::Ignore;
    }
    let Some(good) = sp.econ.vendor.slots.get(slot).and_then(Option::as_ref) else {
        return Verdict::Ignore;
    };
    let Some(it) = sp
        .items
        .inventory
        .get_at_id(good.obj_id)
        .filter(|it| it.serial == good.serial && it.item_id == good.item_id && it.reserved > 0)
    else {
        return Verdict::Ignore;
    };
    if it.item_id != msg.item_id {
        return Verdict::Ignore;
    }
    let reserved = it.reserved.min(it.count);
    let num = u16::try_from(msg.num.min(i32::from(reserved)).max(1)).unwrap_or(1);
    let price = good.price;
    let total = i64::from(price) * i64::from(num);
    if price > 0 && i64::from(bp.items.gold) < total {
        return Verdict::Text(TID_GAME_LACKMONEY);
    }
    if i64::from(sp.items.gold) + total > GOLD_MAX {
        return Verdict::Text(TID_GAME_VENDOR_MAX_ALL_GOLD);
    }
    // the fit, simulated on a clone (nothing changes when it does not fit)
    let mut probe = bp.items.inventory.clone();
    let mut seller_probe = sp.items.inventory.clone();
    let mut serials = PoolSerials::new(vec![u64::MAX]);
    let Ok(taken) = take_stack(
        &mut seller_probe,
        good.obj_id,
        num,
        reserved - num,
        &mut serials,
    ) else {
        return Verdict::Ignore;
    };
    if put_stack(&mut probe, &taken, &core.data, &mut serials).is_err() {
        return Verdict::Text(TID_GAME_LACKSPACE);
    }
    Verdict::Buy(Box::new(Facts {
        slot,
        obj_id: good.obj_id,
        reserved,
        num,
        total,
        inv: [bp.items.inventory.clone(), sp.items.inventory.clone()],
        gold: [bp.items.gold, sp.items.gold],
    }))
}

/// `OnBuyPVendorItem` (see the module documentation).
pub fn on_buy(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::BuyPVendorItem) {
    let slot = msg.slot as usize;
    let vendor = ObjId(msg.vendor);
    if slot >= MAX_VENDITEM || msg.num < 1 || vendor == who.0 {
        return;
    }
    let buyer = who.0;
    match judge(svc, ctx, buyer, vendor, &msg) {
        Verdict::Ignore => {}
        Verdict::Text(tid) => {
            crate::chat::ingress::notice::defined(ctx, who, tid, Vec::new());
        }
        Verdict::Buy(f) => match simulate(svc, ctx, *f) {
            Ok(plan) => commit(svc, ctx, buyer, vendor, plan),
            Err(e) => {
                tracing::error!(error = %e, "vendor purchase: the validated exchange failed");
            }
        },
    }
}

/// The simulated purchase: both inventories after it.
struct Plan {
    slot: usize,
    reserved: u16,
    num: u16,
    total: i64,
    gold: [i32; 2],
    inv: [ItemContainer; 2],
    taken: Taken,
    arrived: Arrived,
}

fn simulate(svc: &mut Services, ctx: &mut GameCtx<'_>, f: Facts) -> Result<Plan, String> {
    let Facts {
        slot,
        obj_id,
        reserved,
        num,
        total,
        mut inv,
        gold,
    } = f;
    // a part of a stack needs one fresh serial; allocate it before anything changes
    let need = serials_needed(&inv[1], &[(obj_id, num)]);
    let mut fresh = Vec::with_capacity(need);
    {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        for _ in 0..need {
            fresh.push(src.next_serial().map_err(|e| e.to_string())?);
        }
    }
    let core = svc.items.handle();
    let mut pool = PoolSerials::new(fresh);
    let map = |e: MoveError| e.to_string();
    let [buyer_inv, seller_inv] = &mut inv;
    let taken = take_stack(seller_inv, obj_id, num, reserved - num, &mut pool).map_err(map)?;
    let arrived = put_stack(buyer_inv, &taken, &core.data, &mut pool).map_err(map)?;
    Ok(Plan {
        slot,
        reserved,
        num,
        total,
        gold,
        inv,
        taken,
        arrived,
    })
}

fn commit(svc: &mut Services, ctx: &mut GameCtx<'_>, buyer: ObjId, vendor: ObjId, plan: Plan) {
    let Plan {
        slot,
        reserved,
        num,
        total,
        gold,
        inv,
        taken,
        arrived,
    } = plan;
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let [buyer_inv, seller_inv] = inv;
    let remaining = reserved - num;
    let (buyer_gold, seller_gold) = (
        (i64::from(gold[0]) - total) as i32,
        (i64::from(gold[1]) + total) as i32,
    );
    // names and characters for the audit and the sale line
    let who = |o: ObjId| {
        ctx.worlds
            .player(o)
            .and_then(|m| m.player())
            .map(|p| (p.ident.character_id, p.ident.name.clone()))
            .unwrap_or((CharacterId(0), String::new()))
    };
    let ((buyer_chr, buyer_name), (seller_chr, _)) = (who(buyer), who(vendor));
    // the seller: the inventory, the wallet, the good
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(vendor) {
        if let Some(p) = m.player_mut() {
            p.items.inventory = seller_inv;
            p.items.gold = seller_gold;
            if remaining == 0 {
                p.econ.vendor.slots[slot] = None;
            } else if let Some(good) = p.econ.vendor.slots[slot].as_mut() {
                good.count = remaining;
            }
            sync_vending(p);
        }
        fx.out.to_self(update_msg(&taken));
        crate::stats::pool::emit_gold(m, env.stats(), &mut fx.out);
    }
    fx.flush(svc, ctx, PlayerRef(vendor));
    // the buyer: the inventory, the wallet
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(buyer) {
        if let Some(p) = m.player_mut() {
            p.items.inventory = buyer_inv;
            p.items.gold = buyer_gold;
        }
        fx.out.to_self(create_msg(&arrived));
        crate::stats::pool::emit_gold(m, env.stats(), &mut fx.out);
    }
    fx.flush(svc, ctx, PlayerRef(buyer));
    // the near set sees the sale
    ctx.broadcast(
        Some(&*svc),
        vendor,
        Audience::NearInclSelf,
        pb::PVendorItemNum {
            slot: slot as u32,
            remaining: i32::from(remaining),
            buyer_name,
        },
    );
    svc.econ.vendor.stats.sales += 1;
    // one batch: [snap(buyer), snap(seller)] (VR-018)
    match (
        atomic_snapshot(svc, ctx, buyer),
        atomic_snapshot(svc, ctx, vendor),
    ) {
        (Some(b), Some(s)) => ctx.persist.atomic(AtomicBatch::vendor_purchase(b, s)),
        _ => {
            tracing::error!(
                buyer = buyer.0,
                vendor = vendor.0,
                "sale committed without a batch"
            );
            svc.saves.mark_urgent(buyer);
            svc.saves.mark_urgent(vendor);
        }
    }
    let mut event = vendor_sale(
        seller_chr,
        buyer_chr,
        &taken.item.to_record(0),
        taken.item.count,
        total,
    );
    if let Some(actor) = crate::audit::actor_of(ctx, PlayerRef(buyer)) {
        event = event.by(actor);
    }
    ctx.audit.emit(event);
}
