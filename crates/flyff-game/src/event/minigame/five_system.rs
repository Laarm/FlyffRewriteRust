//! Five-system (dice betting on six faces; Plan/22 §16.4; src:_Common/MiniGame.cpp:433-614,
//! src:WORLDSERVER/DPSrvr.cpp:9656-9716).
//!
//! The stakes of a player (`MiniGamePlayer.five_bets`) are taken from the wallet when they are placed and paid back
//! when the window closes, a new window opens (FIX: the original zeroed the stakes there, the penya were lost), the
//! player logs out or the dice roll. `MiniGame_FiveSystem.txt` gives the stake limits, the payout `MULTIPLE` and, per
//! number of faces that hold the top stake, the chances of such a face and of every other face.
//!
//! Answers (`MiniGame.FiveSystemResult.result`): `996` stake accepted (`penya`, `bet_num`), `997` refused, `998`
//! stake outside the limits, `999` not enough penya, `0..=5` the rolled face (`penya` = the payout).

use super::deliver::{audit_bet, credit_penya};
use super::{reply, wallet};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::item::gold::add_gold;
use flyff_common::rng::GameRng;
use flyff_data::events::minigame::{FiveSystemRow, MiniGameTables};
use flyff_proto::pb::mini_game::{FiveSystemOpenWnd, FiveSystemResult, Sub};
use serde_json::json;

/// `FIVESYSTEM_BETOK`.
pub const BET_OK: i32 = 996;
/// `FIVESYSTEM_FAILED`.
pub const FAILED: i32 = 997;
/// `FIVESYSTEM_OVERMAX`.
pub const OVER_MAX: i32 = 998;
/// `FIVESYSTEM_NOTENOUGH`.
pub const NOT_ENOUGH: i32 = 999;
/// The number of dice faces.
pub const FACES: usize = 6;

fn result(fx: &mut Fx, result: i32, penya: i32, bet_num: i32) {
    reply(
        fx,
        Sub::FiveSystemResult(FiveSystemResult {
            result,
            penya,
            bet_num,
        }),
    );
}

/// Pays back every outstanding stake of `who` and clears them: the close of the window (`DestroyWnd_FiveSystem`),
/// the start of a new one and the logout. No answer message; nothing happens without a stake. The penya are mailed
/// when the wallet would overflow.
pub fn close_five_system(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(g) = svc.event.minigames.players.get_mut(&who.0) else {
        return;
    };
    let total: i64 = g.five_bets.iter().map(|b| i64::from(*b)).sum();
    g.five_bets = [0; FACES];
    if total <= 0 {
        return;
    }
    let mut fx = Fx::new();
    let credit = credit_penya(svc, ctx, who, total, &mut fx);
    audit_bet(
        &mut fx,
        "fivesystem",
        "refund",
        json!({"penya": total, "mailed": credit == super::deliver::Credit::Mailed}),
    );
    fx.flush(svc, ctx, who);
}

/// `FiveSystemOpenWnd` (after the NPC check): refunds the stakes of an earlier window, then answers the limits.
pub(crate) fn open_wnd(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    close_five_system(svc, ctx, who);
    svc.event.minigames.player_mut(who.0);
    let data = ctx.data;
    let t = &data.minigames;
    let mut fx = Fx::new();
    reply(
        &mut fx,
        Sub::FiveSystemOpenWnd(FiveSystemOpenWnd {
            min_bet: t.five_min_penya,
            max_bet: t.five_max_penya,
            multiple: t.five_multiple,
        }),
    );
    fx.flush(svc, ctx, who);
}

