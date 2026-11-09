//! The GM and admin seam of the secret room (Plan/22 §20, Plan/23 rows 166/167/171; `FuncTextCmd.cpp:4412-4505`).
//!
//! Pure calls: they never print and never check authority (`gm/cmd/content/` of gm-content-control parses the line,
//! checks, prints and audits). The legacy commands:
//!
//! * `/sropen` -> [`force_open`]: `SecretRoomOpen()` when the manager is `Close`, else the text `Is Not Close State!!!`
//!   (the open still checks flag 24 and `MinGuildNum`: with nothing to open the caption `NOTOPEN` goes out),
//! * `/srnext` -> [`skip_wait`]: `m_dwRemainTime = 0` in ANY state, so the next `process` passes the manager's deadline,
//! * `/srclose` -> [`force_close`]: in `War` every side that is not `Close` gets its deadline set to 0 (a `War` side
//!   ends with `NOWINNER` and the close-wait, a `CloseWait` side closes at the next second), else `Is Not War State!!!`,
//! * [`state_line`]: the one status line `ui-secret-tax` and the browser flows poll,
//! * [`SecretRoomService::to_json`](super::SecretRoomService::to_json): the admin view of phase C.
//!
//! `/srtender`, `/srlineup`, `/srview`, `/srentrance [1]` and `/srcancel` are client-side openers that send the normal
//! C2S messages; there is no server command for them.

use super::{SrMngState, SrSideState, remain_ms};
use crate::ctx::{GameCtx, Services};
use flyff_common::GameMs;
use serde_json::Value;

/// Why a GM call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretRoomGmError {
    /// The manager is not `Close` (`/sropen`).
    #[error("Is Not Close State!!!")]
    NotClose,
    /// The manager is not in `War` (`/srclose`).
    #[error("Is Not War State!!!")]
    NotWar,
}

/// `/sropen`: opens the manager now (see the module documentation). `Ok` also when nothing could open (flag 24 off, no
/// side with `MinGuildNum` tenders: the caption `NOTOPEN` went out, the manager stays `Close`).
pub fn force_open(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), SecretRoomGmError> {
    if svc.event.secret_room.state != SrMngState::Close {
        return Err(SecretRoomGmError::NotClose);
    }
    let _ = super::war::open(svc, ctx);
    Ok(())
}

/// `/srnext`: the manager deadline is 0, whatever the state.
pub fn skip_wait(svc: &mut Services, _ctx: &mut GameCtx<'_>) {
    svc.event.secret_room.deadline = Some(GameMs(0));
}

/// `/srclose`: every side that is not `Close` gets deadline 0 while the manager is in `War`.
pub fn force_close(svc: &mut Services, _ctx: &mut GameCtx<'_>) -> Result<(), SecretRoomGmError> {
    let sr = &mut svc.event.secret_room;
    if sr.state != SrMngState::War {
        return Err(SecretRoomGmError::NotWar);
    }
    for side in sr.sides.values_mut() {
        if side.state != SrSideState::Close {
            side.deadline = Some(GameMs(0));
        }
    }
    Ok(())
}

/// ONE status line: `sr: <state>[, <n> s left], East <side state> <k> tenders, West <side state> <k> tenders`, e.g.
/// `sr: Close, East Close 0 tenders, West Close 0 tenders`. `<n>` (the whole seconds to the manager deadline) is shown
/// in the states that run on the manager's clock: `LineUp`, `Entrance` and `WarWait`.
pub fn state_line(svc: &Services, ctx: &GameCtx<'_>) -> String {
    let sr = &svc.event.secret_room;
    let mut line = format!("sr: {:?}", sr.state);
    if matches!(
        sr.state,
        SrMngState::LineUp | SrMngState::Entrance | SrMngState::WarWait
    ) {
        let secs = remain_ms(sr.deadline, ctx.clock.game_ms) / 1000;
        line.push_str(&format!(", {secs} s left"));
    }
    for (k, name) in [
        (crate::world::continent::CONT_EAST, "East"),
        (crate::world::continent::CONT_WEST, "West"),
    ] {
        match sr.side(k) {
            Some(side) => line.push_str(&format!(
                ", {name} {:?} {} tenders",
                side.state,
                side.tenders.len()
            )),
            None => line.push_str(&format!(", {name} Close 0 tenders")),
        }
    }
    line
}

/// The admin view of phase C (WP-ADMIN-31): the manager, the sides and their tenders.
pub fn to_json(svc: &Services, ctx: &GameCtx<'_>) -> Value {
    svc.event.secret_room.to_json(ctx.clock.game_ms)
}
