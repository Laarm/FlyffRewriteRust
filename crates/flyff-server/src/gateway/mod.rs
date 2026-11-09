//! The WebSocket gateway (Plan/10 §6.3, §7; 11 §1–§3; WP-NET-01..03).
//!
//! `GET /ws` upgrades to one session task per socket ([`session`]): the state machine
//! `Connected → CharSelect → Entering → InWorld → Leaving` (VR-003), the inbound limits of §7.4
//! ([`pipeline`]), keep-alive and the per-state timeouts of §7.5, the `ServerTimeReq` echo of §7.6
//! ([`clock`]) and the closing rules of §7.7 ([`wire`]). Everything the game thread sends arrives through the
//! socket's [`WsSink`] (one ordered, bounded queue; [`sink`]); everything a client sends is routed to the lobby
//! (`CharSelect`, via the [`LobbyPort`] trait) or to the game thread (`InWorld`, `GameInput::Client`).
//!
//! The router is mounted with [`router`](fn@router); [`GatewayShared`] is its state. [`StubLobby`] is the test double
//! of the lobby until doc 12's implementation of [`LobbyPort`] lands.

pub mod clock;
pub mod iplimit;
pub mod lobby_port;
pub mod pipeline;
pub mod router;
pub mod session;

pub mod sink;
pub mod stats;
pub mod stub_lobby;
pub mod wire;

pub use iplimit::{ConnSlot, IpLimiter, IpLimits, UpgradeRefusal};
pub use lobby_port::{
    DirectoryInfo, GatewaySession, GatewayState, HelloOutcome, LobbyOutcome, LobbyPort, SessionCtl,
    SessionDirectory, SessionHandle, SessionMeta,
};
pub use pipeline::{DropReason, InboundPipeline, Limits, Route, Verdict, route};
pub use router::run_net_event_router;
pub use sink::{SinkMode, SinkShared, WsSink, close_code};
pub use stats::{GatewayStats, StatsSnapshot};
pub use stub_lobby::{EnterBehaviour, LobbyDeps, StubLobby, StubOptions};

/// The plan's name for [`LobbyPort`] in the task description; the trait is one and the same.
pub use lobby_port::LobbyPort as LobbyService;

use crate::config::GatewayConfig;
use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use flyff_common::SessionId;
use flyff_common::rt::Telemetry;
use flyff_common::rt::telemetry::HealthState;
use flyff_game::GameInput;
use flyff_game::tick::WallClockCell;
use futures_util::{SinkExt, StreamExt};
use session::{WsIn, WsInError, WsOut};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Everything the gateway needs (the state of the `/ws` route).
pub struct GatewayShared {
    /// The limits and timeouts (`net.*`).
    pub cfg: GatewayConfig,
    /// C1: the game inbox.
    pub game_tx: flume::Sender<GameInput>,
    /// Shared counters (`frames_completed` for `ServerTime`, session gauges).
    pub telemetry: Arc<Telemetry>,
    /// The lobby behind the handshake and the character list.
    pub lobby: Arc<dyn LobbyPort>,
    /// The wall clock behind `ServerTime.server_unix_ms`.
    pub wall: WallClockCell,
    /// Live session tasks.
    pub directory: Arc<SessionDirectory>,
    /// Inbound counters.
    pub stats: GatewayStats,
    /// `net.allowed_origins`, live (a reload replaces it; `cfg.allowed_origins` is the startup value).
    allowed_origins: parking_lot::RwLock<Vec<String>>,
    /// `log.client_warn_per_min`, live.
    pub client_warn_per_min: std::sync::atomic::AtomicU32,
    /// The per-IP upgrade bucket and open-socket counts of `/ws` (10 §7.1, WP-NET-04); `admin::auth` charges its
    /// failed token attempts to the same bucket.
    pub ip_limiter: Arc<IpLimiter>,
    /// `--dev`: the Vite dev origins pass the Origin check.
    pub dev: bool,
    next_session: AtomicU64,
}

impl GatewayShared {
    /// The shared gateway state. `directory` must be the one the lobby was built with.
    pub fn new(
        cfg: GatewayConfig,
        game_tx: flume::Sender<GameInput>,
        telemetry: Arc<Telemetry>,
        lobby: Arc<dyn LobbyPort>,
        wall: WallClockCell,
        directory: Arc<SessionDirectory>,
        dev: bool,
    ) -> Arc<GatewayShared> {
        let ip_limiter = IpLimiter::new(
            IpLimits {
                upgrade_per_sec: cfg.upgrade_per_ip_per_sec,
                upgrade_burst: cfg.upgrade_per_ip_burst,
                max_conns: cfg.max_conns_per_ip,
            },
            Arc::new(crate::assets::ServerClock::default()),
        );
        Arc::new(GatewayShared {
            allowed_origins: parking_lot::RwLock::new(cfg.allowed_origins.clone()),
            client_warn_per_min: std::sync::atomic::AtomicU32::new(cfg.client_warn_per_min),
            cfg,
            game_tx,
            telemetry,
            lobby,
            wall,
            directory,
            stats: GatewayStats::new(),
            ip_limiter,
            dev,
            next_session: AtomicU64::new(1),
        })
    }

    /// Replaces `net.allowed_origins` (configuration reload).
    pub fn set_allowed_origins(&self, origins: &[String]) {
        *self.allowed_origins.write() = origins.to_vec();
    }

