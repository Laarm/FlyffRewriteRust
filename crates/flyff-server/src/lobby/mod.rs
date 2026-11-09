//! The lobby (Plan/12): everything between "the browser has loaded the page" and "the player stands in the
//! world". This task (`lobby-auth`) owns the account and session side: the HTTP API (`GET /api/bootstrap`,
//! `POST /api/login`, `POST /api/register`), account storage and administration, password hashing and the
//! login throttle, the online registry with the session tokens, and the first-start administrator. The
//! WebSocket side (`Hello`, the character list, create, delete: [`charsel`], task `lobby-chars`; enter world and
//! leave: `world-flow`) is built on top of [`LobbyCore`].
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`auth`] | [`auth::PwHash`], [`auth::PasswordHasher`] (2 permits), [`auth::LoginThrottle`] | AUTH-02 |
//! | [`accounts`] | [`accounts::AccountName`], [`accounts::AccountAdmin`] | AUTH-02, AUTH-05 |
//! | [`tokens`], [`online`], [`events`], [`sweeper`] | session tokens, [`online::OnlineRegistry`], the `NetEvent` consumer, the 1 Hz sweeper | AUTH-03 |
//! | [`login`], [`bootstrap`], [`errors`], [`http`] | the claim/takeover flow, `/api/bootstrap`, `/api/login`, the routes | AUTH-04 |
//! | [`bootstrap_admin`] | the first-start `admin` account | AUTH-05 |
//! | [`register`] | `/api/register` and its per-IP limiter | AUTH-10 |
//! | [`charsel`] | [`Lobby`]: `Hello`/`HelloAck`, `CharList`, create, delete (the `LobbyPort` of the gateway) | AUTH-06, -07, -08 |
//! | [`stats`] | counters for `/metrics` | AUTH-17 (counters only) |
//!
//! [`LobbyCore`] is the shared state: registry, throttle, hasher, settings, the persistence handle, the game
//! inbox and the session directory. The rule of 12 §2 (LB1) holds everywhere: the registry lock is only held
//! for synchronous state changes; every wait happens outside it and the state is re-checked afterwards.

#![deny(clippy::await_holding_lock, clippy::await_holding_refcell_ref)]

pub mod accounts;
pub mod admin_routes;
pub mod auth;
pub mod bootstrap;
pub mod bootstrap_admin;
pub mod charsel;
pub mod errors;
pub mod events;
pub mod http;
pub mod login;
pub mod online;
pub mod register;
pub mod stats;
pub mod sweeper;
pub mod tokens;

use self::accounts::{AccountAdmin, GmPolicy, OnlineEffects, OnlineState};
use self::auth::{LoginThrottle, PasswordHasher};
use self::online::{EntryKind, LobbyClock, OnlineRegistry, RegistryAction};
use self::register::RegisterLimiter;
use self::stats::LobbyStats;
use crate::audit::AuditFile;
use crate::config::{AuthConfig, Config};
use crate::gateway::{GatewayState, SessionDirectory};
use crate::http::cidr::CidrList;
use flyff_common::config::ClientFeatureConfig;
use flyff_common::gm::{Authority, GmConfig};
use flyff_common::rt::{KickReason, LostCause, Telemetry};
use flyff_common::{AccountId, CharacterId, SaveGen, SessionId};
use flyff_db::PersistHandle;
use flyff_game::GameInput;
use flyff_game::input::{AdminActor, AdminCmd, JoinFailReason, NetEvent};
use flyff_proto::pb;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

pub use self::charsel::{Lobby, LobbyInfo};
pub use self::online::{ClaimRequest, ClaimStep};

/// The persistence queue depth above which logins answer `503 / 109` (12 §5.8).
pub const DEGRADED_QUEUE_DEPTH: usize = 10_000;

