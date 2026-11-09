//! The guild-siege control commands (Plan/23 §4.5; WP-GM-24): `/GCIn` (69), `/GCOpen` (70), `/GCClose` (71),
//! `/GCNext` (72), `/GCRequest (gcrquest)` (138) and `/GCCancel` (139).
//!
//! Each handler is a thin wrapper over the function of siege-a / siege-b that the wire path calls: the master check,
//! the state, the level, `JOINPENYA`, the refund mail and the entry rules are all inside the service. While
//! `events.guild_combat` is off every command prints [`GUILD_COMBAT_OFF`] (the legacy printed nothing visible).

use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::social::siege::{SiegeState, StopWar, bids, lineup, schedule};
use flyff_common::config::EventId;

/// What every command prints while `events.guild_combat` is off.
pub const GUILD_COMBAT_OFF: &str = "guild combat is off (events.guild_combat)";
/// `/gcopen` outside the closed state (`TextCmd_GuildCombatOpen`).
pub const NOT_CLOSE_STATE: &str = "Not GuildCombat Open :: Not CLOSE_STATE";
/// `/gcclose` and `/gcnext` in the closed state (`TextCmd_GuildCombatClose`, `TextCmd_GuildCombatNext`).
pub const IS_CLOSE_STATE: &str = "Not GuildCombat Close :: Is CLOSE_STATE";

/// The issuer, or `None` after printing [`GUILD_COMBAT_OFF`] when the event is off.
fn gate(cx: &mut CmdCtx<'_, '_>) -> Result<Option<crate::ctx::PlayerRef>, CmdError> {
    let who = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    if cx.ctx.live.event_state(EventId::GuildCombat) == 0 {
        cx.out.text(GUILD_COMBAT_OFF);
        return Ok(None);
    }
    Ok(Some(who))
}

/// Row 69 `/GCIn`: the entry that `JoinGuildCombat` calls after its NPC check ([`lineup::enter`]; no NPC and no guild
/// needed): `TID_GAME_GUILDCOMBAT_NOT_JOIN` in `Open` outside the entry window, `TID_GAME_GUILDCOMBAT_NOT_OPEN` in
/// `CompleteClose`, nothing in `GmClose`; the player joins as an observer in `Close`, in `Open` during the entry
/// window and in `War`.
pub fn gc_in(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if let Some(who) = gate(cx)? {
        lineup::enter(cx.svc, cx.ctx, who);
    }
    Ok(CmdDone::default())
}

/// Row 70 `/GCOpen`: in the state `Close` the siege opens WITHOUT the bidder count of the wall job
/// ([`schedule::open`]; an empty ranking is fine, the siege cancels itself at its maintenance minute); in any other
/// state it prints [`NOT_CLOSE_STATE`].
pub fn gc_open(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if gate(cx)?.is_none() {
        return Ok(CmdDone::default());
    }
    if cx.svc.social.siege.state() != SiegeState::Close {
        cx.out.text(NOT_CLOSE_STATE);
    } else if let Err(e) = schedule::open(cx.svc, cx.ctx) {
        tracing::debug!(error = %e, "/gcopen was refused");
    }
    Ok(CmdDone::default())
}

/// Row 71 `/GCClose [n]`: in the state `Close` it prints [`IS_CLOSE_STATE`]. Otherwise `n == 0` (a missing argument
/// reads as 0, as the legacy `GetNumber` did) requests [`StopWar::Early`] and any other number [`StopWar::GmCancel`].
/// This only sets the flag: the next `siege::process` (at most a second later) jumps the script, `Early` to the row of
/// `STATE 3` (`CompleteClose`, then the fight is closed and the results are written: the siege ENDS WITH results),
/// `GmCancel` to the row of `STATE 4` (`GmClose`, 30 s later everybody goes home, nothing is persisted). The plan's
/// "close now / close-wait" are legacy function names, not effects (open question 1 of the task).
pub fn gc_close(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    if gate(cx)?.is_none() {
        return Ok(CmdDone::default());
    }
    let n = args.number();
    if cx.svc.social.siege.state() == SiegeState::Close {
        cx.out.text(IS_CLOSE_STATE);
    } else if n == 0 {
        cx.svc.social.siege.request_stop(StopWar::Early);
    } else {
        cx.svc.social.siege.request_stop(StopWar::GmCancel);
    }
    Ok(CmdDone::default())
}

/// Row 72 `/GCNext`: in the state `Close` it prints [`IS_CLOSE_STATE`], otherwise the timer is skipped
/// (`SiegeService::skip_timer`: the next script row is due at the next one-second slot; one row per call as always).
pub fn gc_next(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if gate(cx)?.is_none() {
        return Ok(CmdDone::default());
    }
    if cx.svc.social.siege.state() == SiegeState::Close {
        cx.out.text(IS_CLOSE_STATE);
    } else {
        cx.svc.social.siege.skip_timer();
    }
    Ok(CmdDone::default())
}

/// Row 138 `/GCRequest (gcrquest) <penya>`: the bid of the issuer's guild ([`bids::request`]: the master check
/// `TID_GAME_GUILDCOMBAT_NOT_GUILD_LEADER`, the state, the level, `JOINPENYA`, higher than before and the gold are all
/// inside the service). The penya is read as `u32`, so a negative or a larger-than-`i32::MAX` value is silent after
/// the master, state and level checks, like the legacy `DWORD > INT_MAX` return.
pub fn gc_request(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let Some(who) = gate(cx)? else {
        return Ok(CmdDone::default());
    };
    // the legacy read a DWORD: a negative number is a huge one and is refused by the INT_MAX rule of the service
    let penya = args.number() as u32;
    bids::request(cx.svc, cx.ctx, who, penya);
    Ok(CmdDone::default())
}

/// Row 139 `/GCCancel`: cancels the guild's application with the refund mail ([`bids::cancel`]).
pub fn gc_cancel(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if let Some(who) = gate(cx)? {
        bids::cancel(cx.svc, cx.ctx, who);
    }
    Ok(CmdDone::default())
}
