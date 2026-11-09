//! THE gateway-to-lobby boundary (Plan/10 §7.3, WP-NET-01): the session state machine, the [`LobbyPort`] trait
//! that doc 12 implements, the handles through which the lobby (and spawned lobby tasks) reach a session, and
//! the [`SessionDirectory`].
//!
//! A `GatewaySession` is owned by the session task; `LobbyPort` methods get it by `&mut` and answer by
//! returning an outcome. Anything that happens later (the enter-world flush wait, a `NetEvent`) goes through a
//! [`SessionHandle`] from the directory and arrives at the session task as a [`SessionCtl`].

use super::sink::{SinkMode, SinkShared, WsSink};
use crate::gateway::wire;
use async_trait::async_trait;
use bytes::Bytes;
use flyff_common::rt::{CloseReason, LostCause, SessionSink, SinkError, Telemetry};
use flyff_common::{AccountId, SessionId};
use flyff_game::NetEvent;
use flyff_proto::pb;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// The states of one socket (VR-003, 11 §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GatewayState {
    /// Upgraded, waiting for `Hello`.
    Connected,
    /// Authenticated, at the character list.
    CharSelect,
    /// `EnterWorld` accepted; the lobby flushes, loads and sends `Join`.
    Entering,
    /// The avatar is in the world: game messages go to the game thread.
    InWorld,
    /// Back to character select or closing while in the world.
    Leaving,
}

impl GatewayState {
    /// Lower-case name (logs, `/admin/api/sessions`).
    pub const fn name(self) -> &'static str {
        match self {
            GatewayState::Connected => "connected",
            GatewayState::CharSelect => "char_select",
            GatewayState::Entering => "entering",
            GatewayState::InWorld => "in_world",
            GatewayState::Leaving => "leaving",
        }
    }
}

impl GatewayState {
    /// The numeric form kept in [`SessionMeta`].
    const fn index(self) -> u8 {
        match self {
            GatewayState::Connected => 0,
            GatewayState::CharSelect => 1,
            GatewayState::Entering => 2,
            GatewayState::InWorld => 3,
            GatewayState::Leaving => 4,
        }
    }

    const fn from_index(i: u8) -> GatewayState {
        match i {
            1 => GatewayState::CharSelect,
            2 => GatewayState::Entering,
            3 => GatewayState::InWorld,
            4 => GatewayState::Leaving,
            _ => GatewayState::Connected,
        }
    }

    /// Every state, in `flyff_sessions{state}` order.
    pub const ALL: [GatewayState; 5] = [
        GatewayState::Connected,
        GatewayState::CharSelect,
        GatewayState::Entering,
        GatewayState::InWorld,
        GatewayState::Leaving,
    ];
}

/// What the admin surfaces (`/admin/api/sessions`, `/metrics`) may read of one socket without asking its task:
/// the state, the account, the age and the queued bytes. Written by the session task only.
pub struct SessionMeta {
    state: AtomicU8,
    account: AtomicU32,
    opened: Instant,
    sink: OnceLock<Arc<SinkShared>>,
}

impl std::fmt::Debug for SessionMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionMeta")
            .field("state", &self.state())
            .field("account", &self.account())
            .finish_non_exhaustive()
    }
}

impl SessionMeta {
    fn new() -> Arc<SessionMeta> {
        Arc::new(SessionMeta {
            state: AtomicU8::new(GatewayState::Connected.index()),
            account: AtomicU32::new(0),
            opened: Instant::now(),
            sink: OnceLock::new(),
        })
    }

    /// The state of the socket.
    pub fn state(&self) -> GatewayState {
        GatewayState::from_index(self.state.load(Ordering::Relaxed))
    }

    /// The authenticated account (`None` before `Hello`).
    pub fn account(&self) -> Option<AccountId> {
        match self.account.load(Ordering::Relaxed) {
            0 => None,
            n => Some(AccountId(n)),
        }
    }

    /// Seconds since the socket was upgraded.
    pub fn age_s(&self) -> u64 {
        self.opened.elapsed().as_secs()
    }

    /// Bytes waiting in the outbound queue of the socket.
    pub fn queued_bytes(&self) -> usize {
        self.sink.get().map_or(0, |s| s.queued().1)
    }

    pub(crate) fn set_state(&self, st: GatewayState) {
        self.state.store(st.index(), Ordering::Relaxed);
    }

    pub(crate) fn set_account(&self, a: AccountId) {
        self.account.store(a.0, Ordering::Relaxed);
    }

    pub(crate) fn set_sink(&self, shared: Arc<SinkShared>) {
        let _ = self.sink.set(shared);
    }
}

