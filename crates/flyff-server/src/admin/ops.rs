//! `AdminOps`: the one object behind every admin surface (Plan/10 §13, 23 §8; WP-OPS-09): the stdin console, the
//! `/admin/api/*` routes and the `/admin` page all call these methods, so a command does the same thing wherever it
//! comes from.
//!
//! It owns the *plumbing*, not the semantics: commands for the game thread travel as [`AdminCmd`]s over C1 and are
//! answered within the tick ([`AdminOps::game`]); the online registry, the session directory, the shutdown
//! coordinator and the persistence handle are consulted directly where the answer is theirs.

use crate::admin::auth::AdminAuth;
use crate::admin::json;
use crate::admin::shared::AdminShared;
use crate::audit::{Actor, AuditFile};
use crate::config::{CliOverrides, Config};
use crate::gateway::{GatewayShared, GatewayState};
use crate::http::HttpState;
use crate::lobby::LobbyCore;
use crate::lobby::accounts::AccountAdmin;
use crate::logging::LogHandles;
use crate::metrics::BuildLabels;
use crate::shutdown::{ShutdownCoordinator, ShutdownTrigger};
use flyff_common::rt::{HealthState, KickReason, Telemetry, reply_pair};
use flyff_common::{AccountId, SessionId};
use flyff_db::{PersistHandle, queries};
use flyff_game::GameInput;
use flyff_game::input::{
    AdminActor, AdminCmd, AdminOutput, AdminTarget, OnlineEntry, StatusSnapshot,
};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// How long a command waits for room in the game inbox.
pub(crate) const SEND_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a command waits for the answer of the game thread (it answers within the tick).
pub(crate) const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a command did not reach an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpsError {
    /// The game thread did not accept or answer the command in time.
    #[error("the game thread did not answer")]
    Timeout,
    /// The game thread has ended.
    #[error("the game thread is not running")]
    GameGone,
}

/// One row of `GET /admin/api/sessions` (`gateway`): a WebSocket of the gateway (10 §13.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GatewaySessionInfo {
    /// Session id.
    pub id: u64,
    /// The gateway state (`connected`, `char_select`, `entering`, `in_world`, `leaving`).
    pub state: &'static str,
    /// Client address.
    pub peer: String,
    /// Account name, once `Hello` succeeded.
    pub account: Option<String>,
    /// Seconds since the upgrade.
    pub age_s: u64,
    /// Bytes waiting in the outbound queue.
    pub queued_bytes: usize,
}

/// A request to start (or cancel) the shutdown countdown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShutdownReq {
    /// Seconds until everyone is logged out (`None` = `server.shutdown_countdown_s`).
    pub countdown_s: Option<u32>,
    /// Operator message for the countdown notices.
    pub message: Option<String>,
    /// Cancel the running countdown instead.
    pub cancel: bool,
}

/// What `reload` did (`POST /admin/api/reload`, console `reload`; `GET /admin/api/reload` answers what it would do):
/// `{changed, overridden, restart_required, errors, warnings}` (Plan/10 §8.6, WP-OPS-16).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ReloadReport {
    /// Keys whose new value is in force now (including the runtime keys the game took).
    pub changed: Vec<String>,
    /// Runtime keys (`[rates]`, `[events]`) whose file value differs but an override in force keeps winning.
    pub overridden: Vec<String>,
    /// Changed keys that need a restart; they keep their running value, so the next reload lists them again.
    pub restart_required: Vec<String>,
    /// Why the file could not be applied (it is not applied at all then), or what part of it was not.
    pub errors: Vec<String>,
    /// Findings of the loader that do not stop the reload (retired keys, reserved keys, odd variables).
    pub warnings: Vec<String>,
}

/// Where `reload` reads the configuration from: the same sources `serve` used.
#[derive(Clone, Debug)]
pub struct ReloadSource {
    /// The runtime home.
    pub home: PathBuf,
    /// The command-line overrides of `serve`.
    pub cli: CliOverrides,
    /// The `FLYFF__*` environment pairs.
    pub env: Vec<(String, String)>,
    /// Programmatic overrides (tests).
    pub overrides: Vec<(String, toml::Value)>,
    /// The resolved time zone name.
    pub zone_name: String,
}

