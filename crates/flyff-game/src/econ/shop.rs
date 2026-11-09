//! NPC shops (Plan/19 §7.1-§7.6, WP-ECON-01; ref/13 §12, ref/07a §8.3-§8.7; src:WORLDSERVER/DPSrvr.cpp:2778-3231):
//! the stock of a vendor, the shop session (`OpenShopWndReq`, `CloseShopWnd`) and the three transactions
//! `BuyItem`, `BuyChipItem` and `SellItem`.
//!
//! * **Stock.** [`ShopStock`] is the four 100-slot tabs of a vendor NPC definition, built from the precomputed
//!   `NpcDef.vendor.tabs[i].stock` of `flyff-data` (the exact `GenerateVendorItem` port, Plan/14 §6.8.2). It is
//!   *infinite*: a purchase never decrements it, the stack count only caps one purchase. The stock of one
//!   definition is shared by every NPC and every player ([`super::EconService::stock_of`]).
//! * **Session.** The shop window is the player's trade partner ([`super::TradePartner::Shop`]). The requests
//!   work on that link; the NPC must still exist. Opening needs the vendor within `gameplay.npc_menu_range`
//!   of the player in the same world and layer (**FIX**, VR-012: the original had no distance check); buying,
//!   selling and quick sell keep the original's rule, *some* `MMI_TRADE` NPC within that range
//!   (`CNpcChecker::IsCloseNpc`).
//! * **Atomicity.** Every handler validates everything, then commits: the stackable price, the quantity and
//!   the gold are computed before the first mutation, the inventory space is checked before the gold or the
//!   chips leave (**FIX** VR-020 for the chip shop, PL-065).
//!
//! The price arithmetic is `flyff_rules::item::price` (`f32` and `int` truncation exactly where the original
//! cast); the browser shows the same numbers through the wasm `RulesContext`.

use super::TradePartner;
use super::tax::{Continent, TaxHooks, TaxKind, TaxQuote};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::event::EventFactors;
use crate::item::container::{ContainerKind, DeferSerials, ItemContainer, StorageId};
use crate::item::env::ItemEnv;
use crate::item::fx::{Fx, item_json};
use crate::item::gold::{GOLD_BUSINESS_CAP, GOLD_MAX, core::add_gold};
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemError, ItemOrigin, RemoveReason, core as ops};
use crate::mover::Mover;
use flyff_common::ObjId;
use flyff_common::defs::item::{II_CHP_RED, II_SYS_SYS_SCR_PERIN, II_SYS_SYS_SCR_SEALCHARACTER};
use flyff_common::defs::itemkind::IK3_EVENTMAIN;
use flyff_common::defs::neuz::MMI_TRADE;
use flyff_common::defs::text::{
    TID_GAME_CANNOT_DO_USINGITEM, TID_GAME_EQUIPTRADE, TID_GAME_LACKCHIP, TID_GAME_LACKMONEY,
    TID_GAME_LACKSPACE, TID_GAME_SECRETROOM_STORE_BUY, TID_GAME_TRADELIMITNPC,
};
use flyff_data::GameData;
use flyff_data::character::{MAX_VENDOR_INVENTORY, MAX_VENDOR_INVENTORY_TAB, NpcDef};
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::item::{buy_unit_price, chip_cost, get_cost, sell_unit_price};
use serde_json::json;
use std::sync::Arc;

/// The stock of one vendor NPC definition: four tabs of 100 slots (`m_ShopInventory[4]`, Plan/19 §7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShopStock {
    /// `SetVenderType`: 0 penya shop, 1 Red Chip shop.
    pub vendor_type: i32,
    /// The tabs; the storage id of an entry is the `obj_id` the client sends.
    pub tabs: [ItemContainer; MAX_VENDOR_INVENTORY_TAB],
}

impl ShopStock {
    /// The stock of `def`: every precomputed entry is a full stack (`count = pack_max`, `endurance` from the
    /// prop) placed with the container's `Add`, in order, serial 0 (stock is never owned or persisted). An
    /// entry whose prop is missing is skipped.
    pub fn build(def: &NpcDef, data: &GameData) -> ShopStock {
        let tabs = std::array::from_fn(|i| {
            let mut c = ItemContainer::new(ContainerKind::ShopTab, MAX_VENDOR_INVENTORY as u16, 0);
            if let Some(tab) = def.vendor.tabs.get(i) {
                for e in &tab.stock {
                    if e.count < 1 {
                        continue;
                    }
                    let mut it = Item::new(e.item, e.count as u16);
                    it.endurance = e.hit_point as i32;
                    // a full container ignores the rest, an unknown prop is skipped (`CItemContainer::Add`)
                    let _ = c.add(&it, data, &mut DeferSerials);
                }
            }
            c
        });
        ShopStock {
            vendor_type: def.vendor.vendor_type,
            tabs,
        }
    }

