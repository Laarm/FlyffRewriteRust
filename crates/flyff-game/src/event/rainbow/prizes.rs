//! The prizes of the rainbow race (Plan/22 §14.4, §14.5, §2.7; `CRainbowRaceMng::SetPrize`, `SetMiniGamePrize`).
//!
//! **Mini-game prizes.** Every completed game queues `(racer, games done so far, game ms)`. The queue is worked once a
//! second in Progress for the entries at least 15 s old, and completely at the end of the race. The row is
//! `RainbowRace.lua tMiniGamePrize[weekday][count]` of the weekday **at the delivery** (the legacy asked Lua, which read
//! `os.date("%a")` of the call); the shipped script has rows for 4-7 on Saturday only. The prize is a system mail
//! (title = text = `TID_GAME_RAINBOWRACE_MINIGAMEPRIZE`, no gold) to the racer, online or not (mail-only reward, KEEP),
//! one `Other("rainbow_minigame_prize")` batch per mail. An unknown item prop or a count outside `1..=u16::MAX` skips the
//! prize with a warning (FIX: the legacy dereferenced null, PL-105).
//!
//! **Rank prizes.** At the end, for rank `r` over the finishers: `penya = cvttsd2si(row.penya * applicants)` where the
//! applicants are the racers left at the end ([`cvttsd2si`] is the x86 conversion: an out-of-range product is
//! `i32::MIN`, then `< 0` is 0 - KEEP-QUIRK), the item (if any) keeps flag **0** whatever the script says (KQ-19), one
//! mail to the finisher even without penya and item. They ride in the ONE `rainbow_result` batch of [`finish_round`].

use super::{RainbowRaceService, RrState};
use crate::ctx::{GameCtx, Services};
use crate::econ::mail::MailEntry;
use crate::econ::mail::commit::on_send_result;
use crate::item::container::SerialSource;
use crate::item::model::{Item, ItemFlags};
use crate::item::ops::CtxSerials;
use crate::item::upgrade::random_option::{Arg, format_text};
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::defs::text::{
    TID_GAME_RAINBOWRACE_MINIGAMEPRIZE, TID_GAME_RAINBOWRACE_POSTPRIZE_TEXT_ITEM,
    TID_GAME_RAINBOWRACE_POSTPRIZE_TEXT_PENYA, TID_GAME_RAINBOWRACE_POSTPRIZE_TEXT_RANKING,
    TID_GAME_RAINBOWRACE_POSTPRIZE_TITLE,
};
use flyff_common::{CharacterId, ItemSerial, MailId};
use flyff_db::audit::content as audit;
use flyff_db::model::MailRecord;
use flyff_db::{AtomicBatch, AtomicKind, RainbowEntryState, RainbowRaceEntryRecord};
use flyff_rules::consts::NULL_ID;

/// x86 `cvttsd2si` as the legacy `static_cast<int>(double)` compiled to: truncation toward zero; NaN, infinities and
/// values outside the `i32` range give `i32::MIN` (`0x80000000`). Never Rust's saturating `as` (KEEP-QUIRK).
pub fn cvttsd2si(x: f64) -> i32 {
    if x.is_finite() && x > -2_147_483_649.0 && x < 2_147_483_648.0 {
        x.trunc() as i32
    } else {
        i32::MIN
    }
}

/// The penya of one rank: `(int)(nPenya * applicants)`, a negative result (the overflow) is 0.
pub fn rank_penya(row_penya: f64, applicants: usize) -> i32 {
    cvttsd2si(row_penya * applicants as f64).max(0)
}