    /// The `Origin` values accepted besides the serving host.
    pub fn allowed_origins(&self) -> Vec<String> {
        self.allowed_origins.read().clone()
    }

    /// The next process-wide session id (monotonic from 1).
    pub fn alloc_session(&self) -> SessionId {
        SessionId(self.next_session.fetch_add(1, Ordering::Relaxed))
    }
}

/// The `/ws` route. Mount it on the HTTP router with `Router::merge` before the middleware stack is layered
/// (WP-WEB-01: the stack's layer 0 `net.ip_block` and the origin/IP checks of WP-NET-04 apply to it).
pub fn router(shared: Arc<GatewayShared>) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .with_state(shared)
}

/// `true` when the `Origin` header may open a socket (10 §7.1 check 3, 11 §1.1): absent (a non-browser client
/// such as the bot) is allowed; otherwise its authority must equal the request `Host`, be listed in
/// `net.allowed_origins`, or (only with `--dev`) be one of the Vite dev origins. This is
/// `http::origin::is_allowed` without a known request scheme (the upgrade handler passes the scheme of the
/// request when the HTTP stack computed it).
pub fn origin_allowed(
    origin: Option<&str>,
    host: Option<&str>,
    allowed: &[String],
    dev: bool,
) -> bool {
    crate::http::origin::is_allowed(origin, host, None, allowed, dev)
}

async fn ws_upgrade(
    State(gw): State<Arc<GatewayShared>>,
    peer: Result<ConnectInfo<SocketAddr>, axum::extract::rejection::ExtensionRejection>,
    client: Option<axum::Extension<crate::http::ClientInfo>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // 2. the process must be serving
    if !matches!(
        gw.telemetry.health.state(),
        HealthState::Running | HealthState::ShuttingDown
    ) {
        gw.stats
            .upgrades_rejected_state
            .fetch_add(1, Ordering::Relaxed);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::RETRY_AFTER, "5")],
            "not serving\n",
        )
            .into_response();
    }
    // 3. Origin
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let scheme = client.as_ref().map(|c| c.0.scheme());
    let allowed = gw.allowed_origins();
    if !crate::http::origin::is_allowed(origin, host, scheme, &allowed, gw.dev) {
        gw.stats
            .upgrades_rejected_origin
            .fetch_add(1, Ordering::Relaxed);
        return (StatusCode::FORBIDDEN, "origin not allowed\n").into_response();
    }
    // The client address is the HTTP stack's (`http::client_ip`, Plan/24 §3.2): trusted-proxy aware.
    let peer_ip = client.map(|c| c.0.ip).unwrap_or_else(|| {
        peer.map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |ConnectInfo(a)| {
            a.ip().to_canonical()
        })
    });
    // 4 and 5. the per-IP upgrade bucket and the open-socket count (WP-NET-04); the slot lives as long as the
    // session task (dropped with it, also when it panics or the handshake never completes)
    let slot = match gw.ip_limiter.try_upgrade(peer_ip) {
        Ok(slot) => slot,
        Err(UpgradeRefusal::Rate { retry_after_s }) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, retry_after_s.to_string())],
                "too many connection attempts\n",
            )
                .into_response();
        }
        Err(UpgradeRefusal::Conns) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, "2".to_owned())],
                "too many open connections\n",
            )
                .into_response();
        }
    };
    let max = gw.cfg.max_client_msg_bytes as usize;
    ws.max_message_size(max)
        .max_frame_size(max)
        .on_upgrade(move |socket| serve_socket(gw, socket, peer_ip, slot))
}

/// Adapts an axum `WebSocket` to the session task. `_slot` is the per-IP connection slot of the upgrade.
async fn serve_socket(gw: Arc<GatewayShared>, socket: WebSocket, peer: IpAddr, _slot: ConnSlot) {
    let id = gw.alloc_session();
    let (tx, rx) = socket.split();
    let inbound = rx.map(map_in);
    let outbound = Box::pin(
        tx.with(|out: WsOut| std::future::ready(Ok::<Message, axum::Error>(map_out(out)))),
    );
    session::run_session(gw, id, peer, inbound, outbound).await;
}

fn map_in(r: Result<Message, axum::Error>) -> Result<WsIn, WsInError> {
    match r {
        Ok(Message::Binary(b)) => Ok(WsIn::Binary(b)),
        Ok(Message::Text(_)) => Ok(WsIn::Text),
        Ok(Message::Ping(_)) => Ok(WsIn::Ping),
        Ok(Message::Pong(_)) => Ok(WsIn::Pong),
        Ok(Message::Close(_)) => Ok(WsIn::Close),
        Err(e) => {
            let text = e.to_string();
            Err(
                if text.contains("too long") || text.contains("Space limit") {
                    WsInError::TooBig
                } else if text.contains("reset")
                    || text.contains("closed")
                    || text.contains("Broken pipe")
                    || text.contains("Io error")
                {
                    WsInError::Closed
                } else {
                    WsInError::Protocol
                },
            )
        }
    }
}

fn map_out(o: WsOut) -> Message {
    match o {
        WsOut::Binary(b) => Message::Binary(b),
        WsOut::Ping(b) => Message::Ping(b),
        WsOut::Close(code) => Message::Close(Some(CloseFrame {
            code,
            reason: "".into(),
        })),
    }
}