    /// The stock entry `(tab, obj_id)`.
    pub fn entry(&self, tab: usize, obj_id: StorageId) -> Option<&Item> {
        self.tabs.get(tab)?.get_at_id(obj_id)
    }

    /// The `OpenShopWndNtf` payload: the four containers.
    pub fn to_pb(&self) -> Vec<pb::ItemContainer> {
        self.tabs.iter().map(ItemContainer::to_pb).collect()
    }
}

/// Which currency a purchase uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Currency {
    /// Penya (`BuyItem`, vendor type 0).
    Penya,
    /// Red Chips (`BuyChipItem`, vendor type 1).
    Chip,
}

impl Currency {
    fn vendor_type(self) -> i32 {
        match self {
            Currency::Penya => 0,
            Currency::Chip => 1,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Currency::Penya => "penya",
            Currency::Chip => "chip",
        }
    }
}

/// An open shop as a request sees it.
pub struct ShopSession {
    /// The vendor NPC (the trade partner).
    pub npc: ObjId,
    /// The vendor's `character.inc` key.
    pub key: String,
    /// The stock.
    pub stock: Arc<ShopStock>,
}

/// `IsDie()` of the requesting player: the original had no such guard in the shop handlers, but a dead player
/// has no shop window (death ends the session), so a request after death is stale.
fn is_dead(m: &Mover) -> bool {
    crate::combat::state::is_die(m)
}

/// The shop the player has open: the partner is an NPC that still exists and has a definition.
pub fn open_session(
    svc: &mut Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    data: &GameData,
) -> Option<ShopSession> {
    let m = ctx.player(who)?;
    let npc = m.player()?.econ.shop_npc()?;
    let n = ctx.worlds.mover(npc)?;
    let key = n.npc()?.character_key.clone();
    let def = data.npc(&key)?;
    let stock = svc.econ.stock_of(def, data);
    Some(ShopSession { npc, key, stock })
}

/// `CNpcChecker::IsCloseNpc(MMI_TRADE, world, pos)`: some `MMI_TRADE` NPC is within the NPC-menu range of the
/// player (same world and layer, VR-012).
pub fn near_trade_npc(ctx: &GameCtx<'_>, m: &Mover) -> bool {
    ctx.worlds
        .close_npc(MMI_TRADE, m.core.world, m.core.layer, m.core.pos)
        .is_some()
}

/// `GetPropensityPenalty(propensity).nShop`: whether a chaotic player may shop. The penalty row is the one
/// whose propensity range holds the value, else step 8 (the last), else a default row (no shopping).
pub fn chaotic_may_shop(data: &GameData, propensity: u32) -> bool {
    let p = i64::from(propensity);
    let row = data
        .pk
        .penalties
        .iter()
        .find(|r| i64::from(r.min) <= p && p <= i64::from(r.max))
        .or_else(|| data.pk.penalties.iter().find(|r| r.step == 8));
    row.is_some_and(|r| r.shop != 0)
}

/// `OpenShopWndReq{npc_obj_id}` (Plan/19 §7.2; ref/07a §8.3). Ignored (silently) unless: the player has no
/// partner, is alive and not vending; the object is an NPC with a shop of this world and layer within
/// `gameplay.npc_menu_range`; a chaotic player's penalty row allows shopping. An open bank answers
/// `TID_GAME_TRADELIMITNPC`. Effect: the NPC becomes the partner and the owner gets `OpenShopWndNtf` (header
/// object = the NPC) with the four tabs.
pub fn on_open(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::OpenShopWndReq) {
    let core = svc.items.handle();
    let data = &*core.data;
    let npc_id = ObjId(msg.npc_obj_id);
    let range = ctx.worlds.npc_checker.range();
    let Some(m) = ctx.player(who) else { return };
    let Some(p) = m.player() else { return };
    if is_dead(m) || p.econ.has_partner() {
        return;
    }
    // the target: an NPC of this world and layer within the menu range
    let Some(n) = ctx.worlds.mover(npc_id) else {
        return;
    };
    let Some(ext) = n.npc() else { return };
    if n.core.world != m.core.world || n.core.layer != m.core.layer {
        return;
    }
    let d = n.core.pos - m.core.pos;
    if d.length_squared() >= range * range {
        return;
    }
    // chaotic players: the propensity penalty row decides
    if p.pk.propensity > 0 && !chaotic_may_shop(data, p.pk.propensity) {
        return;
    }
    let Some(def) = data.npc(&ext.character_key) else {
        return;
    };
    if !def.is_vendor() {
        tracing::debug!(npc = %ext.character_key, "OpenShopWndReq: not a vendor");
        return;
    }
    if p.econ.bank_open {
        let mut fx = Fx::new();
        fx.text(TID_GAME_TRADELIMITNPC);
        fx.flush(svc, ctx, who);
        return;
    }
    if p.econ.vending {
        return;
    }
    let stock = svc.econ.stock_of(def, data);
    if let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        p.econ.partner = TradePartner::Shop { npc: npc_id };
    }
    if let Some(sid) = ctx.session_of(who) {
        ctx.outbox.to_session(
            sid,
            npc_id.0,
            pb::OpenShopWndNtf {
                tabs: stock.to_pb(),
            },
        );
    }
}

