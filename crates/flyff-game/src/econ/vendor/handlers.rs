//! The vendor handlers (Plan/19 §9.2; ref/07c §5.1-§5.5; src:WORLDSERVER/DPSrvr.cpp:8928-9217).

use super::close::{close_vendor, leave_browse};
use super::{
    MAX_VENDITEM, MAX_VENDOR_REVISION, MAX_VENDOR_TITLE, VendorSlot, live_player, sync_vending,
};
use crate::chat::ingress::markup::has_item_buff;
use crate::chat::ingress::notice;
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::TradePartner;
use crate::econ::trade::send;
use crate::item::container::StorageId;
use crate::outbox::Audience;
use crate::stats::tick::is_attack_mode;
use flyff_common::ObjId;
use flyff_common::defs::define::RA_FIGHT;
use flyff_common::defs::item::II_SYS_SYS_SCR_FONTEDIT;
use flyff_common::defs::text::{
    TID_GAME_CANNOT_DO_USINGITEM, TID_GAME_CHAOTIC_NOT_VENDOR, TID_GAME_FAIL_TO_OPEN_SHOP,
    TID_GAME_GUILDCOMBAT_CANNOT_TRADE, TID_GAME_NPC_RADIUS, TID_GAME_VENDOR_MAX_ALL_GOLD,
};
use flyff_common::defs::world::{WI_WORLD_GUILDWAR, WI_WORLD_MINIROOM, WI_WORLD_QUIZ};
use flyff_proto::pb;
use flyff_rules::chat::parsing_effect;
use flyff_rules::item::{TradeView, tradeable};
use flyff_rules::names::{NameKind, NameLimits, name_error_tid, validate_name};

/// The business limit of VR-019: the total of the wallet and the goods stays below this (the legacy client rule,
/// `WndRegVend.cpp:265-350`).
const TOTAL_LIMIT: i64 = 2_100_000_000;

/// `GetPropensityPenalty(propensity).nVendor`: whether a chaotic player may open a shop. The penalty row is the
/// one whose propensity range holds the value, else step 8, else no right.
pub fn chaotic_may_vend(data: &flyff_data::GameData, propensity: u32) -> bool {
    let p = i64::from(propensity);
    let row = data
        .pk
        .penalties
        .iter()
        .find(|r| i64::from(r.min) <= p && p <= i64::from(r.max))
        .or_else(|| data.pk.penalties.iter().find(|r| r.step == 8));
    row.is_some_and(|r| r.vendor != 0)
}

/// `OnRegisterPVendorItem` (Plan/19 §9.2): puts `num` pieces of the entry `obj_id` into shop slot `slot` at
/// `cost` each.
///
/// In this order: `slot < 20` and `cost < 1 -> 1`; the player has no partner, is alive and the shop is not open;
/// the region is not `RA_FIGHT` (`TID_GAME_FAIL_TO_OPEN_SHOP`), the world is not the mini-room (silent); no NPC
/// that offers a menu within `gameplay.vendor_npc_min_dist` (XZ, VR-013: `TID_GAME_NPC_RADIUS`); not the quiz
/// world; the slot is free (**FIX**: the original overwrote the pointer and leaked the old reservation); the item
/// is usable (not reserved, so an item that is already registered is refused), tradeable (not a quest item, not
/// bound, not worn, not expired, not a guild cloak, not a vagrant ride) and not in use
/// (`TID_GAME_CANNOT_DO_USINGITEM`); `num` is clamped to the stack; **FIX (VR-019)** the total of the wallet,
/// the goods and this one must be in `(0, 2.1e9)` (`TID_GAME_VENDOR_MAX_ALL_GOLD`). The item is reserved and the
/// seller gets `RegisterPVendorItemNtf`.
pub fn on_register(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::RegisterPVendorItemReq,
) {
    let slot = msg.slot as usize;
    if slot >= MAX_VENDOR_REVISION {
        return;
    }
    let cost = msg.cost.max(1);
    let Ok(obj_id) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let radius = ctx.cfg.rules.gameplay.vendor_npc_min_dist;
    let Some(m) = live_player(ctx, who.0) else {
        return;
    };
    let Some(p) = m.player() else { return };
    if p.econ.has_partner() || p.econ.vendor.is_open() || is_die(m) {
        return;
    }
    if p.region_attr & RA_FIGHT != 0 {
        notice::defined(ctx, who, TID_GAME_FAIL_TO_OPEN_SHOP, Vec::new());
        return;
    }
    if m.core.world.0 == WI_WORLD_MINIROOM {
        return;
    }
    if ctx
        .worlds
        .any_menu_npc_within_xz(m.core.world, m.core.layer, m.core.pos, radius)
    {
        notice::defined(ctx, who, TID_GAME_NPC_RADIUS, Vec::new());
        return;
    }
    if m.core.world.0 == WI_WORLD_QUIZ {
        notice::defined(ctx, who, TID_GAME_FAIL_TO_OPEN_SHOP, Vec::new());
        return;
    }
    if p.econ.vendor.slots[slot].is_some() {
        return;
    }
    let Some(it) = p.items.inventory.get_at_id(obj_id) else {
        return;
    };
    if !it.is_usable() {
        return;
    }
    let Some(prop) = env.data().item(it.item_id) else {
        return;
    };
    let view = TradeView {
        reserved: false,
        flags: it.flags.0,
        keep_until: it.keep_until,
        equipped: p.items.inventory.is_equip(obj_id),
        in_use: p.items.is_using(obj_id),
        guild_id: it.guild_id,
        binds_by_option: it.is_binds(prop, env.cfg().only_armor_awakes),
    };
    if let Some(tid) = tradeable(&view, prop, env.now.0) {
        if tid == TID_GAME_CANNOT_DO_USINGITEM {
            notice::defined(ctx, who, tid, Vec::new());
        }
        return;
    }
    let num = u16::try_from(msg.num.min(i32::from(it.count)).max(1)).unwrap_or(1);
    // VR-019: the legacy client's total rule, enforced by the server
    let total = i64::from(p.items.gold)
        + p.econ.vendor.registered_value()
        + i64::from(cost) * i64::from(num);
    if !(1..TOTAL_LIMIT).contains(&total) {
        notice::defined(ctx, who, TID_GAME_VENDOR_MAX_ALL_GOLD, Vec::new());
        return;
    }
    let good = VendorSlot {
        obj_id,
        serial: it.serial,
        item_id: it.item_id,
        count: num,
        price: cost,
    };
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        if let Some(it) = p.items.inventory.get_at_id_mut(obj_id) {
            it.reserved = num;
            it.vendor_price = cost;
        }
        p.econ.vendor.slots[slot] = Some(good);
        sync_vending(p);
    }
    svc.econ.vendor.active.insert(who.0);
    send(
        ctx,
        who.0,
        ObjId::NULL,
        pb::RegisterPVendorItemNtf {
            slot: msg.slot,
            item_type: 0,
            obj_id: msg.obj_id,
            num: i32::from(num),
            cost,
        },
    );
}

