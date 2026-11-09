//! The WebSocket lobby (Plan/12 §8-§10; WP-AUTH-06, -07, -08): the gateway's [`LobbyPort`] over the
//! [`LobbyCore`] of lobby-auth. `Hello`/`HelloAck`, the `CharList`, character creation and deletion live here;
//! `EnterWorld` is a seam for the world-flow task (`enter`).
//!
//! | file | content |
//! |---|---|
//! | `mod.rs` | [`Lobby`], the `Hello` flow (12 §8.2), the [`LobbyPort`] and [`SessionWake`] implementations |
//! | `list.rs` | `CharSlot` mapping (12 §8.4) and the list reads |
//! | `create.rs` | `CreatePlayer` (12 §9.1) |
//! | `delete.rs` | `DelPlayer` (12 §10.2) |
//! | `enter.rs` | `EnterWorld`: the refusal until world-flow's enter task lands |
//!
//! Rules of this module: the registry lock is never held across an await (it is only touched through the
//! synchronous methods of [`OnlineRegistry`](super::online::OnlineRegistry)); a create or delete is awaited
//! inside `char_select` (the gateway handles one request at a time per session) and its state is re-checked
//! afterwards through the registry; every failure is a `LobbyError{code, tid}` of 12 §16.2.

mod create;
mod delete;
mod enter;
mod list;

use super::errors::code;
use super::online::{AttachResult, EntryKind};
use super::tokens::SessionToken;
use super::{Availability, LobbyCore, SessionWake, lobby_error};
use crate::audit::Actor;
use crate::build_info;
use crate::gateway::{GatewaySession, HelloOutcome, LobbyDeps, LobbyOutcome, LobbyPort, SinkMode};
use async_trait::async_trait;
use flyff_common::defs::text::{
    TID_DB_INSPECTION, TID_DIAG_0039, TID_DIAG_0053, TID_ERROR_CERT_GENERAL,
};
use flyff_common::rt::{CloseReason, KickReason, LostCause, reply_pair};
use flyff_common::{AccountId, SaveGen, SessionId};
use flyff_data::GameData;
use flyff_db::{DbError, DbErrorKind};
use flyff_game::input::ReattachResult;
use flyff_game::{GameInput, NetEvent};
use flyff_proto::{PROTO_HASH, pb};
use std::sync::{Arc, Weak};
use std::time::Duration;

/// How long a `Hello` waits for the enter task of a replaced socket or for an avatar to leave (12 §8.2 step 8,
/// §8.1 M1 re-attach rule).
pub const ENTER_WAIT: Duration = Duration::from_secs(5);

/// How long the lobby waits for the game thread's answer to a create or delete (12 §15: 10 s).
pub const GAME_REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// The close codes of a failed `Hello` (12 §8.2; the `Kick.Reason` numbers of 11 §1.5 plus 4000).
mod close_code {
    /// `HelloAck{RELOAD_REQUIRED}`.
    pub const RELOAD_REQUIRED: u16 = 4012;
    /// `HelloAck{SHUTTING_DOWN}`.
    pub const SHUTTING_DOWN: u16 = 4009;
    /// `HelloAck{BAD_TOKEN}`.
    pub const BAD_TOKEN: u16 = 4013;
    /// `HelloAck{BANNED}`.
    pub const BANNED: u16 = 4008;
    /// `HelloAck{SERVER_FULL}`: a normal close.
    pub const SERVER_FULL: u16 = 1000;
}

/// What the `HelloAck` carries besides the session (12 §8.2 last paragraph).
#[derive(Clone, Debug)]
pub struct LobbyInfo {
    /// `BUILD_ID` of the executable.
    pub server_build: String,
    /// The embedded web build id: `Hello.client_build` must equal it (skipped with `dev`).
    pub web_build: String,
    /// The client data-bundle version (14 §10.3).
    pub data_version: String,
    /// `net.flush_interval_ms` (`SessionLimits.flush_interval_ms`).
    pub flush_interval_ms: u32,
    /// `--dev`: a client build mismatch is logged, not refused.
    pub dev: bool,
}

impl LobbyInfo {
    /// The values of a test server: this executable's build, no web build to compare.
    pub fn for_tests() -> LobbyInfo {
        LobbyInfo {
            server_build: build_info::BUILD_ID.to_owned(),
            web_build: "test".to_owned(),
            data_version: String::new(),
            flush_interval_ms: 67,
            dev: false,
        }
    }
}

