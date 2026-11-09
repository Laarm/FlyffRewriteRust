//! The tax payout (Plan/22 §10.4; legacy `CTaxDBController::PayTaxToPost`, `Tax.cpp:653-726`, `CheckPayTime`).
//!
//! * [`pay_daily`] is the body of the wall job `tax_pay` (`Daily{0, 0}` from `PayTime`, `CatchUp::RunOnceAtStart`, PL-100):
//!   skipped when `tax.last_paid_on` already is today's local date, otherwise `run`.
//! * `run` is the payout: for East, West and All in that order the receiver is the master of the owning guild (the lord
//!   character for `All`); no owner, no master or a receiver the directory does not know skips the entry, which KEEPS its
//!   counters. For every kind of a paid entry, in kind order, ONE system mail (even when empty, KEEP: up to eight a day)
//!   with the title `TID_GAME_TAX_PAY_<KIND>_TITLE` of the continent, the report `TID_GAME_TAX_PAY_INFO`, the penya below one
//!   Perin and `II_SYS_SYS_SCR_PERIN × perin` when there is one; then the counters are zero. ONE `TaxPayout` batch carries
//!   the mails, the reset rows and the date, also when nobody was paid (it records the date).
//! * `collect` is the part the weekly change shares (it pays before it changes, with the rows of the old round).
//!
//! The mails are in the mail service at once; the batch ticket removes them again when the batch fails (siege pattern,
//! 21 §2.4: the rest of the memory is kept).

use super::{CONTINENTS, TaxService, kinds_of, submit};
use crate::ctx::{GameCtx, Services};
use crate::econ::mail::MailEntry;
use crate::econ::tax::{Continent, TaxKind};
use crate::gm::text::format_positional;
use crate::item::model::Item;
use crate::social::guild::salary::date_of;
use crate::tick::LocalStamp;
use crate::world::continent::{CONT_ALL, CONT_EAST, CONT_WEST};
use flyff_common::defs::item::II_SYS_SYS_SCR_PERIN;
use flyff_common::defs::text::{
    TID_GAME_CONT_ALL, TID_GAME_CONT_EAST, TID_GAME_CONT_WEST, TID_GAME_TAX_PAY_ADMISSION_TITLE,
    TID_GAME_TAX_PAY_DATE, TID_GAME_TAX_PAY_INFO, TID_GAME_TAX_PAY_PURCHASE_TITLE,
    TID_GAME_TAX_PAY_SALES_TITLE,
};
use flyff_common::{CharacterId, MailId, UnixMs};
use flyff_db::audit::content as audit;
use flyff_db::{AtomicBatch, TaxDetailRecord};

/// The most Perins one mail item carries (the legacy `short`).
const MAX_ITEM_COUNT: i32 = 32_767;

/// What a payout did.
#[derive(Debug, Default)]
pub(super) struct Paid {
    /// The mails now in the mail service, in order.
    pub mails: Vec<MailId>,
    /// The paid details of the round in force with their counters at zero: their rows go into the batch.
    pub details: Vec<TaxDetailRecord>,
    /// The penya below one Perin paid in total.
    pub gold: i64,
    /// The Perins paid in total.
    pub perin: i64,
}

/// Wall job `tax_pay` (22 §2.4, `Daily{0,0}`, `RunOnceAtStart`): the daily payout, skipped when `tax.last_paid_on` is
/// today's local date of the server zone (a restart on the same date, PL-100).
pub fn pay_daily(s: &mut Services, ctx: &mut GameCtx<'_>, _at: &LocalStamp) {
    let today = date_of(&ctx.clock.local);
    if s.event.tax.last_paid_on.as_deref() == Some(today.as_str()) {
        return;
    }
    run(s, ctx, &today);
}

/// The payout without its date guard (also `gm::pay_now`): mails every owner's counters now and records `today`.
pub(super) fn run(s: &mut Services, ctx: &mut GameCtx<'_>, today: &str) {
    let paid = collect(s, ctx);
    s.event.tax.last_paid_on = Some(today.to_owned());
    let round = s.event.tax.round;
    let records: Vec<_> = paid
        .mails
        .iter()
        .filter_map(|id| s.econ.mail.get(*id).map(MailEntry::to_record))
        .collect();
    let batch = AtomicBatch::tax_payout(
        Vec::new(),
        paid.details,
        records,
        Some(today.to_owned()),
        None,
    );
    ctx.audit.emit(audit::tax_payout(
        round,
        Some(today),
        u32::try_from(paid.mails.len()).unwrap_or(u32::MAX),
        paid.gold,
        paid.perin,
    ));
    submit(ctx, "tax_payout", batch, paid.mails, false);
}

