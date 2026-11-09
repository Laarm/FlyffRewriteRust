//! `event::rainbow::join`: the `RainbowRaceNowState` of a racer who joins or re-attaches while the race runs
//! (Plan/22 §19.2; `DPDatabaseClient.cpp:1258-1266`, `JoinStep::RainbowRaceState`).
//!
//! Only in Progress and only for a character in the racer table - finished racers included (the legacy sent the state of
//! every entry; an unfinished racer who left is no racer any more, he was eliminated at the logout). Nothing for any
//! other joiner, so no exact join-sequence assertion moves.

use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};

/// `RainbowRaceNowState` at `JoinStep::RainbowRaceState`.
pub struct RainbowRaceStateStep;

impl JoinContributor for RainbowRaceStateStep {
    fn step(&self) -> JoinStep {
        JoinStep::RainbowRaceState
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let rr = &cx.svc.event.rainbow;
        if rr.state != super::RrState::Progress {
            return;
        }
        let Some(c) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        if let Some(msg) = rr.now_state_of(c, cx.clock.game_ms) {
            out.to_session(cx.session, 0, msg);
        }
    }
}