/// Everything the admin surfaces use (cheap to share: it is an `Arc` everywhere).
pub struct AdminOps {
    /// C1.
    pub game_tx: flume::Sender<GameInput>,
    /// Shared counters.
    pub telemetry: Arc<Telemetry>,
    /// The gateway: session directory, limiter, counters.
    pub gateway: Arc<GatewayShared>,
    /// The lobby: registry and settings.
    pub core: Arc<LobbyCore>,
    /// The account service with its online effects (the admin API and the console words use it).
    pub accounts: AccountAdmin,
    /// The shutdown coordinator.
    pub coordinator: ShutdownCoordinator,
    /// The persistence handle.
    pub persist: PersistHandle,
    /// The log filter.
    pub log: Arc<LogHandles>,
    /// The HTTP state (live block and origin lists).
    pub http: HttpState,
    /// The audit appender (`gm.command`, `server.shutdown_requested`).
    pub audit: Arc<AuditFile>,
    /// Build identity.
    pub build: BuildLabels,
    /// The `flyff version` line.
    pub version_line: String,
    /// Where `reload` reads from.
    pub reload_source: ReloadSource,
    /// The configuration in force (what a reload is compared against).
    pub current: Mutex<Arc<Config>>,
    /// One reload (or preview) at a time: a SIGHUP storm and parallel requests queue here (`admin/reload.rs`).
    pub reload_lock: tokio::sync::Mutex<()>,
    /// The process start.
    pub started: Instant,
    /// The admin console's shared state: sessions, capabilities, guard policy, limits, counters (Plan/25).
    pub admin: Arc<AdminShared>,
    /// The authentication rule (`/metrics`, `/admin`, `/admin/api/*`): the login routes use its token comparison and
    /// failure window.
    pub auth: Arc<AdminAuth>,
}

impl std::fmt::Debug for AdminOps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminOps").finish_non_exhaustive()
    }
}

impl AdminOps {
    // ---- the game thread --------------------------------------------------------------------------------------