/// `OnUnregisterPVendorItem` (Plan/19 §9.2): takes the good in `slot` (`< 30`) back while the shop is closed;
/// the seller gets `UnregisterPVendorItemNtf` when a slot was cleared.
pub fn on_unregister(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UnregisterPVendorItemReq,
) {
    let slot = msg.slot as usize;
    if slot >= MAX_VENDITEM {
        return;
    }
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
        return;
    };
    if p.econ.has_partner() || p.econ.vendor.is_open() {
        return;
    }
    let Some(good) = p.econ.vendor.slots[slot].take() else {
        return;
    };
    if let Some(it) = p.items.inventory.get_at_id_mut(good.obj_id)
        && it.serial == good.serial
    {
        it.reserved = 0;
        it.vendor_price = 0;
    }
    sync_vending(p);
    let idle = !p.econ.vending;
    if idle {
        svc.econ.vendor.active.remove(&who.0);
    }
    send(
        ctx,
        who.0,
        ObjId::NULL,
        pb::UnregisterPVendorItemNtf { slot: msg.slot },
    );
}

/// `OnPVendorOpen` (Plan/19 §9.2): opens the shop with a title.
///
/// **FIX (VR-046, PL-155):** every check runs before the title is set. The title is cut at 47 bytes, loses its
/// colour and effect codes unless the player has the FONTEDIT buff, and goes through the vendor-title name rule
/// (`flyff_rules::names::NameKind::VendorTitle`: the `InvalidName` and `Letter1` lists, the chat filter, 6..=32
/// characters of the filtered text); an invalid title answers its text (`TID_DIAG_0020` for a forbidden one) and
/// stops. Then: guild-war world or 1:1 guild combat -> `TID_GAME_GUILDCOMBAT_CANNOT_TRADE`; mini-room, a partner,
/// a duel, attack mode, flying, dead and an already open shop stop silently; a chaotic player without the vendor
/// right gets `TID_GAME_CHAOTIC_NOT_VENDOR`. The title is set; with goods registered the shop is announced
/// (`PVendorOpenNtf` to the near set) and [`VendorRoomHooks::on_vendor_open`] runs; without goods the title is set
/// and nothing is broadcast (**KEEP-QUIRK**).
///
/// [`VendorRoomHooks::on_vendor_open`]: super::VendorRoomHooks::on_vendor_open
pub fn on_open(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::PVendorOpenReq) {
    if msg.title.len() > MAX_VENDOR_TITLE {
        return;
    }
    let Some(m) = live_player(ctx, who.0) else {
        return;
    };
    let Some(p) = m.player() else { return };
    // 1. the title
    let typed = if has_item_buff(m, II_SYS_SYS_SCR_FONTEDIT) {
        msg.title.clone()
    } else {
        parsing_effect(&msg.title)
    };
    let title = match validate_name(
        NameKind::VendorTitle,
        &typed,
        &ctx.data.name_rules,
        NameLimits::default(),
        Some(&svc.chat.data.filter),
    ) {
        Ok(t) => t,
        Err(e) => {
            let tid = u32::try_from(name_error_tid(NameKind::VendorTitle, e)).unwrap_or(0);
            notice::defined(ctx, who, tid, Vec::new());
            return;
        }
    };
    // 2. the places and states that refuse a shop
    if m.core.world.0 == WI_WORLD_GUILDWAR
        || crate::econ::trade::guild_combat_1to1_participant(svc, ctx, who.0)
    {
        notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_CANNOT_TRADE, Vec::new());
        return;
    }
    if m.core.world.0 == WI_WORLD_MINIROOM
        || p.econ.has_partner()
        || p.econ.vendor.is_open()
        || m.combat.duel.duel > 0
        || is_attack_mode(m)
        || m.sim.act.flags.fly()
        || is_die(m)
    {
        return;
    }
    if p.pk.propensity > 0 && !chaotic_may_vend(ctx.data, p.pk.propensity) {
        notice::defined(ctx, who, TID_GAME_CHAOTIC_NOT_VENDOR, Vec::new());
        return;
    }
    // 3. open
    let now = ctx.clock.game_ms;
    let mut goods = false;
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.econ.vendor.title = Some(title.clone());
        p.econ.vendor.opened_at = now;
        goods = p.econ.vendor.is_vendor();
        if goods {
            // `m_dwHonorCheckTime = GetTickCount()` (21 §13.3): the hour of vending counts from here
            crate::social::honor::restart_clock(p, now);
        }
        sync_vending(p);
    }
    svc.econ.vendor.active.insert(who.0);
    if goods {
        svc.econ.vendor.stats.opened += 1;
        ctx.broadcast(
            Some(&*svc),
            who.0,
            Audience::NearInclSelf,
            pb::PVendorOpenNtf { title },
        );
        svc.econ.vendor.rooms.on_vendor_open(ctx, who);
    }
}

