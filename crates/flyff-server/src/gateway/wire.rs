//! Frames the gateway itself writes: single-message `ServerBatch`es (11 §3.5), `Kick` and the close mapping of
//! 10 §7.7.

use super::sink::close_code;
use bytes::Bytes;
use flyff_common::rt::{CloseReason, KickReason};
use flyff_proto::{SessionOutbox, pb};

/// Encodes one message as a complete `ServerBatch` frame with `server_frame` (written last). A `ServerMsg`
/// without a body encodes as an empty batch entry-less frame (never produced by the gateway).
pub fn single_frame(m: pb::ServerMsg, server_frame: u64) -> Bytes {
    let mut out = SessionOutbox::new();
    if let Some(body) = m.msg {
        out.push_new(m.obj_id, body);
    }
    let mut frames = out.take_frames(server_frame, usize::MAX);
    if frames.is_empty() {
        return Bytes::new();
    }
    frames.remove(0)
}

/// A `Kick` message (11 §8.1).
pub fn kick_msg(reason: KickReason, reconnect_after_ms: u32) -> pb::Kick {
    pb::Kick {
        reason: pb::kick::Reason::from(reason) as i32,
        tid: 0,
        detail: String::new(),
        reconnect_after_ms,
    }
}

/// The `Kick` frame that goes before the close of `reason` (10 §7.7 table), if the reason has one.
pub fn kick_for_close(reason: CloseReason) -> Option<pb::Kick> {
    match reason {
        CloseReason::Kicked(r) => Some(kick_msg(r, 0)),
        CloseReason::Replaced => Some(kick_msg(KickReason::DuplicateLogin, 0)),
        CloseReason::ServerShutdown => Some(kick_msg(KickReason::Shutdown, 30_000)),
        CloseReason::ProtocolError => Some(kick_msg(KickReason::ProtocolError, 0)),
        CloseReason::InternalError => Some(kick_msg(KickReason::ServerError, 0)),
        CloseReason::Logout | CloseReason::SlowConsumer => None,
    }
}

/// The `Kick{reason}` frame as the bytes the gateway queues.
pub fn kick_frame(reason: CloseReason, server_frame: u64) -> Option<Bytes> {
    kick_for_close(reason).map(|k| {
        single_frame(
            pb::ServerMsg {
                obj_id: 0,
                msg: Some(k.into()),
            },
            server_frame,
        )
    })
}

/// The WebSocket close code of a reason (re-export for the session task).
pub fn code_of(reason: CloseReason) -> u16 {
    close_code(reason)
}
