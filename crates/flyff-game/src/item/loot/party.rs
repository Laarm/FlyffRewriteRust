//! The party share of a pickup (Plan/19 §6.7; WP-ITEM-23; src:_Common/MoverActEvent.cpp:2378-2544 `SubLootDropMobParty`,
//! src:_Common/MoverEquip.cpp:2338-2420 `PickupGold`): the receiver choice for a monster item, the round-robin
//! cursor, the texts, and the split of monster gold among the members within 32 m.
//!
//! This is the only implementation of these rules. Doc 21 only exposes the party state: the picker's
//! [`PartyLink`](crate::social::party::PartyLink) (the members in party order with their avatars, the item share
//! mode and the cursor), which is all the world phase can reach.

use super::{LootResult, audit_pickup, flush, ground_gold};
use crate::item::container::DeferSerials;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::gold::core::pickup_gold_core;
use crate::item::model::Item;
use crate::item::ops::{ItemError, ItemOrigin, core as ops};
use crate::social::party::share::{InRange, SHARE_RANGE, link_members_in_range};
use crate::social::party::{ItemShareMode, PartyLink, write_cursor};
use crate::world::ItemDrop;
use crate::world::phases::FrameCtx;
use flyff_common::defs::itemkind::IK1_GOLD;
use flyff_common::defs::text::{TID_GAME_FROM2, TID_GAME_LACKSPACE, TID_GAME_REAPITEM};
use flyff_common::{CharacterId, ObjId};
use flyff_db::AuditLog;

/// The picker's party link when the pickup is shared: a monster drop picked up by a member of a party.
fn shared_link(ctx: &FrameCtx<'_>, picker: ObjId, drop: &ItemDrop) -> Option<PartyLink> {
    if !drop.meta.from_monster {
        return None;
    }
    ctx.store.mover(picker)?.player()?.social.party_link.clone()
}

/// `DoLoot` with the party rules: `None` when the pickup is not shared (no party, or not a monster drop) and the
/// solo path applies; else the result of the shared pickup. The effects of the picker are appended to `fx` (so its
/// messages keep the position the solo path gives them); every other member's messages are flushed at once.
pub(super) fn do_loot_shared(
    env: &ItemEnv<'_>,
    ctx: &mut FrameCtx<'_>,
    picker: ObjId,
    drop: &ItemDrop,
    fx: &mut Fx,
    audit: &AuditLog,
) -> Option<LootResult> {
    let link = shared_link(ctx, picker, drop)?;
    let prop = env.data().item(drop.item.item_id)?;
    let cands = link_members_in_range(ctx.store, &link, picker, SHARE_RANGE);
    let mut fxs: Vec<(ObjId, Fx)> = Vec::new();
    let result = if prop.item_kind1 == IK1_GOLD {
        split_gold(env, ctx, picker, drop, &cands, &mut fxs)
    } else {
        share_item(env, ctx, picker, drop, &link, &cands, &mut fxs)
    };
    for (o, f) in fxs {
        if o == picker {
            merge(fx, f);
        } else {
            flush(ctx, o, f, audit);
        }
    }
    Some(result)
}

/// Appends the messages, audit lines and flags of `from` to `into`.
fn merge(into: &mut Fx, mut from: Fx) {
    for m in from.out.take() {
        into.out.push(m.aud, m.body);
    }
    into.audit.append(&mut from.audit);
    into.urgent |= from.urgent;
    into.follow.append(&mut from.follow);
}

/// The effects collected for `o` (created on first use, in first-use order).
fn fx_of(fxs: &mut Vec<(ObjId, Fx)>, o: ObjId) -> &mut Fx {
    if let Some(i) = fxs.iter().position(|(x, _)| *x == o) {
        return &mut fxs[i].1;
    }
    fxs.push((o, Fx::new()));
    let last = fxs.len() - 1;
    &mut fxs[last].1
}

