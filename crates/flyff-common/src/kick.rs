//! `KickReason`: the dependency-free mirror of `pb::kick::Reason` (Plan/11 §8.1, 10 §7.7).
//!
//! It lives outside `rt` so that `flyff-proto` (wasm-compatible, no `rt`) can implement
//! `From<KickReason> for pb::kick::Reason`; `flyff_common::rt::KickReason` re-exports it.

/// Mirror of `pb::kick::Reason` (11 §8.1) so `flyff-common` stays dependency-free. The WebSocket close
/// code of a `Kick` is `4000 + value` (10 §7.7).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KickReason {
    /// Unspecified.
    Unspecified = 0,
    /// Protocol violation.
    ProtocolError = 1,
    /// Rate limit exceeded.
    RateLimit = 2,
    /// Client message above the size cap.
    MessageTooBig = 3,
    /// Message invalid in the current session state.
    InvalidState = 4,
    /// Same account logged in elsewhere.
    DuplicateLogin = 5,
    /// Session timeout.
    Timeout = 6,
    /// GM kick.
    GmKick = 7,
    /// Account banned.
    Banned = 8,
    /// Server shutting down.
    Shutdown = 9,
    /// Internal server error.
    ServerError = 10,
    /// Slow consumer.
    SlowConsumer = 11,
    /// The browser must reload (new client build).
    ReloadRequired = 12,
    /// Session token expired.
    SessionExpired = 13,
}

impl KickReason {
    /// Every reason in value order.
    pub const ALL: [KickReason; 14] = [
        KickReason::Unspecified,
        KickReason::ProtocolError,
        KickReason::RateLimit,
        KickReason::MessageTooBig,
        KickReason::InvalidState,
        KickReason::DuplicateLogin,
        KickReason::Timeout,
        KickReason::GmKick,
        KickReason::Banned,
        KickReason::Shutdown,
        KickReason::ServerError,
        KickReason::SlowConsumer,
        KickReason::ReloadRequired,
        KickReason::SessionExpired,
    ];

    /// Numeric value (the proto enum value; the WebSocket close code is `4000 + value`).
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// The WebSocket close code of a `Kick` with this reason (`4000 + value`, 11 §1.5).
    pub const fn close_code(self) -> u16 {
        4000 + self as u16
    }
}
