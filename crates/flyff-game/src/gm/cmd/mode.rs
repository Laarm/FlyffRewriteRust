//! The mode and mute commands (Plan/23 §2, §4.3-§4.7; WP-GM-12): `/invisible` (29), `/noinvisible` (30), `/talk`
//! (34), `/notalk` (35), `/Mute` (37), `/undying` (46), `/undying2` (47), `/noundying` (48), `/onekill` (49),
//! `/noonekill` (50), `/freeze` (63), `/nofreeze` (64), `/gmitem` ... `/gmnotobserve` (125-132), `/ExpUpStop` (145)
//! and `/unmute` (N15).
//!
//! Every command changes the mode word through `gm::modes::change_mode` (one `ModifyMode` to the near set and the
//! target, nothing when the word did not change). The commands that name another player need the target **online**:
//! an unknown name answers `ReturnSay{3, name}`, an existing but offline character changes nothing and answers
//! nothing (KEEP-QUIRK, 23 §2.4: the legacy core forwarded the request to online players only).

use crate::ctx::PlayerRef;
use crate::gm::modes::{self, ModeSource};
use crate::gm::target::{self, Target};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult, Issuer};
use flyff_common::gm::ModeFlags;

/// The issuer's character.
fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// Who the trace line names.
fn source(cx: &CmdCtx<'_, '_>) -> ModeSource {
    match cx.issuer {
        Issuer::Item { item, .. } => ModeSource::Item { item },
        _ => cx
            .self_player
            .and_then(|p| cx.ctx.player(p))
            .and_then(|m| m.player())
            .map_or(ModeSource::System, |p| ModeSource::Command {
                issuer: p.ident.character_id,
            }),
    }
}

/// Changes `set` and `clear` on the issuer.
fn on_self(cx: &mut CmdCtx<'_, '_>, set: ModeFlags, clear: ModeFlags) -> CmdResult {
    let who = me(cx)?;
    let by = source(cx);
    modes::change_mode(cx.svc, cx.ctx, who, set, clear, by);
    Ok(CmdDone::default())
}

/// Row 29 `/invisible (inv)`: `TRANSPARENT` on self.
pub fn invisible(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::TRANSPARENT, ModeFlags::NONE)
}

/// Row 30 `/noinvisible (noinv)`: clears `TRANSPARENT`.
pub fn no_invisible(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::NONE, ModeFlags::TRANSPARENT)
}

/// Row 46 `/undying (ud)`: clears `MATCHLESS2`, sets `MATCHLESS` (no damage). Item-privileged (23 §5).
pub fn undying(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::MATCHLESS, ModeFlags::MATCHLESS2)
}

/// Row 47 `/undying2 (ud2)`: clears `MATCHLESS`, sets `MATCHLESS2` (damage taken, never dies). Item-privileged.
pub fn undying2(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::MATCHLESS2, ModeFlags::MATCHLESS)
}

/// Row 48 `/noundying (noud)`: clears both. Item-privileged.
pub fn no_undying(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(
        cx,
        ModeFlags::NONE,
        ModeFlags::MATCHLESS.with(ModeFlags::MATCHLESS2),
    )
}

/// Row 49 `/onekill (ok)`: sets `ONEKILL` on self.
pub fn onekill(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::ONEKILL, ModeFlags::NONE)
}

/// Row 50 `/noonekill (nook)`: clears `ONEKILL`.
pub fn no_onekill(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::NONE, ModeFlags::ONEKILL)
}

/// Row 125 `/gmitem`: sets `ITEM` (no pick-up, no drop).
pub fn gm_item(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::ITEM, ModeFlags::NONE)
}

/// Row 126 `/gmnotitem`.
pub fn gm_not_item(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::NONE, ModeFlags::ITEM)
}

/// Row 127 `/gmattck`: sets `NO_ATTACK`.
pub fn gm_attack(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::NO_ATTACK, ModeFlags::NONE)
}

/// Row 128 `/gmnotattck`.
pub fn gm_not_attack(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::NONE, ModeFlags::NO_ATTACK)
}

/// Row 129 `/gmcommunity`: sets `COMMUNITY` (no server effect, KEEP-QUIRK 23 §2.1).
pub fn gm_community(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::COMMUNITY, ModeFlags::NONE)
}

/// Row 130 `/gmnotcommunity`.
pub fn gm_not_community(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::NONE, ModeFlags::COMMUNITY)
}

/// Row 131 `/gmobserve`: sets `OBSERVE` (`ITEM | NO_ATTACK`).
pub fn gm_observe(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::OBSERVE, ModeFlags::NONE)
}