/// `FiveSystemBet{bet_num, bet_penya}` (after the NPC check): the checks in the order of the original with the face
/// first (PL-105: the original indexed the stake array before it validated the face).
pub(crate) fn bet(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    bet_num: i32,
    penya: i32,
) {
    let mut fx = Fx::new();
    let Some(slot) = usize::try_from(bet_num).ok().filter(|s| *s < FACES) else {
        result(&mut fx, FAILED, 0, 0);
        fx.flush(svc, ctx, who);
        return;
    };
    let data = ctx.data;
    let t = &data.minigames;
    let old = svc.event.minigames.player_mut(who.0).five_bets[slot];
    if old == 0 && penya == 0 {
        // nothing to withdraw, nothing to place: no answer, no log (legacy)
        return;
    }
    if penya < 0 {
        result(&mut fx, FAILED, 0, 0);
        fx.flush(svc, ctx, who);
        return;
    }
    if penya != 0 && (penya < t.five_min_penya || penya > t.five_max_penya) {
        result(&mut fx, OVER_MAX, 0, 0);
        fx.flush(svc, ctx, who);
        return;
    }
    let gold = wallet(ctx, who);
    if gold + i64::from(old) < i64::from(penya) {
        result(&mut fx, NOT_ENOUGH, 0, 0);
        fx.flush(svc, ctx, who);
        return;
    }
    // the difference to the old stake: a smaller stake pays back (mailed on overflow), a bigger one is taken
    let diff = i64::from(old) - i64::from(penya);
    let mut mailed = false;
    if diff > 0 {
        mailed = credit_penya(svc, ctx, who, diff, &mut fx) == super::deliver::Credit::Mailed;
    } else if diff < 0 {
        add_gold(svc, ctx, who, diff, true);
        fx.mark_urgent();
    }
    svc.event.minigames.player_mut(who.0).five_bets[slot] = penya;
    audit_bet(
        &mut fx,
        "fivesystem",
        "bet",
        json!({"face": slot, "old": old, "stake": penya, "mailed": mailed}),
    );
    result(&mut fx, BET_OK, penya, bet_num);
    fx.flush(svc, ctx, who);
}

/// `Result_FiveSystem` over the stakes and the table (no clock, no player): `(winner face, the row that applied)`.
/// `max_bet` is the highest stake and `k` the number of faces that hold it (`k == 6` counts as 0); the row of
/// `five_percent` with `count == k` gives the chance of a face that holds the top stake (`min`, the data's `dwMin`)
/// and of every other face (`max`). The faces are summed in order and the first whose running sum is above `roll`
/// wins (face 0 as the fallback). A table without the row gives face 0 and no row (the original returned 0 before it
/// rolled).
pub fn pick_winner(
    tables: &MiniGameTables,
    bets: &[i32; FACES],
    roll: impl FnOnce() -> u32,
) -> (usize, Option<FiveSystemRow>) {
    let max_bet = bets.iter().copied().fold(0, i32::max);
    let held = bets.iter().filter(|b| **b == max_bet).count();
    let k = if held == FACES { 0 } else { held };
    let Some(row) = tables
        .five_percent
        .iter()
        .find(|r| usize::try_from(r.count).is_ok_and(|c| c == k))
        .copied()
    else {
        return (0, None);
    };
    let roll = u64::from(roll());
    let mut sum: u64 = 0;
    for (face, bet) in bets.iter().enumerate() {
        sum += u64::from(if *bet == max_bet { row.min } else { row.max });
        if sum > roll {
            return (face, Some(row));
        }
    }
    (0, Some(row))
}

/// `FiveSystemStart` (after the NPC check): rolls the dice over the stakes, pays `stake x multiple` of the winning
/// face (mailed on overflow), clears the stakes and answers the face and the payout. No stake at all is legal (the
/// original rolled and paid 0).
pub(crate) fn start(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let data = ctx.data;
    let t = &data.minigames;
    let bets = svc.event.minigames.player_mut(who.0).five_bets;
    let rng = ctx.rng.event();
    let (winner, row) = pick_winner(t, &bets, || rng.random_below(1_000_000));
    if row.is_none() && svc.event.minigames.warn_once(u32::MAX) {
        tracing::warn!(
            "MiniGame_FiveSystem.txt has no PERCENT row for this stake pattern; face 0 wins"
        );
    }
    let stake = bets.get(winner).copied().unwrap_or(0);
    let payout = i64::from(stake) * i64::from(t.five_multiple);
    svc.event.minigames.player_mut(who.0).five_bets = [0; FACES];
    let mut fx = Fx::new();
    let mut mailed = false;
    if payout > 0 {
        mailed = credit_penya(svc, ctx, who, payout, &mut fx) == super::deliver::Credit::Mailed;
    }
    audit_bet(
        &mut fx,
        "fivesystem",
        "roll",
        json!({"bets": bets, "face": winner, "payout": payout, "mailed": mailed}),
    );
    // `penya` is a sint32: the shipped 100 000 000 x 5 fits, a configured larger payout is clamped
    let shown = i32::try_from(payout).unwrap_or(i32::MAX);
    result(&mut fx, i32::try_from(winner).unwrap_or(0), shown, 0);
    fx.flush(svc, ctx, who);
}
