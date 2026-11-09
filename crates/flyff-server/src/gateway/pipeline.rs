//! The inbound pipeline of the gateway (Plan/10 §7.4, WP-NET-02): a pure, clock-free state machine per
//! session that turns one WebSocket frame into a [`Verdict`], and the state table of VR-003 ([`route`]).
//!
//! Steps 1–6 of §7.4 live in [`InboundPipeline::binary`] / [`InboundPipeline::text`] (size cap, binary only,
//! token bucket with the 10 s drop window, strict decode, generated length limits); step 7 is [`route`]; step 8
//! (routing) is the session task's. The bucket is counted before decoding, so floods of garbage are limited too.

use super::lobby_port::GatewayState;
use flyff_common::rt::KickReason;
use flyff_proto::{ClientCase, ClientMsg, Limits as _, ProtoError, decode_client};

/// Why a message was dropped (the `reason` label of `flyff_inbound_dropped_total`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum DropReason {
    /// A frame above the size cap (closes with 1009).
    Size = 0,
    /// A text frame.
    Text = 1,
    /// Over the session's token bucket.
    Rate = 2,
    /// A decode error (or an envelope without a message) with `net.strict_decode = false`.
    Decode = 3,
    /// A generated length limit exceeded (`(flyff.max_len)`) with `net.strict_decode = false`.
    Len = 4,
    /// A message class the session state does not accept.
    State = 5,
}

impl DropReason {
    /// Number of reasons.
    pub const COUNT: usize = 6;
    /// Every reason in value order.
    pub const ALL: [DropReason; Self::COUNT] = [
        DropReason::Size,
        DropReason::Text,
        DropReason::Rate,
        DropReason::Decode,
        DropReason::Len,
        DropReason::State,
    ];

    /// The metric label.
    pub const fn label(self) -> &'static str {
        match self {
            DropReason::Size => "size",
            DropReason::Text => "text",
            DropReason::Rate => "rate",
            DropReason::Decode => "decode",
            DropReason::Len => "len",
            DropReason::State => "state",
        }
    }
}

/// The limits of one session (from `GatewayConfig`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// `net.max_client_msg_bytes`.
    pub max_msg_bytes: usize,
    /// `net.strict_decode`.
    pub strict_decode: bool,
    /// `net.client_msgs_per_sec`.
    pub per_sec: u32,
    /// `net.client_msgs_burst`.
    pub burst: u32,
    /// `net.rate_kick_factor`.
    pub kick_factor: u32,
}

/// A token bucket in milliseconds.
#[derive(Clone, Copy, Debug)]
pub struct TokenBucket {
    tokens: f64,
    last_ms: u64,
    rate_per_ms: f64,
    burst: f64,
}

impl TokenBucket {
    /// A full bucket at `now_ms` (`per_sec` tokens per second, at most `burst`).
    pub fn new(per_sec: u32, burst: u32, now_ms: u64) -> Self {
        Self {
            tokens: f64::from(burst.max(1)),
            last_ms: now_ms,
            rate_per_ms: f64::from(per_sec) / 1000.0,
            burst: f64::from(burst.max(1)),
        }
    }

    /// Takes one token if available.
    pub fn take(&mut self, now_ms: u64) -> bool {
        let elapsed = now_ms.saturating_sub(self.last_ms);
        self.last_ms = self.last_ms.max(now_ms);
        self.tokens = (self.tokens + elapsed as f64 * self.rate_per_ms).min(self.burst);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Counts drops in the last 10 seconds (ten one-second slots).
#[derive(Clone, Copy, Debug, Default)]
pub struct DropWindow {
    slots: [u32; 10],
    last_sec: u64,
}

impl DropWindow {
    /// Records one drop at `now_ms` and returns the number of drops within the last 10 s.
    pub fn record(&mut self, now_ms: u64) -> u32 {
        let sec = now_ms / 1000;
        if sec > self.last_sec {
            let gap = (sec - self.last_sec).min(10);
            for i in 1..=gap {
                let idx = usize::try_from((self.last_sec + i) % 10).unwrap_or(0);
                if let Some(s) = self.slots.get_mut(idx) {
                    *s = 0;
                }
            }
            self.last_sec = sec;
        }
        let idx = usize::try_from(sec % 10).unwrap_or(0);
        if let Some(s) = self.slots.get_mut(idx) {
            *s = s.saturating_add(1);
        }
        self.slots.iter().sum()
    }
}

/// What to do with one inbound frame.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// A decoded message that passed every check; route it (step 7/8).
    Message(Box<ClientMsg>),
    /// Dropped and counted.
    Dropped(DropReason),
    /// Close with `Kick{reason}` (close code `4000 + reason`); the counter label is `why`.
    Kick {
        /// The kick reason.
        reason: KickReason,
        /// Counted under this drop reason.
        why: DropReason,
    },
    /// A frame above the cap: close with 1009, no `Kick` frame.
    TooBig,
}

/// The per-session pipeline state (bucket and drop window).
#[derive(Clone, Copy, Debug)]
pub struct InboundPipeline {
    limits: Limits,
    bucket: TokenBucket,
    drops: DropWindow,
}

impl InboundPipeline {
    /// A pipeline whose bucket is full at `now_ms`.
    pub fn new(limits: Limits, now_ms: u64) -> Self {
        Self {
            limits,
            bucket: TokenBucket::new(limits.per_sec, limits.burst, now_ms),
            drops: DropWindow::default(),
        }
    }