/// What `LobbyPort::hello` decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelloOutcome {
    /// `HelloAck{OK, reattached = false}` and `CharList` were pushed: state `CharSelect`.
    CharSelect,
    /// Re-attached: `HelloAck{OK, reattached = true}` was pushed and the gate opened: state `InWorld`.
    InWorld,
    /// A failure `HelloAck` was pushed: drain, then close with this WebSocket code (no `Kick` frame; 12 §8.2:
    /// 4012, 4009, 4013, 4008, 1000).
    Close {
        /// WebSocket close code.
        code: u16,
    },
}

/// What `LobbyPort::char_select` decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyOutcome {
    /// The request was answered (`CharList` / `LobbyError` pushed); stay in `CharSelect`.
    Stay,
    /// `EnterWorld` accepted: state `Entering` (the lobby's enter task continues and reports through the
    /// [`SessionDirectory`]).
    Entering,
    /// The `Join` was already sent: state `InWorld`.
    Joined,
    /// Close the socket (with a `Kick` frame where the reason has one).
    Close(CloseReason),
}

/// Commands to a session task from outside (the `NetEvent` router, lobby tasks); processed in order.
#[derive(Debug)]
pub enum SessionCtl {
    /// Change the state (e.g. `InWorld` after `Join` was sent, `CharSelect` after an abort).
    SetState(GatewayState),
    /// Push a single-message `ServerBatch` frame into the socket queue.
    PushFrame(Box<pb::ServerMsg>),
    /// Open the gate of a gated game sink (after `HelloAck{reattached}` was pushed).
    OpenGate,
    /// Close the socket.
    Close(CloseReason),
}

/// A cheap, clonable address of one session task, for lobby tasks and the `NetEvent` router.
#[derive(Clone, Debug)]
pub struct SessionHandle {
    id: SessionId,
    ctl: flume::Sender<SessionCtl>,
    meta: Arc<SessionMeta>,
}

impl SessionHandle {
    /// The session.
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// What the admin surfaces may read of the socket.
    pub fn meta(&self) -> &Arc<SessionMeta> {
        &self.meta
    }

    /// Sends a command; `false` when the session task is gone.
    pub fn send(&self, ctl: SessionCtl) -> bool {
        self.ctl.send(ctl).is_ok()
    }

    /// `SessionCtl::SetState`.
    pub fn set_state(&self, state: GatewayState) -> bool {
        self.send(SessionCtl::SetState(state))
    }

    /// `SessionCtl::PushFrame`.
    pub fn push_frame(&self, msg: pb::ServerMsg) -> bool {
        self.send(SessionCtl::PushFrame(Box::new(msg)))
    }

    /// `SessionCtl::OpenGate`.
    pub fn open_gate(&self) -> bool {
        self.send(SessionCtl::OpenGate)
    }

    /// `SessionCtl::Close`.
    pub fn close(&self, reason: CloseReason) -> bool {
        self.send(SessionCtl::Close(reason))
    }
}

/// One row of the directory.
#[derive(Clone, Debug)]
pub struct DirectoryInfo {
    /// Address of the session.
    pub handle: SessionHandle,
    /// Peer address.
    pub peer: IpAddr,
}

/// All live session tasks (10 §7.2): lets the `NetEvent` router and the lobby reach a session.
#[derive(Debug, Default)]
pub struct SessionDirectory {
    map: Mutex<BTreeMap<SessionId, DirectoryInfo>>,
}

impl SessionDirectory {
    /// An empty directory.
    pub fn new() -> Arc<SessionDirectory> {
        Arc::new(SessionDirectory::default())
    }

    /// Registers a session; the entry is removed when the guard drops (also when the task panics).
    pub(crate) fn register(
        self: &Arc<Self>,
        id: SessionId,
        peer: IpAddr,
    ) -> (DirectoryGuard, flume::Receiver<SessionCtl>, SessionHandle) {
        let (tx, rx) = flume::unbounded();
        let handle = SessionHandle {
            id,
            ctl: tx,
            meta: SessionMeta::new(),
        };
        self.map.lock().insert(
            id,
            DirectoryInfo {
                handle: handle.clone(),
                peer,
            },
        );
        (
            DirectoryGuard {
                dir: Arc::clone(self),
                id,
            },
            rx,
            handle,
        )
    }

    /// The handle of a live session.
    pub fn get(&self, id: SessionId) -> Option<SessionHandle> {
        self.map.lock().get(&id).map(|i| i.handle.clone())
    }

    /// Number of live sessions.
    pub fn len(&self) -> usize {
        self.map.lock().len()
    }

    /// `true` when no session is alive.
    pub fn is_empty(&self) -> bool {
        self.map.lock().is_empty()
    }

    /// The live sessions, ascending.
    pub fn list(&self) -> Vec<DirectoryInfo> {
        self.map.lock().values().cloned().collect()
    }
}

/// RAII removal of a directory entry.
pub(crate) struct DirectoryGuard {
    dir: Arc<SessionDirectory>,
    id: SessionId,
}

impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        self.dir.map.lock().remove(&self.id);
    }
}

/// The session as the lobby sees it (10 §7.3).
pub struct GatewaySession {
    id: SessionId,
    peer: IpAddr,
    state: GatewayState,
    account: Option<AccountId>,
    shared: Arc<SinkShared>,
    telemetry: Arc<Telemetry>,
    handle: SessionHandle,
}

impl GatewaySession {
    pub(crate) fn new(
        id: SessionId,
        peer: IpAddr,
        shared: Arc<SinkShared>,
        telemetry: Arc<Telemetry>,
        handle: SessionHandle,
    ) -> Self {
        Self {
            id,
            peer,
            state: GatewayState::Connected,
            account: None,
            shared,
            telemetry,
            handle,
        }
    }

    /// The session id.
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// The client address (`http::client_ip`).
    pub fn peer(&self) -> IpAddr {
        self.peer
    }

    /// The current state.
    pub fn state(&self) -> GatewayState {
        self.state
    }

    /// The authenticated account, once `Hello` succeeded.
    pub fn account(&self) -> Option<AccountId> {
        self.account
    }

    /// Records the account of the session.
    pub fn set_account(&mut self, a: AccountId) {
        self.account = Some(a);
        self.handle.meta.set_account(a);
    }

    /// Changes the state (the session task re-arms its timers).
    pub fn set_state(&mut self, st: GatewayState) {
        self.state = st;
        self.handle.meta.set_state(st);
    }

    /// A clonable handle for tasks that outlive this call (`char_select` returns `Entering` at once).
    pub fn handle(&self) -> SessionHandle {
        self.handle.clone()
    }

    /// Pushes one message as a single-message `ServerBatch` into the socket queue, stamped with the latest
    /// frame the game published (11 §3.5). The queue is ordered with the game's batches (G1).
    pub fn push_frame(&self, m: pb::ServerMsg) -> Result<(), SinkError> {
        let frame = wire::single_frame(m, self.telemetry.frames_completed.load(Ordering::Acquire));
        self.shared.push_gateway_frame(frame)
    }

    /// Pushes a pre-encoded frame (tests, the stub lobby).
    pub fn push_raw_frame(&self, frame: Bytes) -> Result<(), SinkError> {
        self.shared.push_gateway_frame(frame)
    }

    /// The game's sink for `Join` (`Open`) or `Reattach` (`Gated`; `open_gate` after `HelloAck`).
    pub fn make_game_sink(&self, mode: SinkMode) -> Box<dyn SessionSink> {
        Box::new(WsSink::new(Arc::clone(&self.shared), mode))
    }

    /// Opens the gate of a gated game sink (12 RA3): the held batches follow what is already queued.
    pub fn open_gate(&self) {
        self.shared.open_gate();
    }

    /// The shared socket queue (tests, metrics).
    pub fn sink_shared(&self) -> &Arc<SinkShared> {
        &self.shared
    }
}

/// The lobby as the gateway sees it (10 §7.3). Doc 12 implements it; [`crate::gateway::StubLobby`] is the test
/// double used until then.
#[async_trait]
pub trait LobbyPort: Send + Sync + 'static {
    /// 12 §8.2 checks; the lobby pushes `HelloAck` (and `CharList`) itself through `push_frame`.
    async fn hello(&self, s: &mut GatewaySession, m: pb::Hello) -> HelloOutcome;

    /// `CreatePlayer` / `DelPlayer` / `EnterWorld` in `CharSelect`. Awaits inside are allowed only when bounded
    /// (create/delete: the hash-permit timeout plus the game reply, 12 §15); the enter-world flush wait and load
    /// MUST run in a spawned lobby task that reports through the [`SessionDirectory`] — return
    /// [`LobbyOutcome::Entering`] at once.
    async fn char_select(
        &self,
        s: &mut GatewaySession,
        m: pb::client_envelope::Msg,
    ) -> LobbyOutcome;

    /// A `NetEvent` of the game thread (C3): registry bookkeeping (12) and routing to the session (10 §7.8).
    fn on_net_event(&self, ev: NetEvent);

    /// The socket is gone (doc 12 `on_socket_closed` semantics).
    fn on_session_end(&self, s: &GatewaySession, cause: LostCause);

    /// `Entering` lasted `net.timeout_entering_s` without a `Join` (11 Q1 interpretation): the lobby answers
    /// with its `LobbyError` (code of doc 12, `133`/`TID_DB_INSPECTION`), cancels its enter task and moves the
    /// session back to `CharSelect`. The default does the same with the plan's code.
    async fn entering_timeout(&self, s: &mut GatewaySession) {
        let _ = s.push_frame(pb::ServerMsg {
            obj_id: 0,
            msg: Some(pb::LobbyError { code: 133, tid: 0 }.into()),
        });
        s.set_state(GatewayState::CharSelect);
    }
}
