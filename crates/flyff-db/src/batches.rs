//! Typed constructors for the atomic batches of the economy (Plan/13 §18, Plan/19 §20.3).
//!
//! Every multi-party commit of the economy is **one** [`AtomicBatch`] = one SQLite transaction: the snapshots of
//! the characters involved plus the mail / grant ops, all or nothing. The constructors below are the exact rows
//! of the §18 table, so the econ handlers cannot forget a snapshot or the counter raise:
//!
//! | constructor | [`AtomicKind`] | snapshots | ops |
//! |---|---|---|---|
//! | [`AtomicBatch::trade`] | `Trade` | A, B | - |
//! | [`AtomicBatch::vendor_purchase`] | `VendorPurchase` | buyer, seller | - |
//! | [`AtomicBatch::mail_send`] | `MailSend` | sender | `InsertMail`, `RaiseCounter(next_mail_id)` |
//! | [`AtomicBatch::mail_take_item`] | `MailTakeItem` | receiver | `UpdateMail{take_item}` |
//! | [`AtomicBatch::mail_take_gold`] | `MailTakeGold` | receiver | `UpdateMail{take_gold}` (+ `InsertMail`, `RaiseCounter` for a re-mail) |
//! | [`AtomicBatch::grant_delivery`] | `GrantDelivery` | receiver | `MarkGrantsDelivered` |
//! | [`AtomicBatch::removal_apply`] | `RemovalApply` | target (character removals) | `MarkRemovalsDone` (+ guild-bank ops) |
//!
//! The in-memory change and the `persist.atomic(..)` call belong into the same panic-free commit block; a batch
//! built here is queued with `PersistHandle::atomic` (reply through [`AtomicBatch::with_ticket`]).

use crate::model::{CharacterSnapshot, MailRecord, MailStateRecord};
use crate::ops::{AtomicBatch, AtomicKind, BatchReply, DbOp, Ticket};
use crate::repo::meta::counter;
use flyff_common::{MailId, UnixTime};

impl DbOp {
    /// `RaiseCounter { next_mail_id, at_least: id + 1 }`: pairs with every `InsertMail` of a freshly allocated id,
    /// so the counter can never fall behind a persisted mail.
    pub fn raise_mail_counter(id: MailId) -> DbOp {
        DbOp::RaiseCounter {
            key: counter::NEXT_MAIL_ID,
            at_least: i64::from(id.0) + 1,
        }
    }

    /// The two ops of a new mail, in the order Plan/19 §11.1 `MailService::send_system` returns them:
    /// `[InsertMail(mail), RaiseCounter(next_mail_id)]`.
    pub fn mail_ops(mail: MailRecord) -> [DbOp; 2] {
        let raise = DbOp::raise_mail_counter(mail.id);
        [DbOp::InsertMail(mail), raise]
    }
}

impl AtomicBatch {
    fn with_parts(
        kind: AtomicKind,
        snapshots: Vec<CharacterSnapshot>,
        ops: Vec<DbOp>,
    ) -> AtomicBatch {
        AtomicBatch {
            kind,
            snapshots,
            ops,
            reply: BatchReply::None,
        }
    }

    /// The same batch answered to the game thread with `DbReply::{Committed, Failed}` for `ticket`.
    #[must_use]
    pub fn with_ticket(mut self, ticket: Ticket) -> AtomicBatch {
        self.reply = BatchReply::Game(ticket);
        self
    }

    /// `Trade`: both parties confirmed; the two inventories and wallets change together or not at all
    /// (Plan/19 §8.3). Audit `trade.complete` is emitted by the caller.
    pub fn trade(a: CharacterSnapshot, b: CharacterSnapshot) -> AtomicBatch {
        Self::with_parts(AtomicKind::Trade, vec![a, b], Vec::new())
    }

    /// `VendorPurchase`: a personal-shop buy; buyer and seller change together (Plan/19 §9.4). Audit
    /// `vendor.sale` is emitted by the caller.
    pub fn vendor_purchase(buyer: CharacterSnapshot, seller: CharacterSnapshot) -> AtomicBatch {
        Self::with_parts(AtomicKind::VendorPurchase, vec![buyer, seller], Vec::new())
    }

    /// `MailSend`: the sender's snapshot (item, attached gold and postage already removed) and the new mail
    /// (`InsertMail` + `RaiseCounter(next_mail_id)`), Plan/19 §11.2. A system mail is
    /// `DbOp::mail_ops(mail)` in whatever batch the reward belongs to.
    pub fn mail_send(sender: CharacterSnapshot, mail: MailRecord) -> AtomicBatch {
        Self::with_parts(
            AtomicKind::MailSend,
            vec![sender],
            DbOp::mail_ops(mail).into(),
        )
    }

    /// `MailTakeItem`: the receiver's snapshot (item added, fee paid) and the mail's attachment cleared
    /// (`item = NULL`, `item_taken_at = at`). A mail whose item is already gone fails the whole batch with
    /// `Conflict` (Plan/19 §11.4).
    pub fn mail_take_item(receiver: CharacterSnapshot, mail: MailId, at: UnixTime) -> AtomicBatch {
        Self::with_parts(
            AtomicKind::MailTakeItem,
            vec![receiver],
            vec![DbOp::UpdateMail(MailStateRecord::take_item(mail, at))],
        )
    }

    /// `MailTakeGold`: the receiver's snapshot (gold added) and the mail's gold cleared (`gold = 0`,
    /// `gold_taken_at = at`). `remail` is the new system mail that carries the gold when the wallet could not hold
    /// it (Plan/19 §11.5; the original's gold is zeroed either way).
    pub fn mail_take_gold(
        receiver: CharacterSnapshot,
        mail: MailId,
        at: UnixTime,
        remail: Option<MailRecord>,
    ) -> AtomicBatch {
        let mut ops = vec![DbOp::UpdateMail(MailStateRecord::take_gold(mail, at))];
        if let Some(remail) = remail {
            ops.extend(DbOp::mail_ops(remail));
        }
        Self::with_parts(AtomicKind::MailTakeGold, vec![receiver], ops)
    }

    /// `GrantDelivery`: the receiver's snapshot (items and gold added) and `MarkGrantsDelivered{ids}` (Plan/13
    /// §18, Plan/19 §20.4). One batch per delivery pass.
    pub fn grant_delivery(
        receiver: CharacterSnapshot,
        grant_ids: Vec<i64>,
        at: UnixTime,
    ) -> AtomicBatch {
        Self::with_parts(
            AtomicKind::GrantDelivery,
            vec![receiver],
            vec![DbOp::MarkGrantsDelivered { ids: grant_ids, at }],
        )
    }

    /// `RemovalApply`: the target's snapshot after the removal (`None` for guild-bank removals), the closed
    /// removals `(id, removed_count)` and the extra ops of a guild-bank removal: pass
    /// [`DbOp::guild_bank_removal_ops`] (`SetGuildBank` with the bank after the removal and the log row `R`) for
    /// items, [`DbOp::guild_gold_removal_ops`] (`UpsertGuild` and the log row `R`) for penya.
    pub fn removal_apply(
        target: Option<CharacterSnapshot>,
        done: Vec<(i64, u32)>,
        at: UnixTime,
        extra_ops: Vec<DbOp>,
    ) -> AtomicBatch {
        let mut ops = vec![DbOp::MarkRemovalsDone { done, at }];
        ops.extend(extra_ops);
        Self::with_parts(AtomicKind::RemovalApply, target.into_iter().collect(), ops)
    }
}