/// The lobby of the WebSocket (12 §8.1): implements [`LobbyPort`] for the gateway.
pub struct Lobby {
    core: Arc<LobbyCore>,
    deps: LobbyDeps,
    data: Arc<GameData>,
    info: LobbyInfo,
    runtime: tokio::runtime::Handle,
}

impl std::fmt::Debug for Lobby {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lobby").finish_non_exhaustive()
    }
}

/// `SessionWake` over a weak reference: the core must not keep the lobby alive (the lobby holds the core).
struct Wake(Weak<Lobby>);

impl SessionWake for Wake {
    fn char_select_after_leave(&self, session: SessionId, account: AccountId, last_gen: SaveGen) {
        if let Some(lobby) = self.0.upgrade() {
            let l = Arc::clone(&lobby);
            lobby.runtime.spawn(async move {
                l.send_list_after_flush(session, account, Some(last_gen))
                    .await;
            });
        }
    }

    fn join_failed(
        &self,
        session: SessionId,
        account: AccountId,
        _reason: flyff_game::input::JoinFailReason,
    ) {
        if let Some(lobby) = self.0.upgrade() {
            let l = Arc::clone(&lobby);
            lobby.runtime.spawn(async move {
                l.send_list_after_flush(session, account, None).await;
            });
        }
    }
}

/// The flush wait of 13 §19 step 2 over the core (the enter task has no `Lobby` at hand).
pub(crate) async fn wait_flush_core(
    core: &LobbyCore,
    account: AccountId,
    generation: SaveGen,
) -> Result<(), Box<pb::ServerMsg>> {
    let timeout = core.settings().flush_wait;
    let started = std::time::Instant::now();
    let rx = core.persist.wait_flushed(account, generation);
    let waited = tokio::time::timeout(timeout, rx.recv_async()).await;
    core.stats
        .flush_wait
        .observe_ns(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
    match waited {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(e))) => {
            tracing::error!(account = account.0, "flush wait failed: {e}");
            Err(Box::new(db_error_msg(&e)))
        }
        Ok(Err(_)) => Err(Box::new(lobby_error(
            code::CERT_GENERAL,
            tid(TID_ERROR_CERT_GENERAL),
        ))),
        Err(_) => Err(Box::new(lobby_error(
            code::DB_JOB_ING,
            tid(TID_DB_INSPECTION),
        ))),
    }
}

/// `ServerMsg` with `obj_id` 0.
pub(crate) fn server_msg(body: impl Into<pb::server_msg::Msg>) -> pb::ServerMsg {
    pb::ServerMsg {
        obj_id: 0,
        msg: Some(body.into()),
    }
}

/// The `LobbyError` frame for an internal database error: 109 while shutting down, else 136 (12 §16.3).
pub(crate) fn db_error_msg(e: &DbError) -> pb::ServerMsg {
    match e.kind {
        DbErrorKind::ShuttingDown => lobby_error(code::UNAVAILABLE, tid(TID_DIAG_0053)),
        _ => lobby_error(code::CERT_GENERAL, tid(TID_ERROR_CERT_GENERAL)),
    }
}

/// A `TID_*` as the `LobbyError.tid` field.
pub(crate) fn tid(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(0)
}

/// The `result` label of a lobby counter for a refusal frame: the `LobbyError` code as text.
pub(crate) fn lobby_error_label(msg: &pb::ServerMsg) -> String {
    match &msg.msg {
        Some(pb::server_msg::Msg::LobbyError(e)) => e.code.to_string(),
        _ => "other".to_owned(),
    }
}

/// 104 / `TID_DIAG_0039`: "Oops. Try again."
pub(crate) fn illegal_access() -> pb::ServerMsg {
    lobby_error(code::ILLEGAL_ACCESS, tid(TID_DIAG_0039))
}

