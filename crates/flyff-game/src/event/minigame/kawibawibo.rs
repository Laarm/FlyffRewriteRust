//! Kawibawibo (rock-paper-scissors for a win-streak prize; Plan/22 §16.1; src:_Common/MiniGame.cpp:74-199,
//! src:WORLDSERVER/DPSrvr.cpp:9569-9594).
//!
//! `MiniGame_KawiBawiBo.txt` lists, per win streak 0..=10, the chances (parts per million) of a draw and a loss and
//! the prize of that streak. A round needs a coupon `II_SYS_SYS_EVE_KAWIBAWIBO` unless the last round was a draw or
//! a win (the free replay); at the top streak the next round is a forced loss. `KawibawiboGetItem` pays the prize of
//! the current streak and resets it.

use super::deliver::{audit_bet, give_prize};
use super::{KawiResult, first_usable, reply};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::item::ops::{RemoveReason, remove_item};
use flyff_common::defs::item::II_SYS_SYS_EVE_KAWIBAWIBO;
use flyff_common::defs::text::TID_MMI_KAWIBAWIBO;
use flyff_common::rng::GameRng;
use flyff_proto::pb::mini_game::{KawibawiboResult, Sub};
use serde_json::json;

/// `KawibawiboStart` (after the NPC check): one round.
pub(crate) fn start(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let data = ctx.data;
    let tables = &data.minigames;
    let (streak, last) = {
        let g = svc.event.minigames.player_mut(who.0);
        (g.kawi_streak, g.kawi_last)
    };
    let free = matches!(last, KawiResult::Draw | KawiResult::Win);
    let coupon = first_usable(ctx, who, II_SYS_SYS_EVE_KAWIBAWIBO);
    let row_ok = tables.find_kawibawibo(streak).is_some_and(|r| r.count != 0);
    let mut streak = streak;
    let result = if (!free && coupon.is_none()) || !row_ok {
        // no coupon, or no prize row for the streak (the original: a row without items): nothing is consumed
        KawiResult::Failed
    } else {
        if !free && let Some(id) = coupon {
            remove_item(svc, ctx, who, id, 1, RemoveReason::Other("kawibawibo"));
        }
        let draw_lost = tables
            .find_kawibawibo(streak)
            .map_or((0, 0), |r| (r.draw, r.lost));
        if streak >= tables.kawibawibo_max_win {
            // the top streak is a loss without a roll
            streak = 0;
            KawiResult::Lost
        } else {
            let r = ctx.rng.event().random_below(1_000_000);
            if r < draw_lost.0 {
                KawiResult::Draw
            } else if r < draw_lost.0.saturating_add(draw_lost.1) {
                streak = 0;
                KawiResult::Lost
            } else {
                streak += 1;
                KawiResult::Win
            }
        }
    };
    {
        let g = svc.event.minigames.player_mut(who.0);
        g.kawi_streak = streak;
        g.kawi_last = result;
    }
    let mut fx = Fx::new();
    let mut msg = KawibawiboResult {
        result: result as i32,
        win_count: streak,
        ..KawibawiboResult::default()
    };
    if result == KawiResult::Win {
        let row = tables.find_kawibawibo(streak);
        let next = tables.find_kawibawibo(streak.saturating_add(1));
        msg.item_id = row.map_or(0, |r| r.item);
        msg.item_count = row.map_or(0, |r| r.count);
        msg.next_item_id = next.map_or(0, |r| r.item);
        msg.next_item_count = next.map_or(0, |r| r.count);
        msg.max_win_count = tables.kawibawibo_max_win;
    }
    if result != KawiResult::Failed {
        audit_bet(
            &mut fx,
            "kawibawibo",
            "round",
            json!({"result": result as i32, "streak": streak, "coupon": !free}),
        );
    }
    reply(&mut fx, Sub::KawibawiboResult(msg));
    fx.flush(svc, ctx, who);
}

/// `KawibawiboGetItem` (after the NPC check): the prize of the current streak; the streak and the free replay end.
/// The answer carries the streak that was won (FIX of Plan §16.1: the original sent it before the reset too) and is
/// sent whether the prize went to the inventory or to the mailbox (FIX: the original left the window stuck on the
/// mail path).
pub(crate) fn get_item(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let data = ctx.data;
    let streak = svc.event.minigames.player_mut(who.0).kawi_streak;
    if streak <= 0 {
        return;
    }
    let Some(row) = data
        .minigames
        .find_kawibawibo(streak)
        .filter(|r| r.count != 0)
        .copied()
    else {
        return;
    };
    if data.item(row.item).is_none() {
        if svc.event.minigames.warn_once(row.item) {
            tracing::warn!(
                item = row.item,
                "Kawibawibo prize: the item has no prop; no prize is given"
            );
        }
        return;
    }
    {
        let g = svc.event.minigames.player_mut(who.0);
        g.kawi_streak = 0;
        g.kawi_last = KawiResult::GetItem;
    }
    let mut fx = Fx::new();
    let delivered = give_prize(
        svc,
        ctx,
        who,
        row.item,
        row.count,
        TID_MMI_KAWIBAWIBO,
        &mut fx,
    );
    audit_bet(
        &mut fx,
        "kawibawibo",
        "prize",
        json!({"streak": streak, "item": row.item, "count": row.count, "delivered": delivered.is_some()}),
    );
    reply(
        &mut fx,
        Sub::KawibawiboResult(KawibawiboResult {
            result: KawiResult::GetItem as i32,
            win_count: streak,
            item_id: row.item,
            item_count: row.count,
            ..KawibawiboResult::default()
        }),
    );
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}
