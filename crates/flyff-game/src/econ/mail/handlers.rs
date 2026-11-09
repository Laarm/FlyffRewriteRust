//! The mail handlers (Plan/19 §11.2-§11.5): `QueryPostMail`, `QueryMailBoxReq`, `ReadMail`, `QueryRemoveMail`,
//! `QueryGetMailItem` and `QueryGetMailGold`.
//!
//! The three handlers that move an attachment (send, take item, take gold) are multi-party commit blocks
//! (VR-018): everything that can refuse is checked first, then ONE panic-free block changes the in-memory state
//! and queues ONE `AtomicBatch` whose snapshot is built after the change. The messages and the audit lines
//! follow in the same block.

use super::commit::{can_snapshot, on_send_result, snapshot_for_batch, submit};
use super::{
    MAIL_SEND_MIN_INTERVAL_MS, MAILBOX_QUERY_MIN_INTERVAL_MS, MAX_MAIL, MAX_TEXT_BYTES,
    MAX_TITLE_BYTES, MailEntry, POSTAGE, clip, refresh_envelope, remove_kind,
};
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::{SerialSource, StorageId};
use crate::item::fx::Fx;
use crate::item::gold::{MAIL_GOLD_CAP, can_add, core::add_gold};
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemOrigin, RemoveReason, core as ops};
use crate::tickets::TicketResult;
use flyff_common::defs::itemkind::IK3_QUEST;
use flyff_common::defs::neuz::MMI_POST;
use flyff_common::defs::text::{
    TID_GAME_CANNOT_POST, TID_GAME_LACKMONEY, TID_GAME_LACKSPACE, TID_GAME_MAILBOX_FULL,
    TID_GAME_MSGSELFSENDERROR, TID_MAIL_SEND_OK, TID_MAIL_UNKNOW,
};
use flyff_common::{ItemSerial, MailId};
use flyff_data::GameData;
use flyff_db::audit::econ as audit;
use flyff_db::model::SYSTEM_SENDER;
use flyff_db::{AtomicBatch, CharacterState, DbOp, MailRecord, MailStateRecord};
use flyff_proto::pb;
use flyff_rules::chat::TEXT_DIAG;
use flyff_rules::item::{get_cost, mail_storage_fee};

/// `AddDiagText(prj.GetText(tid))`: the text resolved on the server, shown as a message box.
fn diag(fx: &mut Fx, data: &GameData, tid: u32) {
    fx.out.to_self(pb::Text {
        kind: TEXT_DIAG,
        text: data.texts.get(tid).to_owned(),
    });
}

/// The audit event of `ev` with the actor context of `who`.
fn emit(ctx: &GameCtx<'_>, who: PlayerRef, ev: flyff_db::AuditEvent) {
    let ev = match crate::audit::actor_of(ctx, who) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
}

/// The character of a living player.
fn receiver_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<flyff_common::CharacterId> {
    let m = ctx.player(who)?;
    if is_die(m) {
        return None;
    }
    Some(m.player()?.ident.character_id)
}

// ---- send -----------------------------------------------------------------------------------------------

/// What the sender attaches, decided on the immutable state.
struct Attachment {
    id: StorageId,
    /// The mailed part (count already clamped, runtime fields cleared).
    item: Item,
    /// The stack is split: the mailed part gets a fresh serial.
    split: bool,
}

