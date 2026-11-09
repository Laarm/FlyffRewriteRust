//! `event::payout`: the common money payout of the events (Plan/22 §2.7, `pay_out`; legacy `PostDeposit`,
//! src:databaseserver/tlord.cpp:393-404).
//!
//! An amount of penya is paid as system mail: whole Perins (`II_SYS_SYS_SCR_PERIN`, worth 100 000 000 penya each) as ONE
//! item and the remainder as penya of the same mail. More Perins than one item can carry go into further mails with
//! the same texts and no penya (FIX: the original cast the count to a `short` and wrapped). In B9 only the lord election
//! uses it (the refunds of the deposits); the tax payout builds its own mails (`event/tax/payout.rs`) and the
//! secret-room refunds are penya-only.
//!
//! The caller puts the mail records into the batch it sends (`svc.econ.mail.get(id).map(MailEntry::to_record)`) and
//! removes the mails again from the mail service when that batch fails.

use crate::ctx::{GameCtx, Services};
use crate::item::container::SerialSource;
use crate::item::model::Item;
use crate::item::ops::CtxSerials;
use flyff_common::defs::item::II_SYS_SYS_SCR_PERIN;
use flyff_common::{CharacterId, ItemSerial, MailId};

/// What one Perin is worth in penya (`PERIN_VALUE`).
pub const PERIN_VALUE: i64 = 100_000_000;

/// The most Perins one mail carries when the item data does not say (`short`).
const MAX_ITEM_COUNT: u32 = 32_767;

/// The Perins and the penya of an amount: `(amount / PERIN_VALUE, amount % PERIN_VALUE)`; a negative amount is `(0, 0)`.
pub fn split_amount(amount: i64) -> (i64, i64) {
    if amount <= 0 {
        return (0, 0);
    }
    (amount / PERIN_VALUE, amount % PERIN_VALUE)
}

/// The Perins one mail item carries: `min(pack_max, 32 767)` (at least 1).
pub fn perins_per_mail(ctx: &GameCtx<'_>) -> u32 {
    ctx.data
        .items
        .get(II_SYS_SYS_SCR_PERIN)
        .map_or(MAX_ITEM_COUNT, |p| p.pack_max)
        .clamp(1, MAX_ITEM_COUNT)
}

/// A fresh item serial from the game's allocator (`None` when the allocator is exhausted: a persistence fault).
pub(crate) fn fresh_serial(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Option<ItemSerial> {
    let Services { serials, .. } = &mut *svc;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    match src.next_serial() {
        Ok(s) => Some(ItemSerial(s)),
        Err(e) => {
            tracing::error!(error = %e, "an event item could not get a serial");
            None
        }
    }
}

/// `pay_out(receiver, amount)` (22 §2.7): mails `amount` penya to `receiver` as Perins plus penya, with `title` and
/// `body` (already formatted). Returns the ids of the mails written, in order (empty for an amount of 0 or less and for
/// a receiver the directory does not know: the mail row has a foreign key to the character).
pub fn pay_out(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    receiver: CharacterId,
    title: &str,
    body: &str,
    amount: i64,
) -> Vec<MailId> {
    let (perins, penya) = split_amount(amount);
    if perins == 0 && penya == 0 {
        return Vec::new();
    }
    if svc.presence.get(receiver).is_none() {
        tracing::error!(
            receiver = receiver.0,
            amount,
            "pay_out: the receiver is unknown to the directory: nothing was mailed"
        );
        return Vec::new();
    }
    let per_mail = i64::from(perins_per_mail(ctx));
    let mut ids = Vec::new();
    let mut left = perins;
    let mut gold = penya;
    loop {
        let count = left.min(per_mail);
        let item = if count > 0 {
            let Some(serial) = fresh_serial(svc, ctx) else {
                break;
            };
            let mut item = Item::new(
                II_SYS_SYS_SCR_PERIN,
                u16::try_from(count).unwrap_or(u16::MAX),
            );
            item.serial = serial;
            Some(item)
        } else {
            None
        };
        let (id, _ops) = svc
            .econ
            .mail
            .send_system_with_id(receiver, title, body, item, gold);
        ids.push(id);
        left -= count;
        gold = 0;
        if left <= 0 {
            break;
        }
    }
    ids
}
