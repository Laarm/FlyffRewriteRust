//! The trade handlers (Plan/19 §8.2; ref/07b §19.3; src:WORLDSERVER/DPSrvr.cpp:8602-8926). The state machine:
//!
//! ```text
//! ConfirmTradeReq -> [request] -> TradeReq -> [Item,Item]
//!   Item: TradePut / TradePull / TradePutGold, both sides in Item
//!   TradeOk: Item -> Ok          (partner Ok -> TradeLastConfirm to both, else TradeOk to both)
//!   TradeConfirm: Ok -> Confirm  (partner Ok -> TradeLastConfirmOk, partner Confirm -> commit)
//!   TradeCancelReq / any trigger -> cancelled
//! ```
//!
//! Every handler acts on the **sender's own** session (VR-001): the payload never names the session. A request
//! that does not fit the state is ignored without a message, as in the original.

use super::cancel::{Cancel, cancel_session};
use super::{MAX_TRADE, TradeId, TradeOffer, TradeSession, TradeSide, TradeStep, commit, send};
use crate::chat::ingress::notice;
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::TradePartner;
use crate::item::container::StorageId;
use crate::mover::Mover;
use crate::stats::tick::is_attack_mode;
use flyff_common::defs::text::{
    TID_GAME_BATTLE_NOTTRADE, TID_GAME_CANNOTTRADE_ITEM, TID_GAME_GUILDCOMBAT_CANNOT_TRADE,
    TID_GAME_TRADELIMITPC,
};
use flyff_common::defs::world::WI_WORLD_GUILDWAR;
use flyff_common::{GameMs, ObjId};
use flyff_proto::pb;
use flyff_rules::item::{TradeView, tradeable};

/// A player that is in a world (the legacy `IsValidObj` + `GetWorld()`).
fn live_player<'a>(ctx: &'a GameCtx<'_>, obj: ObjId) -> Option<&'a Mover> {
    ctx.worlds.player(obj).filter(|m| !m.core.deleted)
}

/// A participant of the 1:1 guild combat (`g_GuildCombat1to1Mng.IsPossibleUser`, WP-SOCIAL-26).
pub(crate) fn guild_combat_1to1_participant(svc: &Services, ctx: &GameCtx<'_>, who: ObjId) -> bool {
    crate::social::gc1to1::is_participant(svc, ctx, who)
}

/// The trade of the sender: its id and its side, when the pair is intact (both avatars exist, the same world,
/// the partner links point at each other; the legacy `pTrader->m_vtInfo.GetOther() == pUser`).
fn session_of_sender(svc: &Services, ctx: &GameCtx<'_>, who: ObjId) -> Option<(TradeId, usize)> {
    let id = svc.econ.trade.session_id_of(who)?;
    let s = svc.econ.trade.session(id)?;
    let me = s.index_of(who)?;
    let other = s.obj(1 - me);
    let (ma, mb) = (live_player(ctx, who)?, live_player(ctx, other)?);
    if ma.core.world != mb.core.world
        || ma.player()?.econ.partner != (TradePartner::Player { other })
        || mb.player()?.econ.partner != (TradePartner::Player { other: who })
    {
        return None;
    }
    Some((id, me))
}

/// Both sides still edit their offers (`TRADE_STEP_ITEM` twice).
fn both_in_item_step(s: &TradeSession) -> bool {
    s.side.iter().all(|x| x.step == TradeStep::Item)
}