/// `QueryPostMail{obj_id, num, receiver, gold, title, text}` (Plan/19 §11.2; ref/07b §16.1; PL-157, VR-010,
/// VR-012, VR-029). Validated in order: `gold < 0 || gold + 500 <= 0` ignored; a trade partner, a personal shop or
/// a bank session ignored; less than 1 000 ms since the last accepted send ignored (VR-029); an unknown or
/// inactive receiver `TID_MAIL_UNKNOW`; oneself `TID_GAME_MSGSELFSENDERROR`; no `MMI_POST` NPC within range
/// silent; a box of 50 `TID_GAME_MAILBOX_FULL` (the receiver's name) (**FIX** PL-157: for every receiver, online or not); an
/// attached item that is reserved, worn, a quest item, bound, in use, a guild cloak, a vagrant ride or charged
/// `TID_GAME_CANNOT_POST` (`num <= 0` ignored, `num` clamped to the stack); too little penya `TID_GAME_LACKMONEY`.
/// Commit: the penya and the postage go, the item leaves the bag (a split gets a fresh serial), the mail is
/// stored in memory and ONE `MailSend` batch (the sender's snapshot + `InsertMail` + `RaiseCounter`) is queued;
/// the sender hears `TID_MAIL_SEND_OK` (a message box), the receiver's envelope follows at the end of the tick.
pub fn on_post(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::QueryPostMail) {
    let gold = i64::from(msg.gold);
    if gold < 0 || gold + POSTAGE <= 0 {
        return;
    }
    let core = svc.items.handle();
    let now = ctx.clock.unix_ms.to_unix_time();
    let env = core.env(now);
    let now_ms = ctx.clock.game_ms;
    let sender = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        if is_die(m) || p.econ.has_partner() || p.econ.vending || p.econ.bank.is_some() {
            return;
        }
        if p.econ
            .mail_last_send
            .is_some_and(|t| now_ms.0.saturating_sub(t.0) < MAIL_SEND_MIN_INTERVAL_MS)
        {
            return;
        }
        p.ident.character_id
    };
    if !can_snapshot(ctx, who) {
        return;
    }
    if let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        p.econ.mail_last_send = Some(now_ms);
    }
    let mut fx = Fx::new();
    // the receiver
    let receiver = svc
        .presence
        .by_name(&msg.receiver)
        .filter(|e| e.state == CharacterState::Active)
        .map(|e| e.id);
    let Some(receiver) = receiver else {
        diag(&mut fx, env.data(), TID_MAIL_UNKNOW);
        fx.flush(svc, ctx, who);
        return;
    };
    if receiver == sender {
        diag(&mut fx, env.data(), TID_GAME_MSGSELFSENDERROR);
        fx.flush(svc, ctx, who);
        return;
    }
    {
        let Some(m) = ctx.player(who) else { return };
        if ctx
            .worlds
            .close_npc(MMI_POST, m.core.world, m.core.layer, m.core.pos)
            .is_none()
        {
            return;
        }
    }
    if svc.econ.mail.box_len(receiver) >= MAX_MAIL {
        fx.defined(TID_GAME_MAILBOX_FULL, vec![msg.receiver.clone()]);
        fx.flush(svc, ctx, who);
        return;
    }
    // the attachment and the penya
    let attach: Option<Attachment> = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        let found = (msg.obj_id != 0xFF)
            .then(|| StorageId::try_from(msg.obj_id).ok())
            .flatten()
            .and_then(|id| p.items.inventory.get_at_id(id).map(|it| (id, it)));
        let mut attach = None;
        if let Some((id, it)) = found {
            let Some(prop) = env.data().item(it.item_id) else {
                diag(&mut fx, env.data(), TID_GAME_CANNOT_POST);
                fx.flush(svc, ctx, who);
                return;
            };
            let refused = !it.is_usable()
                || p.items.inventory.is_equip(id)
                || prop.item_kind3 == IK3_QUEST
                || it.is_binds(prop, env.cfg().only_armor_awakes)
                || p.items.is_using(id)
                || it.is_guild_cloak(prop)
                || Item::is_vagrant_ride(prop);
            if refused {
                diag(&mut fx, env.data(), TID_GAME_CANNOT_POST);
                fx.flush(svc, ctx, who);
                return;
            }
            if msg.num <= 0 {
                return; // hacking
            }
            let n = u16::try_from(msg.num).unwrap_or(u16::MAX).min(it.count);
            if it.is_charged() {
                diag(&mut fx, env.data(), TID_GAME_CANNOT_POST);
                fx.flush(svc, ctx, who);
                return;
            }
            let mut mailed = it.clone();
            mailed.count = n;
            mailed.reserved = 0;
            mailed.vendor_price = 0;
            attach = Some(Attachment {
                id,
                item: mailed,
                split: n < it.count,
            });
        }
        if i64::from(p.items.gold) < POSTAGE + gold || gold > MAIL_GOLD_CAP {
            diag(&mut fx, env.data(), TID_GAME_LACKMONEY);
            fx.flush(svc, ctx, who);
            return;
        }
        attach
    };
    // ---- the commit block: nothing below can refuse ----
    let Services { serials, econ, .. } = &mut *svc;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    let fresh = match attach.as_ref().filter(|a| a.split) {
        Some(_) => match src.next_serial() {
            Ok(s) => Some(ItemSerial(s)),
            Err(e) => {
                tracing::warn!(error = %e, "mail send: no serial for the split stack");
                return;
            }
        },
        None => None,
    };
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    add_gold(m, &env, -(POSTAGE + gold), true, &mut fx);
    let mut mail_item = None;
    if let Some(a) = attach {
        ops::remove_item(m, &env, a.id, a.item.count, RemoveReason::Moved, &mut fx);
        let mut it = a.item;
        if let Some(s) = fresh {
            it.serial = s;
        }
        mail_item = Some(it);
    }
    let id = econ.mail.alloc_id();
    let entry = MailEntry {
        id,
        receiver,
        sender,
        title: clip(&msg.title, MAX_TITLE_BYTES),
        body: clip(&msg.text, MAX_TEXT_BYTES),
        gold,
        item: mail_item,
        created_at: now,
        read: false,
    };
    let record = entry.to_record();
    econ.mail.insert(entry);
    econ.mail.notify(receiver);
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        tracing::error!(
            mail = id.0,
            "mail send: no snapshot of the sender (unreachable)"
        );
        return;
    };
    let batch = AtomicBatch::mail_send(snap, record.clone());
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        move |svc, _ctx, result| {
            on_send_result(svc, id, result);
        },
    );
    emit(ctx, who, audit::mail_send(&record, POSTAGE));
    diag(&mut fx, env.data(), TID_MAIL_SEND_OK);
    fx.flush(svc, ctx, who);
}

