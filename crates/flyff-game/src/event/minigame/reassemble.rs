//! Reassemble (nine puzzle cards for a weighted prize; Plan/22 §16.2; src:_Common/MiniGame.cpp:201-285).
//!
//! `MiniGame_Reassemble.txt` lists the cards in order (shipped: HOLYRED x3, HOLYBLUE x3, HOLYYELLOW x3) and the prize
//! list. `ReassembleStart` hands in one inventory item per card slot; every slot must hold the card of its place, a
//! stack that fills several slots must hold at least as many pieces as slots (FIX: the original checked no count, so
//! one card paid for three slots). Any failure is silent (legacy). The cards are consumed, then the prize is picked.

use super::deliver::{audit_bet, give_prize};
use super::pick::weighted_pick;
use super::{reply, usable_item};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{RemoveReason, remove_item};
use flyff_common::defs::text::TID_MMI_REASSEMBLE;
use flyff_common::rng::GameRng;
use flyff_proto::pb::mini_game::{ReassembleOpenWnd, ReassembleResult, Sub};
use serde_json::json;
use std::collections::BTreeMap;

/// `ReassembleOpenWnd` (after the NPC check): the card list in order.
pub(crate) fn open_wnd(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let mut fx = Fx::new();
    reply(
        &mut fx,
        Sub::ReassembleOpenWnd(ReassembleOpenWnd {
            card_item_ids: ctx.data.minigames.reassemble_cards.clone(),
        }),
    );
    fx.flush(svc, ctx, who);
}

/// The storage ids of the nine slots when they are all valid, else `None`: every slot an own usable item of the
/// card of its place, and no stack used more often than it holds pieces.
fn check_slots(
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    cards: &[u32],
    ids: &[u32],
) -> Option<Vec<StorageId>> {
    if ids.len() != cards.len() {
        return None;
    }
    let mut uses: BTreeMap<StorageId, u32> = BTreeMap::new();
    let mut slots = Vec::with_capacity(ids.len());
    for (obj_id, card) in ids.iter().zip(cards) {
        let (id, item_id, count) = usable_item(ctx, who, *obj_id)?;
        if item_id != *card {
            return None;
        }
        let n = uses.entry(id).or_default();
        *n += 1;
        if *n > u32::from(count) {
            return None;
        }
        slots.push(id);
    }
    Some(slots)
}

/// `ReassembleStart` (after the NPC check): the nine cards are consumed and the weighted prize is given.
pub(crate) fn start(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, ids: &[u32]) {
    let data = ctx.data;
    let tables = &data.minigames;
    // a wrong slot count (VR-064) or any wrong card: silent
    let Some(slots) = check_slots(ctx, who, &tables.reassemble_cards, ids) else {
        return;
    };
    for id in &slots {
        remove_item(svc, ctx, who, *id, 1, RemoveReason::Other("reassemble"));
    }
    let r = ctx.rng.event().random_below(1_000_000);
    let mut fx = Fx::new();
    let prize = weighted_pick(&tables.reassemble_items, r).copied();
    let (item, count) = match prize {
        Some(p) => {
            let given = give_prize(svc, ctx, who, p.item, p.count, TID_MMI_REASSEMBLE, &mut fx);
            if given.is_some() {
                (p.item, p.count)
            } else {
                (0, 0)
            }
        }
        None => (0, 0),
    };
    audit_bet(
        &mut fx,
        "reassemble",
        "prize",
        json!({"roll": r, "item": item, "count": count}),
    );
    reply(
        &mut fx,
        Sub::ReassembleResult(ReassembleResult {
            item_id: item,
            count,
        }),
    );
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}
