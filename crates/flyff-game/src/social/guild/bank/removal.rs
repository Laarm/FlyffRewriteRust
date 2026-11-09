//! GM removals from the guild bank (Plan/21 §5.9 `RemoveGuildBankItem`, 13 §18 `RemovalApply`; WP-SOCIAL-13 and the
//! seam of WP-ECON-09 / WP-ADMIN-24).
//!
//! **The P13 poll.** Scheduler job `guild_bank_removal_poll` (every 450 ticks, phase 28, gate `events.guild_bank`)
//! reads the open `pending_removals` rows of the location `guild_bank` ([`removal_poll`], one
//! `GameReadQuery::OpenGuildRemovals`). The continuation applies every row that the in-memory [`RemovalGuard`] has not
//! applied yet, one `RemovalApply` batch per row:
//!
//! * the guild is gone: the row is closed with `removed_count 0`;
//! * `item_id == 0`: penya, `min(count, guild.gold)` ([`DbOp::guild_gold_removal_ops`], log row `R`);
//! * else up to `count` pieces are taken from the bank stacks in slot order that match `removal_matches`
//!   (`remove_matching`; [`DbOp::guild_bank_removal_ops`], log row `R`).
//!
//! Every online member gets `RemoveGuildBankItem` (one per touched stack with the bank object id, or `NULL_ID` for
//! penya, `announce_removal`). A row that was cancelled by an administrator is never in the poll's answer
//! (`cancelled_at IS NULL` in the query) and is skipped again here.
//!
//! **Sharing.** `removal_matches` is the one match rule of the removal rows (WP-ECON-09 reuses it for the
//! character rows or moves it to `econ/grants/removals.rs`); `remove_matching` and `announce_removal` are shared
//! with the trusted-caller API: [`remove_stack`] is the admin console's `GuildEdit::BankRemove`.

use super::{GuildBank, emit_bank, now, remember};
use crate::ctx::{GameCtx, Services};
use crate::item::container::StorageId;
use crate::item::model::Item;
use crate::social::guild::commit::{self, Origin};
use crate::social::guild::ops::{CommitCb, GuildActor, GuildChange, GuildRefusal};
use crate::social::guild::wire;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{CharacterId, GuildId};
use flyff_db::{
    AtomicBatch, AtomicKind, DbOp, GameReadQuery, GameReadResult, GuildBankLogKind,
    GuildBankLogRecord, PendingRemovalRecord, RemovalLocation,
};
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;
use serde_json::json;
use std::collections::BTreeSet;

/// The ids of the removal rows the game applied in this run: a poll that sees such an id again (its
/// `MarkRemovalsDone` has not committed yet, or failed) skips it, so a row is never applied twice. Ids that the
/// poll no longer lists are forgotten.
#[derive(Clone, Debug, Default)]
pub struct RemovalGuard {
    applied: BTreeSet<i64>,
}

impl RemovalGuard {
    /// Marks `id` as applied; `true` when it was new (apply it).
    pub fn mark(&mut self, id: i64) -> bool {
        self.applied.insert(id)
    }

    /// `true` when `id` was applied in this run.
    pub fn contains(&self, id: i64) -> bool {
        self.applied.contains(&id)
    }

    /// Forgets the ids that are not in `open` any more (their rows are closed).
    pub fn retain_open(&mut self, open: &[PendingRemovalRecord]) {
        self.applied.retain(|id| open.iter().any(|r| r.id == *id));
    }

    /// How many ids are remembered.
    pub fn len(&self) -> usize {
        self.applied.len()
    }

    /// `true` when nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.applied.is_empty()
    }
}

/// Does `item` match the removal row `rec`? The item id always; the serial, the upgrade level, the element and the
/// element level only when the row names them (`None` = any).
pub(crate) fn removal_matches(rec: &PendingRemovalRecord, item: &Item) -> bool {
    item.item_id == rec.item_id
        && rec.serial.is_none_or(|s| item.serial.0 == s)
        && rec.upgrade_level.is_none_or(|u| item.upgrade_level == u)
        && rec.element.is_none_or(|e| item.element as u8 == e)
        && rec.element_level.is_none_or(|l| item.element_level == l)
}

/// One stack that a removal touched.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Removed {
    /// The bank object id (storage id) of the stack.
    pub obj_id: StorageId,
    /// The visible slot it was shown in.
    pub slot: u16,
    /// The stack as it was before the removal.
    pub item: Item,
    /// How many pieces left it.
    pub removed: u16,
}

/// Takes `n` pieces (at most the stack) out of the entry `obj_id` shown in `slot`: a stack that is used up leaves
/// the bank, a larger one keeps its serial and the rest.
fn take_from(bank: &mut GuildBank, slot: u16, obj_id: StorageId, n: u16) -> Option<Removed> {
    let before = bank.items.get_at_id(obj_id)?.clone();
    let take = n.min(before.count);
    if take >= before.count {
        bank.items.remove_at_id(obj_id);
    } else if let Some(it) = bank.items.get_at_id_mut(obj_id) {
        it.count = before.count - take;
    }
    Some(Removed {
        obj_id,
        slot,
        item: before,
        removed: take,
    })
}

