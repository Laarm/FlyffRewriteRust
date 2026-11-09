//! `econ::grants::apply_removals`: the pending removals of a character that is in the world (Plan/19 §20.4, 13 §18;
//! WP-ECON-09). The GM side that inserts the rows is the operator CLI and the admin console; the guild-bank rows
//! (`location = guild_bank`) are the poll P13 of the guild bank and never come here.
//!
//! * **Penya** (`item_id == 0`): `count` is taken from the wallet (inventory) or from the gold of the character's own
//!   bank, clamped at 0; `removed_count = min(count, have)`.
//! * **Items**: the stacks that match the row ([`removal_matches`](crate::social::guild::bank::removal): the item id
//!   always; serial, upgrade level, element and element level only when the row names them) are taken in ascending
//!   storage order from the bag (worn items included), the character's own bank or the three pockets, `count` pieces
//!   across the stacks or the whole stack: a removal of 5 from a stack of 3 removes 3 and reports 3. The owner gets
//!   the normal messages (`UpdateItem`, `PocketRemoveItem`, `UpdateBankItem`, `SetPointParam`); a worn item is
//!   unequipped by `item::ops::remove_item`.
//! * **A matching stack that is reserved** by a trade or a vendor, or in use (a summoned pet, an active ticket or
//!   feed pocket) is never touched: the whole row stays open (nothing of it is removed), the next enter-world tries
//!   again. A row that matches nothing is closed with `removed_count = 0` (the original marked first).
//!
//! ONE `AtomicBatch::removal_apply(Some(snapshot), done)` per target; audit `removal.apply {id, item_id, removed_count}`
//! and the `item.destroy {reason: "removal"}` lines of the touched stacks. Without a live untainted session nothing
//! happens and every row stays open.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::item::container::{ItemContainer, StorageId};
use crate::item::fx::{Fx, item_json};
use crate::item::gold::add_gold;
use crate::item::model::Item;
use crate::item::ops::{RemoveReason, remove_item};
use crate::mover::Mover;
use crate::social::guild::bank::removal::removal_matches;
use crate::social::relation::character_of;
use crate::tickets::TicketResult;
use flyff_common::CharacterId;
use flyff_db::audit::econ as audit;
use flyff_db::audit::ev;
use flyff_db::{AtomicBatch, PendingRemovalRecord, RemovalLocation};
use flyff_proto::pb;
use serde_json::json;

/// Where a stack lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// The bag and the worn items.
    Bag,
    /// Pocket `0..3`.
    Pocket(usize),
    /// The character's own bank compartment (its slot).
    Bank(usize),
}

/// One stack a row takes pieces from.
#[derive(Clone, Debug)]
struct Take {
    place: Place,
    id: StorageId,
    item: Item,
    n: u32,
}

/// Applies the open character removals of `who` (see the module documentation). Rows of another character, closed
/// or cancelled rows, guild-bank rows and ids the guard remembers are ignored.
pub fn apply_removals(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    removals: Vec<PendingRemovalRecord>,
) {
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let mut rows: Vec<PendingRemovalRecord> = removals
        .into_iter()
        .filter(|r| {
            r.is_open()
                && r.character_id == Some(me)
                && r.location != RemovalLocation::GuildBank
                && !svc.grants.removal_refuses(r.id)
        })
        .collect();
    rows.sort_by_key(|r| r.id);
    rows.dedup_by_key(|r| r.id);
    if rows.is_empty() || !can_snapshot(ctx, who) {
        return;
    }
    let mut done: Vec<(i64, u32)> = Vec::with_capacity(rows.len());
    for rec in &rows {
        let Some(removed) = apply_one(svc, ctx, who, me, rec) else {
            tracing::info!(
                removal = rec.id,
                "a pending removal is deferred: a matching stack is reserved or in use"
            );
            continue;
        };
        svc.grants.removal_once(rec.id);
        let mut event = audit::removal_apply(rec, removed);
        if let Some(actor) = crate::audit::actor_of(ctx, who) {
            event = event.by(actor);
        }
        ctx.audit.emit(event);
        done.push((rec.id, removed));
    }
    if done.is_empty() {
        return;
    }
    let ids: Vec<i64> = done.iter().map(|(id, _)| *id).collect();
    let now = ctx.clock.unix_ms.to_unix_time();
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        tracing::error!(
            ?ids,
            "pending removals were applied in memory but no snapshot could be taken: not marked"
        );
        return;
    };
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        AtomicBatch::removal_apply(Some(snap), done, now, Vec::new()),
        move |svc, ctx, result| match result {
            TicketResult::Committed(_) => svc.grants.removal_resolved(&ids, ctx.clock.game_ms),
            TicketResult::Failed(e) => {
                tracing::error!(?ids, error = %e, "CRITICAL: the RemovalApply batch failed; the ids stay applied for the rest of the run");
            }
            other => {
                tracing::error!(
                    ?ids,
                    ?other,
                    "CRITICAL: the RemovalApply batch completed with an unexpected result"
                );
            }
        },
    );
}

