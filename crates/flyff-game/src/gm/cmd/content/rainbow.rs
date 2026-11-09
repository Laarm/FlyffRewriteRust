//! The rainbow-race control commands (Plan/23 §4.5 rows 197-199; WP-GM-26): `/RROpen (rropen)`, `/RRNext (rrnext)` and
//! `/RRPass (rrpass)` (`FuncTextCmd.cpp:4733-4762`), over `event::rainbow::gm` (rainbow-race).
//!
//! While `events.rainbow_race` is off (the default, PD-14) every row prints [`RAINBOW_OFF`] and changes nothing. The
//! start NPC exists only when the flag was on at the world load; `/le 25 1` switches the process on at runtime.
//! `/rrpass` is a FIX: the legacy also "completed" game 0 for an applicant outside the race and started his first game
//! early; here only a racer in `Progress` who has not finished and has a current game passes it.

use super::{RAINBOW_OFF, gate};
use crate::event::rainbow::gm::{self, RrGmError};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use flyff_common::config::EventId;

/// The action of `/rropen`: `Closed` -> `Open` (`from_application = false`) and of `/content rr open`
/// (`from_application = true`: `Application` too, the GM hold cleared).
pub(super) fn open(cx: &mut CmdCtx<'_, '_>, from_application: bool) -> Result<(), RrGmError> {
    gm::force_open(cx.svc, cx.ctx, from_application)
}

/// The action of `/content rr application`: `Closed` -> `Application`, held by the GM.
pub(super) fn application(cx: &mut CmdCtx<'_, '_>) -> Result<(), RrGmError> {
    gm::force_application(cx.svc, cx.ctx)
}

/// The action of `/rrnext` and `/content rr next`: the next transition is due now, in every state (silent).
pub(super) fn next(cx: &mut CmdCtx<'_, '_>) {
    gm::skip_wait(cx.svc);
}

/// Row 197 `/RROpen`: from `Closed` the race opens (the next slot goes to `Wait` with the applicants, or back to
/// `Closed` without); other states are a debug log and silent.
pub fn rr_open(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::RainbowRace, RAINBOW_OFF) {
        return Ok(CmdDone::default());
    }
    if let Err(e) = open(cx, false) {
        tracing::debug!(error = %e, "/rropen was refused");
    }
    Ok(CmdDone::default())
}

/// Row 198 `/RRNext`: skips the current timer in every state (`Wait` -> `Ready` -> `Progress` -> the end), silent.
pub fn rr_next(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::RainbowRace, RAINBOW_OFF) {
        return Ok(CmdDone::default());
    }
    next(cx);
    Ok(CmdDone::default())
}

/// Row 199 `/RRPass` (needs a character): the current mini-game of the issuer counts as won when he is a racer in
/// `Progress`; otherwise silent.
pub fn rr_pass(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    if !gate(cx, EventId::RainbowRace, RAINBOW_OFF) {
        return Ok(CmdDone::default());
    }
    let _ = gm::pass_current_game(cx.svc, cx.ctx, who);
    Ok(CmdDone::default())
}