/// `PickupGold( nGold, TRUE )` with a party: `share = amount / n` to each member in range (when above 0), the rest
/// to one of them chosen at random (`random(n)`); every payment is a `PickupGoldCore` with its own gold line. The
/// cursor is never touched. `n == 0` cannot happen (the picker is always in range); the amount then goes to the
/// picker.
fn split_gold(
    env: &ItemEnv<'_>,
    ctx: &mut FrameCtx<'_>,
    picker: ObjId,
    drop: &ItemDrop,
    cands: &InRange,
    fxs: &mut Vec<(ObjId, Fx)>,
) -> LootResult {
    let amount = ground_gold(drop);
    if amount <= 0 {
        tracing::error!(amount, "loot: a ground gold seed without gold");
        return LootResult::Looted;
    }
    let pay = |ctx: &mut FrameCtx<'_>, fxs: &mut Vec<(ObjId, Fx)>, o: ObjId, gold: i64| {
        if let Some(m) = ctx.store.mover_mut(o) {
            pickup_gold_core(m, env, &mut DeferSerials, gold, true, fx_of(fxs, o));
        }
    };
    let n = cands.len();
    if n == 0 {
        debug_assert!(false, "the picker is always in range of itself");
        pay(ctx, fxs, picker, amount);
        return LootResult::Looted;
    }
    let n64 = n as i64;
    let (share, rest) = (amount / n64, amount % n64);
    if share > 0 {
        for &(_, o) in cands {
            pay(ctx, fxs, o, share);
        }
    }
    if rest > 0 {
        let i = ctx.rng.random_below(n as u32) as usize;
        if let Some(&(_, o)) = cands.get(i) {
            pay(ctx, fxs, o, rest);
        }
    }
    LootResult::Looted
}

/// `SubLootDropMobParty` (19 §6.7): chooses the receiver by the item share mode among the members in range,
/// moves the cursor to it **before** the creation (a full inventory still advances it), creates the item for the
/// receiver and writes the texts: `TID_GAME_REAPITEM "name"` to the receiver, `"<receiver><from> <name>"` to every
/// other member in range, only `TID_GAME_LACKSPACE` to the receiver when it has no room (the item stays).
fn share_item(
    env: &ItemEnv<'_>,
    ctx: &mut FrameCtx<'_>,
    picker: ObjId,
    drop: &ItemDrop,
    link: &PartyLink,
    cands: &InRange,
    fxs: &mut Vec<(ObjId, Fx)>,
) -> LootResult {
    let Some(prop) = env.data().item(drop.item.item_id) else {
        return LootResult::Gone;
    };
    let picker_c = ctx
        .store
        .mover(picker)
        .and_then(|m| m.player())
        .map_or(CharacterId(0), |p| p.ident.character_id);
    let (receiver_c, receiver_o) = choose_receiver(ctx, link, cands, (picker_c, picker));
    write_cursor(ctx.store, link, receiver_c);

    let item = Item::from_record(&drop.item);
    let count = u32::from(item.count);
    let receiver_fx = fx_of(fxs, receiver_o);
    let created = match ctx.store.mover_mut(receiver_o) {
        Some(m) => ops::create_item(
            m,
            env,
            &mut DeferSerials,
            item,
            ItemOrigin::Pickup,
            receiver_fx,
        ),
        None => return LootResult::Gone,
    };
    match created {
        Ok(_) => {
            let receiver_name = ctx
                .store
                .mover(receiver_o)
                .and_then(|m| m.player())
                .map(|p| p.ident.name.clone())
                .unwrap_or_default();
            let from = env.data().texts.get(TID_GAME_FROM2).to_owned();
            for &(_, o) in cands {
                if o == receiver_o {
                    let f = fx_of(fxs, o);
                    f.defined(TID_GAME_REAPITEM, vec![format!("\"{}\"", prop.name)]);
                    audit_pickup(f, drop, count);
                } else {
                    fx_of(fxs, o).defined(
                        TID_GAME_REAPITEM,
                        vec![format!("\"{receiver_name}{from} {}\"", prop.name)],
                    );
                }
            }
            LootResult::Looted
        }
        Err(ItemError::LackSpace) => {
            fx_of(fxs, receiver_o).text(TID_GAME_LACKSPACE);
            LootResult::LackSpace
        }
        Err(ItemError::NoProp) => LootResult::Gone,
        Err(e) => {
            tracing::warn!(error = %e, "loot: the item could not be created");
            LootResult::LackSpace
        }
    }
}

/// The receiver of a monster item (`switch( m_nTroupeShareItem )`): `cands` are the members within 32 m in party
/// order, `picker` the member that walked onto the item.
fn choose_receiver(
    ctx: &mut FrameCtx<'_>,
    link: &PartyLink,
    cands: &InRange,
    picker: (CharacterId, ObjId),
) -> (CharacterId, ObjId) {
    let n = cands.len();
    let Some(&first) = cands.first() else {
        return picker;
    };
    match link.item_share {
        ItemShareMode::RoundRobin => match cands.iter().position(|(c, _)| *c == link.loot_cursor) {
            Some(i) => cands.get((i + 1) % n).copied().unwrap_or(first),
            None => first,
        },
        ItemShareMode::Leader => {
            if link.is_leader(first.0) {
                first
            } else {
                picker
            }
        }
        ItemShareMode::Random => {
            let i = ctx.rng.random_below(n as u32) as usize;
            cands.get(i).copied().unwrap_or(first)
        }
        ItemShareMode::Finder => picker,
    }
}