/// `CloseShopWnd` (ref/07a §8.4): clears the shop link. No answer.
pub fn on_close(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::CloseShopWnd,
) {
    if let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        p.econ.end_shop_session();
    }
}

/// `BuyItem` (Plan/19 §7.3; ref/07a §8.5).
pub fn on_buy(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::BuyItem) {
    buy(
        svc,
        ctx,
        who,
        Currency::Penya,
        Request {
            tab: msg.tab,
            obj_id: msg.obj_id,
            num: msg.num,
            item_id: msg.item_id,
        },
    );
}

/// `BuyChipItem` (Plan/19 §7.3 chip shop; ref/07a §8.6).
pub fn on_buy_chip(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::BuyChipItem,
) {
    buy(
        svc,
        ctx,
        who,
        Currency::Chip,
        Request {
            tab: msg.tab,
            obj_id: msg.obj_id,
            num: msg.num,
            item_id: msg.item_id,
        },
    );
}

/// The four fields of `BuyItem` / `BuyChipItem`.
#[derive(Clone, Copy, Debug)]
struct Request {
    tab: i32,
    obj_id: u32,
    num: i32,
    item_id: u32,
}

/// What a purchase decided.
enum Verdict {
    /// Ignored without a message.
    Ignore,
    /// Refused with a text.
    Text(u32),
    /// Go: the stack to create and the price.
    Buy { item: Box<Item>, num: u16, pay: Pay },
}

/// What the purchase costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pay {
    /// `total` penya of which `tax` is the purchase tax; `continent` books it.
    Penya {
        total: i64,
        tax: i64,
        continent: Continent,
    },
    /// `chips` Red Chips.
    Chips { chips: u32 },
}

/// The shared prefix of both purchases up to the verdict: everything is checked on the immutable state, the
/// first mutation comes after the verdict.
fn judge_purchase(
    svc: &Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    cur: Currency,
    sess: &ShopSession,
    env: &ItemEnv<'_>,
    r: Request,
) -> Verdict {
    let (Ok(tab), Ok(obj_id)) = (usize::try_from(r.tab), StorageId::try_from(r.obj_id)) else {
        return Verdict::Ignore;
    };
    if sess.stock.vendor_type != cur.vendor_type() {
        return Verdict::Ignore;
    }
    let Some(m) = ctx.player(who) else {
        return Verdict::Ignore;
    };
    let Some(p) = m.player() else {
        return Verdict::Ignore;
    };
    if is_dead(m) || !near_trade_npc(ctx, m) {
        return Verdict::Ignore;
    }
    let Some(entry) = sess.stock.entry(tab, obj_id) else {
        return Verdict::Ignore;
    };
    if entry.item_id != r.item_id {
        return Verdict::Ignore;
    }
    let Some(prop) = env.data().item(entry.item_id) else {
        return Verdict::Ignore;
    };
    let want = i64::from(r.num).min(i64::from(entry.count));
    // the occupation shop, penya and chip alike: only the guild that occupies the continent the buyer stands on buys
    // (`TID_GAME_SECRETROOM_STORE_BUY`, before the price)
    let tax = &svc.event.tax;
    if tax.is_occupation_shop_item(entry.item_id)
        && !tax
            .is_occupation_guild_member(tax.continent_of(m.core.pos, m.core.world), p.social.guild)
    {
        return Verdict::Text(TID_GAME_SECRETROOM_STORE_BUY);
    }
    let gold = i64::from(p.items.gold);
    match cur {
        Currency::Penya => {
            let quote = TaxQuote::of(&svc.event.tax, m.core.pos, m.core.world, entry.item_id);
            let cost = get_cost(prop, 0);
            let price = buy_unit_price(
                cost,
                ctx.live.rates.shop_cost,
                svc.event.factors().shop_buy(),
                entry.item_id == II_SYS_SYS_SCR_PERIN,
                quote.purchase,
            );
            // the quantity is limited by the price *before* tax; the tax can then make the total unaffordable,
            // which is a silent refusal (KEEP-QUIRK)
            let num = want.min(gold / i64::from(price.unit));
            if num <= 0 {
                return Verdict::Text(TID_GAME_LACKMONEY);
            }
            let unit = i64::from(price.unit) + i64::from(price.tax);
            let total = unit * num;
            if total <= 0 || total > GOLD_MAX || gold < total {
                return Verdict::Ignore;
            }
            let mut item = entry.clone();
            item.count = num as u16;
            item.serial = flyff_common::ItemSerial(0);
            Verdict::Buy {
                item: Box::new(item),
                num: num as u16,
                pay: Pay::Penya {
                    total,
                    tax: i64::from(price.tax) * num,
                    continent: quote.continent,
                },
            }
        }
        Currency::Chip => {
            let per = chip_cost(prop);
            if per < 1 {
                return Verdict::Ignore;
            }
            let chips = i64::from(per) * want;
            let have = i64::from(p.items.inventory.count_of(II_CHP_RED));
            if have < chips {
                return Verdict::Text(TID_GAME_LACKCHIP);
            }
            // VR-020: simulate the payment, then the fit; nothing changes when the item does not fit
            let mut sim = p.items.inventory.clone();
            remove_all_of(&mut sim, II_CHP_RED, chips);
            let mut item = entry.clone();
            item.count = want as u16;
            item.serial = flyff_common::ItemSerial(0);
            if sim.is_full(&item, prop.pack_max.max(1), want as u32) {
                return Verdict::Text(TID_GAME_LACKSPACE);
            }
            Verdict::Buy {
                item: Box::new(item),
                num: want as u16,
                pay: Pay::Chips {
                    chips: chips as u32,
                },
            }
        }
    }
}

