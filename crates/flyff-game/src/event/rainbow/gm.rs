//! The GM and admin calls of the rainbow race (Plan/22 §14, §20 rows `/rropen`, `/rrnext`, `/rrpass`; the original
//! `FuncTextCmd.cpp:4725-4762`).
//!
//! These are pure calls: `gm/cmd/content/` (gm-content-control, WP-GM-26) parses the line, checks the authority and the
//! flag `events.rainbow_race`, and prints; nothing here prints, checks authority or reads the flag.
//!
//! * [`force_application`] (`/content rr application`): Closed -> Application with the hold set - the application window
//!   predicate is ignored until [`force_open`] or [`skip_wait`] clears it;
//! * [`force_open`] (`/rropen` is `from_application = false`, `/content rr open` is `true`): Closed (and Application)
//!   -> Open, the hold cleared; the next second goes on to Wait, or back to Closed without applicants;
//! * [`skip_wait`] (`/rrnext`): the next transition is due now, in any state;
//! * [`pass_current_game`] (`/rrpass`): the racer's current game counts as won;
//! * [`state_line`]: the one status line.
//!
//! With these a packaged server compresses the race: applications, `force_open`, two `skip_wait`, `pass_current_game`
//! seven times, the finish at the end NPC, `skip_wait`. The prize tables stay per weekday (22 §14.1, KEEP): a race that
//! ends on a day without rows mails no prize.

use super::{RainbowRaceService, RrState, complete_current};
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::GameMs;
use flyff_common::defs::text::TID_GAME_RAINBOWRACE_CAPTION_APPLICATION;

/// Why a GM call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RrGmError {
    /// The manager is not in a state the call can leave.
    #[error("the rainbow race is not closed")]
    NotClosed,
}

/// Closed -> Application with `gm_hold`, and the application caption of the natural transition. `Err(NotClosed)` in any
/// other state.
pub fn force_application(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), RrGmError> {
    if svc.event.rainbow.state != RrState::Closed {
        return Err(RrGmError::NotClosed);
    }
    svc.event.rainbow.gm_hold = true;
    svc.event.rainbow.go(RrState::Application);
    let line = ctx
        .data
        .texts
        .get(TID_GAME_RAINBOWRACE_CAPTION_APPLICATION)
        .to_owned();
    crate::chat::ingress::notice::caption(
        svc,
        ctx,
        crate::outbox::Audience::AllOnline,
        false,
        flyff_common::WorldId(0),
        &line,
    );
    Ok(())
}

/// Closed (and Application when `from_application`) -> Open, the hold cleared. `Err(NotClosed)` otherwise.
pub fn force_open(
    svc: &mut Services,
    _ctx: &mut GameCtx<'_>,
    from_application: bool,
) -> Result<(), RrGmError> {
    let rr = &mut svc.event.rainbow;
    let ok = match rr.state {
        RrState::Closed => true,
        RrState::Application => from_application,
        _ => false,
    };
    if !ok {
        return Err(RrGmError::NotClosed);
    }
    rr.gm_hold = false;
    rr.go(RrState::Open);
    Ok(())
}

/// `/rrnext`: `next_at = 0` and the hold cleared, in any state and silent. Wait goes to Ready and Ready to Progress at
/// the next second, and in Progress the race ends at the next second.
pub fn skip_wait(svc: &mut Services) {
    let rr = &mut svc.event.rainbow;
    rr.next_at = GameMs(0);
    rr.gm_hold = false;
}

/// `/rrpass` for the racer `who`: the current game counts as completed (`complete_current` without a game reply: the
/// `RainbowRaceNowState` follows and the prize is queued). Only a racer in Progress who has not finished and has a game
/// (the legacy had no check and could restart a finished racer); `false` and nothing otherwise.
pub fn pass_current_game(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) -> bool {
    let Some(c) = super::character_of(ctx, who) else {
        return false;
    };
    let rr = &svc.event.rainbow;
    if rr.state != RrState::Progress {
        return false;
    }
    if !rr
        .racers
        .get(&c)
        .is_some_and(|r| !r.finished && r.now_game != 0)
    {
        return false;
    }
    complete_current(svc, ctx, who, c);
    true
}

/// ONE status line: `rr: <State>[, <n> s left][, <a> applicants][, <f> finished]`. `<n>` is the whole seconds of
/// `remain_ms` in Wait, Ready and Progress; `<a>` the number of applicants (shown in every state but a Closed one
/// without any); `<f>` the finishers, in Progress. Never prints and never checks authority.
pub fn state_line(svc: &Services, ctx: &GameCtx<'_>) -> String {
    line_of(&svc.event.rainbow, ctx.clock.game_ms)
}

fn line_of(rr: &RainbowRaceService, now: GameMs) -> String {
    let mut line = format!("rr: {}", rr.state.name());
    if matches!(rr.state, RrState::Wait | RrState::Ready | RrState::Progress) {
        line.push_str(&format!(", {} s left", rr.remain_ms(now) / 1000));
    }
    if rr.state != RrState::Closed || !rr.racers.is_empty() {
        line.push_str(&format!(", {} applicants", rr.racers.len()));
    }
    if rr.state == RrState::Progress {
        let finished = rr.racers.values().filter(|r| r.finished).count();
        line.push_str(&format!(", {finished} finished"));
    }
    line
}
