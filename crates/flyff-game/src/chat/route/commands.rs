//! The chat-channel command handlers of Plan/21 §10.7, registered in the table of Plan/23: `cmd_whisper` (row 3),
//! `cmd_say` (row 4), `cmd_shout` (row 6), `cmd_party` (row 7) and `cmd_guild` (row 9).

use super::whisper::{WhisperKind, whisper};
use crate::admin::chat_tap;
use crate::chat::ingress::{LenRule, channel_text, mute_text, notice, shout_pipeline};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};

fn run_whisper(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>, kind: WhisperKind) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let name = args.token().to_owned();
    if name.is_empty() {
        return Ok(CmdDone::default());
    }
    let rest = args.rest_raw();
    whisper(cx.svc, cx.ctx, me, &name, rest, kind);
    Ok(CmdDone::on(name))
}

/// Row 3 `/whisper (w) <name> <text...>` (21 §10.4).
pub fn cmd_whisper(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    run_whisper(cx, args, WhisperKind::Whisper)
}

/// Row 4 `/say <name> <text...>`: the messenger instant message (21 §10.4).
pub fn cmd_say(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    run_whisper(cx, args, WhisperKind::Im)
}

/// Row 6 `/shout (s) <text...>` (21 §10.3): `shout_pipeline` on the untrimmed remainder, then `route::shout`.
pub fn cmd_shout(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let body = args.rest_raw();
    if let Some(text) = shout_pipeline(cx.svc, cx.ctx, me, body, cx.shortcut) {
        super::shout::shout(cx.svc, cx.ctx, me, &text);
    }
    Ok(CmdDone::default())
}

/// The body of `/p` and `/g` (21 §10.4): the mute text stops it, `channel_text` (word filter, 260-byte cut, trim,
/// markup) cleans the line, then `route` delivers it. The membership check is the route's and comes **after** the
/// length check (a line of 260 bytes or more from a non-member is dropped silently, the legacy order). Not blocked
/// by `DONTALK`.
fn run_channel(
    cx: &mut CmdCtx<'_, '_>,
    args: &mut CmdArgs<'_>,
    route: fn(&mut Services, &mut GameCtx<'_>, PlayerRef, &str),
) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    if let Some(tid) = cx.ctx.player(me).and_then(mute_text) {
        notice::defined(cx.ctx, me, tid, Vec::new());
        return Ok(CmdDone::default());
    }
    let limit = cx.ctx.cfg.rules.gameplay.chat_drop_bytes as usize;
    let body = args.rest_raw();
    if let Some(text) = channel_text(
        cx.svc,
        cx.ctx,
        me,
        body,
        LenRule::DropAtOrAbove(limit),
        true,
    ) {
        // the admin console's chat tap shows P staff what was typed before the filter (25 §4.9)
        if cx.svc.chat.tap.enabled() && cx.ctx.cfg.rules.fidelity.chat_filter_server_side {
            let filtered = cx.svc.chat.data.filter.word_change(body);
            chat_tap::note_original(cx.svc, me.0, body, &filtered);
        }
        route(cx.svc, cx.ctx, me, &text);
    }
    Ok(CmdDone::default())
}

/// Row 7 `/partychat (p) <text...>` (21 §10.4): the shared channel body with `route::party`.
pub fn cmd_party(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    run_channel(cx, args, super::party::party)
}

/// Row 9 `/GuildChat (g) <text...>` (21 §10.4): the shared channel body with `route::guild`.
pub fn cmd_guild(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    run_channel(cx, args, super::guild::guild)
}