/// `OnConfirmTrade` (Plan/19 §8.2): records a pending request and asks the target.
///
/// The requester has no partner and is alive, not in the guild-war world and not in the 1:1 guild combat
/// (`TID_GAME_GUILDCOMBAT_CANNOT_TRADE`); the target is another online player of the same world and layer with no
/// partner; neither duels. A target in attack mode answers the requester `TID_GAME_BATTLE_NOTTRADE`; otherwise
/// the request is recorded (replacing an older one of the requester) and the target gets `ConfirmTradeNtf`
/// (header object = the requester). The requester's invisibility ends in every case that reaches the target
/// lookup.
pub fn on_confirm_trade_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ConfirmTradeReq,
) {
    let me = who.0;
    let target = ObjId(msg.target);
    let Some(m) = live_player(ctx, me) else {
        return;
    };
    let Some(p) = m.player() else { return };
    if p.econ.has_partner() || is_die(m) || target == me {
        return;
    }
    if m.core.world.0 == WI_WORLD_GUILDWAR || guild_combat_1to1_participant(svc, ctx, me) {
        notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_CANNOT_TRADE, Vec::new());
        return;
    }
    let now = ctx.clock.game_ms;
    let ttl = GameMs(u64::from(ctx.cfg.rules.gameplay.invite_ttl_s) * 1000);
    if let Some(t) = live_player(ctx, target)
        && let Some(tp) = t.player()
        && !tp.econ.has_partner()
        && t.core.world == m.core.world
        && t.core.layer == m.core.layer
    {
        if m.combat.duel.duel > 0 || t.combat.duel.duel > 0 {
            return;
        }
        if is_attack_mode(t) {
            notice::defined(ctx, who, TID_GAME_BATTLE_NOTTRADE, Vec::new());
        } else if !crate::chat::privacy::accepts_trade(&*ctx.worlds, target) {
            // 21 §9.4 / 23 §6.11 (PL-095): the target's `privacy.trade` is off; no request is recorded
            let name = tp.ident.name.clone();
            crate::social::block_notice(
                ctx,
                who,
                crate::social::messenger::block::BLOCK_TRADE,
                &name,
            );
        } else {
            svc.econ
                .trade
                .request(me, target, GameMs(now.0.saturating_add(ttl.0)));
            send(ctx, target, me, pb::ConfirmTradeNtf {});
        }
    }
    crate::skill::procs::remove_invisible(svc, ctx, me);
}

/// `OnConfirmTradeCancel` (Plan/19 §8.2, VR-001): the target declines. **FIX:** the sender must be the target of a
/// pending request from `requester`; the record is deleted and the requester gets `ConfirmTradeCancelNtf`
/// (header object = the requester).
pub fn on_confirm_trade_cancel_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ConfirmTradeCancelReq,
) {
    let requester = ObjId(msg.requester);
    if !svc
        .econ
        .trade
        .take_request(requester, who.0, ctx.clock.game_ms)
    {
        return;
    }
    if live_player(ctx, requester).is_some() {
        send(ctx, requester, requester, pb::ConfirmTradeCancelNtf {});
    }
}

/// `OnTrade` (Plan/19 §8.2): the target accepts. **FIX (VR-017):** a pending, unexpired request from `requester`
/// must exist (it is consumed). Both players: online in the same world and layer, no partner, no duel, no stall
/// (an open one or registered goods), no bank or guild bank open (`TID_GAME_TRADELIMITPC`). Then the trade opens
/// and both get `TradeNtf` (header object = the other, payload the accepter's character id) without the partner
/// inventory (PL-063).
pub fn on_trade_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::TradeReq) {
    let me = who.0;
    let requester = ObjId(msg.requester);
    if requester == me
        || !svc
            .econ
            .trade
            .take_request(requester, me, ctx.clock.game_ms)
    {
        return;
    }
    let (Some(mm), Some(rm)) = (live_player(ctx, me), live_player(ctx, requester)) else {
        return;
    };
    let (Some(mp), Some(rp)) = (mm.player(), rm.player()) else {
        return;
    };
    if mp.econ.has_partner()
        || rp.econ.has_partner()
        || mm.core.world != rm.core.world
        || mm.core.layer != rm.core.layer
        || is_die(mm)
        || is_die(rm)
    {
        return;
    }
    if mm.combat.duel.duel > 0 || rm.combat.duel.duel > 0 {
        return;
    }
    if mp.econ.vending || rp.econ.vending {
        return;
    }
    if mp.econ.bank_open
        || rp.econ.bank_open
        || mp.social.guild_bank_open
        || rp.social.guild_bank_open
    {
        notice::defined(ctx, who, TID_GAME_TRADELIMITPC, Vec::new());
        return;
    }
    let session = TradeSession {
        a: requester,
        b: me,
        chars: [rp.ident.character_id, mp.ident.character_id],
        world: mm.core.world,
        layer: mm.core.layer,
        side: [TradeSide::default(), TradeSide::default()],
        revs: [rp.items.content_rev(), mp.items.content_rev()],
        opened_at: ctx.clock.game_ms,
    };
    let initiator = mp.ident.character_id.0;
    svc.econ.trade.drop_requests_of(me);
    svc.econ.trade.drop_requests_of(requester);
    svc.econ.trade.open(session);
    for (obj, other) in [(me, requester), (requester, me)] {
        if let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) {
            p.econ.partner = TradePartner::Player { other };
            // a stale flag of an earlier teleport must not end the new trade
            p.econ.interrupted = false;
        }
        send(
            ctx,
            obj,
            other,
            pb::TradeNtf {
                id_player: initiator,
            },
        );
    }
}

