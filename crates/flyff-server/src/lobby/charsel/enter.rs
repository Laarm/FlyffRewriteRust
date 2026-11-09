//! `EnterWorld` (Plan/12 §11.1, WP-AUTH-09 lobby side): the checks of the character-select state, the spawned
//! enter task and the `Join` hand-over.
//!
//! | step | where | failure -> `LobbyError`, back to `CharSelect` |
//! |---|---|---|
//! | 1 | state `CharSelect`, entry `Claimed` with this socket attached (`registry.begin_enter`) | dropped / `Kick{INVALID_STATE}` |
//! | 2 | `id_player` is an active character of the account at `slot` (a fresh list read) | 104, `TID_DIAG_0039` |
//! | 3 | not blocked (`login_blocked_until <= now`) | **2001**, `TID_DIAG_0073` |
//! | 4 | the game sink (`SinkMode::Open`), the spawned task, `LobbyOutcome::Entering` | - |
//! | 5 | `wait_flushed(account, last_gen)` (the logout save of the previous stay, VR-084) | 133 `TID_DB_INSPECTION` / 136 |
//! | 6 | `queries::load_character` in one read transaction | `NotFound` -> 104, error -> 136 |
//! | 7 | `resolve_authority([gm], DB values, peer IP)` (12 §5.9, 23 §1.3) | - |
//! | 8 | the entry is still ours and not cancelled; the socket is still open | abort -> `Claimed` |
//! | 9 | `registry.enter_joined`, `GameInput::Join`, the gateway goes `InWorld` | the entering timer fires first -> 133 |
//!
//! Rule EW1 (the gateway reports `SessionLost` only after the enter task finished) is kept by the gateway: while
//! its state is `Entering` the session task waits for the `SessionCtl::SetState` this task sends last.

use super::{Lobby, tid, wait_flush_core};
use crate::audit::Actor;
use crate::gateway::{GatewaySession, GatewayState, LobbyOutcome, SessionHandle, SinkMode};
use crate::lobby::errors::code;
use crate::lobby::online::EnterRefusal;
use crate::lobby::{LobbyCore, lobby_error};
use flyff_common::defs::text::TID_DIAG_0073;
use flyff_common::gm::{Authority, AuthorityInputs, resolve_authority};
use flyff_common::rt::{CloseReason, KickReason};
use flyff_common::{AccountId, CharacterId, SaveGen, SessionId};
use flyff_db::{CharacterLoad, DbError, DbErrorKind, queries};
use flyff_game::GameInput;
use flyff_game::input::JoinRequest;
use flyff_proto::pb;
use std::net::IpAddr;
use std::sync::Arc;

/// What the enter task needs besides the core: everything cloneable and owned.
struct EnterJob {
    core: Arc<LobbyCore>,
    account: AccountId,
    session: SessionId,
    character: CharacterId,
    peer: IpAddr,
    handle: SessionHandle,
    shared: Arc<crate::gateway::SinkShared>,
    sink: Box<dyn flyff_common::rt::SessionSink>,
    client_build: String,
    cancel: crate::lobby::online::CancelFlag,
    /// `EnterWorld` accepted (`flyff_lobby_enter_world_seconds`).
    started: std::time::Instant,
}

impl Lobby {
    /// `EnterWorld{slot, id_player}` (12 §11.1 steps 1-4); the rest runs in the spawned [`EnterJob`].
    pub(super) async fn on_enter_world(
        &self,
        s: &mut GatewaySession,
        m: pb::EnterWorld,
    ) -> LobbyOutcome {
        let Some(account) = s.account() else {
            return LobbyOutcome::Close(CloseReason::Kicked(KickReason::InvalidState));
        };
        // 2: an active character of the account at that slot (a fresh read: the list may be stale)
        let entries = match self.read_list(account).await {
            Ok(e) => e,
            Err(e) => {
                tracing::error!(account = account.0, "enter world: list read failed: {e}");
                let _ = s.push_frame(super::db_error_msg(&e));
                return LobbyOutcome::Stay;
            }
        };
        let Some(entry) = entries
            .iter()
            .find(|e| e.id.0 == m.id_player && u32::from(e.slot) == m.slot)
        else {
            let _ = s.push_frame(super::illegal_access());
            return LobbyOutcome::Stay;
        };
        // 3: not blocked
        if entry
            .login_blocked_until
            .is_some_and(|t| t > self.core.clock.unix())
        {
            let _ = s.push_frame(lobby_error(code::CHARACTER_BLOCKED, tid(TID_DIAG_0073)));
            return LobbyOutcome::Stay;
        }
        // 1/4: the registry moves to Entering (the entry must be Claimed with this socket)
        let cancel = match self.core.registry.begin_enter(account, s.id(), entry.id) {
            Ok(c) => c,
            Err(EnterRefusal::NoEntry | EnterRefusal::NotClaimed) => {
                return LobbyOutcome::Close(CloseReason::Kicked(KickReason::InvalidState));
            }
        };
        let job = EnterJob {
            core: Arc::clone(&self.core),
            account,
            session: s.id(),
            character: entry.id,
            peer: s.peer(),
            handle: s.handle(),
            shared: Arc::clone(s.sink_shared()),
            sink: s.make_game_sink(SinkMode::Open),
            client_build: self.info.web_build.clone(),
            cancel,
            started: std::time::Instant::now(),
        };
        self.runtime.spawn(job.run());
        LobbyOutcome::Entering
    }
}

