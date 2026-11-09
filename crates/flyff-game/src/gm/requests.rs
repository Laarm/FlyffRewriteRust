//! Work a GM command cannot do from its handler because it needs the whole [`Game`] rather than the
//! `(Services, GameCtx)` pair: the in-game `/shutdown` trigger (the `ShutdownHandle` of `GameInit`, Plan/23 §8.6,
//! §13 Q14) and the live-configuration change of `/rates` (Plan/10 §8.6, `Game::change_runtime`).
//!
//! A handler pushes a [`GmRequest`] onto `Services.gm.requests`; `Game::drain_gm_requests` executes them in order
//! right after the input that carried the command (so a `/rates` is effective in the same tick), the same
//! pattern as `GameCtx::request_leave`.

use crate::overrides::RuntimeKey;
use crate::tick::Game;
use flyff_common::CharacterId;

/// A deferred effect of a GM command.
#[derive(Clone, Debug, PartialEq)]
pub enum GmRequest {
    /// `/shutdown <seconds> [message]`: starts or shortens the countdown through the `ShutdownCoordinator`.
    Shutdown {
        /// Seconds until everyone is logged out (`0..=3600`).
        countdown_s: u32,
        /// The operator message of the countdown notices.
        message: Option<String>,
        /// The issuing character (`CharacterId(0)` for the console and the admin API).
        by: CharacterId,
    },
    /// `/shutdown cancel`.
    CancelShutdown {
        /// The issuing character (`CharacterId(0)` for the console and the admin API).
        by: CharacterId,
    },
    /// `/rates <key> <value|default>`: a new `LiveConfig` generation and the `runtime_override` row.
    SetRuntime {
        /// The key (already range-checked by `gm::rates::validate`).
        key: RuntimeKey,
        /// The new value; `None` removes the override.
        value: Option<serde_json::Value>,
        /// The `runtime_override.set_by` label (`gm:<account>`).
        set_by: String,
    },
}

/// What a GM command asks of the server (Plan/23 §4.6, §4.9; WP-GM-25): the work the game thread cannot do itself
/// because it needs the disk (`scripts/npc/`, Plan/20 §3.9: "the game thread never touches the disk") or an async
/// call (`AdminOps::reload`, WP-OPS-16). The one game-to-server channel of the M7 batch next to
/// [`crate::input::AccountEventSink`]; an enum so that a later command can add a variant.
///
/// `notify` is the character who gets the result as a `[System]` line (the player or item issuer; `None` for the
/// console and the admin API, also with `as <name>`: they read the log), `by` the issuer label of
/// `gm::audit::issuer_label` (`gm:<account id>`, `console`, `owner:local`, ...).
#[derive(Clone, Debug, PartialEq)]
pub enum ServerRequest {
    /// `/loadscript`, `/reload dialog`: read `scripts/npc/` and send it to the game as `ReloadScripts{Npc, Some(..)}`.
    ReloadNpcScripts {
        /// Who hears the result.
        notify: Option<CharacterId>,
        /// The issuer label.
        by: String,
    },
    /// `/reload config`: `AdminOps::reload(ReloadBy::Gm(by))`.
    ReloadConfig {
        /// Who hears the result.
        notify: Option<CharacterId>,
        /// The issuer label.
        by: String,
    },
}

/// The game's end of the [`ServerRequest`] channel (`Services.gm.server_requests`, filled by `Game::new` from
/// `Handlers.server_requests`). Sending never blocks; the default sink is disconnected (tests, a game without a
/// server) and every send fails.
#[derive(Clone, Debug, Default)]
pub struct ServerRequestSink(Option<flume::Sender<ServerRequest>>);

impl ServerRequestSink {
    /// A sink over `tx`.
    pub fn new(tx: flume::Sender<ServerRequest>) -> Self {
        Self(Some(tx))
    }

    /// Queues `request`; `false` when the sink is disconnected, the server is gone or the channel is full (a burst of
    /// commands): the handler then answers "reload is not available here".
    pub fn send(&self, request: ServerRequest) -> bool {
        self.0
            .as_ref()
            .is_some_and(|tx| tx.try_send(request).is_ok())
    }
}

impl Game {
    /// Executes the requests the GM handlers queued, in order.
    pub(crate) fn drain_gm_requests(&mut self) {
        if self.services.gm.requests.is_empty() {
            return;
        }
        for req in std::mem::take(&mut self.services.gm.requests) {
            match req {
                GmRequest::Shutdown {
                    countdown_s,
                    message,
                    by,
                } => {
                    tracing::info!(countdown_s, by = by.0, "GM shutdown request");
                    self.services.gm.record(
                        "gm.shutdown_request",
                        serde_json::json!({"countdown_s": countdown_s, "message": message, "by": by.0}),
                    );
                    match &self.services.gm.shutdown_override {
                        Some(h) => h.0.request(Some(countdown_s), message, by),
                        None => self
                            .shutdown_handle()
                            .request(Some(countdown_s), message, by),
                    }
                }
                GmRequest::CancelShutdown { by } => {
                    tracing::info!(by = by.0, "GM shutdown cancel");
                    self.services
                        .gm
                        .record("gm.shutdown_cancel", serde_json::json!({"by": by.0}));
                    let _ = match &self.services.gm.shutdown_override {
                        Some(h) => h.0.cancel(by),
                        None => self.shutdown_handle().cancel(by),
                    };
                }
                GmRequest::SetRuntime { key, value, set_by } => {
                    if let Err(e) = self.change_runtime_by(key, value.as_ref(), &set_by) {
                        tracing::warn!(error = %e, "a GM runtime change was refused");
                    }
                }
            }
        }
    }
}