/// `OnTradePut` (Plan/19 §8.2): offers `num` pieces of the inventory entry `obj_id` in trade slot `slot`.
///
/// `slot < 25`, `num >= 1`; the pair is intact and both sides are in the item step (else `TradePutError` to the
/// sender); an expired item is ignored silently. The slot must be free; the item must pass
/// `flyff_rules::item::tradeable` (usable = not reserved, not worn, not a quest item, not bound, not in use, not
/// a guild cloak, not a vagrant ride): `TID_GAME_CANNOTTRADE_ITEM`, or `TID_GAME_CANNOT_DO_USINGITEM` for an item
/// in use. The count is clamped to the stack. Both sides get `TradePutNtf` with the full item (header object = the
/// sender).
pub fn on_trade_put_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::TradePutReq,
) {
    let slot = msg.slot as usize;
    if slot >= MAX_TRADE || msg.num < 1 {
        return;
    }
    let Some((id, me)) = session_of_sender(svc, ctx, who.0) else {
        return;
    };
    let Ok(obj_id) = StorageId::try_from(msg.obj_id) else {
        // an id outside the storage cannot name an item: the legacy `GetItemId` found nothing
        notice::defined(ctx, who, TID_GAME_CANNOTTRADE_ITEM, Vec::new());
        return;
    };
    let Some(s) = svc.econ.trade.session(id) else {
        return;
    };
    let other = s.obj(1 - me);
    let both_item = both_in_item_step(s);
    let slot_free = s.side[me].slots[slot].is_none();
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let Some(p) = ctx.worlds.player(who.0).and_then(|m| m.player()) else {
        return;
    };
    if p.items
        .inventory
        .get_at_id(obj_id)
        .is_some_and(|it| it.is_expired())
    {
        return;
    }
    if !both_item {
        send(ctx, who.0, ObjId::NULL, pb::TradePutError {});
        return;
    }
    // TradeSetItem2
    let refusal = (|| {
        let it = p.items.inventory.get_at_id(obj_id)?;
        if !slot_free || !it.is_usable() {
            return Some(TID_GAME_CANNOTTRADE_ITEM);
        }
        let Some(prop) = env.data().item(it.item_id) else {
            return Some(TID_GAME_CANNOTTRADE_ITEM);
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
        tradeable(&view, prop, env.now.0)
    })();
    let Some(it) = p.items.inventory.get_at_id(obj_id) else {
        notice::defined(ctx, who, TID_GAME_CANNOTTRADE_ITEM, Vec::new());
        return;
    };
    if let Some(tid) = refusal {
        notice::defined(ctx, who, tid, Vec::new());
        return;
    }
    let count = u16::try_from(msg.num.min(i32::from(it.count)).max(1)).unwrap_or(1);
    let offer = TradeOffer {
        obj_id,
        serial: it.serial,
        item_id: it.item_id,
        count,
    };
    let mut shown = it.to_pb(obj_id);
    shown.num = i32::from(count);
    // reserve, record, echo
    if let Some(it) = ctx
        .worlds
        .player_mut(who.0)
        .and_then(|m| m.player_mut())
        .and_then(|p| p.items.inventory.get_at_id_mut(obj_id))
    {
        it.reserved = count;
    }
    if let Some(s) = svc.econ.trade.session_mut(id) {
        s.side[me].slots[slot] = Some(offer);
    }
    for to in [who.0, other] {
        send(
            ctx,
            to,
            who.0,
            pb::TradePutNtf {
                slot: msg.slot,
                item_type: msg.item_type,
                obj_id: msg.obj_id,
                num: i32::from(count),
                item: Some(shown.clone()),
            },
        );
    }
}

/// `OnTradePull` (Plan/19 §8.2): takes the offer in `slot` back (the repack client never sends it).
pub fn on_trade_pull_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::TradePullReq,
) {
    let slot = msg.slot as usize;
    if slot >= MAX_TRADE {
        return;
    }
    let Some((id, me)) = session_of_sender(svc, ctx, who.0) else {
        return;
    };
    let Some(s) = svc.econ.trade.session_mut(id) else {
        return;
    };
    if !both_in_item_step(s) {
        return;
    }
    let other = s.obj(1 - me);
    let Some(offer) = s.side[me].slots[slot].take() else {
        return;
    };
    if let Some(it) = ctx
        .worlds
        .player_mut(who.0)
        .and_then(|m| m.player_mut())
        .and_then(|p| p.items.inventory.get_at_id_mut(offer.obj_id))
        && it.serial == offer.serial
    {
        it.reserved = 0;
    }
    for to in [who.0, other] {
        send(ctx, to, who.0, pb::TradePullNtf { slot: msg.slot });
    }
}

