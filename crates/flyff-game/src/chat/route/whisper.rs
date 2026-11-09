//! Whisper (`/w`) and the messenger instant message (`/say`), Plan/21 §10.4 (ref/24 §7.3-§7.4).

use super::filter::{ChatChannel, delivers};
use crate::admin::chat_tap::{self, ChatChannel as TapChannel};
use crate::chat::ingress::SnoopKind;
use crate::chat::ingress::{
    LenRule, PrivateKind, audit_private, channel_text, is_mode_of, mute_text, notice, snoop_copy,
};
use crate::chat::privacy::accepts_whisper;
use crate::chat::{actor, send_to};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::presence::DirEntry;
use crate::social::block_notice;
use crate::social::messenger::block::{BLOCK_WHISPER, friend_blocks};
use flyff_common::defs::text::TID_GAME_MSGBLOCKCHR;
use flyff_common::gm::ModeFlags;
use flyff_proto::pb;

/// Which message the route sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhisperKind {
    /// `/w` (`Whisper`).
    Whisper,
    /// `/say` (`Say`, the messenger IM).
    Im,
}

impl WhisperKind {
    fn rule(self, limit: usize) -> LenRule {
        match self {
            WhisperKind::Whisper => LenRule::DropAtOrAbove(limit),
            WhisperKind::Im => LenRule::DropAbove(limit),
        }
    }

    fn private(self) -> PrivateKind {
        match self {
            WhisperKind::Whisper => PrivateKind::Whisper,
            WhisperKind::Im => PrivateKind::Im,
        }
    }
}

fn message(
    kind: WhisperKind,
    from: &str,
    to: &str,
    text: &str,
    id_from: u32,
    id_to: u32,
    search: i32,
) -> pb::server_msg::Msg {
    match kind {
        WhisperKind::Whisper => pb::Whisper {
            from: from.to_owned(),
            to: to.to_owned(),
            text: text.to_owned(),
            id_from,
            id_to,
            search,
        }
        .into(),
        WhisperKind::Im => pb::Say {
            from: from.to_owned(),
            to: to.to_owned(),
            text: text.to_owned(),
            id_from,
            id_to,
            search,
        }
        .into(),
    }
}

/// `route::whisper(sender, target_name, body, kind)` in the order of 21 §10.4:
///
/// - `SAYTALK` is silent; the mute text goes out as `DefinedText` and stops;
/// - the target is the sender: `ReturnSay{2, " "}`; an unknown name: `ReturnSay{3, name}`;
/// - `channel_text` (no filter, no spam check); a whisper with an empty text sends nothing, a `/say` sends the
///   empty text;
/// - an offline target: the sender gets `Say{search: 1}` (both commands use the `Say` message for "not
///   connected");
/// - an online target is judged in this order (PL-095, WP-SOCIAL-20):
///   1. the SENDER friend-blocked the target: the sender gets `DefinedText(TID_GAME_MSGBLOCKCHR, [target name])`,
///      nothing is delivered and there is no echo;
///   2. the target has `privacy.whisper` off: the sender gets `BlockNtf{1, target name}`, nothing is delivered, no
///      echo;
///   3. the target friend-blocked or ignores the sender ([`delivers`]): nothing is delivered but the sender still
///      gets its echo (KEEP: the original receiving client dropped the line silently);
///   4. else the target gets the line first, then the sender's echo;
/// - `audit_private` runs in all four cases; the snoop copy and the chat tap only when the target got the line.
pub fn whisper(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    sender: PlayerRef,
    target_name: &str,
    body: &str,
    kind: WhisperKind,
) {
    let Some(a) = actor(ctx, sender) else {
        return;
    };
    if is_mode_of(ctx, sender, ModeFlags::SAYTALK) {
        return;
    }
    if let Some(tid) = ctx.player(sender).and_then(mute_text) {
        notice::defined(ctx, sender, tid, Vec::new());
        return;
    }
    let target: Option<DirEntry> = svc.presence.by_name(target_name).cloned();
    if target.as_ref().is_some_and(|t| t.id == a.character) {
        send_to(
            ctx,
            sender,
            pb::ReturnSay {
                flag: 2,
                text: " ".to_owned(),
            },
        );
        return;
    }
    let Some(target) = target else {
        send_to(
            ctx,
            sender,
            pb::ReturnSay {
                flag: 3,
                text: target_name.to_owned(),
            },
        );
        return;
    };
    let limit = ctx.cfg.rules.gameplay.chat_drop_bytes as usize;
    let Some(text) = channel_text(svc, ctx, sender, body, kind.rule(limit), false) else {
        return;
    };
    if kind == WhisperKind::Whisper && text.is_empty() {
        return;
    }
    let Some(online) = target.online else {
        // offline: "That player is not connected." (the audit of a line to nobody is still the sender's)
        send_to(ctx, sender, message(WhisperKind::Im, "", "", "", 0, 0, 1));
        audit_private(svc, ctx, sender, target.id, kind.private(), &text);
        return;
    };
    let target_obj = PlayerRef(online.obj);
    // 1. the sender blocked the target as a friend
    if friend_blocks(svc, a.character, target.id) {
        notice::defined(ctx, sender, TID_GAME_MSGBLOCKCHR, vec![target.name.clone()]);
        audit_private(svc, ctx, sender, target.id, kind.private(), &text);
        return;
    }
    // 2. the target refuses whispers and instant messages
    if !accepts_whisper(ctx.worlds, online.obj) {
        block_notice(ctx, sender, BLOCK_WHISPER, &target.name);
        audit_private(svc, ctx, sender, target.id, kind.private(), &text);
        return;
    }
    let m = message(
        kind,
        &a.name,
        &target.name,
        &text,
        a.character.0,
        target.id.0,
        0,
    );
    // 3. the target blocked or ignores the sender: not delivered, the echo stays
    let channel = match kind {
        WhisperKind::Whisper => ChatChannel::Whisper,
        WhisperKind::Im => ChatChannel::Im,
    };
    let received = delivers(svc, ctx.worlds, online.obj, sender.0, channel);
    // 4. the target first, then the echo to the sender (the order of the core's `SendWhisper`)
    if received {
        send_to(ctx, target_obj, m.clone());
    }
    send_to(ctx, sender, m);
    audit_private(svc, ctx, sender, target.id, kind.private(), &text);
    if !received {
        return;
    }
    snoop_copy(svc, ctx, SnoopKind::Whisper, sender, target.id, &text);
    // the admin console's chat tap (25 §4.9); a line to nobody (offline or refusing target) is not a conversation
    let tap_channel = match kind {
        WhisperKind::Whisper => TapChannel::Whisper,
        WhisperKind::Im => TapChannel::Messenger,
    };
    chat_tap::record(
        svc,
        ctx,
        sender,
        tap_channel,
        Some((target.id, &target.name)),
        &text,
    );
}