/// Takes up to `count` pieces from the stacks of `bank` that satisfy `matches`, in slot order. Returns what was
/// touched, in slot order.
pub(crate) fn remove_matching(
    bank: &mut GuildBank,
    count: u32,
    matches: impl Fn(&Item) -> bool,
) -> Vec<Removed> {
    let mut left = count;
    let mut out = Vec::new();
    for slot in 0..bank.items.index_num() {
        if left == 0 {
            break;
        }
        let Some(obj_id) = bank.items.obj_at_slot(slot) else {
            continue;
        };
        if !bank.items.get_at_id(obj_id).is_some_and(&matches) {
            continue;
        }
        let want = u16::try_from(left).unwrap_or(u16::MAX);
        if let Some(r) = take_from(bank, slot, obj_id, want) {
            left -= u32::from(r.removed);
            out.push(r);
        }
    }
    out
}

/// `RemoveGuildBankItem{id_guild, item_obj_id, num}` to the online members, one message per touched stack.
pub(crate) fn announce_removal(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    removed: &[Removed],
) {
    for r in removed {
        wire::to_members(
            svc,
            ctx,
            guild,
            pb::RemoveGuildBankItem {
                id_guild: guild.0,
                item_obj_id: u32::from(r.obj_id),
                num: u32::from(r.removed),
            },
        );
    }
}

/// `RemoveGuildBankItem{item_obj_id: NULL_ID, num}`: penya left the pool (the clients lower the pool).
pub(crate) fn announce_penya_removal(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    amount: u32,
) {
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::RemoveGuildBankItem {
            id_guild: guild.0,
            item_obj_id: NULL_ID,
            num: amount,
        },
    );
}

// ---- the P13 poll ------------------------------------------------------------------------------------------------

/// P13 `guild_bank_removal_poll`: one `OpenGuildRemovals` read; the answer is handled by `on_open_removals`.
pub fn removal_poll(_svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let ticket = ctx
        .tickets
        .issue(Pending::Service(ServiceTicket::new(on_open_removals)));
    ctx.persist
        .read_for_game(ticket, GameReadQuery::OpenGuildRemovals { after_id: 0 });
}

/// The continuation of the poll: every open row the game has not applied yet is applied. A failed read is logged;
/// the next poll asks again.
fn on_open_removals(svc: &mut Services, ctx: &mut GameCtx<'_>, result: TicketResult) {
    let rows = match result {
        TicketResult::ReadDone(Ok(GameReadResult::GuildRemovals(rows))) => rows,
        TicketResult::ReadDone(Err(e)) => {
            tracing::warn!(error = %e, "the guild bank removal poll failed; it asks again");
            return;
        }
        other => {
            tracing::error!(
                ?other,
                "the guild bank removal poll got an unexpected answer"
            );
            return;
        }
    };
    apply_rows(svc, ctx, rows);
}

/// Applies the open rows of one poll answer: the guard forgets the ids that are not listed any more, every listed row
/// that was not applied in this run is applied (in id order).
pub(crate) fn apply_rows(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    rows: Vec<PendingRemovalRecord>,
) {
    svc.social.guild.removals.retain_open(&rows);
    for row in rows {
        apply_row(svc, ctx, row);
    }
}

/// Closes the row `id` with what was removed; `extra` are the ops of the guild side (`SetGuildBank` / `UpsertGuild`
/// and the log row `R`), in the SAME batch.
fn close(svc: &mut Services, ctx: &mut GameCtx<'_>, id: i64, removed: u32, extra: Vec<DbOp>) {
    let batch = AtomicBatch::removal_apply(None, vec![(id, removed)], now(ctx), extra);
    commit::submit(svc, ctx, "RemovalApply", batch, &[], None);
}