    /// Sends `cmd` to the game thread and waits for its answer.
    pub async fn game(&self, cmd: AdminCmd, actor: AdminActor) -> Result<AdminOutput, OpsError> {
        let (reply, rx) = reply_pair();
        let sent = tokio::time::timeout(
            SEND_TIMEOUT,
            self.game_tx
                .send_async(GameInput::Admin { cmd, actor, reply }),
        )
        .await;
        match sent {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(OpsError::GameGone),
            Err(_) => return Err(OpsError::Timeout),
        }
        match tokio::time::timeout(ANSWER_TIMEOUT, rx.recv_async()).await {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(_)) => Err(OpsError::GameGone),
            Err(_) => Err(OpsError::Timeout),
        }
    }

    /// The game's status line.
    pub async fn status_snapshot(&self) -> Result<StatusSnapshot, OpsError> {
        match self.game(AdminCmd::Status, AdminActor::Console).await? {
            AdminOutput::Status(s) => Ok(s),
            _ => Err(OpsError::GameGone),
        }
    }

    /// `GET /admin/api/status`: the game's status, the build and the `/healthz` fields.
    pub async fn status(&self) -> Result<Value, OpsError> {
        let s = self.status_snapshot().await?;
        let t = &self.telemetry;
        let health = t.health.state();
        let mut v = json::status_json(&s);
        if let Some(o) = v.as_object_mut() {
            o.insert("build".into(), self.build.version.clone().into());
            o.insert("proto_hash".into(), self.build.proto_hash.clone().into());
            o.insert("web_build".into(), self.build.web_build.clone().into());
            o.insert("state".into(), health.name().into());
            o.insert(
                "last_tick_age_ms".into(),
                t.last_tick_age_ms().map_or(Value::Null, Value::from),
            );
            o.insert(
                "players".into(),
                t.players_in_world.load(Ordering::Relaxed).into(),
            );
            o.insert(
                "accepting_logins".into(),
                t.health.accepting_logins.load(Ordering::Relaxed).into(),
            );
            o.insert("sessions".into(), self.session_counts());
            o.insert(
                "running".into(),
                matches!(health, HealthState::Running).into(),
            );
        }
        Ok(v)
    }

    // ---- sessions ---------------------------------------------------------------------------------------------

    /// The gateway sessions per state (`flyff_sessions{state}`).
    pub fn session_counts(&self) -> Value {
        let mut counts = [0u64; 5];
        for info in self.gateway.directory.list() {
            let i = GatewayState::ALL
                .iter()
                .position(|s| *s == info.handle.meta().state())
                .unwrap_or(0);
            if let Some(c) = counts.get_mut(i) {
                *c += 1;
            }
        }
        let mut m = serde_json::Map::new();
        for (s, n) in GatewayState::ALL.iter().zip(counts) {
            m.insert(s.name().to_owned(), n.into());
        }
        Value::Object(m)
    }

    /// The most recent `limit` gateway sessions (newest first).
    pub fn gateway_sessions(&self, limit: usize) -> Vec<GatewaySessionInfo> {
        let mut rows: Vec<(SessionId, GatewaySessionInfo)> = self
            .gateway
            .directory
            .list()
            .into_iter()
            .map(|i| {
                let meta = i.handle.meta();
                let account = meta
                    .account()
                    .and_then(|a| self.core.registry.view(a))
                    .map(|v| v.name.as_str().to_owned());
                (
                    i.handle.id(),
                    GatewaySessionInfo {
                        id: i.handle.id().0,
                        state: meta.state().name(),
                        peer: i.peer.to_string(),
                        account,
                        age_s: meta.age_s(),
                        queued_bytes: meta.queued_bytes(),
                    },
                )
            })
            .collect();
        rows.sort_by_key(|a| std::cmp::Reverse(a.0));
        rows.into_iter().take(limit).map(|(_, r)| r).collect()
    }

    /// The avatars in the world with the client address filled from the online registry.
    pub async fn online(
        &self,
        limit: Option<u32>,
        actor: AdminActor,
    ) -> Result<Vec<OnlineEntry>, OpsError> {
        match self.game(AdminCmd::ListOnline { limit }, actor).await? {
            AdminOutput::Online(mut rows) => {
                let ips: std::collections::BTreeMap<AccountId, String> = self
                    .core
                    .registry
                    .views()
                    .into_iter()
                    .filter_map(|v| {
                        self.core
                            .registry
                            .ip_of(v.account)
                            .map(|ip| (v.account, ip.to_string()))
                    })
                    .collect();
                for r in &mut rows {
                    if let Some(ip) = ips.get(&r.account_id) {
                        r.ip.clone_from(ip);
                    }
                }
                Ok(rows)
            }
            _ => Ok(Vec::new()),
        }
    }

    // ---- kick and notice --------------------------------------------------------------------------------------

    /// `POST /admin/api/kick` / console `kick`: an account name first, then a character name. Returns the number
    /// of sessions that took the kick, or `None` when nothing is called that.
    pub async fn kick(&self, name: &str, actor: AdminActor) -> Result<Option<u32>, OpsError> {
        let wanted = name.trim().to_owned();
        if let Ok(acct) = crate::lobby::accounts::AccountName::parse(&wanted) {
            let n = acct.as_str().to_owned();
            if let Ok(Some(record)) = self
                .persist
                .read(move |c| queries::find_account(c, &n))
                .await
            {
                return self.kick_account(record.id).await.map(Some);
            }
        }
        match self
            .game(
                AdminCmd::Kick {
                    target: AdminTarget::Character(wanted),
                    reason: None,
                    banned: false,
                },
                actor,
            )
            .await?
        {
            AdminOutput::Done { affected } => Ok(Some(affected)),
            _ => Ok(None),
        }
    }

    /// Kicks the avatar of an account (`KickAccount{GmKick}`: immediate logout, 12 §12.2).
    pub async fn kick_account(&self, account: AccountId) -> Result<u32, OpsError> {
        let (reply, rx) = reply_pair();
        let input = GameInput::KickAccount {
            account,
            reason: KickReason::GmKick,
            reply: Some(reply),
        };
        match tokio::time::timeout(SEND_TIMEOUT, self.game_tx.send_async(input)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(OpsError::GameGone),
            Err(_) => return Err(OpsError::Timeout),
        }
        match tokio::time::timeout(ANSWER_TIMEOUT, rx.recv_async()).await {
            Ok(Ok(found)) => Ok(u32::from(found)),
            Ok(Err(_)) => Err(OpsError::GameGone),
            Err(_) => Err(OpsError::Timeout),
        }
    }

    /// `POST /admin/api/notice`: a server-wide notice (`/sys`).
    pub async fn notice(&self, text: &str, actor: AdminActor) -> Result<AdminOutput, OpsError> {
        self.game(
            AdminCmd::Broadcast {
                text: text.to_owned(),
            },
            actor,
        )
        .await
    }

    // ---- shutdown ---------------------------------------------------------------------------------------------

    /// `POST /admin/api/shutdown` / console `shutdown`. Returns the countdown that was started, or the text of
    /// the refusal (cancelling when nothing counts down).
    pub fn shutdown(&self, req: &ShutdownReq, actor: AdminActor) -> Result<Option<u32>, String> {
        let trigger = match actor {
            AdminActor::Owner { remote, .. } | AdminActor::Staff { remote, .. } => {
                ShutdownTrigger::AdminApi {
                    ip: remote.to_string(),
                }
            }
            AdminActor::Console | AdminActor::Cli => ShutdownTrigger::Console,
        };
        if req.cancel {
            // the coordinator audits every trigger and cancel (`server.shutdown_requested`)
            if self.coordinator.cancel(&trigger) {
                return Ok(None);
            }
            return Err("no shutdown countdown is running".to_owned());
        }
        let n = self
            .coordinator
            .request(req.countdown_s, req.message.clone(), trigger);
        Ok(Some(n))
    }

    // ---- reload: see `admin/reload.rs` ------------------------------------------------------------------------

    /// Emits `gm.command` for a command of the console or the admin API (10 §11): the closing record of the
    /// command (`state: done`, or `failed` for `result: "error"`).
    pub fn audit_command(&self, actor: AdminActor, line: &str, result: &str) {
        self.audit.emit(
            "gm.command",
            &Self::command_actor(actor),
            json!({
                "issuer": Self::issuer(actor), "actor": actor.label(), "line": line, "result": result,
                "state": if result == "ok" { "done" } else { "failed" },
            }),
        );
    }

    /// The `requested` record of a command, acknowledged by the audit writer *before* the command is dispatched
    /// (Plan/25 §10.1): `Err(503 audit_unavailable)` when it is not and `[audit] admin_required` is on. Commands of
    /// the stdin console and the CLI (no HTTP identity) are not held back: nobody is waiting for a status code.
    pub fn audit_requested(
        &self,
        actor: AdminActor,
        line: &str,
    ) -> Result<(), crate::admin::error::ApiError> {
        let sent = self.audit.emit_admin(
            "gm.command",
            &Self::command_actor(actor),
            json!({
                "issuer": Self::issuer(actor), "actor": actor.label(), "line": line, "state": "requested",
            }),
        );
        match sent {
            Ok(()) => Ok(()),
            Err(e) if self.audit.admin_required() && actor.is_http() => {
                tracing::error!(error = %e, "the audit record of a console command was not acknowledged: refused");
                Err(crate::admin::error::ApiError::audit_unavailable())
            }
            Err(e) => {
                tracing::warn!(error = %e, "the audit record of a console command was not acknowledged");
                Ok(())
            }
        }
    }

    fn issuer(actor: AdminActor) -> &'static str {
        match actor {
            AdminActor::Console => "console",
            AdminActor::Owner { .. } | AdminActor::Staff { .. } => "admin-api",
            AdminActor::Cli => "cli",
        }
    }

    fn command_actor(actor: AdminActor) -> Actor {
        match actor.remote() {
            Some(ip) => Actor::ip(ip),
            None => Actor::none(),
        }
    }
}
