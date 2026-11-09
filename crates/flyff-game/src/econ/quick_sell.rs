//! Quick sell (Plan/19 §7.7, WP-ECON-02; ref/13 §12.5; src:WORLDSERVER/DPSrvr.cpp:13092-13217, PL-066,
//! VR-021): one request sells every plain weapon, armour piece, jewel and pet egg of the bag to the open shop.
//!
//! The rules, in the order of the original:
//!
//! * **FIX (VR-021).** The player must have a shop window open (the original dereferenced a null partner) and
//!   some `MMI_TRADE` NPC within the menu range. `features.quick_sell` switches the request off.
//! * For every storage id of the inventory in ascending order: the item is usable and is `IK1_WEAPON`,
//!   `IK1_ARMOR`, `IK2_JEWELRY` or the pet egg `II_PET_EGG`; it is not worn, not in use and has no upgrade
//!   (`+0`; random options, sockets and elements do **not** protect it - KEEP).
//! * Price: `max(cost / 4, 1)`; the sales tax only when the item is taxable *and* the seller has GENERAL
//!   authority; a bound item pays 1 penya but the tax is still credited (KEEP, KQ-15).
//! * At the 2 100 000 000 line (`(gold + unit) as f32 >= 2.1e9`): **FIX (PL-066)** one Perin is created and
//!   100 000 000 deducted when it fits; otherwise the legacy sentence is sent and the whole sale stops (what was
//!   sold stays sold).
//! * One piece per stack is sold per request (KEEP): a stack of 5 arrows... of an egg stack keeps 4.
//! * An urgent save follows (the original called `SavePlayer`) and one `shop.quick_sell` audit line lists the
//!   sale.
//!
//! The gold message is sent once at the end instead of after every item (the client sees the same final
//! value; up to 199 `SetPointParam` per request are saved).

use super::shop::{near_trade_npc, open_session};
use super::tax::{TaxHooks, TaxKind, TaxQuote};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::input::Authority;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::gold::{GOLD_BUSINESS_CAP, PERIN_VALUE, core::add_gold};
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemOrigin, RemoveReason, core as ops};
use crate::stats::pool::emit_gold;
use flyff_common::defs::item::{II_PET_EGG, II_SYS_SYS_SCR_PERIN};
use flyff_common::defs::itemkind::{IK1_ARMOR, IK1_WEAPON, IK2_JEWELRY};
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::item::{get_cost, quick_sell_unit_price};
use serde_json::json;

/// The sentence of the legacy refusal at the cap (`pUser->AddText(...)`, kept literally).
pub const TOO_MUCH_PENYA: &str = "You cannot sell your items because you have too much penya";

/// `QuickSell` (see the module documentation).
pub fn on_quick_sell(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::QuickSell,
) {
    if !ctx.cfg.rules.features.quick_sell {
        return;
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let Some(sess) = open_session(svc, ctx, who, env.data()) else {
        return;
    };
    {
        let Some(m) = ctx.player(who) else { return };
        if crate::combat::state::is_die(m) || !near_trade_npc(ctx, m) {
            return;
        }
    }
    let mut fx = Fx::new();
    let sold = {
        let Services { serials, event, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return;
        };
        let (pos, world) = (m.core.pos, m.core.world);
        let general = m
            .player()
            .is_some_and(|p| p.ident.authority == Authority::GENERAL);
        let only_armor_awakes = env.cfg().only_armor_awakes;
        let max = m.player().map_or(0, |p| p.items.inventory.item_max());
        let mut sold: Vec<serde_json::Value> = Vec::new();
        let (mut gold_in, mut tax_total) = (0_i64, 0_i64);
        'items: for raw in 0..max {
            let id: StorageId = raw;
            let Some(p) = m.player() else { break };
            let Some(item) = p.items.inventory.get_at_id(id) else {
                continue;
            };
            if !item.is_usable() {
                continue;
            }
            let Some(prop) = env.data().item(item.item_id) else {
                continue;
            };
            if prop.item_kind1 != IK1_WEAPON
                && prop.item_kind1 != IK1_ARMOR
                && prop.item_kind2 != IK2_JEWELRY
                && item.item_id != II_PET_EGG
            {
                continue;
            }
            if p.items.inventory.is_equip(id) || p.items.is_using(id) || item.upgrade_level > 0 {
                continue;
            }
            let quote = TaxQuote::of(&event.tax, pos, world, item.item_id);
            let price = quick_sell_unit_price(
                get_cost(prop, 0),
                quote.sales.filter(|_| general),
                item.is_binds(prop, only_armor_awakes),
            );
            let (item_id, serial) = (item.item_id, item.serial.0);
            let gold = i64::from(p.items.gold);
            if ((gold + i64::from(price.unit)) as f32) >= GOLD_BUSINESS_CAP as f32 {
                let perin = Item::new(II_SYS_SYS_SCR_PERIN, 1);
                if ops::can_create(m, &env, &perin) {
                    // a Perin takes 100 000 000 of the wallet
                    if ops::create_item(m, &env, &mut src, perin, ItemOrigin::Perin, &mut fx)
                        .is_err()
                    {
                        break 'items;
                    }
                    add_gold(m, &env, -PERIN_VALUE, false, &mut fx);
                } else {
                    fx.out.to_self(pb::Text {
                        kind: flyff_rules::chat::TEXT_GENERAL,
                        text: TOO_MUCH_PENYA.to_owned(),
                    });
                    break 'items;
                }
            }
            let credit = i64::from(price.unit).max(1);
            add_gold(m, &env, credit, false, &mut fx);
            if price.tax > 0 {
                event
                    .tax
                    .add_tax(quote.continent, i64::from(price.tax), TaxKind::Sales);
            }
            ops::remove_item(m, &env, id, 1, RemoveReason::Sell, &mut fx);
            gold_in += credit;
            tax_total += i64::from(price.tax);
            sold.push(json!({ "item_id": item_id, "serial": serial, "gold": credit }));
        }
        if !sold.is_empty() || !fx.out.is_empty() {
            emit_gold(m, env.stats(), &mut fx.out);
        }
        if !sold.is_empty() {
            fx.audit(
                ev::SHOP_QUICK_SELL,
                json!({
                    "npc": sess.key,
                    "items": sold,
                    "count": sold.len(),
                    "gold": gold_in,
                    "tax": tax_total,
                }),
            );
            fx.mark_urgent();
        }
        sold.len()
    };
    let _ = sold;
    // 13 §11: the booked counters reach the database at most every 10 s
    crate::event::tax::flush_due(&mut svc.event.tax, ctx);
    fx.flush(svc, ctx, who);
}