/// One row: the number of pieces (or penya) taken, `None` when a matching stack blocks it.
fn apply_one(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    me: CharacterId,
    rec: &PendingRemovalRecord,
) -> Option<u32> {
    if rec.item_id == 0 {
        return Some(take_penya(svc, ctx, who, me, rec));
    }
    let takes = plan_takes(ctx, who, me, rec)?;
    let mut removed = 0_u32;
    for t in takes {
        removed += t.n;
        take(svc, ctx, who, rec, &t);
    }
    Some(removed)
}

/// Which stacks give how many pieces; `None` when a matching stack that the row would reach is reserved or in use.
fn plan_takes(
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    me: CharacterId,
    rec: &PendingRemovalRecord,
) -> Option<Vec<Take>> {
    let p = ctx.player(who).and_then(Mover::player)?;
    let mut places: Vec<(Place, &ItemContainer)> = Vec::new();
    match rec.location {
        RemovalLocation::Inventory => places.push((Place::Bag, &p.items.inventory)),
        RemovalLocation::Pocket => {
            for (i, pk) in p.items.pockets.iter().enumerate() {
                places.push((Place::Pocket(i), &pk.items));
            }
        }
        RemovalLocation::Bank => {
            if let Some((i, b)) = p
                .items
                .banks
                .iter()
                .enumerate()
                .find_map(|(i, b)| b.as_ref().filter(|b| b.character_id == me).map(|b| (i, b)))
            {
                places.push((Place::Bank(i), &b.items));
            }
        }
        RemovalLocation::GuildBank => return Some(Vec::new()),
    }
    let mut remaining = rec.count;
    let mut out = Vec::new();
    for (place, container) in places {
        for (id, it) in container.iter() {
            if remaining == 0 {
                return Some(out);
            }
            if !removal_matches(rec, it) {
                continue;
            }
            if !it.is_usable() || (place == Place::Bag && p.items.is_using(id)) {
                return None;
            }
            let n = remaining.min(u32::from(it.count));
            remaining -= n;
            out.push(Take {
                place,
                id,
                item: it.clone(),
                n,
            });
        }
    }
    Some(out)
}