/// The slice of the configuration the lobby reads, one immutable snapshot (12 §4). The reloadable keys
/// (`auth.allow_registration`, `delete_requires_password`, `max_in_world`, `register_per_ip_per_hour`,
/// `maintenance`, `maintenance_allow_ips`) change by [`LobbyCore::reload`].
#[derive(Clone, Debug)]
pub struct LobbySettings {
    /// `[auth]`.
    pub auth: AuthConfig,
    /// `features.servers_authorize`.
    pub servers_authorize: bool,
    /// `[gm]` (validated).
    pub gm: Arc<GmConfig>,
    /// `gameplay.min_name_letters`.
    pub min_name_letters: u32,
    /// `gameplay.max_name_letters`.
    pub max_name_letters: u32,
    /// `server.name`.
    pub server_name: String,
    /// The effective IANA zone name (`server.timezone`, `"local"` resolved).
    pub server_timezone: String,
    /// `database.flush_wait_timeout_ms`.
    pub flush_wait: Duration,
    /// `features.wrong_pw_fix`.
    pub wrong_pw_fix: bool,
    /// What the browser learns of `[features]`, `[gameplay]` and `[fidelity]`.
    pub client_features: ClientFeatureConfig,
    /// `auth.maintenance_allow_ips` parsed.
    pub maintenance_allow: CidrList,
}

impl LobbySettings {
    /// Projects the loaded configuration. `zone_name` is the resolved server time zone. A bad `[gm]` grant
    /// list is a configuration error that the loader reports before the server starts; here it is logged and
    /// ignored (an empty grant list), so hand-built configurations still work.
    pub fn from_config(cfg: &Config, zone_name: &str) -> LobbySettings {
        let gm = cfg.gm.typed().unwrap_or_else(|problems| {
            for p in problems {
                tracing::error!("[gm] grants ignored: {p}");
            }
            GmConfig::default()
        });
        let (maintenance_allow, _bad) = CidrList::parse_all(&cfg.auth.maintenance_allow_ips);
        LobbySettings {
            auth: cfg.auth.clone(),
            servers_authorize: cfg.features.servers_authorize,
            gm: Arc::new(gm),
            min_name_letters: cfg.gameplay.min_name_letters,
            max_name_letters: cfg.gameplay.max_name_letters,
            server_name: cfg.server.name.clone(),
            server_timezone: zone_name.to_owned(),
            flush_wait: Duration::from_millis(u64::from(cfg.database.flush_wait_timeout_ms)),
            wrong_pw_fix: cfg.features.wrong_pw_fix,
            client_features: ClientFeatureConfig::from(&cfg.rules()),
            maintenance_allow,
        }
    }
}

/// How the lobby sees the server (12 §5.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Logins are accepted.
    Open,
    /// A graceful shutdown started (or the server is not up yet): `503 / 109`.
    ShuttingDown,
    /// The persistence layer is degraded (retry mode or a queue above 10 000): `503 / 109`.
    Degraded,
}

/// What a lobby front-end installs to continue sessions after registry events that need its list/CharList
/// code (12 §7.5): the character-select continuation after `Left{next: CharSelect}` and the `LobbyError` after
/// `JoinFailed`. Without a hook the lobby sets the session state and (for `JoinFailed`) pushes the
/// `LobbyError` itself; the hook adds the `CharList`.
pub trait SessionWake: Send + Sync {
    /// The avatar of `account` went back to character select on `session`: wait for the flush of `last_gen`,
    /// then send the `CharList` (12 §7.5, §12.4).
    fn char_select_after_leave(&self, session: SessionId, account: AccountId, last_gen: SaveGen);
    /// The join of `session` failed; the `LobbyError` was pushed and the session is back at character select:
    /// send the `CharList`.
    fn join_failed(&self, session: SessionId, account: AccountId, reason: JoinFailReason);
}

/// What [`LobbyCore::new`] needs.
pub struct LobbyCoreInit {
    /// The configuration snapshot.
    pub settings: LobbySettings,
    /// The persistence handle.
    pub persist: PersistHandle,
    /// C1: the game inbox.
    pub game_tx: flume::Sender<GameInput>,
    /// All live session tasks (kick a socket, wake a session).
    pub directory: Arc<SessionDirectory>,
    /// Shared counters (`health.accepting_logins`).
    pub telemetry: Arc<Telemetry>,
    /// The lobby clock (tests pass a manual one).
    pub clock: LobbyClock,
    /// Login-gated asset cookies (`HttpState.tokens`), revoked on a ban.
    pub asset_tokens: Option<Arc<crate::http::asset_auth::AssetTokens>>,
    /// A hasher other than the production one (tests).
    pub hasher: Option<PasswordHasher>,
    /// Where the audit events of 12 §19.1 go (`None` = nowhere).
    pub audit: Option<Arc<AuditFile>>,
}