// ---- box, read, delete ----------------------------------------------------------------------------------

/// `QueryMailBoxReq` (Plan/19 §11.3; ref/07b §16.4; VR-029): the whole box as `QueryMailBoxNtf{id_receiver,
/// mails}`; at most one per 3 000 ms (the excess is ignored).
pub fn on_query_box(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::QueryMailBoxReq,
) {
    let now_ms = ctx.clock.game_ms;
    let Some(receiver) = receiver_of(ctx, who) else {
        return;
    };
    {
        let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) else {
            return;
        };
        if p.econ
            .mail_last_query
            .is_some_and(|t| now_ms.0.saturating_sub(t.0) < MAILBOX_QUERY_MIN_INTERVAL_MS)
        {
            return;
        }
        p.econ.mail_last_query = Some(now_ms);
    }
    let mut fx = Fx::new();
    fx.out.to_self(pb::QueryMailBoxNtf {
        id_receiver: receiver.0,
        mails: svc
            .econ
            .mail
            .box_of(receiver)
            .map(MailEntry::to_pb)
            .collect(),
    });
    fx.flush(svc, ctx, who);
}

/// `ReadMail{mail_id}` (Plan/19 §11.3): the own mail becomes read (one `UpdateMail` op), the answer is
/// `RemoveMail{mail_id, READ}`, and the envelope goes when no unread mail is left. An unknown or foreign mail is
/// ignored.
pub fn on_read(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::ReadMail) {
    let Some(receiver) = receiver_of(ctx, who) else {
        return;
    };
    let id = MailId(msg.mail_id);
    let now = ctx.clock.unix_ms.to_unix_time();
    if svc.econ.mail.get_of(receiver, id).is_none() {
        return;
    }
    let Some(entry) = svc.econ.mail.get_mut(id) else {
        return;
    };
    let newly = !entry.read;
    entry.read = true;
    if newly {
        ctx.persist
            .op(DbOp::UpdateMail(MailStateRecord::read(id, now)));
    }
    let mut fx = Fx::new();
    fx.out.to_self(pb::RemoveMail {
        mail_id: id.0,
        kind: remove_kind::READ,
    });
    fx.flush(svc, ctx, who);
    refresh_envelope(svc, ctx, receiver);
}

