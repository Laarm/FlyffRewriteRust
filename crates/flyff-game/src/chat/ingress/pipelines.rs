//! `preprocess`, `say_pipeline`, `shout_pipeline` and `channel_text` (Plan/23 §6.1, §6.2, §6.4, §6.5, §6.6).

use super::audit;
use super::markup::markup_text;
use super::mode::{is_mode, is_shout_full, mute_text};
use super::normalize::{last_full, normalize_line, trim_right};
use super::notice;
use super::spam::{SpamCfg, SpamReply, SpamVerdict};
use crate::admin::chat_tap;
use crate::chat::{Actor, actor};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::{self, DispatchOutcome, Routing};
use flyff_common::defs::text::TID_GAME_CANT_SHOUT_BELOW_20;
use flyff_common::defs::world::WI_WORLD_QUIZ;
use flyff_common::gm::{Authority, ModeFlags};
use flyff_rules::chat::{cut_chars, is_right};

/// What `preprocess` decided (23 §6.0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ingress {
    /// Nothing to deliver: a command ran (rows 3/4/6/7/9-12 run 21's handlers), the line was ignored, or the
    /// unknown-command reply was queued.
    Command,
    /// A normal chat line for [`say_pipeline`].
    Say {
        /// The normalised text.
        text: String,
        /// Run the spam check and the word filter (`false` for the emote / unknown-command pass-through).
        spam_ok: bool,
        /// The line came from a shortcut/macro slot (`ChatReq.shortcut`): selects the 3000 ms spam window.
        shortcut: bool,
    },
}

/// The recipient-length rule of `channel_text` (23 §6.0): 260 = `gameplay.chat_drop_bytes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LenRule {
    /// Whisper, party and guild: a body of `>=` the limit in bytes is dropped.
    DropAtOrAbove(usize),
    /// `/say`: a body of more than the limit in bytes is dropped.
    DropAbove(usize),
}

fn queue_replies(ctx: &mut GameCtx<'_>, who: PlayerRef, replies: Vec<SpamReply>) {
    for r in replies {
        notice::defined(ctx, who, r.tid, r.args);
    }
}

/// `preprocess` (23 §6.1): the server half of `ParsingChat` plus `OnChat`: normalisation, the `gm.chat` line
/// audit, command dispatch and the §3.6 routing of unknown lines.
pub fn preprocess(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    line: &str,
    shortcut: bool,
) -> Ingress {
    let Some(a) = actor(ctx, who) else {
        return Ingress::Command;
    };
    let max = ctx.cfg.rules.gameplay.chat_max_chars as usize;
    let Some(text) = normalize_line(line, max) else {
        return Ingress::Command;
    };
    if a.authority.at_least(Authority::LOG_CHATTING) {
        audit::gm_chat(svc, ctx, &a, "line", &text);
    }
    if !text.starts_with('/') {
        return Ingress::Say {
            text,
            spam_ok: true,
            shortcut,
        };
    }
    match gm::run_player_line(svc, ctx, who, &text, shortcut) {
        DispatchOutcome::Handled => Ingress::Command,
        DispatchOutcome::NotACommand => match gm::route_unknown(svc, ctx, who, &text) {
            Routing::AsChat => Ingress::Say {
                text,
                spam_ok: false,
                shortcut,
            },
            Routing::Replied => Ingress::Command,
        },
    }
}

/// `say_pipeline` (23 §6.2 steps 1-6): spam check and filter (when `spam_ok`), the `TALK` bit, the mute text,
/// markup and CRLF. `None` = stopped (the reply, if any, is already queued).
pub fn say_pipeline(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    text: String,
    spam_ok: bool,
    shortcut: bool,
) -> Option<String> {
    let a = actor(ctx, who)?;
    let mut text = text;
    if spam_ok {
        let cfg = SpamCfg::from_gameplay(&ctx.cfg.rules.gameplay);
        let now = ctx.clock.game_ms;
        let (verdict, replies) = ctx.player_mut(who).and_then(|m| m.player_mut()).map(|p| {
            p.chat
                .spam
                .check_normal(&text, shortcut, a.authority, now, &cfg)
        })?;
        queue_replies(ctx, who, replies);
        if verdict == SpamVerdict::Refuse {
            return None;
        }
        if ctx.cfg.rules.fidelity.chat_filter_server_side {
            let filtered = svc.chat.data.filter.word_change(&text);
            let max = ctx.cfg.rules.gameplay.chat_max_chars as usize;
            let filtered = cut_chars(&filtered, max).to_owned();
            // the admin console's chat tap shows P staff what was typed before the filter (25 §4.9)
            chat_tap::note_original(svc, who.0, &text, &filtered);
            text = filtered;
        }
    }
    let m = ctx.player(who)?;
    if is_mode(m, ModeFlags::TALK) {
        return None;
    }
    if let Some(tid) = mute_text(m) {
        notice::defined(ctx, who, tid, Vec::new());
        return None;
    }
    let m = ctx.player(who)?;
    Some(markup_text(m, &text))
}