/// The shared state of the lobby.
pub struct LobbyCore {
    settings: RwLock<Arc<LobbySettings>>,
    /// The persistence handle.
    pub persist: PersistHandle,
    /// argon2id behind 2 permits.
    pub hasher: PasswordHasher,
    /// The online registry and the session tokens.
    pub registry: OnlineRegistry,
    /// The per-IP wrong-password lock.
    pub throttle: LoginThrottle,
    /// The per-IP registration window.
    pub register_limiter: RegisterLimiter,
    /// The lobby clock.
    pub clock: LobbyClock,
    /// The live sessions.
    pub directory: Arc<SessionDirectory>,
    /// Shared counters.
    pub telemetry: Arc<Telemetry>,
    /// Lobby counters.
    pub stats: LobbyStats,
    /// The admin console's account events (25 §1.8): `AccountAdmin` publishes bans, authority and password changes
    /// here; the admin sessions of the account end.
    pub account_events: crate::admin::account_events::AccountEvents,
    /// The audit events of 12 §19.1 (WP-AUTH-17).
    pub audit: Arc<AuditFile>,
    game_tx: flume::Sender<GameInput>,
    asset_tokens: Option<Arc<crate::http::asset_auth::AssetTokens>>,
    wake: OnceLock<Arc<dyn SessionWake>>,
    /// How often a drain task of a poisoned account looks for the release (milliseconds).
    drain_poll_ms: AtomicU64,
    runtime: OnceLock<tokio::runtime::Handle>,
    sweeper: OnceLock<sweeper::SweeperHandle>,
    me: Weak<LobbyCore>,
}

impl std::fmt::Debug for LobbyCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LobbyCore")
            .field("online", &self.registry.len())
            .finish_non_exhaustive()
    }
}

impl LobbyCore {
    /// Builds the core. Drain tasks run on the tokio runtime of the caller (when `new` runs inside one) or on
    /// the runtime given to [`LobbyCore::start_background`].
    pub fn new(init: LobbyCoreInit) -> Arc<LobbyCore> {
        let grace = init.settings.auth.token_grace_s;
        let wrong_pw_fix = init.settings.wrong_pw_fix;
        let clock = init.clock;
        Arc::new_cyclic(|me| LobbyCore {
            settings: RwLock::new(Arc::new(init.settings)),
            persist: init.persist,
            hasher: init.hasher.unwrap_or_default(),
            registry: OnlineRegistry::new(clock.clone(), grace),
            throttle: LoginThrottle::new(wrong_pw_fix),
            register_limiter: RegisterLimiter::new(),
            clock,
            directory: init.directory,
            telemetry: init.telemetry,
            stats: LobbyStats::default(),
            account_events: crate::admin::account_events::AccountEvents::new(),
            audit: init.audit.unwrap_or_else(AuditFile::disabled),
            game_tx: init.game_tx,
            asset_tokens: init.asset_tokens,
            wake: OnceLock::new(),
            drain_poll_ms: AtomicU64::new(5000),
            runtime: tokio::runtime::Handle::try_current()
                .ok()
                .map_or_else(OnceLock::new, OnceLock::from),
            sweeper: OnceLock::new(),
            me: me.clone(),
        })
    }

    /// Starts the 1 Hz sweeper on `rt` and remembers `rt` for the drain tasks. The task holds only a weak
    /// reference: it ends when the core is dropped. Call it once, when the server starts.
    pub fn start_background(self: &Arc<Self>, rt: &tokio::runtime::Handle) {
        let _ = self.runtime.set(rt.clone());
        let handle = sweeper::spawn(self, rt);
        let _ = self.sweeper.set(handle);
    }

    /// How often the drain task of a poisoned account checks for `db release` (default 5 s; tests shorten it).
    pub fn set_drain_poll(&self, every: Duration) {
        self.drain_poll_ms.store(
            u64::try_from(every.as_millis()).unwrap_or(u64::MAX).max(1),
            Ordering::Relaxed,
        );
    }

    /// The current settings snapshot.
    pub fn settings(&self) -> Arc<LobbySettings> {
        Arc::clone(&self.settings.read())
    }

    /// Replaces the settings (`reload`, WP-OPS-16): only the keys marked reloadable in 12 §4 are meant to
    /// differ; the throttle, the registry grace and the hasher keep their startup values.
    pub fn reload(&self, settings: LobbySettings) {
        *self.settings.write() = Arc::new(settings);
    }

