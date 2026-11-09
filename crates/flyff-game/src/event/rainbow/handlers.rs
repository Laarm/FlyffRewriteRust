//! The five client messages of the rainbow race (Plan/22 §14.3, §19.1; `DPSrvr.cpp:12283-12335`).
//!
//! Every handler acts for the session's player (VR-001: the message carries no actor). The NPC rules of 22 §2.6 are
//! checked FIRST, silently (VR-012, PL-107): the application and its window need `MMI_LORD_RAINBOWAPPLICATION` (107),
//! the previous ranking 110, the finish report 112 and the opening of a game window the NPC of that game (113-119) -
//! the legacy checked no distance at all. The mini-game packet goes through the VR-064 / PL-105 pipeline of
//! [`on_mini_game_packet`].

use super::games::{self, ALL_COMPLETED, MpState, RaceGameId};
use super::{RrState, character_of, complete_current, finish_racer};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::tickets::TicketResult;
use flyff_common::UnixTime;
use flyff_common::config::EventId;
use flyff_common::defs::neuz::{
    MMI_LORD_RAINBOWAPPLICATION, MMI_LORD_RAINBOWEND, MMI_LORD_RAINBOWTOPTEN,
};
use flyff_common::defs::text::{
    TID_GAME_LACKMONEY, TID_GAME_RAINBOWRACE_APPLICATION_COMPLETE,
    TID_GAME_RAINBOWRACE_ISAPPLICATIONED, TID_GAME_RAINBOWRACE_ISFINISHED,
    TID_GAME_RAINBOWRACE_ISNOTALLCOMPLETED, TID_GAME_RAINBOWRACE_ISNOTAPPLICATIONTIME,
    TID_GAME_RAINBOWRACE_ISNOTNOWMINIGAME,
};
use flyff_db::audit::content as audit;
use flyff_db::{AtomicBatch, RainbowRaceEntryRecord};
use flyff_proto::pb;

/// `true` when an NPC offering `menu` stands within the NPC range of `who` (VR-012, PL-107).
fn near_npc(ctx: &GameCtx<'_>, who: PlayerRef, menu: u32) -> bool {
    ctx.player(who).is_some_and(|m| {
        ctx.worlds
            .close_npc(menu, m.core.world, m.core.layer, m.core.pos)
            .is_some()
    })
}

fn defined(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    notice::defined(ctx, who, tid, Vec::new());
}

/// `RainbowRaceApplicationOpenWndReq` (NPC 107): `RainbowRaceApplicationOpenWndNtf{applicants}` (no flag check, as the
/// legacy).
pub fn on_application_open_wnd(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !near_npc(ctx, who, MMI_LORD_RAINBOWAPPLICATION) {
        return;
    }
    let applicants = i32::try_from(svc.event.rainbow.racers.len()).unwrap_or(i32::MAX);
    crate::chat::send_to(
        ctx,
        who,
        pb::RainbowRaceApplicationOpenWndNtf { applicants },
    );
}

/// `RainbowRacePrevRankingOpenWndReq` (NPC 110): `RainbowRacePrevRankingOpenWndNtf{id_players}` with the previous
/// ranking, at most five (no flag check, as the legacy).
pub fn on_prev_ranking_open_wnd(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !near_npc(ctx, who, MMI_LORD_RAINBOWTOPTEN) {
        return;
    }
    let id_players = svc
        .event
        .rainbow
        .prev_ranking
        .iter()
        .take(super::PREV_RANKING_MAX)
        .map(|c| c.0)
        .collect();
    crate::chat::send_to(
        ctx,
        who,
        pb::RainbowRacePrevRankingOpenWndNtf { id_players },
    );
}