/// Row 132 `/gmnotobserve`: clears both bits of `OBSERVE`.
pub fn gm_not_observe(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    on_self(cx, ModeFlags::NONE, ModeFlags::OBSERVE)
}

/// Row 145 `/ExpUpStop (es)`: toggles `EXPUP_STOP` on self; `ModifyMode` to the near set, no text.
pub fn exp_up_stop(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let on = cx
        .ctx
        .player(who)
        .is_some_and(|m| !modes::is_exp_stopped(m));
    let by = source(cx);
    modes::set_mode(cx.svc, cx.ctx, who, ModeFlags::EXPUP_STOP, on, by);
    Ok(CmdDone::default())
}

/// Resolves the target of a mode command (`/talk`, `/notalk`, `/freeze`, `/nofreeze`): the token after the
/// command. `forbid_self` is the `strcmp` of `/freeze`; an unknown name is `NotFound`; an offline one is `None`.
fn mode_target(
    cx: &CmdCtx<'_, '_>,
    args: &mut CmdArgs<'_>,
    forbid_self: bool,
) -> Result<Option<(PlayerRef, String)>, CmdError> {
    let name = args.token().to_owned();
    match target::online(cx, &name, forbid_self) {
        Ok(t) => Ok(Some(t)),
        // an existing but offline character: nothing, and no reply (23 §2.4)
        Err(CmdError::Offline(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// A mode command on a named character.
fn on_target(
    cx: &mut CmdCtx<'_, '_>,
    args: &mut CmdArgs<'_>,
    forbid_self: bool,
    set: ModeFlags,
    clear: ModeFlags,
) -> CmdResult {
    let Some((player, name)) = mode_target(cx, args, forbid_self)? else {
        return Ok(CmdDone::default());
    };
    let by = source(cx);
    modes::change_mode(cx.svc, cx.ctx, player, set, clear, by);
    Ok(CmdDone::on(name))
}

/// Row 34 `/talk (nota) <name>`: clears `DONTALK` on the online target (self allowed). KEEP-QUIRK: the legacy
/// names are inverted, `talk` un-mutes and `notalk` mutes.
pub fn talk(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    on_target(cx, args, false, ModeFlags::NONE, ModeFlags::DONTALK)
}

/// Row 35 `/notalk (nt) <name>`: sets `DONTALK` (`TALK | SAYTALK | SHOUTTALK`) on the online target (self
/// allowed); persisted with the character.
pub fn notalk(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    on_target(cx, args, false, ModeFlags::DONTALK, ModeFlags::NONE)
}

/// Row 63 `/freeze (fr) <name>`: sets `DONMOVE` on the online target (client-enforced only). Self gives
/// `ReturnSay{2}`.
pub fn freeze(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    on_target(cx, args, true, ModeFlags::DONMOVE, ModeFlags::NONE)
}

/// Row 64 `/nofreeze (nofr) <name>`: clears `DONMOVE`.
pub fn no_freeze(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    on_target(cx, args, true, ModeFlags::NONE, ModeFlags::DONMOVE)
}

/// Sets the mute of `name` to `seconds`; unknown name: `Text("player not found")`, an offline character
/// nothing. The audit names the target.
fn do_mute(cx: &mut CmdCtx<'_, '_>, name: &str, seconds: u32) -> CmdResult {
    match target::find(cx.svc, name) {
        Target::Unknown => {
            // the legacy literal; the legacy command did not use ReturnSay here
            cx.out.text("player not found");
            Ok(CmdDone::default())
        }
        Target::Offline { .. } => Ok(CmdDone::default()),
        Target::Online { player, name, .. } => {
            if let Some(m) = cx.ctx.worlds.player_mut(player.0) {
                modes::set_mute(m, seconds);
            }
            Ok(CmdDone::on(name))
        }
    }
}

/// Row 37 `/Mute (mute) <name> <seconds>`: the target must be online; a missing second argument changes nothing
/// and answers nothing; `0` unmutes. The remaining time counts down only while the target is in the world
/// (KQ-22) and is persisted.
pub fn mute(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if args.finished() {
        return Ok(CmdDone::default());
    }
    let seconds = args.number();
    if args.finished() {
        return Ok(CmdDone::default());
    }
    do_mute(cx, &name, u32::try_from(seconds).unwrap_or(0))
}

/// N15 `/unmute <name>`: `/mute <name> 0`.
pub fn unmute(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if name.is_empty() {
        return Err(CmdError::Usage);
    }
    do_mute(cx, &name, 0)
}