/// The mail item for `item_id` x `count`: flags `flag`, `charged` and the endurance of the prop, a fresh serial.
/// `None` (with a warning) for an item without a prop, a count outside `1..=u16::MAX` or an exhausted serial source.
fn prize_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    item_id: u32,
    count: i32,
    flag: u8,
    what: &str,
) -> Option<Item> {
    let Some(prop) = ctx.data.item(item_id) else {
        tracing::warn!(
            item = item_id,
            what,
            "rainbow race prize: the item has no prop: skipped"
        );
        return None;
    };
    let Some(count) = u16::try_from(count).ok().filter(|n| *n >= 1) else {
        tracing::warn!(
            item = item_id,
            count,
            what,
            "rainbow race prize: the count is outside 1..=65535: skipped"
        );
        return None;
    };
    let mut item = Item::new(item_id, count);
    item.flags = ItemFlags(flag);
    item.charged = prop.charged != 0;
    item.endurance = i32::try_from(prop.endurance).unwrap_or(i32::MAX);
    let Services { serials, .. } = &mut *svc;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    match SerialSource::next_serial(&mut src) {
        Ok(s) => item.serial = ItemSerial(s),
        Err(e) => {
            tracing::error!(error = %e, item = item_id, what, "rainbow race prize: no item serial: skipped");
            return None;
        }
    }
    Some(item)
}

/// Delivers the queued mini-game prizes: with `all == false` the entries completed at least 15 s ago (`completed +
/// 15 000 <= now`), with `all` every entry (the end of the race). In queue order; every worked entry leaves the queue.
pub fn deliver_mini_game_prizes(s: &mut Services, ctx: &mut GameCtx<'_>, all: bool) {
    let now = ctx.clock.game_ms;
    let queue = std::mem::take(&mut s.event.rainbow.prize_queue);
    let mut keep = Vec::with_capacity(queue.len());
    for entry in queue {
        let (c, count, at) = entry;
        if !all && at.0.saturating_add(super::MINI_GAME_PRIZE_DELAY_MS) > now.0 {
            keep.push(entry);
            continue;
        }
        mail_mini_game_prize(s, ctx, c, count);
    }
    s.event.rainbow.prize_queue = keep;
}

/// One mini-game prize mail (`GetMiniGamePrize(count)` at the weekday of now).
fn mail_mini_game_prize(s: &mut Services, ctx: &mut GameCtx<'_>, c: CharacterId, count: u32) {
    let weekday = ctx.clock.local.weekday;
    let Some(row) = s
        .event
        .configs
        .rainbow
        .mini_game_prizes
        .get(&weekday)
        .and_then(|day| day.get(&count))
        .cloned()
    else {
        return;
    };
    if row.item == NULL_ID {
        return;
    }
    let Some(item) = prize_item(s, ctx, row.item, row.count, row.flag, "mini-game") else {
        return;
    };
    let title = ctx
        .data
        .texts
        .get(TID_GAME_RAINBOWRACE_MINIGAMEPRIZE)
        .to_owned();
    let (id, ops) = s
        .econ
        .mail
        .send_system_with_id(c, &title, &title, Some(item), 0);
    let record = s.econ.mail.get(id).map(MailEntry::to_record);
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| {
            on_send_result(svc, id, result);
        },
    )));
    ctx.persist.atomic(
        AtomicBatch::of_ops(AtomicKind::Other("rainbow_minigame_prize"), ops.to_vec())
            .with_ticket(ticket),
    );
    if let Some(rec) = record {
        ctx.audit.emit(flyff_db::audit::econ::mail_send(&rec, 0));
    }
}

/// What the rank prizes produced.
#[derive(Debug, Default)]
pub struct RankPrizes {
    /// The mails, rank 1 first.
    pub mails: Vec<MailId>,
    /// The sum of the penya in them.
    pub penya: i64,
}