impl Lobby {
    /// Builds the lobby over `core` and installs its [`SessionWake`] (the `CharList` after a leave or a failed
    /// join). `deps` are the gateway's (clock, limits, session directory); `runtime` runs the tasks that answer
    /// events after the fact.
    pub fn new(
        core: Arc<LobbyCore>,
        deps: LobbyDeps,
        data: Arc<GameData>,
        info: LobbyInfo,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Lobby> {
        let lobby = Arc::new(Lobby {
            core,
            deps,
            data,
            info,
            runtime,
        });
        lobby.core.set_wake(Arc::new(Wake(Arc::downgrade(&lobby))));
        lobby
    }

    /// The shared account/session core.
    pub fn core(&self) -> &Arc<LobbyCore> {
        &self.core
    }

    fn push(&self, s: &GatewaySession, body: impl Into<pb::server_msg::Msg>) {
        let _ = s.push_frame(server_msg(body));
    }

    /// Pushes a frame to a session that is not borrowed (an event handler, a spawned task).
    fn push_to(&self, session: SessionId, msg: pb::ServerMsg) {
        if let Some(h) = self.deps.directory.get(session) {
            h.push_frame(msg);
        }
    }

    // ---- HelloAck -----------------------------------------------------------------------------------------

    fn ack(
        &self,
        result: pb::hello_ack::Result,
        m: &pb::Hello,
        account: Option<(&str, u8)>,
        reattached: bool,
    ) -> pb::HelloAck {
        let c = &self.deps.cfg;
        pb::HelloAck {
            result: result as i32,
            server_build: self.info.server_build.clone(),
            server_unix_ms: self.deps.wall.now_ms().0,
            server_frame: self
                .core
                .telemetry
                .frames_completed
                .load(std::sync::atomic::Ordering::Acquire),
            client_frame_echo: m.client_frame,
            account: account.map(|(n, _)| n.to_owned()).unwrap_or_default(),
            authority: account.map_or(0, |(_, a)| u32::from(a)),
            limits: Some(pb::SessionLimits {
                max_client_msg_bytes: c.max_client_msg_bytes,
                client_msgs_per_sec: c.client_msgs_per_sec,
                client_msgs_burst: c.client_msgs_burst,
                flush_interval_ms: self.info.flush_interval_ms,
                ws_ping_interval_s: c.ws_ping_interval_s,
            }),
            server_name: self.core.settings().server_name.clone(),
            reattached,
            data_version: self.info.data_version.clone(),
        }
    }

    /// A failure `HelloAck`: pushed, then the gateway closes with `code` (no `Kick` frame).
    fn refuse(
        &self,
        s: &GatewaySession,
        m: &pb::Hello,
        result: pb::hello_ack::Result,
        code: u16,
    ) -> HelloOutcome {
        self.push(s, self.ack(result, m, None, false));
        HelloOutcome::Close { code }
    }

    // ---- Hello (12 §8.2) -------------------------------------------------------------------------------------

    async fn on_hello(&self, s: &mut GatewaySession, m: pb::Hello) -> HelloOutcome {
        use pb::hello_ack::Result as R;
        // 2: the protocol hash
        if m.proto_hash != PROTO_HASH {
            return self.refuse(s, &m, R::ReloadRequired, close_code::RELOAD_REQUIRED);
        }
        // 3: the web build (logged only in --dev)
        if m.client_build != self.info.web_build {
            if self.info.dev {
                tracing::info!(
                    client = %m.client_build,
                    server = %self.info.web_build,
                    "client build differs from the embedded web build (dev: accepted)"
                );
            } else {
                return self.refuse(s, &m, R::ReloadRequired, close_code::RELOAD_REQUIRED);
            }
        }
        // 4: shutdown
        if self.core.availability() == Availability::ShuttingDown {
            return self.refuse(s, &m, R::ShuttingDown, close_code::SHUTTING_DOWN);
        }
        // 5-7: the token, the ban flag, the attach
        let Some(token) = SessionToken::parse(&m.token) else {
            return self.refuse(s, &m, R::BadToken, close_code::BAD_TOKEN);
        };
        let info = match self.core.registry.attach(&token, s.id()) {
            AttachResult::BadToken => {
                return self.refuse(s, &m, R::BadToken, close_code::BAD_TOKEN);
            }
            AttachResult::Banned => {
                return self.refuse(s, &m, R::Banned, close_code::BANNED);
            }
            AttachResult::Ok(info) => info,
        };
        let account = info.account;
        s.set_account(account);
        // a newer Hello of the same token wins: the old socket gets Kick{DUPLICATE_LOGIN, 0} and `Replaced` (no
        // SessionLost for it; 12 §8.3). For an avatar in the world the re-attach below moves the avatar to the new
        // socket, so the old one must not report a loss
        if let Some(old) = info.replaced {
            self.core.close_replaced(old);
        }
        // 8: the enter task of the replaced socket is cancelled (the registry no longer maps the old socket to
        // the entry after `attach`, so its close cannot do it): `enter_aborted` returns the entry to `Claimed`
        // and the task's `enter_joined` fails; then wait for the state to settle
        if info.kind == EntryKind::Entering {
            if let Some(old) = info.replaced {
                let actions = self.core.registry.enter_aborted(account, old);
                self.core.run_actions(actions);
            }
            self.core
                .registry
                .wait_for(account, ENTER_WAIT, |v| {
                    v.is_none_or(|v| v.kind != EntryKind::Entering)
                })
                .await;
        }
        // 9: the avatar is still in the world (a reload, a reconnect inside the linger, a second tab): the
        // re-attach of 12 §13 (RA1-RA4, WP-AUTH-16)
        let mut view = self.core.registry.view(account);
        if view.as_ref().is_some_and(|v| v.kind == EntryKind::InWorld) {
            if let Some(outcome) = self.try_reattach(s, &m, account).await {
                return outcome;
            }
            // RA4: the avatar left between the check and the request: wait for the entry to leave `InWorld`,
            // then continue as for a `Claimed` entry
            let left = self
                .core
                .registry
                .wait_for(account, ENTER_WAIT, |v| {
                    v.is_none_or(|v| v.kind != EntryKind::InWorld)
                })
                .await;
            if !left {
                tracing::warn!(
                    account = account.0,
                    "the avatar did not leave within {ENTER_WAIT:?}; continuing at character select"
                );
            }
            view = self.core.registry.view(account);
        }
        let Some(view) = view else {
            // the entry vanished in the meantime (the sweeper): the token is gone
            return self.refuse(s, &m, R::BadToken, close_code::BAD_TOKEN);
        };
        // 10: capacity (legacy `lCount > lMax`)
        if self.core.registry.in_world() > self.core.settings().auth.max_in_world {
            self.push(
                s,
                self.ack(
                    R::ServerFull,
                    &m,
                    Some((view.name.as_str(), view.authority.as_u8())),
                    false,
                ),
            );
            return HelloOutcome::Close {
                code: close_code::SERVER_FULL,
            };
        }
        // 11: HelloAck{OK}, the flush wait, the list
        self.push(
            s,
            self.ack(
                R::Ok,
                &m,
                Some((view.name.as_str(), view.authority.as_u8())),
                false,
            ),
        );
        match self.wait_flush(account, view.last_gen).await {
            Ok(()) => self.push_list(s, account).await,
            Err(msg) => {
                let _ = s.push_frame(*msg);
            }
        }
        HelloOutcome::CharSelect
    }

    // ---- re-attach (12 §13) ------------------------------------------------------------------------------

    /// RA1-RA3: `GameInput::Reattach` with a gated sink; on `Attached` the registry follows, `HelloAck{OK,
    /// reattached = true}` is the first frame of the new socket and the gate opens behind it. `None` = no avatar
    /// (RA4: the caller waits for the entry to leave `InWorld` and continues as a normal `Hello`).
    async fn try_reattach(
        &self,
        s: &mut GatewaySession,
        m: &pb::Hello,
        account: AccountId,
    ) -> Option<HelloOutcome> {
        let (reply, rx) = reply_pair();
        let sink = s.make_game_sink(SinkMode::Gated);
        let input = GameInput::Reattach {
            account,
            session: s.id(),
            sink,
            reply,
        };
        if self.core.game_sender().send_async(input).await.is_err() {
            return None;
        }
        let answer = tokio::time::timeout(ENTER_WAIT, rx.recv_async()).await;
        match answer {
            Ok(Ok(ReattachResult::Attached)) => {}
            Ok(Ok(ReattachResult::NoAvatar)) => {
                self.core.stats.reattach("no_avatar");
                return None;
            }
            Ok(Err(_)) | Err(_) => {
                tracing::warn!(
                    account = account.0,
                    "the game thread did not answer a re-attach"
                );
                self.core.stats.reattach("no_avatar");
                return None;
            }
        }
        // the game bound the avatar to this socket; the registry follows (`false`: the entry vanished while the
        // game answered, which only happens when the avatar was removed meanwhile: the socket still gets the
        // session state of an avatar and the next `Left` cleans up)
        let character = self.core.registry.view(account).and_then(|v| v.character);
        if !self.core.registry.reattached(account, s.id()) {
            tracing::warn!(
                account = account.0,
                "re-attach: the registry entry is no longer in the world"
            );
        }
        let view = self.core.registry.view(account)?;
        // the avatar stays the one of the entry, so the authority of the ack is the login one, as for any Hello
        self.push(
            s,
            self.ack(
                pb::hello_ack::Result::Ok,
                m,
                Some((view.name.as_str(), view.authority.as_u8())),
                true,
            ),
        );
        s.open_gate();
        self.core.stats.reattach("attached");
        self.core.audit.emit(
            "session.reattach",
            &Actor {
                account: Some(account),
                character,
                name: None,
                ip: Some(s.peer().to_string()),
            },
            serde_json::json!({
                "account": view.name.as_str(),
                "character": character.map(|c| c.0),
                // `Hello.reattach` is the client's expectation; the server decides by the entry state (12 §13.2)
                "expected": m.reattach,
            }),
        );
        Some(HelloOutcome::InWorld)
    }

    // ---- the flush wait (13 §19 step 2) -----------------------------------------------------------------

    /// Waits until the account's generation `gen` is committed, at most `database.flush_wait_timeout_ms`:
    /// `Err(LobbyError{133})` on a timeout, `{136}` for a poisoned account or a lost reply, `{109}` when the
    /// writer stopped.
    pub(crate) async fn wait_flush(
        &self,
        account: AccountId,
        generation: SaveGen,
    ) -> Result<(), Box<pb::ServerMsg>> {
        wait_flush_core(&self.core, account, generation).await
    }

    /// The `CharList` of `session` after a leave or a failed join (called from `SessionWake` tasks): the flush
    /// wait first (13 §19 step 2), then the list; an error becomes a `LobbyError`.
    async fn send_list_after_flush(
        &self,
        session: SessionId,
        account: AccountId,
        generation: Option<SaveGen>,
    ) {
        let generation = generation
            .or_else(|| self.core.registry.view(account).map(|v| v.last_gen))
            .unwrap_or(SaveGen::ZERO);
        if let Err(msg) = self.wait_flush(account, generation).await {
            self.push_to(session, *msg);
            return;
        }
        match self.read_list(account).await {
            Ok(entries) => {
                let list = list::char_list(&entries, self.core.clock.unix());
                self.push_to(session, server_msg(list));
            }
            Err(e) => self.push_to(session, db_error_msg(&e)),
        }
    }
}

#[async_trait]
impl LobbyPort for Lobby {
    async fn hello(&self, s: &mut GatewaySession, m: pb::Hello) -> HelloOutcome {
        self.on_hello(s, m).await
    }

