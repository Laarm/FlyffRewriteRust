//! The GM control seam of the quiz (Plan/22 line 1716 `quiz.open()`, `quiz.state_next(p)`, `quiz.close()`; Plan/23 rows
//! 211 `/qeo`, 213 `/qsn`, 214 `/qec` and the uniform control seam of gm-content-control).
//!
//! Pure calls: they never print, never check authority and never look at the issuer's world. The GM layer parses,
//! gates and prints. The only text the quiz sends itself is the refusal of an open without questions (PD-12): it is
//! asynchronous (the questions are read from the database), goes to `by` if he is still online and is kept for
//! [`state_line`] because a console issuer has no session.
//!
//! * [`open`] reads the enabled questions of the configured type (`GameReadQuery::QuizQuestions`, answered by
//!   `flow::begin`), `Ok` means the read was issued; the NPC, the entrance time and the audit line follow with the answer.
//! * [`state_next`] is the legacy `SetNextTime(0); Process()`: ONE step body at once, in auto AND manual mode (the
//!   synchronous exception of the seam). The rule "the GM must stand inside world 211" of `/qsn` is the row's.
//! * [`close`] is `CloseQuizEvent` (a pending open is cancelled; the late answer of its read opens nothing).
//!
//! With `AutoMode false` nothing runs from the slot and only [`state_next`] advances (legacy manual mode). The legacy
//! `QUIZ_CHANGED` Lua reload is not ported: a changed `overrides/QuizEvent.lua` applies after a restart.

use super::{QuizPhase, flow};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_db::{GameReadQuery, GameReadResult, QuizType};
use std::fmt;

/// Why a GM call did nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuizGmError {
    /// A quiz runs (the entrance time, the questions or the closing wait): legacy `/qeo` "if !IsRun()".
    Running,
    /// An earlier open still waits for its questions.
    Loading,
    /// `QuizEvent.lua` is missing or names a type that is neither OX nor 4C.
    NoConfig(String),
    /// No quiz runs (`/qsn` and `/qec`).
    NotRunning,
}

impl fmt::Display for QuizGmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuizGmError::Running => f.write_str("a quiz is already running"),
            QuizGmError::Loading => f.write_str("the quiz is still loading its questions"),
            QuizGmError::NoConfig(why) => write!(f, "{why}"),
            QuizGmError::NotRunning => f.write_str("no quiz is running"),
        }
    }
}

impl std::error::Error for QuizGmError {}

/// `/qeo` (`CQuiz::OpenQuizEvent`): starts a quiz of the configured type. `Err(Running)` while one runs (the closing wait
/// included), `Err(Loading)` while an earlier open waits for its questions, `Err(NoConfig)` without a usable
/// `QuizEvent.lua`. `Ok` = the read of the questions was issued; with questions the quiz opens when the answer
/// arrives (no question: refused, PD-12).
pub fn open(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    by: Option<PlayerRef>,
) -> Result<(), QuizGmError> {
    let q = &s.event.quiz;
    if q.running {
        return Err(QuizGmError::Running);
    }
    if q.loading.is_some() {
        return Err(QuizGmError::Loading);
    }
    let Some(cfg) = ctx.data.lua.quiz.as_ref() else {
        return Err(QuizGmError::NoConfig(
            "QuizEvent.lua is not loaded".to_owned(),
        ));
    };
    let Some(quiz_type) = super::configured_type(ctx) else {
        return Err(QuizGmError::NoConfig(format!(
            "QuizEvent.lua: QuizType {} is neither TYPE_OX (1) nor TYPE_4C (2)",
            cfg.quiz_type
        )));
    };
    let q = &mut s.event.quiz;
    q.load_gen += 1;
    let generation = q.load_gen;
    q.loading = Some(generation);
    q.opened_by = by;
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, ctx, result| on_questions(svc, ctx, generation, quiz_type, result),
    )));
    ctx.persist.read_for_game(
        ticket,
        GameReadQuery::QuizQuestions {
            quiz_type: quiz_type.code(),
        },
    );
    Ok(())
}

/// The answer of the question read. Ignored when `/qec` cancelled the open meanwhile.
fn on_questions(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    generation: u64,
    quiz_type: QuizType,
    result: TicketResult,
) {
    if s.event.quiz.loading != Some(generation) {
        return;
    }
    s.event.quiz.loading = None;
    match result {
        TicketResult::ReadDone(Ok(GameReadResult::QuizQuestions(rows))) => {
            flow::begin(s, ctx, quiz_type, rows);
        }
        TicketResult::ReadDone(Err(e)) => flow::refuse(
            s,
            ctx,
            format!("Quiz: the questions could not be read: {e}"),
        ),
        other => flow::refuse(
            s,
            ctx,
            format!("Quiz: the questions read got an unexpected answer ({other:?})"),
        ),
    }
}

/// `/qsn` (`SetNextTime(0); Process()`): the deadline of the current phase is now and ONE step body runs at once. In auto
/// mode the 1 s slot continues from there. `Err(NotRunning)` and no effect unless a quiz runs.
pub fn state_next(s: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), QuizGmError> {
    if !s.event.quiz.running {
        return Err(QuizGmError::NotRunning);
    }
    s.event.quiz.next = flyff_common::GameMs(0);
    flow::step(s, ctx, true);
    Ok(())
}

/// `/qec` (`CloseQuizEvent`): a running quiz closes (the NPC leaves, the asked questions are marked used, everybody
/// goes to Madrigal); a pending open is cancelled. `Err(NotRunning)` otherwise.
pub fn close(s: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), QuizGmError> {
    let q = &mut s.event.quiz;
    if q.running {
        flow::close(s, ctx);
        return Ok(());
    }
    if q.loading.take().is_some() {
        return Ok(());
    }
    Err(QuizGmError::NotRunning)
}

/// `true` while a quiz runs (the closing wait included).
pub fn is_running(s: &Services) -> bool {
    s.event.quiz.running
}

/// ONE line for `/content quiz`: `quiz: <Phase>[, <n> s left][, <q> questions, <p> players]` (`Phase` is the
/// [`QuizPhase`] name, `Loading` while the read of an open is in flight; the closing wait reads `Close, <n> s left`;
/// `, refused: <why>` follows after a refused open until the next successful one), so a flow can poll it with a regex
/// such as `^quiz: Open`.
pub fn state_line(s: &Services, ctx: &GameCtx<'_>) -> String {
    let q = &s.event.quiz;
    let mut line = String::from("quiz: ");
    if q.loading.is_some() {
        line.push_str("Loading");
    } else {
        line.push_str(q.phase.name());
        if let Some(ms) = q.remaining(ctx.clock.game_ms) {
            line.push_str(&format!(", {} s left", ms.div_ceil(1000)));
        }
        if q.running && q.phase != QuizPhase::Close {
            line.push_str(&format!(
                ", {} questions, {} players",
                q.questions.len(),
                super::players(ctx).len()
            ));
        }
    }
    if let Some(why) = &q.last_refusal {
        line.push_str(", refused: ");
        line.push_str(why);
    }
    line
}