    /// Installs the character-select continuation of the WebSocket lobby (once).
    pub fn set_wake(&self, wake: Arc<dyn SessionWake>) -> bool {
        self.wake.set(wake).is_ok()
    }

    /// The game inbox (C1), for the enter task (`GameInput::Join`) and the re-attach.
    pub fn game_sender(&self) -> &flume::Sender<GameInput> {
        &self.game_tx
    }

    /// Open, shutting down or degraded (12 §5.8).
    pub fn availability(&self) -> Availability {
        if !self
            .telemetry
            .health
            .accepting_logins
            .load(Ordering::Relaxed)
        {
            return Availability::ShuttingDown;
        }
        let stats = self.persist.stats();
        if self.persist.is_closed() || stats.degraded || stats.queue_depth > DEGRADED_QUEUE_DEPTH {
            return Availability::Degraded;
        }
        Availability::Open
    }

    /// The account service wired to this server (online effects on).
    pub fn account_admin(self: &Arc<Self>) -> AccountAdmin {
        let s = self.settings();
        AccountAdmin::new(
            self.persist.clone(),
            self.hasher.clone(),
            self.clock.clone(),
            s.auth.legacy_md5_salt.clone(),
            GmPolicy {
                cfg: Arc::clone(&s.gm),
                servers_authorize: s.servers_authorize,
            },
            Some(Arc::clone(self) as Arc<dyn OnlineEffects>),
        )
        .with_audit(Arc::clone(&self.audit))
        .with_events(self.account_events.clone())
    }

    // ---- events from the gateway and the game -----------------------------------------------------------

    /// `LobbyPort::on_net_event` (12 §7.5): registry bookkeeping, then the side effects.
    pub fn on_net_event(&self, ev: &NetEvent) {
        // `session.logout` is the game's (13 §25.2): it knows the play time, gold, level, job and the leave reason
        let actions = self.registry.on_net_event(ev);
        self.run_actions(actions);
    }

    /// `LobbyPort::on_session_end` (12 §8.1): the socket of `session` is gone.
    pub fn on_session_end(&self, session: SessionId, cause: LostCause) {
        let actions = self.registry.socket_closed(session, cause);
        self.run_actions(actions);
    }

    /// Closes the socket that a newer `Hello` of the same token replaced (`AttachInfo.replaced`): the gateway
    /// sends `Kick{DUPLICATE_LOGIN, reconnect_after_ms = 0}` and does not report `SessionLost` for a
    /// `Replaced` socket (12 §8.3).
    pub fn close_replaced(&self, session: SessionId) {
        if let Some(h) = self.directory.get(session) {
            h.close(flyff_common::rt::CloseReason::Replaced);
        }
    }

    /// Executes the side effects of a registry transition (rule LB1: never under the registry lock).
    pub fn run_actions(&self, actions: Vec<RegistryAction>) {
        for action in actions {
            match action {
                RegistryAction::CloseSocket { session, reason } => {
                    if let Some(h) = self.directory.get(session) {
                        h.close(reason);
                    }
                }
                RegistryAction::KickAccount { account, reason } => {
                    self.kick_account(account, reason);
                }
                RegistryAction::SpawnDrain { account, save_gen } => {
                    self.spawn_drain(account, save_gen)
                }
                RegistryAction::EnterCharSelect {
                    session,
                    account,
                    last_gen,
                } => {
                    if let Some(h) = self.directory.get(session) {
                        h.set_state(GatewayState::CharSelect);
                    }
                    if let Some(w) = self.wake.get() {
                        w.char_select_after_leave(session, account, last_gen);
                    }
                }
                RegistryAction::JoinFailed {
                    session,
                    account,
                    reason,
                } => {
                    if let Some(h) = self.directory.get(session) {
                        h.push_frame(lobby_error(join_fail_code(reason), 0));
                        h.set_state(GatewayState::CharSelect);
                    }
                    if let Some(w) = self.wake.get() {
                        w.join_failed(session, account, reason);
                    }
                }
                RegistryAction::RevokeAssets { account } => {
                    if let Some(t) = &self.asset_tokens {
                        t.revoke_account(account);
                    }
                }
            }
        }
    }

