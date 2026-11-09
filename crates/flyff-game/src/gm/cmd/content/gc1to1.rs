//! The 1:1 guild combat control commands (Plan/23 §4.5 rows 96-98; WP-GM-26): `/gc1to1open [1]`, `/gc1to1close` and
//! `/gc1to1next` (`FuncTextCmd.cpp:4283-4334`).
//!
//! Each handler is a thin wrapper over `social::gc1to1::gm` (gc11): the state machine, the captions, the tender rows and
//! the chips are all inside the service. While `events.guild_combat_1to1` is off (the default, PD-14) every row prints
//! [`GC1TO1_OFF`] and changes nothing. The legacy open tested `!= OPEN` and re-ran the opening over a running event; here
//! it only opens from `Close` (`already OPEN State!!!` otherwise). The legacy close in the closed state wiped the tender
//! list of the memory while the rows stayed `T` and printed nothing; here the closed state prints `Is CLOSE State!!!`
//! and a close in `Open` keeps the tenders (gc11's `close_all`).

use super::{ALREADY_OPEN, GC1TO1_OFF, IS_CLOSE_STATE, gate};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdResult};
use crate::social::gc1to1::gm;
use flyff_common::config::EventId;

/// The action of `/gc1to1open` and `/content gc1to1 open`: `server_down` keeps the unclaimed refunds claimable.
pub(super) fn open(cx: &mut CmdCtx<'_, '_>, server_down: bool) -> Result<(), &'static str> {
    gm::open(cx.svc, cx.ctx, server_down).map_err(|_| ALREADY_OPEN)
}

/// The action of `/gc1to1close` and `/content gc1to1 close`.
pub(super) fn close(cx: &mut CmdCtx<'_, '_>) -> Result<(), &'static str> {
    gm::close_all(cx.svc, cx.ctx).map_err(|_| IS_CLOSE_STATE)
}

/// The action of `/gc1to1next` and `/content gc1to1 next`.
pub(super) fn next(cx: &mut CmdCtx<'_, '_>) -> Result<(), &'static str> {
    gm::skip_timer(cx.svc, cx.ctx).map_err(|_| IS_CLOSE_STATE)
}

/// Row 96 `/gc1to1open [1]`: from `Close` the opening runs ([`gm::open`]: the failed tenders of the last event are
/// forfeited unless the argument is 1 (`GetNumber() == 1`), fewer than `MinJoinGuild` tenders give the `NOGAME` caption,
/// else the captions OPEN and LINEUP and the state `Open`); any other state prints [`ALREADY_OPEN`].
pub fn gc1to1_open(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::GuildCombat1to1, GC1TO1_OFF) {
        return Ok(CmdDone::default());
    }
    let server_down = args.number() == 1;
    if let Err(text) = open(cx, server_down) {
        cx.out.text(text);
    }
    Ok(CmdDone::default())
}

/// Row 97 `/gc1to1close`: [`gm::close_all`] (every arena that has not ended runs its close wait now; in `Open` the state
/// returns to `Close` with the tenders kept); the closed state prints [`IS_CLOSE_STATE`].
pub fn gc1to1_close(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::GuildCombat1to1, GC1TO1_OFF) {
        return Ok(CmdDone::default());
    }
    if let Err(text) = close(cx) {
        cx.out.text(text);
    }
    Ok(CmdDone::default())
}

/// Row 98 `/gc1to1next`: [`gm::skip_timer`] (the manager timer in `Open` and `Entrance`, every arena timer in `War`; the
/// next one-second slot runs the transition); the closed state prints [`IS_CLOSE_STATE`].
pub fn gc1to1_next(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::GuildCombat1to1, GC1TO1_OFF) {
        return Ok(CmdDone::default());
    }
    if let Err(text) = next(cx) {
        cx.out.text(text);
    }
    Ok(CmdDone::default())
}
