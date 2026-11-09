//! The commit helpers of the mail batches (Plan/19 §20.3): a fresh snapshot of a player for an `AtomicBatch`,
//! the ticketed submit, and the inventory-or-mail delivery of the other documents (`create_item_or_mail`,
//! Plan/19 §4.1).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::AddPlan;
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemError, ItemOrigin};
use crate::persist::snapshot::{SnapshotCtx, build};
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{MailId, SaveGen};
use flyff_db::{AtomicBatch, AtomicKind, CharacterSnapshot, DbOp, SaveReason};

/// A fresh snapshot of `who` with a new generation, for the batch of the commit block that is running
/// (13 §17.3 rule 4, §18). `None` when the session is tainted (**never** a snapshot after a panic, 13 §20) or
/// `who` is not a live player; check [`can_snapshot`] before the first mutation.
pub(crate) fn snapshot_for_batch(
    svc: &mut Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
) -> Option<(CharacterSnapshot, SaveGen)> {
    let entry = ctx.sessions.get(who.0)?;
    if entry.tainted {
        return None;
    }
    let m = ctx.worlds.player(who.0)?;
    let generation = svc.saves.next_gen();
    let snap = build(
        m,
        &SnapshotCtx {
            entry,
            generation,
            reason: SaveReason::Atomic,
            now: ctx.clock.game_ms,
            unix_ms: ctx.clock.unix_ms,
            max_pack: u16::try_from(ctx.cfg.rules.gameplay.max_pack_item).unwrap_or(u16::MAX),
            location: None,
        },
    )?;
    Some((snap, generation))
}

/// `true` when [`snapshot_for_batch`] can succeed for `who` (a live, untainted player with a session).
pub(crate) fn can_snapshot(ctx: &GameCtx<'_>, who: PlayerRef) -> bool {
    ctx.sessions.get(who.0).is_some_and(|e| !e.tainted) && ctx.worlds.player(who.0).is_some()
}

/// Queues `batch` with a ticket whose continuation is `on_result`, and tells the save scheduler that a snapshot of
/// `who` with generation `generation` went out (the periodic timer restarts).
pub(crate) fn submit(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    generation: Option<SaveGen>,
    batch: AtomicBatch,
    on_result: impl FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) + Send + 'static,
) {
    let ticket = ctx
        .tickets
        .issue(Pending::Service(ServiceTicket::new(on_result)));
    ctx.persist.atomic(batch.with_ticket(ticket));
    if let Some(g) = generation {
        svc.saves
            .record(who.0, g, ctx.clock.game_ms, SaveReason::Atomic);
    }
}

/// What [`create_item_or_mail`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemDelivery {
    /// The item is in the inventory.
    Inventory(AddPlan),
    /// The inventory was full: the item went out as a system mail.
    Mailed(MailId),
}

/// `create_item_or_mail` (Plan/19 §4.1; normative for docs 15, 17, 20, 22): (1) [`crate::item::ops::create_item`];
/// on success the item is in the inventory (the caller sends its own success text). (2) On `LackSpace` the same
/// `item` (serial unchanged; a template without one gets a fresh serial first) goes to
/// [`MailService::send_system`](super::MailService::send_system) with the title and text resolved from
/// `GameData.texts` (`None` = empty, the legacy default), submitted as ONE `AtomicBatch{MailSend, [snap(who)],
/// [InsertMail, RaiseCounter]}`; audit `mail.send{sender: 0}`; an online `who` gets `MODE_MAILBOX` at the end of
/// the tick. (3) `NoProp` / `NotFound` come back unchanged and nothing is mailed.
pub fn create_item_or_mail(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    item: Item,
    origin: ItemOrigin,
    title_tid: Option<u32>,
    text_tid: Option<u32>,
) -> Result<ItemDelivery, ItemError> {
    let data = ctx.data;
    let text = |tid: Option<u32>| tid.map_or("", |t| data.texts.get(t));
    create_item_or_mail_text(svc, ctx, who, item, origin, text(title_tid), text(text_tid))
}

/// [`create_item_or_mail`] with the title and the body of the mail already resolved (the mini-games mail a prize
/// titled with the item's name; WP-EVENT-20). The texts are used only on the mail path.
pub fn create_item_or_mail_text(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    item: Item,
    origin: ItemOrigin,
    title: &str,
    body: &str,
) -> Result<ItemDelivery, ItemError> {
    match crate::item::ops::create_item(svc, ctx, who, item.clone(), origin) {
        Ok(plan) => return Ok(ItemDelivery::Inventory(plan)),
        Err(ItemError::LackSpace) => {}
        Err(e) => return Err(e),
    }
    let Some(character) = ctx
        .player(who)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return Err(ItemError::NotFound);
    };
    let mut item = item;
    if item.serial.0 == 0 {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        item.serial =
            flyff_common::ItemSerial(crate::item::container::SerialSource::next_serial(&mut src)?);
    }
    let (id, ops) = svc
        .econ
        .mail
        .send_system_with_id(character, title, body, Some(item), 0);
    let record = svc.econ.mail.get(id).map(super::MailEntry::to_record);
    let snap = snapshot_for_batch(svc, ctx, who);
    let (snapshots, generation) = match snap {
        Some((s, g)) => (vec![s], Some(g)),
        None => (Vec::new(), None),
    };
    let ops: Vec<DbOp> = ops.into();
    let batch = AtomicBatch {
        snapshots,
        ops,
        ..AtomicBatch::of_ops(AtomicKind::MailSend, Vec::new())
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
        let mut ev = flyff_db::audit::econ::mail_send(&rec, 0);
        if let Some(actor) = crate::audit::actor_of(ctx, who) {
            ev = ev.by(actor);
        }
        ctx.audit.emit(ev);
    }
    Ok(ItemDelivery::Mailed(id))
}

/// The continuation of a `MailSend` batch: a failure is a persistence fault (Plan/19 §20.3): the game does not
/// compensate, it forgets the mail that the database does not have (so that later ops on it cannot fail
/// forever) and logs everything an operator needs.
pub(crate) fn on_send_result(svc: &mut Services, id: MailId, result: TicketResult) {
    match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => {
            let lost = svc.econ.mail.remove(id);
            tracing::error!(
                mail = id.0,
                error = %e,
                lost = ?lost.as_ref().map(|m| (m.receiver.0, m.sender.0, m.gold, m.item.as_ref().map(crate::item::fx::item_json))),
                "MailSend batch failed: the mail does not exist in the database"
            );
        }
        other => tracing::error!(
            mail = id.0,
            ?other,
            "MailSend completed with an unexpected result"
        ),
    }
}
