//! `SessionSink` and the close/loss vocabulary (Plan/01 §4.4, 10 §6.3, §7.7).

use bytes::Bytes;

pub use crate::kick::KickReason;

/// Non-blocking output channel of one session, implemented by the gateway (`flyff-server`) and
/// called only by the game thread (ADR-011): the game never blocks on a socket.
pub trait SessionSink: Send + 'static {
    /// Queues ONE encoded `ServerBatch` for the session's writer task. Never blocks; `Full` means the
    /// sink has already scheduled a `SlowConsumer` close.
    fn send_batch(&self, batch: Bytes) -> Result<(), SinkError>;

    /// Asks the I/O side to close the socket after the already-queued batches were written.
    fn close(&self, reason: CloseReason);
}

/// Why [`SessionSink::send_batch`] failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    /// The outbound queue is full (frame or byte cap): the session is a slow consumer.
    #[error("outbound queue full")]
    Full,
    /// The socket is already closed.
    #[error("session closed")]
    Closed,
}

/// Why the game (or lobby) closes a session (Plan/10 §7.7 maps each to a WebSocket close code).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    /// Normal logout.
    Logout,
    /// Kicked, with the reason mirrored into the `Kick` message.
    Kicked(KickReason),
    /// Replaced by a newer login of the same account.
    Replaced,
    /// Server shutdown.
    ServerShutdown,
    /// Protocol violation.
    ProtocolError,
    /// Outbound queue overflow.
    SlowConsumer,
    /// Internal error (scoped panic, ...).
    InternalError,
}

/// Why a session's socket ended, reported to the game as `GameInput::SessionLost`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LostCause {
    /// The peer closed the connection.
    PeerClosed,
    /// No traffic within the idle timeout.
    IdleTimeout,
    /// A socket write failed.
    WriteFailed,
    /// Protocol violation.
    ProtocolError,
    /// Rate limit exceeded.
    RateLimited,
    /// Outbound queue overflow.
    SlowConsumer,
    /// Replaced by a newer login.
    Replaced,
    /// The socket ended after the game's `close(Kicked(r))` (GM kick, ban; 12 §7.3).
    Kicked(KickReason),
}