/// `QueryRemoveMail{mail_id}` (Plan/19 §11.3): the own mail is soft-deleted with whatever it still holds (KEEP),
/// one `UpdateMail{deleted_at}` op, audit `mail.delete`, answer `RemoveMail{mail_id, MAIL}`; the envelope goes
/// when no unread mail is left. An unknown or foreign mail is ignored.
pub fn on_remove(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::QueryRemoveMail,
) {
    let Some(receiver) = receiver_of(ctx, who) else {
        return;
    };
    let id = MailId(msg.mail_id);
    if svc.econ.mail.get_of(receiver, id).is_none() {
        return;
    }
    let now = ctx.clock.unix_ms.to_unix_time();
    let Some(entry) = svc.econ.mail.remove(id) else {
        return;
    };
    ctx.persist
        .op(DbOp::UpdateMail(MailStateRecord::delete(id, now)));
    emit(ctx, who, audit::mail_delete(&entry.to_record()));
    let mut fx = Fx::new();
    fx.out.to_self(pb::RemoveMail {
        mail_id: id.0,
        kind: remove_kind::MAIL,
    });
    fx.flush(svc, ctx, who);
    refresh_envelope(svc, ctx, receiver);
}

// ---- take -----------------------------------------------------------------------------------------------

/// `QueryGetMailItem{mail_id}` (Plan/19 §11.4; PL-073, VR-018, 13 OQ11).
///
/// The bag needs one empty slot (`TID_GAME_LACKSPACE`, a message box). The own mail must hold an item; the
/// storage fee is computed from full timestamps ([`mail_storage_fee`]: 0 for 7 days, then an eighth of the
/// price per day); a fee above the wallet answers `TID_GAME_LACKMONEY`. Commit: the item is created in the bag
/// with its serial and all its data (a mailed pet keeps its name, its vis keep times and the transform flag,
/// PL-054), the fee is paid, the mail loses the item, ONE `MailTakeItem` batch (the receiver's snapshot +
/// `UpdateMail{take_item_at}`) is queued, `RemoveMail{mail_id, ITEM}`, audit `mail.take_item{fee}`. The
/// database refuses the batch (`Conflict`) when its mail row no longer holds the item: the item can never be
/// in the bag and on the mail at once.
pub fn on_take_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::QueryGetMailItem,
) {
    let core = svc.items.handle();
    let now = ctx.clock.unix_ms.to_unix_time();
    let env = core.env(now);
    let Some(receiver) = receiver_of(ctx, who) else {
        return;
    };
    let mut fx = Fx::new();
    let wallet = {
        let Some(p) = ctx.player(who).and_then(|m| m.player()) else {
            return;
        };
        if p.items.inventory.empty_count() < 1 {
            diag(&mut fx, env.data(), TID_GAME_LACKSPACE);
            fx.flush(svc, ctx, who);
            return;
        }
        i64::from(p.items.gold)
    };
    let id = MailId(msg.mail_id);
    let Some(entry) = svc.econ.mail.get_of(receiver, id) else {
        return;
    };
    let Some(item) = entry.item.clone() else {
        return;
    };
    let before = entry.to_record();
    let elapsed = now.0 - entry.created_at.0;
    let fee = env.data().item(item.item_id).map_or(0, |prop| {
        i64::from(mail_storage_fee(
            get_cost(prop, item.upgrade_level),
            elapsed,
        ))
    });
    if fee > wallet {
        diag(&mut fx, env.data(), TID_GAME_LACKMONEY);
        fx.flush(svc, ctx, who);
        return;
    }
    if !can_snapshot(ctx, who) {
        return;
    }
    // ---- the commit block ----
    {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return;
        };
        // the item first: the only step that can fail (no prop, no serial), nothing has changed then
        if let Err(e) = ops::create_item(m, &env, &mut src, item, ItemOrigin::MailTake, &mut fx) {
            tracing::warn!(mail = id.0, error = %e, "mail take: the item was not created");
            return;
        }
        if fee > 0 {
            add_gold(m, &env, -fee, true, &mut fx);
        }
    }
    if let Some(e) = svc.econ.mail.get_mut(id) {
        e.item = None;
    }
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        tracing::error!(
            mail = id.0,
            "mail take: no snapshot of the receiver (unreachable)"
        );
        return;
    };
    let batch = AtomicBatch::mail_take_item(snap, id, now);
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        move |_svc, _ctx, result| {
            on_take_result("MailTakeItem", id, result);
        },
    );
    emit(ctx, who, audit::mail_take_item(&before, fee));
    fx.out.to_self(pb::RemoveMail {
        mail_id: id.0,
        kind: remove_kind::ITEM,
    });
    fx.flush(svc, ctx, who);
}