/// `shout_pipeline` (23 §6.4 steps 1-9): `body` is the untrimmed remainder of the command line after `/s`
/// (`CmdArgs::rest_raw`). `None` = stopped.
pub fn shout_pipeline(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    body: &str,
    shortcut: bool,
) -> Option<String> {
    let a: Actor = actor(ctx, who)?;
    // 1. IsRight on the left-trimmed body
    if !is_right(body.trim_start_matches([' ', '\t'])) {
        return None;
    }
    let cfg = SpamCfg::from_gameplay(&ctx.cfg.rules.gameplay);
    let now = ctx.clock.game_ms;
    let full = ctx.player(who).is_some_and(is_shout_full);
    // 2.-5. ban, same text, limiter
    let refusal = ctx
        .player_mut(who)
        .and_then(|m| m.player_mut())
        .and_then(|p| p.chat.spam.shout_precheck(body, full, now, &cfg));
    if let Some(replies) = refusal {
        queue_replies(ctx, who, replies);
        return None;
    }
    // 6. the filter
    let mut body = body.to_owned();
    if ctx.cfg.rules.fidelity.chat_filter_server_side {
        let filtered = svc.chat.data.filter.word_change(&body);
        chat_tap::note_original(svc, who.0, &body, &filtered);
        body = filtered;
    }
    // 7. warning timers (no GM exemption)
    let (verdict, replies) = ctx
        .player_mut(who)
        .and_then(|m| m.player_mut())
        .map(|p| p.chat.spam.shout_warnings(shortcut, now, &cfg))?;
    queue_replies(ctx, who, replies);
    if verdict == SpamVerdict::Refuse {
        return None;
    }
    // 8. server pre-delivery checks
    let m = ctx.player(who)?;
    if is_mode(m, ModeFlags::SHOUTTALK) || a.world.0 == WI_WORLD_QUIZ {
        return None;
    }
    if let Some(tid) = mute_text(m) {
        notice::defined(ctx, who, tid, Vec::new());
        return None;
    }
    let min = ctx.cfg.rules.gameplay.shout_min_level;
    if min > 0 && i64::from(a.level) < i64::from(min) {
        notice::defined(ctx, who, TID_GAME_CANT_SHOUT_BELOW_20, Vec::new());
        return None;
    }
    // 9. length, trim, markup
    let drop = ctx.cfg.rules.gameplay.chat_drop_bytes as usize;
    let msg = last_full(&body);
    if msg.len() > drop {
        return None;
    }
    let msg = trim_right(msg).to_owned();
    let m = ctx.player(who)?;
    Some(markup_text(m, &msg))
}

/// `channel_text` (23 §6.0, §6.5, §6.6): the order of the legacy client for whisper, `/say`, party and guild
/// lines. `filter = true` runs `word_change` on the RAW body first (when `chat_filter_server_side`), then
/// `last_full`, the byte-length rule, trim right, markup unless FONTEDIT and CRLF. `None` = dropped silently.
pub fn channel_text(
    svc: &Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    body: &str,
    rule: LenRule,
    filter: bool,
) -> Option<String> {
    let m = ctx.player(who)?;
    let mut b = body.to_owned();
    if filter && ctx.cfg.rules.fidelity.chat_filter_server_side {
        b = svc.chat.data.filter.word_change(&b);
    }
    let msg = last_full(&b);
    let over = match rule {
        LenRule::DropAtOrAbove(n) => msg.len() >= n,
        LenRule::DropAbove(n) => msg.len() > n,
    };
    if over {
        return None;
    }
    let msg = trim_right(msg);
    Some(markup_text(m, msg))
}