/// `RainbowRaceApplication` (`SetApplicationUser`), in this order; the first failure answers and changes nothing:
/// NPC 107 (silent); flag 25 off (silent); the state is not Application - a game master in Closed gets
/// `"GM Application..."` and goes on, everybody else `ISNOTAPPLICATIONTIME`; the gold is below the fee of today
/// (`LACKMONEY`; BEFORE the duplicate check, as the legacy); the character already applied (`ISAPPLICATIONED`); a
/// session that cannot be snapshotted (silent).
///
/// The commit (validate-then-commit, VR-018): the fee leaves the wallet, the racer enters the table, the applicant is
/// snapshotted AFTER the debit and ONE `AtomicBatch::rainbow_apply` (the snapshot and the entry row `N`) goes out, then
/// `APPLICATION_COMPLETE` and the audit line `rainbow.apply`. A failed batch is logged, never compensated (19 §20.3).
pub fn on_application(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !near_npc(ctx, who, MMI_LORD_RAINBOWAPPLICATION)
        || !svc.event.flags.is_on(EventId::RainbowRace)
    {
        return;
    }
    let Some((c, gm)) = ctx
        .player(who)
        .and_then(|m| m.player())
        .map(|p| (p.ident.character_id, p.ident.authority.is_gm()))
    else {
        return;
    };
    let state = svc.event.rainbow.state;
    if state != RrState::Application {
        if gm && state == RrState::Closed {
            notice::text(ctx, who, "GM Application...");
        } else {
            defined(ctx, who, TID_GAME_RAINBOWRACE_ISNOTAPPLICATIONTIME);
            return;
        }
    }
    let fee = svc
        .event
        .configs
        .rainbow
        .fee_on(ctx.clock.local.weekday)
        .max(0);
    if ctx.player(who).map_or(0, crate::item::gold::core::gold) < fee {
        defined(ctx, who, TID_GAME_LACKMONEY);
        return;
    }
    if svc.event.rainbow.racers.contains_key(&c) {
        defined(ctx, who, TID_GAME_RAINBOWRACE_ISAPPLICATIONED);
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!(
            "a rainbow race application of a player without a savable session was refused"
        );
        return;
    }

    // ---- the commit block: nothing below can fail ----
    let now = UnixTime(ctx.clock.unix_ms.0.div_euclid(1000));
    let round = svc.event.rainbow.round;
    crate::item::gold::add_gold(svc, ctx, who, -fee, true);
    svc.event.rainbow.racers.insert(c, super::Racer::new(now));
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        // unreachable after `can_snapshot`; undo rather than lose the fee
        svc.event.rainbow.racers.remove(&c);
        crate::item::gold::add_gold(svc, ctx, who, fee, true);
        tracing::error!("a rainbow race application lost its snapshot after the check: undone");
        return;
    };
    let batch = AtomicBatch::rainbow_apply(snap, RainbowRaceEntryRecord::apply(round, c, now));
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        move |_svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => tracing::error!(
                error = %e,
                character = c.0,
                round,
                "RainbowApply batch failed: the application exists in memory only"
            ),
            other => tracing::error!(?other, "RainbowApply completed unexpectedly"),
        },
    );
    defined(ctx, who, TID_GAME_RAINBOWRACE_APPLICATION_COMPLETE);
    let mut ev = audit::rainbow_apply(round, c, fee);
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
}

/// `RainbowRaceMiniGamePacket` (`OnRainbowRaceMiniGamePacket` -> `CRainbowRace::OnMiniGamePacket`), the VR-064 /
/// PL-105 pipeline. A failure at any step leaves the racer, the `event` RNG and the outbox unchanged; only step 3
/// sends a text:
///
/// 1. the shape ([`games::validate_shape`], silent): the packet exists, `now_game` is exactly one of the seven bits,
///    `state` is 0-5 and an ext packet carries at most three texts of at most 256 bytes;
/// 2. the sender is a racer, the state is Progress and the racer has not finished (silent; FIX: the legacy had no state
///    check, an applicant could play before the race);
/// 3. `now_game` is the racer's game, else `ISNOTNOWMINIGAME` (also after the seventh game, when the racer has none);
/// 4. opening a window needs the NPC of that game within reach (silent);
/// 5. the game accepts the request (silent): no question yet, a typing play without its three texts, a card position
///    outside the cards;
/// 6. the game plays, its replies go out, and a completed game runs `complete_current`.
pub fn on_mini_game_packet(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::RainbowRaceMiniGamePacket,
) {
    // 1
    let Some(req) = games::validate_shape(&m) else {
        return;
    };
    // 2
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let rr = &svc.event.rainbow;
    if rr.state != RrState::Progress {
        return;
    }
    let Some(racer) = rr.racers.get(&c).filter(|r| !r.finished) else {
        return;
    };
    // 3
    if req.now_game != racer.now_game {
        defined(ctx, who, TID_GAME_RAINBOWRACE_ISNOTNOWMINIGAME);
        return;
    }
    // 4
    let Some(id) = RaceGameId::from_bit(u32::from(req.now_game)) else {
        return;
    };
    if req.state == MpState::OpenWnd && !near_npc(ctx, who, id.menu()) {
        return;
    }
    // 5
    if !racer.game.as_ref().is_some_and(|g| g.accepts(&req)) {
        return;
    }
    // 6
    let step = match svc
        .event
        .rainbow
        .racers
        .get_mut(&c)
        .and_then(|r| r.game.as_mut())
    {
        Some(game) => game.play(ctx.rng.event(), &req),
        None => return,
    };
    super::send_replies(ctx, who, step.replies);
    if step.completed {
        complete_current(svc, ctx, who, c);
    }
}

/// `RainbowRaceReqFinish` (`SetRanking`): NPC 112 (silent); not a racer (silent); finished (`ISFINISHED`); not all seven
/// games done (`ISNOTALLCOMPLETED`); else `finish_racer`.
pub fn on_req_finish(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !near_npc(ctx, who, MMI_LORD_RAINBOWEND) {
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let Some(racer) = svc.event.rainbow.racers.get(&c) else {
        return;
    };
    if racer.finished {
        defined(ctx, who, TID_GAME_RAINBOWRACE_ISFINISHED);
        return;
    }
    if racer.completed != ALL_COMPLETED {
        defined(ctx, who, TID_GAME_RAINBOWRACE_ISNOTALLCOMPLETED);
        return;
    }
    finish_racer(svc, ctx, who, c);
}
