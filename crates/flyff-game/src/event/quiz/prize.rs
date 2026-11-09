//! The prize of the quiz (Plan/22 §2.7, §15.3 step 6): mail only. Every survivor of the last round gets
//! `PrizeItemId x PrizeItemNum` by system mail (`TID_GAME_QUIZ_PRIZEITEM_TITLE` / `_TEXT`, sender 0, keep time 0:
//! `DPDatabaseClient` `SendPostPrizeItem`); ONE `AtomicBatch::quiz_prize` per round end carries the mails, the counter
//! and the used marks of the questions asked so far (also with zero survivors or an unusable prize: then only the marks).

use crate::ctx::{GameCtx, Services};
use crate::econ::mail::MailEntry;
use crate::item::container::SerialSource;
use crate::item::model::Item;
use crate::item::ops::CtxSerials;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::defs::text::{TID_GAME_QUIZ_PRIZEITEM_TEXT, TID_GAME_QUIZ_PRIZEITEM_TITLE};
use flyff_common::{CharacterId, ItemSerial, MailId, ObjId};
use flyff_db::AtomicBatch;
use flyff_db::audit::content as audit;
use flyff_db::model::MailRecord;

/// Pays the survivors and marks the asked questions used, in one batch; audits `quiz.winner` for every survivor.
pub(super) fn award(s: &mut Services, ctx: &mut GameCtx<'_>, survivors: &[ObjId]) {
    let Some(cfg) = s.event.quiz.settings.clone() else {
        return;
    };
    let event_id = s.event.quiz.event_id;
    let used = std::mem::take(&mut s.event.quiz.asked_unmarked);
    let prize_known =
        cfg.prize_item != flyff_data::types::NULL_ID && ctx.data.item(cfg.prize_item).is_some();
    let count = u16::try_from(cfg.prize_count.clamp(0, i32::from(u16::MAX))).unwrap_or(0);
    let prize = prize_known && count > 0;
    if !prize {
        tracing::warn!(
            item = cfg.prize_item,
            count = cfg.prize_count,
            "quiz: no usable prize (QuizEvent.lua PrizeItemId / PrizeItemNum): the winners get no mail"
        );
    }
    let title = ctx.data.texts.get(TID_GAME_QUIZ_PRIZEITEM_TITLE).to_owned();
    let body = ctx.data.texts.get(TID_GAME_QUIZ_PRIZEITEM_TEXT).to_owned();

    let mut mails: Vec<MailId> = Vec::new();
    for obj in survivors {
        let Some(character) = ctx
            .worlds
            .player(*obj)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            continue;
        };
        if prize
            && let Some(id) = mail_prize(s, ctx, character, cfg.prize_item, count, &title, &body)
        {
            mails.push(id);
        }
        let (item, n) = if prize {
            (cfg.prize_item, u32::from(count))
        } else {
            (0, 0)
        };
        ctx.audit
            .emit(audit::quiz_winner(event_id, character, item, n));
    }
    let records: Vec<MailRecord> = mails
        .iter()
        .filter_map(|id| s.econ.mail.get(*id).map(MailEntry::to_record))
        .collect();
    if records.is_empty() && used.is_empty() {
        return;
    }
    let batch = AtomicBatch::quiz_prize(records, used, ctx.clock.unix_ms.to_unix_time());
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => {
                for id in &mails {
                    svc.econ.mail.remove(*id);
                }
                tracing::error!(
                    error = %e,
                    event = event_id,
                    "CRITICAL: the quiz prize batch failed: the prize mails are gone again and the questions stay unmarked"
                );
            }
            other => tracing::error!(?other, "the quiz prize batch completed unexpectedly"),
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
}

/// One prize mail with a fresh item serial (memory; the batch carries the record). No serial: that mail is skipped.
fn mail_prize(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    character: CharacterId,
    prize_item: u32,
    count: u16,
    title: &str,
    body: &str,
) -> Option<MailId> {
    let mut item = Item::new(prize_item, count);
    let Services { serials, .. } = &mut *s;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    match SerialSource::next_serial(&mut src) {
        Ok(serial) => item.serial = ItemSerial(serial),
        Err(e) => {
            tracing::error!(error = %e, character = character.0, "quiz: no item serial for the prize mail");
            return None;
        }
    }
    let (id, _) = s
        .econ
        .mail
        .send_system_with_id(character, title, body, Some(item), 0);
    Some(id)
}
