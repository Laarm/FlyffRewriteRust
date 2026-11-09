//! The secret-room control commands (Plan/23 §4.5 rows 166, 167, 171; WP-GM-26): `/sropen`, `/srnext` and `/srclose`
//! (`FuncTextCmd.cpp:4411-4500`), over `event::secret_room::gm` (secret-room).
//!
//! While `events.secret_room` is off every row prints [`SECRET_ROOM_OFF`] and changes nothing. The calendar is not
//! involved: `/sropen` runs the open body at once, the other two move the deadline of the machine to now and the
//! one-second slot item 6 runs the transition. `/srclose` never names a winner (a side in `War` ends with `NOWINNER`).

use super::{NOT_CLOSE_STATE, NOT_WAR_STATE, SECRET_ROOM_OFF, gate};
use crate::event::secret_room::gm::{self, SecretRoomGmError};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdResult};
use flyff_common::config::EventId;

/// The action of `/sropen` and `/content sr open`: the manager must be `Close` (a refusal is the legacy text).
pub(super) fn open(cx: &mut CmdCtx<'_, '_>) -> Result<(), &'static str> {
    gm::force_open(cx.svc, cx.ctx).map_err(|e| match e {
        SecretRoomGmError::NotClose => NOT_CLOSE_STATE,
        SecretRoomGmError::NotWar => NOT_WAR_STATE,
    })
}

/// The action of `/srnext` and `/content sr next`: the manager deadline is now, whatever the state (silent).
pub(super) fn next(cx: &mut CmdCtx<'_, '_>) {
    gm::skip_wait(cx.svc, cx.ctx);
}

/// The action of `/srclose` and `/content sr close`: only in `War`, every side that is not `Close` gets its deadline now.
pub(super) fn close(cx: &mut CmdCtx<'_, '_>) -> Result<(), &'static str> {
    gm::force_close(cx.svc, cx.ctx).map_err(|e| match e {
        SecretRoomGmError::NotWar => NOT_WAR_STATE,
        SecretRoomGmError::NotClose => NOT_CLOSE_STATE,
    })
}

/// Row 166 `/sropen`: in the state `Close` the open body runs without the calendar (per side with enough tenders the
/// `CONTOPEN` caption and the side opens; with none the `NOTOPEN` caption and the manager stays `Close`); any other state
/// prints [`NOT_CLOSE_STATE`].
pub fn sr_open(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::SecretRoom, SECRET_ROOM_OFF) {
        return Ok(CmdDone::default());
    }
    if let Err(text) = open(cx) {
        cx.out.text(text);
    }
    Ok(CmdDone::default())
}

/// Row 167 `/srnext`: `m_dwRemainTime = 0` in every state, silent (the next slot moves `LineUp` to `Entrance` and
/// `Entrance` to `WarWait`; `WarWait` to `War`; `Close`, `Open` and `War` are not moved by it).
pub fn sr_next(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::SecretRoom, SECRET_ROOM_OFF) {
        return Ok(CmdDone::default());
    }
    next(cx);
    Ok(CmdDone::default())
}

/// Row 171 `/srclose`: in the state `War` every side that is not `Close` gets its remaining time 0; any other state
/// prints [`NOT_WAR_STATE`].
pub fn sr_close(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if !gate(cx, EventId::SecretRoom, SECRET_ROOM_OFF) {
        return Ok(CmdDone::default());
    }
    if let Err(text) = close(cx) {
        cx.out.text(text);
    }
    Ok(CmdDone::default())
}
