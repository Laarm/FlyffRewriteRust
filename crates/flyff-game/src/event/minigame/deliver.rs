//! Prize and penya delivery of the NPC mini-games (Plan/22 §16, §2.7; src:_Common/MiniGame.cpp).
//!
//! * `give_prize`: a prize item goes to the inventory, else to the mailbox titled with the item's name
//!   (`create_item_or_mail_text`), with `TID_GAME_MINIGAME_ITEM_POST` on the mail path. The item carries the
//!   `charged` flag and the endurance of its prop explicitly (legacy `m_nHitPoint = dwEndurance`): `Item::new`'s -1
//!   is filled only on the inventory path, so the mailed copy would keep it.
//! * `credit_penya`: a positive credit that would pass the wallet limit goes by mail (`TID_GAME_SELLING_COST_0`
//!   with the penya attached, one `Other("minigame_penya")` batch beside the player's snapshot) with
//!   `TID_GAME_MINIGAME_PENYA_POST`; every other gold move is `add_gold` with the message, and an urgent save.
//! * `audit_bet`: the `minigame.bet` audit line.

use super::NpcMiniGameService;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{
    ItemDelivery, can_snapshot, create_item_or_mail_text, on_send_result, snapshot_for_batch,
    submit,
};
use crate::item::fx::Fx;
use crate::item::gold::{add_gold, can_add};
use crate::item::model::Item;
use crate::item::ops::ItemOrigin;
use flyff_common::defs::text::{
    TID_GAME_MINIGAME_ITEM_POST, TID_GAME_MINIGAME_PENYA_POST, TID_GAME_SELLING_COST_0,
    TID_MMI_FIVESYSTEM,
};
use flyff_db::audit::ev;
use flyff_db::{AtomicBatch, AtomicKind};
use serde_json::{Value, json};

/// Where a prize item went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Into the inventory.
    Inventory,
    /// The inventory was full: a system mail.
    Mailed,
}

/// Where a penya credit went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Credit {
    /// Into the wallet.
    Wallet,
    /// The wallet would overflow (or the credit is for an offline wallet): a system mail.
    Mailed,
}

/// The prize item `id` x `count` with the flags of its prop, or `None` (one warning per item) when the item has no
/// prop: such an entry never yields a prize (the original dereferenced null).
pub(crate) fn prize_item(
    games: &mut NpcMiniGameService,
    data: &flyff_data::GameData,
    id: u32,
    count: i32,
) -> Option<Item> {
    let Some(prop) = data.item(id) else {
        if games.warn_once(id) {
            tracing::warn!(
                item = id,
                "mini-game prize: the item has no prop; no prize is given"
            );
        }
        return None;
    };
    let mut item = Item::new(
        id,
        u16::try_from(count.clamp(1, i32::from(u16::MAX))).unwrap_or(1),
    );
    item.charged = prop.charged != 0;
    item.endurance = i32::try_from(prop.endurance).unwrap_or(i32::MAX);
    Some(item)
}

/// Gives the prize `id` x `count` to `who`: the inventory, else the mailbox (title: the item's name, body: the text
/// `body_tid` of the game). The mail path also queues `TID_GAME_MINIGAME_ITEM_POST` on `fx`. `None` when nothing was
/// given (no prop, the player is gone).
pub(crate) fn give_prize(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    id: u32,
    count: i32,
    body_tid: u32,
    fx: &mut Fx,
) -> Option<Delivery> {
    let data = ctx.data;
    let item = prize_item(&mut svc.event.minigames, data, id, count)?;
    let title = data.item(id).map_or("", |p| p.name.as_str());
    let body = data.texts.get(body_tid);
    match create_item_or_mail_text(svc, ctx, who, item, ItemOrigin::Event, title, body) {
        Ok(ItemDelivery::Inventory(_)) => Some(Delivery::Inventory),
        Ok(ItemDelivery::Mailed(_)) => {
            fx.text(TID_GAME_MINIGAME_ITEM_POST);
            Some(Delivery::Mailed)
        }
        Err(e) => {
            tracing::warn!(item = id, error = %e, "mini-game prize: not delivered");
            None
        }
    }
}

/// Credits `amount` penya to `who` (never call it with a debit: a negative amount goes through `add_gold` as it is).
/// A positive amount that does not fit the wallet is mailed instead; the urgent save is requested on `fx`.
pub(crate) fn credit_penya(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    amount: i64,
    fx: &mut Fx,
) -> Credit {
    let gold = ctx
        .player(who)
        .and_then(|m| m.player())
        .map_or(0, |p| i64::from(p.items.gold));
    if amount > 0 && !can_add(gold, amount) && mail_penya(svc, ctx, who, amount) {
        fx.text(TID_GAME_MINIGAME_PENYA_POST);
        return Credit::Mailed;
    }
    add_gold(svc, ctx, who, amount, true);
    fx.mark_urgent();
    Credit::Wallet
}

/// The overflow mail: `amount` penya from the system, in ONE `Other("minigame_penya")` batch with the player's
/// snapshot. `false` when the mail cannot be written (no live player, tainted session): the caller then pays the
/// wallet, which clamps at the limit.
fn mail_penya(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, amount: i64) -> bool {
    let Some(character) = ctx
        .player(who)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return false;
    };
    if !can_snapshot(ctx, who) {
        return false;
    }
    let data = ctx.data;
    let (id, ops) = svc.econ.mail.send_system_with_id(
        character,
        data.texts.get(TID_GAME_SELLING_COST_0),
        data.texts.get(TID_MMI_FIVESYSTEM),
        None,
        amount,
    );
    let record = svc
        .econ
        .mail
        .get(id)
        .map(crate::econ::mail::MailEntry::to_record);
    let (snapshots, generation) = match snapshot_for_batch(svc, ctx, who) {
        Some((s, g)) => (vec![s], Some(g)),
        None => (Vec::new(), None),
    };
    let batch = AtomicBatch {
        snapshots,
        ops: ops.into(),
        ..AtomicBatch::of_ops(AtomicKind::Other("minigame_penya"), Vec::new())
    };
    submit(
        svc,
        ctx,
        who,
        generation,
        batch,
        move |svc, _ctx, result| {
            on_send_result(svc, id, result);
        },
    );
    if let Some(rec) = record {
        let mut event = flyff_db::audit::econ::mail_send(&rec, 0);
        if let Some(actor) = crate::audit::actor_of(ctx, who) {
            event = event.by(actor);
        }
        ctx.audit.emit(event);
    }
    true
}

/// The `minigame.bet` line: `{game, op, ...fields}` for stakes, results, refunds and the alphabet fee (Plan/22 §2.8).
pub(crate) fn audit_bet(fx: &mut Fx, game: &str, op: &str, fields: Value) {
    let mut data = json!({ "game": game, "op": op });
    if let (Some(map), Value::Object(extra)) = (data.as_object_mut(), fields) {
        map.extend(extra);
    }
    fx.audit(ev::MINIGAME_BET, data);
}