    /// Steps 2 and 4 for a text frame: always a protocol error (binary frames only).
    pub fn text(&mut self) -> Verdict {
        Verdict::Kick {
            reason: KickReason::ProtocolError,
            why: DropReason::Text,
        }
    }

    /// Steps 1, 4, 5, 6 for a binary frame.
    pub fn binary(&mut self, now_ms: u64, data: &[u8]) -> Verdict {
        // 1. the WS layer enforces the cap with 1009; this is the same rule for frames that got through
        if data.len() > self.limits.max_msg_bytes {
            return Verdict::TooBig;
        }
        // 4. token bucket, counted before decoding
        if !self.bucket.take(now_ms) {
            let recent = self.drops.record(now_ms);
            let kick_at = u64::from(self.limits.kick_factor) * u64::from(self.limits.per_sec);
            if kick_at > 0 && u64::from(recent) > kick_at {
                return Verdict::Kick {
                    reason: KickReason::RateLimit,
                    why: DropReason::Rate,
                };
            }
            return Verdict::Dropped(DropReason::Rate);
        }
        // 5. strict decode
        let env = match decode_client(data, self.limits.max_msg_bytes) {
            Ok(env) => env,
            Err(ProtoError::TooBig { .. }) => return Verdict::TooBig,
            Err(_) if !self.limits.strict_decode => return Verdict::Dropped(DropReason::Decode),
            Err(_) => {
                return Verdict::Kick {
                    reason: KickReason::ProtocolError,
                    why: DropReason::Decode,
                };
            }
        };
        // 6. generated length limits
        if env.check_limits().is_err() {
            return if self.limits.strict_decode {
                Verdict::Kick {
                    reason: KickReason::ProtocolError,
                    why: DropReason::Len,
                }
            } else {
                Verdict::Dropped(DropReason::Len)
            };
        }
        match env.msg {
            Some(msg) => Verdict::Message(Box::new(msg)),
            None => Verdict::Dropped(DropReason::Decode),
        }
    }
}

/// What the state table does with a message (10 §7.3, VR-003).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// `Hello` in `Connected`: the lobby's `hello`.
    Hello,
    /// `CreatePlayer` / `DelPlayer` / `EnterWorld` in `CharSelect`: the lobby's `char_select`.
    Lobby,
    /// A game message (incl. `ReqLeave`, `ReturnToCharSelect`) in `InWorld`: `GameInput::Client`.
    Game,
    /// `ServerTimeReq`: answered by the gateway (every state except `Connected`).
    ServerTime,
    /// Dropped and counted (`reason="state"`).
    Drop,
    /// `Kick{INVALID_STATE}`.
    Kick,
}

/// The message class of the state table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Hello,
    Lobby,
    ServerTime,
    Game,
}

fn class_of(case: ClientCase) -> Class {
    match case {
        ClientCase::Hello => Class::Hello,
        ClientCase::CreatePlayer | ClientCase::DelPlayer | ClientCase::EnterWorld => Class::Lobby,
        ClientCase::ServerTimeReq => Class::ServerTime,
        _ => Class::Game,
    }
}

/// The state table of 10 §7.3 / 11 §2.2.
pub fn route(state: GatewayState, case: ClientCase) -> Route {
    let class = class_of(case);
    match (state, class) {
        (GatewayState::Connected, Class::Hello) => Route::Hello,
        (GatewayState::Connected, _) => Route::Kick,
        (GatewayState::CharSelect, Class::Lobby) => Route::Lobby,
        (GatewayState::CharSelect, Class::ServerTime) => Route::ServerTime,
        (GatewayState::CharSelect, _) => Route::Kick,
        (GatewayState::Entering | GatewayState::Leaving, Class::ServerTime) => Route::ServerTime,
        (GatewayState::Entering | GatewayState::Leaving, _) => Route::Drop,
        (GatewayState::InWorld, Class::Game) => Route::Game,
        (GatewayState::InWorld, Class::ServerTime) => Route::ServerTime,
        (GatewayState::InWorld, _) => Route::Kick,
    }
}