/// Removes `n` pieces of `item_id` from a scratch container in storage order (`RemoveItemA`).
fn remove_all_of(c: &mut ItemContainer, item_id: u32, mut n: i64) {
    let ids: Vec<StorageId> = c
        .iter()
        .filter(|(_, it)| it.item_id == item_id)
        .map(|(id, _)| id)
        .collect();
    for id in ids {
        if n <= 0 {
            break;
        }
        let Some(it) = c.get_at_id_mut(id) else {
            continue;
        };
        let take = n.min(i64::from(it.count));
        n -= take;
        it.count -= take as u16;
        if it.count == 0 {
            c.remove_at_id(id);
        }
    }
}

/// A purchase: the shared part of `BuyItem` and `BuyChipItem` (see the module documentation).
fn buy(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, cur: Currency, r: Request) {
    if r.num < 1 || !(0..MAX_VENDOR_INVENTORY_TAB as i32).contains(&r.tab) {
        return;
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let Some(sess) = open_session(svc, ctx, who, env.data()) else {
        return;
    };
    let verdict = judge_purchase(svc, ctx, who, cur, &sess, &env, r);
    let mut fx = Fx::new();
    match verdict {
        Verdict::Ignore => return,
        Verdict::Text(tid) => fx.text(tid),
        Verdict::Buy { item, num, pay } => {
            {
                let Services { serials, event, .. } = &mut *svc;
                let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
                if let Some(m) = ctx.worlds.player_mut(who.0) {
                    commit_purchase(
                        m,
                        &env,
                        &mut src,
                        &mut event.tax,
                        &sess,
                        cur,
                        (*item, num, pay),
                        &mut fx,
                    );
                }
            }
            // 13 §11: the booked counters reach the database at most every 10 s
            crate::event::tax::flush_due(&mut svc.event.tax, ctx);
        }
    }
    fx.flush(svc, ctx, who);
}

/// The commit of a purchase: the item first (the only step that can fail), then the payment.
#[allow(clippy::too_many_arguments)]
fn commit_purchase(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    serials: &mut CtxSerials<'_>,
    tax: &mut dyn TaxHooks,
    sess: &ShopSession,
    cur: Currency,
    (item, num, pay): (Item, u16, Pay),
    fx: &mut Fx,
) {
    let template = item.clone();
    let (origin, chips_taken) = match pay {
        // the chips go before the item is created so that the freed slot is there (the fit was simulated)
        Pay::Chips { chips } => {
            let taken = ops::remove_item_by_id(
                m,
                env,
                II_CHP_RED,
                Some(chips),
                RemoveReason::Other("chip_shop"),
                fx,
            );
            (ItemOrigin::ChipShop, taken)
        }
        Pay::Penya { .. } => (ItemOrigin::ShopBuy, 0),
    };
    match ops::create_item(m, env, serials, item, origin, fx) {
        Ok(_) => {}
        Err(e) => {
            // `LackSpace` cannot happen after the simulation; a serial fault is the only other way
            tracing::warn!(npc = %sess.key, error = %e, "shop purchase: the item was not created");
            if matches!(e, ItemError::LackSpace) {
                fx.text(TID_GAME_LACKSPACE);
            }
            if chips_taken > 0 {
                // best effort: give the chips back
                let back = Item::new(II_CHP_RED, chips_taken.min(u32::from(u16::MAX)) as u16);
                let _ = ops::create_item(m, env, serials, back, ItemOrigin::Other("refund"), fx);
            }
            return;
        }
    }
    match pay {
        Pay::Penya {
            total,
            tax: booked,
            continent,
        } => {
            add_gold(m, env, -total, true, fx);
            if booked > 0 {
                tax.add_tax(continent, booked, TaxKind::Purchase);
            }
            fx.audit(
                ev::SHOP_BUY,
                json!({
                    "npc": sess.key,
                    "item": item_json(&template),
                    "count": num,
                    "gold": total,
                    "tax": booked,
                    "currency": cur.tag(),
                }),
            );
        }
        Pay::Chips { chips } => {
            fx.audit(
                ev::SHOP_BUY,
                json!({
                    "npc": sess.key,
                    "item": item_json(&template),
                    "count": num,
                    "gold": 0,
                    "tax": 0,
                    "chips": chips,
                    "currency": cur.tag(),
                }),
            );
        }
    }
    fx.mark_urgent();
}

/// `SellItem{obj_id, num}` (Plan/19 §7.4; ref/07a §8.7).
pub fn on_sell(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::SellItem) {
    if msg.num < 1 {
        return;
    }
    let Ok(id) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let Some(sess) = open_session(svc, ctx, who, env.data()) else {
        return;
    };
    let mut fx = Fx::new();
    {
        let Some(m) = ctx.worlds.player(who.0) else {
            return;
        };
        if is_dead(m) || !near_trade_npc(ctx, m) {
            return;
        }
        let Some(p) = m.player() else { return };
        let Some(item) = p.items.inventory.get_at_id(id) else {
            return;
        };
        if !item.is_usable() {
            return;
        }
        let Some(prop) = env.data().item(item.item_id) else {
            return;
        };
        if prop.item_kind3 == IK3_EVENTMAIN
            || prop.item_kind3 == flyff_common::defs::itemkind::IK3_QUEST
        {
            return;
        }
        if p.items.inventory.is_equip(id) {
            fx.text(TID_GAME_EQUIPTRADE);
            fx.flush(svc, ctx, who);
            return;
        }
        if item.item_id == II_SYS_SYS_SCR_SEALCHARACTER || item.item_id == II_SYS_SYS_SCR_PERIN {
            return;
        }
        let num = msg.num.min(i32::from(item.count)).max(1);
        let quote = TaxQuote::of(&svc.event.tax, m.core.pos, m.core.world, item.item_id);
        let cost = get_cost(prop, item.upgrade_level);
        let sale = sell_unit_price(cost, svc.event.factors().shop_sell(), quote.sales);
        let total = i64::from(sale.unit) * i64::from(num);
        let total_tax = i64::from(sale.tax) * i64::from(num);
        let gold = i64::from(p.items.gold);
        if (gold + total) as f32 >= GOLD_BUSINESS_CAP as f32 {
            return; // silently
        }
        if Item::is_vagrant_ride(prop) {
            return;
        }
        if p.items.is_using(id) {
            fx.text(TID_GAME_CANNOT_DO_USINGITEM);
            fx.flush(svc, ctx, who);
            return;
        }
        let total = total.max(1);
        let item_json = item_json(item);
        let continent = quote.continent;
        let npc_key = sess.key.clone();
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return;
        };
        // commit: the gold, the tax, the item
        add_gold(m, &env, total, true, &mut fx);
        if total_tax > 0 {
            svc.event.tax.add_tax(continent, total_tax, TaxKind::Sales);
        }
        ops::remove_item(m, &env, id, num as u16, RemoveReason::Sell, &mut fx);
        fx.audit(
            ev::SHOP_SELL,
            json!({
                "npc": npc_key,
                "item": item_json,
                "count": num,
                "gold": total,
                "tax": total_tax,
            }),
        );
        fx.mark_urgent();
    }
    // 13 §11: the booked counters reach the database at most every 10 s
    crate::event::tax::flush_due(&mut svc.event.tax, ctx);
    fx.flush(svc, ctx, who);
}
