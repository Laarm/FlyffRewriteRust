//! The quiz control commands (Plan/23 §4.5 rows 211, 213, 214; WP-GM-26): `/QuizEventOpen (qeo)`, `/QuizStateNext
//! (qsn)` and `/QuizEventClose (qec)` (`FuncTextCmd.cpp:5029-5063`), over `event::quiz::gm` (quiz).
//!
//! There is no event flag (the quiz runs only when a GM opens it, or by its own `AutoMode` slot). PD-12: the shipped data
//! has no questions, so `/qeo` opens nothing and the quiz keeps its refusal line for the `quiz:` state line (a console
//! issuer has no session to tell). `/qsn` is the legacy "only inside the quiz world" command; `/content quiz next` runs the
//! same step without that rule, so a console flow needs no GM inside the quiz.

use crate::event::quiz::gm::{self, QuizGmError};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdResult};
use flyff_common::WorldId;
use flyff_common::defs::world::WI_WORLD_QUIZ;

/// The action of `/qeo` and `/content quiz open`: the read of the questions is issued (the quiz opens when the answer
/// arrives); `by` is the issuer's character, if any.
pub(super) fn open(cx: &mut CmdCtx<'_, '_>) -> Result<(), QuizGmError> {
    let by = cx.self_player;
    gm::open(cx.svc, cx.ctx, by)
}

/// The action of `/content quiz next` and (inside the quiz world) `/qsn`: one step at once.
pub(super) fn state_next(cx: &mut CmdCtx<'_, '_>) -> Result<(), QuizGmError> {
    gm::state_next(cx.svc, cx.ctx)
}

/// The action of `/qec` and `/content quiz close`.
pub(super) fn close(cx: &mut CmdCtx<'_, '_>) -> Result<(), QuizGmError> {
    gm::close(cx.svc, cx.ctx)
}

/// Row 211 `/QuizEventOpen`: opens the quiz when none runs. `Ok` is silent; a running or loading quiz is silent (the
/// legacy `if !IsRun()`); a data set without a usable `QuizEvent.lua` prints `quiz: <reason>` (new text).
pub fn quiz_event_open(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    // `Ok`, `Running` and `Loading` are silent (the legacy `if !IsRun()`)
    if let Err(QuizGmError::NoConfig(reason)) = open(cx) {
        cx.out.text(format!("quiz: {reason}"));
    }
    Ok(CmdDone::default())
}

/// Row 213 `/QuizStateNext` (needs a character): while a quiz runs AND the issuer stands in the quiz world
/// (`WI_WORLD_QUIZ`) the next step runs now (`SetNextTime(0); Process()`); anything else does nothing at all.
pub fn quiz_state_next(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let Some(who) = cx.self_player else {
        return Ok(CmdDone::default());
    };
    let inside = cx.ctx.worlds.core(who.0).map(|c| c.world) == Some(WorldId(WI_WORLD_QUIZ));
    if inside && gm::is_running(cx.svc) {
        let _ = state_next(cx);
    }
    Ok(CmdDone::default())
}

/// Row 214 `/QuizEventClose`: closes the running quiz (a pending open is cancelled); silent.
pub fn quiz_event_close(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let _ = close(cx);
    Ok(CmdDone::default())
}