impl EnterJob {
    /// Steps 5-9. Every failure ends with [`EnterJob::abort`].
    async fn run(self) {
        // 5: the previous stay's logout snapshot must be committed
        let last_gen = self
            .core
            .registry
            .view(self.account)
            .map_or(SaveGen::ZERO, |v| v.last_gen);
        if let Err(msg) = wait_flush_core(&self.core, self.account, last_gen).await {
            return self.abort(Some(*msg));
        }
        // 6 + the account row (the raw `accounts.authority` is an input of 7)
        let (account, character) = (self.account, self.character);
        let loaded = self
            .core
            .persist
            .read(move |c| {
                let load = queries::load_character(c, account, character)?;
                let acct = queries::account_by_id(c, account)?;
                Ok((load, acct))
            })
            .await;
        let (load, acct) = match loaded {
            Ok(v) => v,
            Err(e) if e.kind == DbErrorKind::NotFound => {
                return self.abort(Some(super::illegal_access()));
            }
            Err(e) => {
                tracing::error!(account = account.0, "enter world: load failed: {e}");
                return self.abort(Some(super::db_error_msg(&e)));
            }
        };
        let Some(acct) = acct else {
            return self.abort(Some(super::illegal_access()));
        };
        let load: Box<CharacterLoad> = Box::new(load);
        // 7: the definitive authority of this session
        let settings = self.core.settings();
        let name = self
            .core
            .registry
            .view(account)
            .map(|v| v.name.as_str().to_owned())
            .unwrap_or_default();
        let authority: Authority = resolve_authority(
            &settings.gm,
            settings.servers_authorize,
            &AuthorityInputs {
                account_db: Authority::from_db_byte(acct.authority),
                character_db: Some(Authority::from_db_byte(load.identity.authority)),
                account_name: &name,
                character_name: Some(&load.identity.name),
                peer_ip: self.peer,
            },
        );
        // 8: still ours? the socket still open?
        if self.cancel.is_cancelled() || self.shared.is_closed() || self.shared.peer_gone() {
            return self.abort(None);
        }
        // 9: `Entering` -> `InWorld` BEFORE the send, so that a very fast `Left` / `JoinFailed` finds `InWorld`
        if !self.core.registry.enter_joined(account, self.session) {
            return self.abort(None);
        }
        let world = load.row.world_id;
        let req = JoinRequest {
            session: self.session,
            account,
            authority,
            character: load,
            sink: self.sink,
            client_build: self.client_build,
        };
        if self
            .core
            .game_sender()
            .send_async(GameInput::Join(Box::new(req)))
            .await
            .is_err()
        {
            // the game thread is gone (shutdown): back to Claimed
            let actions = self.core.registry.enter_aborted(account, self.session);
            self.core.run_actions(actions);
            let _ = self
                .handle
                .push_frame(super::db_error_msg(&DbError::shutting_down()));
            self.handle.set_state(GatewayState::CharSelect);
            return;
        }
        self.core
            .stats
            .enter_world
            .observe_ns(u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX));
        // 12 §19.1 `session.login`: the join was handed to the game thread
        self.core.audit.emit(
            "session.login",
            &Actor {
                account: Some(account),
                character: Some(character),
                name: None,
                ip: Some(self.peer.to_string()),
            },
            serde_json::json!({
                "character": character.0,
                "account": account.0,
                "world": world,
                "authority": authority.letter().to_string(),
            }),
        );
        self.handle.set_state(GatewayState::InWorld);
    }

    /// The entry goes back to `Claimed` (or `Draining` when revoked), the player gets the error, the gateway
    /// returns to `CharSelect`.
    fn abort(self, error: Option<pb::ServerMsg>) {
        let actions = self.core.registry.enter_aborted(self.account, self.session);
        self.core.run_actions(actions);
        if let Some(e) = error {
            self.handle.push_frame(e);
        }
        self.handle.set_state(GatewayState::CharSelect);
    }
}
