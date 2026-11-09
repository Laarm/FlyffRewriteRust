//! The lord-election control commands (Plan/23 §4.5 rows 178-181 and 184; WP-GM-26): `/ElectionProcess (ep) <0|1>`,
//! `/ElectionBeginCandidacy (ebc)`, `/ElectionBeginVote (ebv)`, `/ElectionEndVote (eev)` and `/LEventInitialize
//! (leinitialize)` (`FuncTextCmd.cpp:4597-4652`), over `event::lord::gm` (lord).
//!
//! All five are silent, as the legacy: the original sent them to the database server, which refused a step in the wrong
//! state without a reply. A forward-only step that is refused here is a debug log and no output. There is no event flag:
//! the election has its own switch (`/ep`, persisted as `lord.election_enabled`). While the process is on the next minute
//! of P4 compares the state with the calendar again and may step on, so the recipe of a flow is `/ep 0` first.

use crate::event::lord::gm::{self, LordGmError};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdResult};

/// The action of `/ep` and `/content lord on|off`.
pub(super) fn process(cx: &mut CmdCtx<'_, '_>, on: bool) {
    gm::set_process(cx.svc, cx.ctx, on);
}

/// The action of `/ebc`: the candidacy starts now (from `Ready`).
pub(super) fn begin_candidacy(cx: &mut CmdCtx<'_, '_>) -> Result<(), LordGmError> {
    gm::begin_candidacy(cx.svc, cx.ctx)
}

/// The action of `/ebv`: the vote starts now (from `Ready` or `Candidacy`).
pub(super) fn begin_vote(cx: &mut CmdCtx<'_, '_>) -> Result<(), LordGmError> {
    gm::begin_vote(cx.svc, cx.ctx)
}

/// The action of `/eev`: the election ends now (from `Ready`, `Candidacy` or `Vote`).
pub(super) fn end_vote(cx: &mut CmdCtx<'_, '_>) -> Result<(), LordGmError> {
    gm::end_vote(cx.svc, cx.ctx)
}

/// The action of `/leinitialize` and `/content lord events`: every lord event is cleared.
pub(super) fn initialize(cx: &mut CmdCtx<'_, '_>) {
    gm::initialize_events(cx.svc, cx.ctx);
}

/// Logs a refused step at debug level (the legacy refusal was silent).
fn refused(what: &str, r: Result<(), LordGmError>) {
    if let Err(e) = r {
        tracing::debug!(step = what, error = %e, "the lord election GM step was refused");
    }
}

/// Row 178 `/ElectionProcess <0|1>`: runs (nonzero) or stops (0) the election process. An empty argument reads 0 and
/// STOPS the calendar (`GetNumber`, KEEP). Persisted as `lord.election_enabled`.
pub fn election_process(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let on = args.number() != 0;
    process(cx, on);
    Ok(CmdDone::default())
}

/// Row 179 `/ElectionBeginCandidacy`: the election goes to the candidacy now.
pub fn election_begin_candidacy(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let r = begin_candidacy(cx);
    refused("begin_candidacy", r);
    Ok(CmdDone::default())
}

/// Row 180 `/ElectionBeginVote`: the election goes to the vote now.
pub fn election_begin_vote(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let r = begin_vote(cx);
    refused("begin_vote", r);
    Ok(CmdDone::default())
}

/// Row 181 `/ElectionEndVote`: the election ends now and the winner is decided as at the end of the calendar.
pub fn election_end_vote(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let r = end_vote(cx);
    refused("end_vote", r);
    Ok(CmdDone::default())
}

/// Row 184 `/LEventInitialize`: clears every lord event (the factors are 1.0 again, everybody hears
/// `LEventInitialize`).
pub fn lord_event_initialize(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    initialize(cx);
    Ok(CmdDone::default())
}