/// `OnQueryPVendorItem` (Plan/19 §9.2): the sender looks into the open shop of `vendor`. The sender has no
/// partner, is not flying, and the vendor is another online player of the same world with an open shop; the
/// sender then browses it (`partner = VendorBrowse`) and gets `PVendorItem` (header object = the vendor) with
/// the registered goods and the chat flag of the room.
pub fn on_query(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::QueryPVendorItem,
) {
    let vendor = ObjId(msg.vendor);
    if vendor == who.0 {
        return;
    }
    let (Some(m), Some(v)) = (live_player(ctx, who.0), live_player(ctx, vendor)) else {
        return;
    };
    let (Some(p), Some(vp)) = (m.player(), v.player()) else {
        return;
    };
    if p.econ.has_partner()
        || m.sim.act.flags.fly()
        || m.core.world != v.core.world
        || m.core.layer != v.core.layer
        || !vp.econ.vendor.is_open()
    {
        return;
    }
    let entries: Vec<pb::p_vendor_item::Entry> = vp
        .econ
        .vendor
        .goods()
        .filter_map(|(slot, good)| {
            let it = vp.items.inventory.get_at_id(good.obj_id)?;
            (it.serial == good.serial && it.reserved > 0).then(|| pb::p_vendor_item::Entry {
                slot: slot as u32,
                item: Some(it.to_pb(good.obj_id)),
                count: i32::from(it.reserved),
                price: good.price,
            })
        })
        .collect();
    let chatting_state = svc.econ.vendor.rooms.room_open_state(PlayerRef(vendor));
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.econ.partner = TradePartner::VendorBrowse { vendor };
    }
    svc.econ
        .vendor
        .browsers
        .entry(vendor)
        .or_default()
        .insert(who.0);
    send(
        ctx,
        who.0,
        vendor,
        pb::PVendorItem {
            entries,
            chatting_state,
        },
    );
}

/// `OnPVendorClose` / `ClosePVendor` (Plan/19 §9.2): `vendor == sender` closes the own shop (open or with goods:
/// reservations, title and slots go, the near set and the visitors are told, the room is deleted); any other id is
/// a visitor closing the window (**FIX:** only a visit is ended, never an NPC shop or a trade): the link is
/// cleared, the sender gets `PVendorCloseNtf{closed: false}` (header object = `vendor`) and leaves the room.
pub fn on_close(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PVendorCloseReq,
) {
    let vendor = ObjId(msg.vendor);
    if vendor == who.0 {
        close_vendor(svc, ctx, who.0);
    } else {
        leave_browse(svc, ctx, who.0, Some(vendor));
    }
}