/// Applies one open row (see the module documentation).
fn apply_row(svc: &mut Services, ctx: &mut GameCtx<'_>, r: PendingRemovalRecord) {
    if r.done_at.is_some() || r.cancelled_at.is_some() || r.location != RemovalLocation::GuildBank {
        return;
    }
    let Some(guild) = r.guild_id else {
        return;
    };
    if !svc.social.guild.removals.mark(r.id) {
        return;
    }
    let at = now(ctx);
    let Some(g) = svc.social.guild.get(guild) else {
        close(svc, ctx, r.id, 0, Vec::new());
        return;
    };
    if r.item_id == 0 {
        let amount = r.count.min(g.gold);
        if amount == 0 {
            close(svc, ctx, r.id, 0, Vec::new());
            return;
        }
        if let Some(g) = svc.social.guild.get_mut(guild) {
            g.gold -= amount;
        }
        let log = GuildBankLogRecord::new(guild, CharacterId(0), GuildBankLogKind::GmRemoval, at)
            .with_penya(i64::from(amount));
        remember(svc, guild, &log);
        let ops: Vec<DbOp> = svc
            .social
            .guild
            .get(guild)
            .map(|g| DbOp::guild_gold_removal_ops(g.to_record(), log).into())
            .unwrap_or_default();
        close(svc, ctx, r.id, amount, ops);
        announce_penya_removal(svc, ctx, guild, amount);
        emit_bank(
            ctx,
            Origin::System,
            guild,
            CharacterId(0),
            GuildBankLogKind::GmRemoval,
            None,
            i64::from(amount),
        );
        return;
    }
    let mut bank = g.bank.clone();
    let removed = remove_matching(&mut bank, r.count, |it| removal_matches(&r, it));
    let total: u32 = removed.iter().map(|x| u32::from(x.removed)).sum();
    let Some(first) = removed.first() else {
        close(svc, ctx, r.id, 0, Vec::new());
        return;
    };
    let log = GuildBankLogRecord::new(guild, CharacterId(0), GuildBankLogKind::GmRemoval, at)
        .with_item(
            first.item.item_id,
            first.item.upgrade_level,
            i64::from(total),
        );
    remember(svc, guild, &log);
    let ops: Vec<DbOp> = DbOp::guild_bank_removal_ops(guild, bank.to_doc(), log).into();
    if let Some(g) = svc.social.guild.get_mut(guild) {
        g.bank = bank;
    }
    close(svc, ctx, r.id, total, ops);
    announce_removal(svc, ctx, guild, &removed);
    for x in &removed {
        let mut shown = x.item.clone();
        shown.count = x.removed;
        emit_bank(
            ctx,
            Origin::System,
            guild,
            CharacterId(0),
            GuildBankLogKind::GmRemoval,
            Some(&shown),
            0,
        );
    }
}

// ---- the trusted-caller API (the admin console's BankRemove) -----------------------------------------------------

/// Takes `count` pieces out of the stack shown in `slot` of the bank of `guild` (the admin console's
/// `GuildEdit::BankRemove`, 25 §6.2). The stack must still carry `serial` (else [`GuildRefusal::Invalid`], so a
/// repeated click is harmless); `count` is clamped to `1..=stack`. ONE `AdminGuildEdit` batch (the bank document and
/// the log row `R`; for a player or GM actor a `GuildBankRemove` batch of the same ops), `RemoveGuildBankItem` to
/// the online members, audit `guild.bank` kind `R`.
///
/// The returned [`GuildChange`] has `action: "bank"` and `before` / `after` of the shape
/// `{"slot": 3, "item_id": 5001, "serial": 77, "count": 5}` (`count` is what the stack held; 0 after when it is gone).
#[allow(clippy::too_many_arguments)] // the signature of the admin seam (21 task spec)
pub fn remove_stack(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    slot: u8,
    serial: u64,
    count: u16,
    actor: &GuildActor,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    let obj_id = g
        .bank
        .items
        .obj_at_slot(u16::from(slot))
        .ok_or(GuildRefusal::Invalid("the slot is empty"))?;
    let item = g
        .bank
        .items
        .get_at_id(obj_id)
        .ok_or(GuildRefusal::Invalid("the slot is empty"))?;
    if item.serial.0 != serial {
        return Err(GuildRefusal::Invalid("the stack has changed"));
    }
    let n = count.clamp(1, item.count.max(1));
    let stack = |held: u16| json!({"slot": slot, "item_id": item.item_id, "serial": item.serial.0, "count": held});
    let (before, after) = (stack(item.count), stack(item.count.saturating_sub(n)));
    let shown = {
        let mut s = item.clone();
        s.count = n;
        s
    };
    let mut bank = g.bank.clone();
    let removed: Vec<Removed> = take_from(&mut bank, u16::from(slot), obj_id, n)
        .into_iter()
        .collect();
    let origin = Origin::from(actor);
    let character = origin.character(ctx);
    let log = GuildBankLogRecord::new(guild, character, GuildBankLogKind::GmRemoval, now(ctx))
        .with_item(shown.item_id, shown.upgrade_level, i64::from(n));
    // ---- the commit block ----
    remember(svc, guild, &log);
    let ops: Vec<DbOp> = DbOp::guild_bank_removal_ops(guild, bank.to_doc(), log).into();
    if let Some(g) = svc.social.guild.get_mut(guild) {
        g.bank = bank;
    }
    commit::persist(
        svc,
        ctx,
        origin,
        AtomicKind::Other("GuildBankRemove"),
        ops,
        cb,
    );
    announce_removal(svc, ctx, guild, &removed);
    emit_bank(
        ctx,
        origin,
        guild,
        character,
        GuildBankLogKind::GmRemoval,
        Some(&shown),
        0,
    );
    Ok(GuildChange {
        action: "bank",
        guild,
        character: None,
        before: Some(before),
        after: Some(after),
    })
}