/// `QueryGetMailGold{mail_id}` (Plan/19 §11.5): the own mail must hold penya. When the wallet can take them
/// (`CanAdd`) they are added, otherwise the penya are **re-mailed** as a new system mail (empty title and text,
/// KEEP, src:WORLDSERVER/DPDatabaseClient.cpp:2780-2815); either way the original mail's penya become 0. ONE
/// `MailTakeGold` batch: the receiver's snapshot + `UpdateMail{take_gold_at}` (+ `InsertMail` +
/// `RaiseCounter` for the re-mail); `RemoveMail{mail_id, GOLD}`; audit `mail.take_gold`.
pub fn on_take_gold(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::QueryGetMailGold,
) {
    let core = svc.items.handle();
    let now = ctx.clock.unix_ms.to_unix_time();
    let env = core.env(now);
    let Some(receiver) = receiver_of(ctx, who) else {
        return;
    };
    let id = MailId(msg.mail_id);
    let Some(gold) = svc
        .econ
        .mail
        .get_of(receiver, id)
        .map(|e| e.gold)
        .filter(|g| *g > 0)
    else {
        return;
    };
    let wallet = ctx
        .player(who)
        .and_then(|m| m.player())
        .map_or(0, |p| i64::from(p.items.gold));
    if !can_snapshot(ctx, who) {
        return;
    }
    // ---- the commit block ----
    let mut fx = Fx::new();
    let mut remail: Option<MailRecord> = None;
    if can_add(wallet, gold) {
        if let Some(m) = ctx.worlds.player_mut(who.0) {
            add_gold(m, &env, gold, true, &mut fx);
        }
    } else {
        let entry = MailEntry {
            id: svc.econ.mail.alloc_id(),
            receiver,
            sender: SYSTEM_SENDER,
            title: String::new(),
            body: String::new(),
            gold,
            item: None,
            created_at: now,
            read: false,
        };
        remail = Some(entry.to_record());
        svc.econ.mail.insert(entry);
        svc.econ.mail.notify(receiver);
    }
    if let Some(e) = svc.econ.mail.get_mut(id) {
        e.gold = 0;
    }
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        tracing::error!(
            mail = id.0,
            "mail take: no snapshot of the receiver (unreachable)"
        );
        return;
    };
    let remail_id = remail.as_ref().map(|r| r.id);
    let batch = AtomicBatch::mail_take_gold(snap, id, now, remail);
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        move |svc, _ctx, result| {
            if let (Some(rid), TicketResult::Failed(_)) = (remail_id, &result) {
                // the re-mail is not in the database: forget it
                let _ = svc.econ.mail.remove(rid);
            }
            on_take_result("MailTakeGold", id, result);
        },
    );
    emit(ctx, who, audit::mail_take_gold(id, receiver, gold));
    fx.out.to_self(pb::RemoveMail {
        mail_id: id.0,
        kind: remove_kind::GOLD,
    });
    fx.flush(svc, ctx, who);
}

/// The continuation of a take batch. A `Failed` batch (the database rolled the receiver's snapshot back with the
/// mail op: a stale game state, `Conflict`; or a persistence fault) is logged and **not** compensated (Plan/19
/// §20.3).
fn on_take_result(kind: &str, id: MailId, result: TicketResult) {
    match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => {
            tracing::error!(mail = id.0, kind, error = %e, "mail take batch failed (not compensated)");
        }
        other => tracing::error!(
            mail = id.0,
            kind,
            ?other,
            "unexpected result of a mail take"
        ),
    }
}