    fn kick_account(&self, account: AccountId, reason: KickReason) {
        let input = GameInput::KickAccount {
            account,
            reason,
            reply: None,
        };
        if let Err(e) = self.game_tx.try_send(input) {
            tracing::error!(
                account = account.0,
                ?reason,
                "cannot queue KickAccount: {e}"
            );
        }
    }

    fn spawn_drain(&self, account: AccountId, save_gen: SaveGen) {
        let Some(core) = self.me.upgrade() else {
            return;
        };
        let handle = tokio::runtime::Handle::try_current()
            .ok()
            .or_else(|| self.runtime.get().cloned());
        let Some(handle) = handle else {
            tracing::error!(account = account.0, "no runtime for the drain task");
            return;
        };
        handle.spawn(async move { core.drain(account, save_gen).await });
    }

    /// The drain task (13 §19 step 4): waits for the flush of `save_gen`, then removes the entry; logs an error
    /// every 60 s while waiting. A poisoned account (`SaveFailed`) stays until an administrator releases it.
    pub(crate) async fn drain(&self, account: AccountId, save_gen: SaveGen) {
        let rx = self.persist.wait_flushed(account, save_gen);
        let mut fut = Box::pin(rx.recv_async());
        let result = loop {
            tokio::select! {
                r = &mut fut => break r,
                () = tokio::time::sleep(Duration::from_secs(60)) => {
                    tracing::error!(account = account.0, save_gen = save_gen.0, "the logout save is still not committed");
                }
            }
        };
        match result {
            Ok(Ok(())) => {
                self.registry.drain_done(account, save_gen);
            }
            Ok(Err(e)) if e.kind == flyff_db::DbErrorKind::SaveFailed => {
                tracing::error!(
                    account = account.0,
                    "the logout save failed permanently: {e}; the account stays blocked until `db release`"
                );
                loop {
                    tokio::time::sleep(Duration::from_millis(
                        self.drain_poll_ms.load(Ordering::Relaxed),
                    ))
                    .await;
                    let still_draining = self.registry.view(account).is_some_and(|v| {
                        v.kind == EntryKind::Draining && v.draining_gen == Some(save_gen)
                    });
                    if !still_draining {
                        return;
                    }
                    if !self.persist.is_poisoned(account) {
                        self.registry.drain_done(account, save_gen);
                        return;
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::warn!(account = account.0, "drain ended: {e}");
            }
            Err(_) => {}
        }
    }
}

/// `LobbyError` as a server message (12 §16.2).
pub fn lobby_error(code: i32, tid: i32) -> pb::ServerMsg {
    pb::ServerMsg {
        obj_id: 0,
        msg: Some(pb::LobbyError { code, tid }.into()),
    }
}

/// `JoinFailReason` to the lobby code (12 §16.3): `ShuttingDown` is 109, the rest 136.
pub fn join_fail_code(reason: JoinFailReason) -> i32 {
    match reason {
        JoinFailReason::ShuttingDown => errors::code::UNAVAILABLE,
        JoinFailReason::AlreadyOnline
        | JoinFailReason::InvalidWorld
        | JoinFailReason::LoadInvariant
        | JoinFailReason::Internal => errors::code::CERT_GENERAL,
    }
}

impl OnlineEffects for LobbyCore {
    fn account_banned(&self, account: AccountId) {
        let actions = self.registry.on_ban(account);
        self.run_actions(actions);
    }

    fn account_unbanned(&self, account: AccountId) {
        self.registry.on_unban(account);
    }

    fn authority_changed(
        &self,
        account: AccountId,
        character: Option<CharacterId>,
        authority: Authority,
    ) {
        // applied by the game thread's handler of WP-GM-10 from M2; harmless before
        let (reply, _rx) = flyff_common::rt::reply_pair();
        let input = GameInput::Admin {
            cmd: AdminCmd::AuthorityChanged {
                account,
                character,
                authority,
            },
            actor: AdminActor::Cli,
            reply,
        };
        if let Err(e) = self.game_tx.try_send(input) {
            tracing::warn!(account = account.0, "cannot queue AuthorityChanged: {e}");
        }
    }

    fn online_state(&self, account: AccountId) -> OnlineState {
        match self.registry.view(account).map(|v| v.kind) {
            Some(EntryKind::Claimed) => OnlineState::CharSelect,
            Some(EntryKind::Entering | EntryKind::InWorld) => OnlineState::InWorld,
            Some(EntryKind::Draining) | None => OnlineState::Offline,
        }
    }
}
