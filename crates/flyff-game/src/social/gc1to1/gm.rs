//! The GM API of the 1:1 guild combat (`FuncTextCmd.cpp:4280-4334`; the rows `/gc1to1open [1]`, `/gc1to1close`,
//! `/gc1to1next` and `/content gc1to1` of WP-GM-26 call it by these exact names). The functions never print and never
//! check authority; the flag `events.guild_combat_1to1` is the CALLER's check (the GM layer answers its "is off" text
//! first).

use super::{ArenaState, Gc1to1State, arena, big_caption_all, tender, text_of};
use crate::ctx::{GameCtx, Services};
use flyff_common::defs::text::TID_GAME_GUILDCOMBAT1TO1_ALLCLOSE;

/// `/gc1to1open` in a state other than Close ("already OPEN State!!!"; FIX: the original tested `!= OPEN` and re-opened a
/// running Entrance or War).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the 1:1 guild combat is not closed")]
pub struct AlreadyOpen;

/// `/gc1to1close` and `/gc1to1next` in state Close ("Is CLOSE State!!!").
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the 1:1 guild combat is closed")]
pub struct IsClose;

/// `/gc1to1open [1]` (`4284-4300`): [`tender::open`] (`server_down` = the argument 1: the unclaimed refunds of the last
/// event stay claimable).
pub fn open(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    server_down: bool,
) -> Result<(), AlreadyOpen> {
    tender::open(svc, ctx, server_down)
}

/// `/gc1to1close` (`4302-4314`). Close -> `Err(IsClose)` (FIX: the legacy set War over an empty arena list and the next
/// second's all-close dropped the in-memory tenders while their rows stayed `T`).
///
/// * Open (no arena yet): the state returns to Close at once, the big caption `ALLCLOSE` goes to everybody and `{0, 0}` to
///   every online member of every tendering guild; the tenders with their line-ups and their `T` rows are KEPT for the next
///   opening and the round is unchanged (as after `NOGAME`; the `N` and `F` writes of the opening stay).
/// * Entrance or War: the state becomes War and every arena that has not ended runs its close wait with the deadline now
///   (more round wins wins, the chips go to the present members of the winner, the captions, both tenders `T -> E`); the
///   next slot closes the arenas (Madrigal, forced) and runs the all-close. FIX: an arena that already ran its close is
///   skipped (the legacy re-ran it and paid its chips twice).
pub fn close_all(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), IsClose> {
    match svc.social.gc1to1.state() {
        Gc1to1State::Close => Err(IsClose),
        Gc1to1State::Open => {
            {
                let g = &mut svc.social.gc1to1;
                g.set_state(Gc1to1State::Close);
                g.due = flyff_common::GameMs(0);
            }
            let line = text_of(ctx, TID_GAME_GUILDCOMBAT1TO1_ALLCLOSE);
            big_caption_all(svc, ctx, &line);
            for (c, obj) in tender::tender_member_avatars(svc, ctx) {
                super::send_manager_state(svc, ctx, obj, c);
            }
            Ok(())
        }
        Gc1to1State::Entrance | Gc1to1State::War => {
            svc.social.gc1to1.set_state(Gc1to1State::War);
            for idx in 0..svc.social.gc1to1.arenas.len() {
                if svc.social.gc1to1.arenas.get(idx).is_some_and(|a| !a.ended) {
                    arena::close_wait(svc, ctx, idx, true);
                }
            }
            Ok(())
        }
    }
}

/// `/gc1to1next` (`4316-4334`): Close -> `Err(IsClose)`; War -> every arena's deadline is now (Wait starts its round, War
/// applies the timeout rule, CloseWait closes); Open and Entrance -> the manager's deadline is now (Open pairs and opens
/// the entrance, Entrance starts the war, both at the next slot).
pub fn skip_timer(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), IsClose> {
    let now = ctx.clock.game_ms;
    let g = &mut svc.social.gc1to1;
    match g.state() {
        Gc1to1State::Close => Err(IsClose),
        Gc1to1State::War => {
            for a in &mut g.arenas {
                if matches!(
                    a.state,
                    ArenaState::Wait | ArenaState::War | ArenaState::CloseWait
                ) {
                    a.due = now;
                }
            }
            Ok(())
        }
        Gc1to1State::Open | Gc1to1State::Entrance => {
            g.due = now;
            Ok(())
        }
    }
}

/// The `/content gc1to1` line: `"gc1to1: <Close|Open|Entrance|War>[, <n> s left], <t> tenders, <f> failed, <a> arenas"`
/// (`<n>` is the whole seconds of the manager timer in Open and Entrance, shown while it runs).
pub fn state_line(svc: &Services, ctx: &GameCtx<'_>) -> String {
    let g = &svc.social.gc1to1;
    let mut line = format!("gc1to1: {}", g.state().name());
    if matches!(g.state(), Gc1to1State::Open | Gc1to1State::Entrance) {
        let ms = u32::try_from(g.remaining_ms(ctx.clock.game_ms)).unwrap_or(0);
        if ms > 0 {
            line.push_str(&format!(", {} s left", ms.div_ceil(1000)));
        }
    }
    line.push_str(&format!(
        ", {} tenders, {} failed, {} arenas",
        g.tenders().len(),
        g.failed().len(),
        g.arenas().len()
    ));
    line
}
