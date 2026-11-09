//! The test double of the lobby (Plan/10 WP-NET-01): accepts any token, answers with a fixed character list and
//! has a re-attach path that uses a gated sink. It is wired by default until doc 12's `Lobby` lands, so a
//! client can already complete the handshake, "enter the world" through the game thread's stub rules and
//! exercise the whole gateway.
//!
//! Token to account: `acct:<n>` or a plain number `<n>` is account `n`; any other non-empty token hashes to an
//! account id; `bad` or an empty token is `BAD_TOKEN`. `Hello.proto_hash` must equal `PROTO_HASH`.

use super::GatewaySession;
use super::lobby_port::{GatewayState, HelloOutcome, LobbyOutcome, LobbyPort, SessionDirectory};
use super::sink::SinkMode;
use crate::config::GatewayConfig;
use async_trait::async_trait;
use flyff_common::rt::{CloseReason, LostCause, Telemetry, reply_pair};
use flyff_common::{AccountId, CharacterId, SessionId};
use flyff_db::{CharacterIdentity, CharacterLoad};
use flyff_game::input::{Authority, JoinRequest, ReattachResult};
use flyff_game::tick::WallClockCell;
use flyff_game::{GameInput, NetEvent};
use flyff_proto::{PROTO_HASH, pb};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// What `EnterWorld` does in the stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterBehaviour {
    /// `Join` is sent inline: [`LobbyOutcome::Joined`].
    Immediate,
    /// [`LobbyOutcome::Entering`] at once; a spawned task sends `Join` after the delay and reports `InWorld`.
    Delayed(Duration),
    /// [`LobbyOutcome::Entering`] and nothing else: the entering timeout of the gateway fires.
    Never,
}

/// Options of the stub.
#[derive(Clone, Copy, Debug)]
pub struct StubOptions {
    /// Behaviour of `EnterWorld`.
    pub enter: EnterBehaviour,
}

impl Default for StubOptions {
    fn default() -> Self {
        Self {
            enter: EnterBehaviour::Immediate,
        }
    }
}

/// What the stub needs from the running server.
#[derive(Clone)]
pub struct LobbyDeps {
    /// C1.
    pub game_tx: flume::Sender<GameInput>,
    /// The session directory of the gateway.
    pub directory: Arc<SessionDirectory>,
    /// Shared counters (the frame stamp of `HelloAck`).
    pub telemetry: Arc<Telemetry>,
    /// The wall-clock source (`server_unix_ms`).
    pub wall: WallClockCell,
    /// The gateway limits (`HelloAck.limits`).
    pub cfg: GatewayConfig,
}

/// The stub lobby.
pub struct StubLobby {
    deps: LobbyDeps,
    options: StubOptions,
    /// Accounts that were joined and have not left yet, with their last session.
    in_world: Mutex<BTreeMap<AccountId, SessionId>>,
}

impl StubLobby {
    /// A stub with default options.
    pub fn new(deps: LobbyDeps) -> Arc<StubLobby> {
        Self::with_options(deps, StubOptions::default())
    }

    /// A stub with explicit options.
    pub fn with_options(deps: LobbyDeps, options: StubOptions) -> Arc<StubLobby> {
        Arc::new(StubLobby {
            deps,
            options,
            in_world: Mutex::new(BTreeMap::new()),
        })
    }

    /// The account a token stands for (`None` = bad token).
    pub fn account_of(token: &str) -> Option<AccountId> {
        let t = token.trim();
        if t.is_empty() || t == "bad" {
            return None;
        }
        let digits = t.strip_prefix("acct:").unwrap_or(t);
        if let Ok(n) = digits.parse::<u32>() {
            return Some(AccountId(n));
        }
        // FNV-1a: any other token is a stable pseudo account
        let mut h: u32 = 0x811c_9dc5;
        for b in t.bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        Some(AccountId(h | 0x8000_0000))
    }

    /// Accounts the stub believes to be in the world.
    pub fn online_accounts(&self) -> Vec<AccountId> {
        self.in_world.lock().keys().copied().collect()
    }

    fn ack(
        &self,
        result: pb::hello_ack::Result,
        m: &pb::Hello,
        account: Option<AccountId>,
        reattached: bool,
    ) -> pb::HelloAck {
        let c = &self.deps.cfg;
        pb::HelloAck {
            result: result as i32,
            server_build: "stub".to_owned(),
            server_unix_ms: self.deps.wall.now_ms().0,
            server_frame: self.deps.telemetry.frames_completed.load(Ordering::Acquire),
            client_frame_echo: m.client_frame,
            account: account.map(|a| format!("acct{}", a.0)).unwrap_or_default(),
            authority: 70,
            limits: Some(pb::SessionLimits {
                max_client_msg_bytes: c.max_client_msg_bytes,
                client_msgs_per_sec: c.client_msgs_per_sec,
                client_msgs_burst: c.client_msgs_burst,
                flush_interval_ms: 67,
                ws_ping_interval_s: c.ws_ping_interval_s,
            }),
            server_name: "stub".to_owned(),
            reattached,
            data_version: String::new(),
        }
    }

    fn char_list(account: AccountId) -> pb::CharList {
        pb::CharList {
            slots: vec![pb::CharSlot {
                slot: 0,
                name: format!("Stub{}", account.0),
                id_player: account.0.wrapping_add(1000),
                level: 1,
                ..pb::CharSlot::default()
            }],
        }
    }

    fn push(s: &GatewaySession, body: impl Into<pb::server_msg::Msg>) {
        let _ = s.push_frame(pb::ServerMsg {
            obj_id: 0,
            msg: Some(body.into()),
        });
    }