/// The rank prizes of the weekday (`SetPrize`): one system mail per finisher up to the rows of the script. The mails are
/// written to the mail service (memory); the batch that carries their records is [`finish_round`]'s.
pub fn rank_prizes(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    ranking: &[CharacterId],
    applicants: usize,
) -> RankPrizes {
    let mut out = RankPrizes::default();
    let weekday = ctx.clock.local.weekday;
    let Some(rows) = s.event.configs.rainbow.prizes.get(&weekday).cloned() else {
        return out;
    };
    let title = ctx
        .data
        .texts
        .get(TID_GAME_RAINBOWRACE_POSTPRIZE_TITLE)
        .to_owned();
    for (r, row) in rows.iter().enumerate() {
        // the product is computed before the stop test, as the legacy; it has no side effect
        let penya = rank_penya(row.penya, applicants);
        let Some(&winner) = ranking.get(r) else {
            break;
        };
        let rank = i64::try_from(r + 1).unwrap_or(i64::MAX);
        let mut body = format_text(
            ctx.data
                .texts
                .get(TID_GAME_RAINBOWRACE_POSTPRIZE_TEXT_RANKING),
            &[Arg::D(rank)],
        );
        if penya > 0 {
            body.push_str(&format_text(
                ctx.data
                    .texts
                    .get(TID_GAME_RAINBOWRACE_POSTPRIZE_TEXT_PENYA),
                &[Arg::D(i64::from(penya))],
            ));
        }
        let mut item = None;
        if row.item != NULL_ID {
            // KQ-19: the flag is read from the wrong Lua stack slot, so it is always 0
            item = prize_item(s, ctx, row.item, row.count, 0, "rank");
            if let Some(it) = &item {
                let name = ctx.data.item(it.item_id).map_or("", |p| p.name.as_str());
                body.push_str(&format_text(
                    ctx.data.texts.get(TID_GAME_RAINBOWRACE_POSTPRIZE_TEXT_ITEM),
                    &[Arg::S(name), Arg::D(i64::from(it.count))],
                ));
            }
        }
        let (id, _ops) =
            s.econ
                .mail
                .send_system_with_id(winner, &title, &body, item, i64::from(penya));
        out.mails.push(id);
        out.penya += i64::from(penya);
    }
    out
}

/// The database finalisation of the race and the rank prizes in ONE `rainbow_result` batch (22 §2.8): every racer left
/// in the table is `W` (finished, with its rank) or `L`; the rank mails; the round counter raised. The batch has no
/// actor, so it goes out with a service ticket. A race whose racers were all eliminated still ends through it (empty
/// entries and mails, the round raised). Audit `rainbow.result`.
pub fn finish_round(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let rr: &RainbowRaceService = &s.event.rainbow;
    debug_assert_eq!(rr.state, RrState::Progress);
    let round = rr.round;
    let ranking = rr.ranking.clone();
    let applicants = rr.racers.len();
    let failed = rr.failed;
    let mut winners = 0u32;
    let mut losers = 0u32;
    let entries: Vec<RainbowRaceEntryRecord> = rr
        .racers
        .iter()
        .map(|(c, r)| {
            if r.finished {
                winners += 1;
                RainbowRaceEntryRecord {
                    round,
                    character_id: *c,
                    state: RainbowEntryState::Winner,
                    ranking: r.ranking,
                    applied_at: r.applied_at,
                    finished_at: r.finished_at.or(Some(r.applied_at)),
                }
            } else {
                losers += 1;
                RainbowRaceEntryRecord {
                    round,
                    character_id: *c,
                    state: RainbowEntryState::Lost,
                    ranking: 0,
                    applied_at: r.applied_at,
                    finished_at: None,
                }
            }
        })
        .collect();
    let prizes = rank_prizes(s, ctx, &ranking, applicants);
    let records: Vec<MailRecord> = prizes
        .mails
        .iter()
        .filter_map(|id| s.econ.mail.get(*id).map(MailEntry::to_record))
        .collect();
    let mail_count = u32::try_from(prizes.mails.len()).unwrap_or(u32::MAX);
    let ids = prizes.mails;
    let batch = AtomicBatch::rainbow_result(
        Vec::new(), // mail only: no character changes in memory
        entries,
        records,
        Some(round.saturating_add(1)),
    );
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => {
                for id in &ids {
                    svc.econ.mail.remove(*id);
                }
                tracing::error!(
                    error = %e,
                    round,
                    "CRITICAL: the rainbow race result batch failed: the rank mails are gone again, the entries stay unfinalised"
                );
            }
            other => tracing::error!(?other, "the rainbow race result batch completed unexpectedly"),
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
    ctx.audit.emit(audit::rainbow_result(
        round,
        u32::try_from(applicants).unwrap_or(u32::MAX),
        winners,
        losers,
        failed,
        mail_count,
    ));
}