    async fn char_select(
        &self,
        s: &mut GatewaySession,
        m: pb::client_envelope::Msg,
    ) -> LobbyOutcome {
        match m {
            pb::client_envelope::Msg::CreatePlayer(m) => self.on_create(s, m).await,
            pb::client_envelope::Msg::DelPlayer(m) => self.on_delete(s, m).await,
            pb::client_envelope::Msg::EnterWorld(m) => self.on_enter_world(s, m).await,
            // the gateway's state table cannot route anything else here
            _ => LobbyOutcome::Close(CloseReason::Kicked(KickReason::InvalidState)),
        }
    }

    fn on_net_event(&self, ev: NetEvent) {
        self.core.on_net_event(&ev);
    }

    fn on_session_end(&self, s: &GatewaySession, cause: LostCause) {
        self.core.on_session_end(s.id(), cause);
    }

    /// `Entering` lasted `net.timeout_entering_s` without a `Join` (12 §15): the enter task is abandoned (a
    /// later `enter_joined` finds the entry `Claimed` and aborts), the player gets `LobbyError{133}` and the
    /// list again.
    async fn entering_timeout(&self, s: &mut GatewaySession) {
        if let Some(account) = s.account() {
            let actions = self.core.registry.enter_aborted(account, s.id());
            self.core.run_actions(actions);
        }
        let _ = s.push_frame(lobby_error(code::DB_JOB_ING, tid(TID_DB_INSPECTION)));
        s.set_state(crate::gateway::GatewayState::CharSelect);
        if let Some(account) = s.account() {
            self.push_list(s, account).await;
        }
    }
}