/// The receiver of an entry: the master of the owning guild (East, West) or the lord (All); `None` when there is no
/// owner, no master, or the directory does not know the character (a purged lord: the mail row has a foreign key).
fn receiver_of(s: &Services, slot: usize) -> Option<CharacterId> {
    let owner = s.event.tax.entries[slot].owner?;
    let who = if CONTINENTS[slot] == CONT_ALL {
        CharacterId(owner)
    } else {
        s.social
            .guild
            .get(flyff_common::GuildId(owner))
            .and_then(crate::social::guild::model::Guild::master)?
    };
    if s.presence.get(who).is_none() {
        tracing::error!(
            receiver = who.0,
            continent = CONTINENTS[slot],
            "tax payout: the receiver is unknown to the directory: the entry keeps its counters"
        );
        return None;
    }
    Some(who)
}

/// The text of the continent in a title (`CContinent::GetContinentName`).
fn continent_tid(c: Continent) -> u32 {
    match c {
        CONT_EAST => TID_GAME_CONT_EAST,
        CONT_WEST => TID_GAME_CONT_WEST,
        CONT_ALL => TID_GAME_CONT_ALL,
        _ => TID_GAME_CONT_EAST,
    }
}

fn title_tid(kind: TaxKind) -> u32 {
    match kind {
        TaxKind::Sales => TID_GAME_TAX_PAY_SALES_TITLE,
        TaxKind::Purchase => TID_GAME_TAX_PAY_PURCHASE_TITLE,
        TaxKind::Admission => TID_GAME_TAX_PAY_ADMISSION_TITLE,
    }
}

/// Mails the counters of every entry that can be paid and resets them (memory only: the caller builds the batch).
pub(super) fn collect(s: &mut Services, ctx: &mut GameCtx<'_>) -> Paid {
    let mut out = Paid::default();
    // "yesterday" in the text: the local date of now minus one day
    let yesterday = ctx
        .sched
        .zone()
        .local(UnixMs(ctx.clock.unix_ms.0.saturating_sub(86_400_000)));
    let date = format_positional(
        ctx.data.texts.get(TID_GAME_TAX_PAY_DATE),
        &[
            yesterday.year.to_string(),
            yesterday.month.to_string(),
            yesterday.day.to_string(),
        ],
    );
    let now = ctx.clock.game_ms;
    for (slot, cont) in CONTINENTS.iter().copied().enumerate() {
        let Some(receiver) = receiver_of(s, slot) else {
            continue;
        };
        let cont_name = ctx.data.texts.get(continent_tid(cont)).to_owned();
        for kind in kinds_of(slot) {
            let d = s.event.tax.entries[slot].details[*kind as usize];
            let perin = if d.perin > MAX_ITEM_COUNT {
                tracing::error!(
                    perin = d.perin,
                    continent = cont,
                    "tax payout: more Perins than one mail item carries: clamped (the legacy `short`)"
                );
                MAX_ITEM_COUNT
            } else {
                d.perin.max(0)
            };
            let item = if perin > 0 {
                let Some(serial) = crate::event::payout::fresh_serial(s, ctx) else {
                    // no serial, no Perins: the kind keeps its counters until the next payout
                    continue;
                };
                let mut item = Item::new(
                    II_SYS_SYS_SCR_PERIN,
                    u16::try_from(perin).unwrap_or(u16::MAX),
                );
                item.serial = serial;
                Some(item)
            } else {
                None
            };
            let title = format_positional(
                ctx.data.texts.get(title_tid(*kind)),
                std::slice::from_ref(&cont_name),
            );
            let body = format_positional(
                ctx.data.texts.get(TID_GAME_TAX_PAY_INFO),
                &[
                    date.clone(),
                    d.count.to_string(),
                    d.perin.to_string(),
                    d.gold.to_string(),
                ],
            );
            let (id, _ops) =
                s.econ
                    .mail
                    .send_system_with_id(receiver, &title, &body, item, i64::from(d.gold));
            out.mails.push(id);
            out.gold += i64::from(d.gold);
            out.perin += i64::from(perin);
            let e = &mut s.event.tax.entries[slot].details[*kind as usize];
            e.count = 0;
            e.gold = 0;
            e.perin = 0;
            e.written(now);
            out.details.push(e.record(s.event.tax.round, slot, *kind));
        }
    }
    out
}

impl TaxService {
    /// `true` when nothing was booked since the last payout (every counter of `count` is zero).
    pub(super) fn nothing_booked(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.details.iter().all(|d| d.count == 0))
    }
}