    fn join_request(
        account: AccountId,
        session: SessionId,
        sink: Box<dyn flyff_common::rt::SessionSink>,
    ) -> JoinRequest {
        JoinRequest {
            session,
            account,
            authority: Authority(70),
            character: Box::new(CharacterLoad {
                identity: CharacterIdentity {
                    id: CharacterId(account.0.wrapping_add(1000)),
                    account_id: account,
                    slot: 0,
                    name: format!("Stub{}", account.0),
                    ..CharacterIdentity::default()
                },
                ..CharacterLoad::default()
            }),
            sink,
            client_build: "stub".to_owned(),
        }
    }
}

#[async_trait]
impl LobbyPort for StubLobby {
    async fn hello(&self, s: &mut GatewaySession, m: pb::Hello) -> HelloOutcome {
        if m.proto_hash != PROTO_HASH {
            Self::push(
                s,
                self.ack(pb::hello_ack::Result::ReloadRequired, &m, None, false),
            );
            return HelloOutcome::Close { code: 4012 };
        }
        let Some(account) = Self::account_of(&m.token) else {
            Self::push(
                s,
                self.ack(pb::hello_ack::Result::BadToken, &m, None, false),
            );
            return HelloOutcome::Close { code: 4013 };
        };
        s.set_account(account);
        // Re-attach (12 RA1-RA3): the avatar is still in the world.
        let online = self.in_world.lock().get(&account).copied();
        if online.is_some() {
            let (reply, rx) = reply_pair();
            let sink = s.make_game_sink(SinkMode::Gated);
            let sent = self
                .deps
                .game_tx
                .send_async(GameInput::Reattach {
                    account,
                    session: s.id(),
                    sink,
                    reply,
                })
                .await
                .is_ok();
            if sent
                && let Ok(Ok(ReattachResult::Attached)) =
                    tokio::time::timeout(Duration::from_secs(5), rx.recv_async()).await
            {
                self.in_world.lock().insert(account, s.id());
                Self::push(
                    s,
                    self.ack(pb::hello_ack::Result::Ok, &m, Some(account), true),
                );
                s.open_gate();
                return HelloOutcome::InWorld;
            }
            // NoAvatar: the stub forgets the account and continues as a normal login.
            self.in_world.lock().remove(&account);
        }
        Self::push(
            s,
            self.ack(pb::hello_ack::Result::Ok, &m, Some(account), false),
        );
        Self::push(s, Self::char_list(account));
        HelloOutcome::CharSelect
    }

    async fn char_select(
        &self,
        s: &mut GatewaySession,
        m: pb::client_envelope::Msg,
    ) -> LobbyOutcome {
        let Some(account) = s.account() else {
            return LobbyOutcome::Close(CloseReason::ProtocolError);
        };
        match m {
            pb::client_envelope::Msg::EnterWorld(_) => {
                let session = s.id();
                match self.options.enter {
                    EnterBehaviour::Immediate => {
                        let req =
                            Self::join_request(account, session, s.make_game_sink(SinkMode::Open));
                        if self
                            .deps
                            .game_tx
                            .send_async(GameInput::Join(Box::new(req)))
                            .await
                            .is_err()
                        {
                            Self::push(s, pb::LobbyError { code: 136, tid: 0 });
                            return LobbyOutcome::Stay;
                        }
                        self.in_world.lock().insert(account, session);
                        LobbyOutcome::Joined
                    }
                    EnterBehaviour::Delayed(delay) => {
                        let req =
                            Self::join_request(account, session, s.make_game_sink(SinkMode::Open));
                        let game_tx = self.deps.game_tx.clone();
                        let handle = s.handle();
                        self.in_world.lock().insert(account, session);
                        tokio::spawn(async move {
                            tokio::time::sleep(delay).await;
                            if game_tx
                                .send_async(GameInput::Join(Box::new(req)))
                                .await
                                .is_ok()
                            {
                                handle.set_state(GatewayState::InWorld);
                            } else {
                                handle.set_state(GatewayState::CharSelect);
                            }
                        });
                        LobbyOutcome::Entering
                    }
                    EnterBehaviour::Never => LobbyOutcome::Entering,
                }
            }
            _ => {
                // CreatePlayer / DelPlayer: the stub changes nothing and shows the list again.
                Self::push(s, Self::char_list(account));
                LobbyOutcome::Stay
            }
        }
    }

    fn on_net_event(&self, ev: NetEvent) {
        match ev {
            NetEvent::JoinFailed { session, .. } => {
                self.in_world.lock().retain(|_, s| *s != session);
                if let Some(h) = self.deps.directory.get(session) {
                    h.push_frame(pb::ServerMsg {
                        obj_id: 0,
                        msg: Some(pb::LobbyError { code: 133, tid: 0 }.into()),
                    });
                    h.set_state(GatewayState::CharSelect);
                }
            }
            NetEvent::Left {
                session,
                account,
                next,
                ..
            } => {
                {
                    let mut map = self.in_world.lock();
                    if map.get(&account) == Some(&session) {
                        map.remove(&account);
                    }
                }
                let Some(h) = self.deps.directory.get(session) else {
                    return;
                };
                match next {
                    flyff_game::input::AfterLeave::CharSelect => {
                        h.push_frame(pb::ServerMsg {
                            obj_id: 0,
                            msg: Some(Self::char_list(account).into()),
                        });
                        h.set_state(GatewayState::CharSelect);
                    }
                    flyff_game::input::AfterLeave::Close => {
                        h.close(CloseReason::Logout);
                    }
                }
            }
        }
    }

    fn on_session_end(&self, _s: &GatewaySession, _cause: LostCause) {}
}
