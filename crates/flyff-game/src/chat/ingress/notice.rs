//! Notices and free texts (Plan/23 §6.13, F-030, F-042, F-059; 11 §10): the helpers every system calls to send
//! `DefinedText`, `Text`, `SystemNotice` and `Caption`.

use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use flyff_common::WorldId;
use flyff_proto::pb;
use flyff_rules::chat::{TEXT_DIAG, TEXT_GENERAL};

/// `defined_text(tid, args)`: the structured `DefinedText` of 11 §10.1 (arguments already formatted).
pub fn defined_text(tid: u32, args: Vec<String>) -> pb::DefinedText {
    pb::DefinedText {
        tid: i32::try_from(tid).unwrap_or(i32::MAX),
        args,
        display: pb::defined_text::Display::Message as i32,
    }
}

/// `AddDefinedText(tid, ...)` to one player.
pub fn defined(ctx: &mut GameCtx<'_>, to: PlayerRef, tid: u32, args: Vec<String>) {
    send_to(ctx, to, defined_text(tid, args));
}

/// `Text{kind: 1}`: a green chat line to one player (GM and admin output).
pub fn text(ctx: &mut GameCtx<'_>, to: PlayerRef, line: &str) {
    send_to(
        ctx,
        to,
        pb::Text {
            kind: TEXT_GENERAL,
            text: line.to_owned(),
        },
    );
}

/// `Text{kind: 2}`: a message box to one player.
pub fn diag(ctx: &mut GameCtx<'_>, to: PlayerRef, line: &str) {
    send_to(
        ctx,
        to,
        pb::Text {
            kind: TEXT_DIAG,
            text: line.to_owned(),
        },
    );
}

/// `SystemNotice` to every player in a world (the `/sys` broadcast; `text` is cut at the 511-byte limit of the
/// message by the caller).
pub fn system(svc: &Services, ctx: &mut GameCtx<'_>, line: &str) {
    ctx.broadcast(
        Some(svc),
        flyff_common::ObjId::NULL,
        Audience::AllOnline,
        pb::SystemNotice {
            text: line.to_owned(),
        },
    );
}

/// `Caption` to `to` (`Audience::AllOnline` for the world-wide ones; `world` is carried for the display filter).
pub fn caption(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    to: Audience,
    small: bool,
    world: WorldId,
    line: &str,
) {
    ctx.broadcast(
        Some(svc),
        flyff_common::ObjId::NULL,
        to,
        pb::Caption {
            small,
            world_id: world.0,
            text: line.to_owned(),
        },
    );
}