/// Takes `t.n` pieces out of the stack with the messages and the audit line of its container.
fn take(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    rec: &PendingRemovalRecord,
    t: &Take,
) {
    let n = u16::try_from(t.n).unwrap_or(u16::MAX);
    match t.place {
        Place::Bag => remove_item(svc, ctx, who, t.id, n, RemoveReason::Other("removal")),
        Place::Pocket(i) => {
            let mut fx = Fx::new();
            if let Some(pk) = ctx
                .worlds
                .player_mut(who.0)
                .and_then(Mover::player_mut)
                .and_then(|p| p.items.pockets.get_mut(i))
            {
                reduce(&mut pk.items, t.id, t.n);
            }
            fx.out.to_self(pb::PocketRemoveItem {
                pocket: i32::try_from(i).unwrap_or(0),
                obj_id: i32::from(t.id),
                num: i32::try_from(t.n).unwrap_or(i32::MAX),
            });
            fx.audit(
                ev::ITEM_DESTROY,
                json!({"item": item_json(&t.item), "count": t.n, "reason": "removal", "container": format!("pocket{i}"), "removal": rec.id}),
            );
            fx.flush(svc, ctx, who);
        }
        Place::Bank(i) => {
            let mut fx = Fx::new();
            let rest = u32::from(t.item.count).saturating_sub(t.n);
            if let Some(b) = ctx
                .worlds
                .player_mut(who.0)
                .and_then(Mover::player_mut)
                .and_then(|p| p.items.banks.get_mut(i))
                .and_then(Option::as_mut)
            {
                reduce(&mut b.items, t.id, t.n);
            }
            fx.out.to_self(pb::UpdateBankItem {
                slot: u32::try_from(i).unwrap_or(0),
                obj_id: u32::from(t.id),
                param: 0,
                value: rest,
            });
            fx.audit(
                ev::ITEM_DESTROY,
                json!({"item": item_json(&t.item), "count": t.n, "reason": "removal", "container": format!("bank{i}"), "removal": rec.id}),
            );
            fx.flush(svc, ctx, who);
        }
    }
}

/// Removes `n` pieces of the stack `id`; the entry goes when nothing is left.
fn reduce(c: &mut ItemContainer, id: StorageId, n: u32) {
    let Some(it) = c.get_at_id_mut(id) else {
        return;
    };
    let have = u32::from(it.count);
    if n >= have {
        c.remove_at_id(id);
    } else if let Some(it) = c.get_at_id_mut(id) {
        it.count = u16::try_from(have - n).unwrap_or(u16::MAX);
    }
}

/// `item_id == 0`: penya from the wallet or the own bank, clamped at 0; returns what was taken.
fn take_penya(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    me: CharacterId,
    rec: &PendingRemovalRecord,
) -> u32 {
    let want = i64::from(rec.count);
    match rec.location {
        RemovalLocation::Inventory => {
            let have = ctx
                .player(who)
                .and_then(Mover::player)
                .map_or(0, |p| i64::from(p.items.gold));
            let took = want.min(have).max(0);
            if took > 0 {
                add_gold(svc, ctx, who, -took, true);
                crate::audit::emit_for(
                    ctx,
                    who,
                    ev::ITEM_DESTROY,
                    json!({"item": null, "count": 0, "gold": took, "reason": "removal", "container": "inventory", "removal": rec.id}),
                );
            }
            u32::try_from(took).unwrap_or(0)
        }
        RemovalLocation::Bank => {
            let mut fx = Fx::new();
            let mut took = 0_i64;
            let mut wallet = 0;
            let mut slot = 0;
            if let Some(p) = ctx.worlds.player_mut(who.0).and_then(Mover::player_mut) {
                wallet = p.items.gold;
                if let Some((i, b)) =
                    p.items.banks.iter_mut().enumerate().find_map(|(i, b)| {
                        b.as_mut().filter(|b| b.character_id == me).map(|b| (i, b))
                    })
                {
                    took = want.min(b.gold).max(0);
                    b.gold -= took;
                    slot = i;
                }
            }
            if took > 0 {
                let after = ctx
                    .player(who)
                    .and_then(Mover::player)
                    .and_then(|p| p.items.bank(slot))
                    .map_or(0, |b| b.gold);
                fx.out.to_self(pb::PutGoldBank {
                    slot: u32::try_from(slot).unwrap_or(0),
                    gold: u32::try_from(wallet.max(0)).unwrap_or(0),
                    bank_gold: u32::try_from(after.max(0)).unwrap_or(u32::MAX),
                });
                fx.audit(
                    ev::ITEM_DESTROY,
                    json!({"item": null, "count": 0, "gold": took, "reason": "removal", "container": format!("bank{slot}"), "removal": rec.id}),
                );
                fx.flush(svc, ctx, who);
            }
            u32::try_from(took).unwrap_or(0)
        }
        // penya lives in the wallet and in the banks only
        RemovalLocation::Pocket | RemovalLocation::GuildBank => 0,
    }
}
