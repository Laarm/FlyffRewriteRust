//! `event::notice`: the auto notice of `Event.lua` and the world-message helpers of the events (Plan/22 §4.11, §17.4,
//! WP-EVENT-06, WP-EVENT-21).
//!
//! **Auto notice (22 §4.11).** The whole state of the notice (the start time, the interval, the number of notices
//! still to go, the next time) lives in `EventFunc.lua` (`Notice`, `IsNoticeTime`, `SetNextNoticeTime`): the
//! minute pass of [`EventLuaService::minute`](super::EventLuaService::minute) asks `IsNoticeTime()` for every wall
//! minute with the Lua clock pinned to that minute and collects the lines of `GetNoticeMessage()` in
//! [`MinuteReport::notices`](super::MinuteReport::notices). There is no `AutoNoticeService` struct (deviation from
//! the plan's module list, recorded in the NOTES): a Rust copy of the state would only duplicate the script's. The
//! exact-minute comparison and the minute rollover quirk of the script (`nMin = 60 - nMin`: 10:50 + 25 gives
//! `11:0-15`, which never matches again) are left alone.
//!
//! [`send_auto_notice`] sends the lines of one notice as `SystemNotice` to everybody. The Core read each line with
//! `ReadString(sz, 254)`, i.e. at most 253 bytes; a longer string was blanked and, because the read pointer jumps
//! to the end of the packet, so was every later line of the notice (`ar.cpp:109-120`). FIX: a longer line is cut to
//! 253 bytes on a character boundary and the other lines go out unchanged. The limit counts the UTF-8 bytes of the
//! decoded text (Korean text is longer in UTF-8 than in CP949; accepted).
//!
//! **World messages (22 §17.4).** [`world_msg`] is `AddWorldMsg` (`WorldMsg`, the pink event line) to everybody, to
//! one world or to a rectangle; the rectangle test uses x and z (`fidelity.world_msg_rect_xz`, PL-118: the original
//! compared x with the rectangle's left/right and y with its top/bottom). [`event_message`] is `AddEventMessage`
//! (`EventMessage`, the balloon over a mover): no V15 sender exists (`User.cpp:4328` only defines it); the helper is
//! for scripts and GM commands.

use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, Services};
use crate::outbox::Audience;
use flyff_common::{ObjId, WorldId};
use flyff_proto::pb;

/// Longest auto-notice line in bytes: `ReadString(sz, 254)` reads at most `254 - 1`.
pub const AUTO_NOTICE_MAX_BYTES: usize = 253;

/// The wire limit of `WorldMsg.text` (`str<=260`).
pub const WORLD_MSG_MAX_BYTES: usize = 260;

/// The wire limit of `EventMessage.text` (`str<=1024`).
pub const EVENT_MESSAGE_MAX_BYTES: usize = 1024;

/// `text` cut to at most `max` bytes on a character boundary.
pub fn cut_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Sends the lines of one auto notice (`GetNoticeMessage`) as `SystemNotice` to everybody, in order, each cut to
/// [`AUTO_NOTICE_MAX_BYTES`] (see the module documentation).
pub fn send_auto_notice(s: &Services, ctx: &mut GameCtx<'_>, lines: &[String]) {
    for line in lines {
        notice::system(s, ctx, cut_bytes(line, AUTO_NOTICE_MAX_BYTES));
    }
}

/// Who gets a [`world_msg`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldMsgTarget {
    /// Every player in a world (`AddWorldMsg(NULL)`).
    All,
    /// Every player of one world, any layer.
    World(WorldId),
    /// Every player inside the rectangle, in any world (like the C++); x against left/right and z against
    /// top/bottom when `fidelity.world_msg_rect_xz` is on (PL-118).
    Rect {
        /// Left edge.
        left: i32,
        /// Top edge.
        top: i32,
        /// Right edge.
        right: i32,
        /// Bottom edge.
        bottom: i32,
    },
}

/// `AddWorldMsg` (22 §17.4): `WorldMsg{text}` to `to`. `svc` is `None` when the caller is itself a service method
/// (the audiences used here need no services).
pub fn world_msg(svc: Option<&Services>, ctx: &mut GameCtx<'_>, to: WorldMsgTarget, text: &str) {
    let aud = match to {
        WorldMsgTarget::All => Audience::AllOnline,
        WorldMsgTarget::World(w) => Audience::World(w),
        WorldMsgTarget::Rect {
            left,
            top,
            right,
            bottom,
        } => Audience::Rect {
            left,
            top,
            right,
            bottom,
        },
    };
    ctx.broadcast(
        svc,
        ObjId::NULL,
        aud,
        pb::WorldMsg {
            text: cut_bytes(text, WORLD_MSG_MAX_BYTES).to_owned(),
        },
    );
}

/// `AddEventMessage` (22 §17.4): `EventMessage{text, flag, item_id}` with `obj_id = subject` to the viewers of
/// `subject` and the subject itself. `flag`: 0 fail effect, 1 per-item effect, 2 chat line (+ win effect for the
/// own player), 3 `TID_GAME_LACKSPACE`.
pub fn event_message(ctx: &mut GameCtx<'_>, subject: ObjId, text: &str, flag: i32, item_id: u32) {
    ctx.broadcast(
        None,
        subject,
        Audience::NearInclSelf,
        pb::EventMessage {
            text: cut_bytes(text, EVENT_MESSAGE_MAX_BYTES).to_owned(),
            flag,
            item_id,
        },
    );
}