/// `OnTradePutGold` (Plan/19 §8.2, PL-062): offers penya. **FIX:** a second put accumulates; the offer is
/// clamped to the wallet and the penya stay in the wallet (a reservation, E5). Both sides get `TradePutGoldNtf`
/// with the total offered so far.
pub fn on_trade_put_gold_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::TradePutGoldReq,
) {
    let add = msg.gold as i32;
    if add <= 0 {
        return;
    }
    let Some((id, me)) = session_of_sender(svc, ctx, who.0) else {
        return;
    };
    let Some(wallet) = ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| i64::from(p.items.gold))
    else {
        return;
    };
    let Some(s) = svc.econ.trade.session_mut(id) else {
        return;
    };
    if !both_in_item_step(s) {
        return;
    }
    let other = s.obj(1 - me);
    let side = &mut s.side[me];
    side.gold = (side.gold + i64::from(add)).min(wallet);
    let total = u32::try_from(side.gold).unwrap_or(0);
    for to in [who.0, other] {
        send(ctx, to, who.0, pb::TradePutGoldNtf { gold: total });
    }
}

/// `OnTradeOk` (Plan/19 §8.2): the sender is satisfied. Item -> Ok; when the partner is already Ok both get
/// `TradeLastConfirm` (the final box), else both get `TradeOkNtf` (header object = the sender).
pub fn on_trade_ok_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::TradeOkReq,
) {
    let Some((id, me)) = session_of_sender(svc, ctx, who.0) else {
        return;
    };
    let Some(s) = svc.econ.trade.session_mut(id) else {
        return;
    };
    if s.side[me].step != TradeStep::Item {
        return;
    }
    s.side[me].step = TradeStep::Ok;
    let other = s.obj(1 - me);
    let partner_ok = s.side[1 - me].step == TradeStep::Ok;
    for to in [who.0, other] {
        if partner_ok {
            send(ctx, to, ObjId::NULL, pb::TradeLastConfirm {});
        } else {
            send(ctx, to, who.0, pb::TradeOkNtf {});
        }
    }
}

/// `OnTradelastConfrim` (Plan/19 §8.2): the final confirmation. The sender must be Ok; a partner who is Ok makes
/// the sender Confirm (`TradeLastConfirmOk` to both), a partner who already confirmed triggers the commit.
pub fn on_trade_confirm(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::TradeConfirm,
) {
    let Some((id, me)) = session_of_sender(svc, ctx, who.0) else {
        return;
    };
    let Some(s) = svc.econ.trade.session_mut(id) else {
        return;
    };
    if s.side[me].step != TradeStep::Ok {
        return;
    }
    let other = s.obj(1 - me);
    match s.side[1 - me].step {
        TradeStep::Ok => {
            s.side[me].step = TradeStep::Confirm;
            for to in [who.0, other] {
                send(ctx, to, who.0, pb::TradeLastConfirmOk {});
            }
        }
        TradeStep::Confirm => commit::run(svc, ctx, id),
        TradeStep::Item => {}
    }
}

/// `OnTradeCancel` (Plan/19 §8.2, VR-001): the sender's own trade ends; both get `TradeCancelNtf{sender, mode}`.
pub fn on_trade_cancel_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::TradeCancelReq,
) {
    let Some((id, _)) = session_of_sender(svc, ctx, who.0) else {
        return;
    };
    cancel_session(
        svc,
        ctx,
        id,
        Cancel::By {
            player: who.0,
            mode: msg.mode,
        },
    );
}
