//! The gateway session task (Plan/10 §7.2–§7.7, WP-NET-01/02/03).
//!
//! One task per WebSocket: a reader loop (in-order, so the `ServerTimeReq` lockstep barrier of §7.6 holds) plus a
//! writer task that drains the ordered queue of [`SinkShared`]. The loop is generic over the socket halves
//! ([`WsIn`] stream, [`WsOut`] sink), so the whole life cycle — states, limits, keep-alive, timeouts, close
//! codes — is tested in memory with tokio's paused clock; `serve_socket` only adapts axum's `WebSocket`.
//!
//! ```text
//! reader loop (biased select): SessionCtl · inbound frame · state deadline · ping · liveness · writer ended
//!   frame → InboundPipeline (size, text, token bucket, strict decode, length limits) → state table →
//!           lobby.hello / lobby.char_select / GameInput::Client (send_async) / ServerTime
//!   end   → Entering: wait for the enter task · SessionLost (InWorld/Leaving) · lobby.on_session_end · writer
//! ```

use super::GatewayShared;
use super::clock::{OffsetFilter, estimate_server_frame};
use super::lobby_port::{GatewaySession, GatewayState, HelloOutcome, LobbyOutcome, SessionCtl};
use super::pipeline::{DropReason, InboundPipeline, Limits, Route, Verdict, route};
use super::sink::{Closing, Control, SinkShared, close_code, transport};
use super::wire::{kick_frame, single_frame};
use bytes::Bytes;
use flyff_common::SessionId;
use flyff_common::log_budget::LogBudget;
use flyff_common::rt::{CloseReason, KickReason, LostCause};
use flyff_game::GameInput;
use flyff_proto::{ClientMsg, pb};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use std::fmt::Display;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::Instant;

/// One inbound WebSocket frame as the session sees it.
#[derive(Debug, PartialEq, Eq)]
pub enum WsIn {
    /// A binary message.
    Binary(Bytes),
    /// A text message (a protocol error).
    Text,
    /// A ping (answered by the WebSocket layer).
    Ping,
    /// A pong.
    Pong,
    /// A close frame.
    Close,
}

/// How reading a frame failed.
#[derive(Debug, PartialEq, Eq)]
pub enum WsInError {
    /// The message exceeds `net.max_client_msg_bytes` (close 1009).
    TooBig,
    /// The connection ended abruptly (reset, broken pipe).
    Closed,
    /// A WebSocket protocol violation.
    Protocol,
}

/// What the writer sends to the socket.
#[derive(Debug, PartialEq, Eq)]
pub enum WsOut {
    /// A binary message.
    Binary(Bytes),
    /// A ping with a payload.
    Ping(Bytes),
    /// A close frame with a code.
    Close(u16),
}

/// How long a single write may block the writer before it gives up.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the session task waits for the writer to flush the close frame after it decided to end.
const WRITER_GRACE: Duration = Duration::from_secs(10);

// ---- writer ---------------------------------------------------------------------------------------------

/// Writes one message; `false` when the socket failed, the write timed out or (only with `interruptible`, i.e.
/// in the main loop) a non-draining close interrupted a blocked write: a slow consumer, nothing is worth waiting
/// for. The writes of the closing phase are never interrupted, only bounded by [`WRITE_TIMEOUT`].
async fn write<S>(out: &mut S, msg: WsOut, shared: &SinkShared, interruptible: bool) -> bool
where
    S: Sink<WsOut> + Unpin,
    S::Error: Display,
{
    let interrupt = async {
        if !interruptible {
            std::future::pending::<()>().await;
        }
        shared.close_requested().await;
        if shared.closing().is_some_and(|c| c.drain) {
            // A draining close waits for the frame in flight; the loop notices the close afterwards.
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        r = tokio::time::timeout(WRITE_TIMEOUT, out.send(msg)) => match r {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "websocket write failed");
                false
            }
            Err(_) => {
                tracing::debug!("websocket write timed out");
                false
            }
        },
        () = interrupt => false,
    }
}

/// The writer task: drains the data queue and the control path to the socket; on a close request it writes the
/// queued frames (unless the close does not drain), then the close frame, then closes the socket.
pub(crate) async fn writer_task<S>(
    mut out: S,
    rx: flume::Receiver<Bytes>,
    ctrl: flume::Receiver<Control>,
    shared: Arc<SinkShared>,
) where
    S: Sink<WsOut> + Unpin,
    S::Error: Display,
{
    let mut ctrl_open = true;
    let mut failed = false;
    'run: loop {
        if shared.is_closed() {
            break;
        }
        tokio::select! {
            biased;
            () = shared.close_requested() => break,
            c = ctrl.recv_async(), if ctrl_open => match c {
                Ok(Control::Ping(p)) => {
                    if !write(&mut out, WsOut::Ping(p), &shared, true).await {
                        failed = true;
                        break 'run;
                    }
                }
                Err(_) => ctrl_open = false,
            },
            f = rx.recv_async() => match f {
                Ok(frame) => {
                    let len = frame.len();
                    let ok = write(&mut out, WsOut::Binary(frame), &shared, true).await;
                    shared.frame_written(len);
                    if !ok {
                        failed = true;
                        break 'run;
                    }
                }
                // Every sender is gone: nothing more can arrive.
                Err(_) => break 'run,
            },
        }
    }
    if failed && !shared.is_closed() {
        // A failed write while the session still believes the socket is fine (or merely stalled): the reader
        // must stop too. A close that was already requested (slow consumer, kick) keeps its own code.
        shared.writer_failed();
    }
    let closing = shared.closing().unwrap_or(Closing {
        code: close_code(CloseReason::Logout),
        reason: Some(CloseReason::Logout),
        drain: false,
    });
    if closing.drain {
        while let Ok(frame) = rx.try_recv() {
            let len = frame.len();
            let ok = write(&mut out, WsOut::Binary(frame), &shared, false).await;
            shared.frame_written(len);
            if !ok {
                break;
            }
        }
    }
    let _ = write(&mut out, WsOut::Close(closing.code), &shared, false).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), out.close()).await;
}

// ---- the session ----------------------------------------------------------------------------------------

/// Why the reader loop ended (before the cause of the loss is derived).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// The peer closed or the stream ended.
    Peer,
    /// A read error that is not a clean close.
    ProtocolError,
    /// No frame of any kind within `net.ws_idle_timeout_s`.
    Idle,
    /// The writer ended (a failed write or a completed close).
    WriterGone,
    /// The gateway closed the socket itself.
    Local,
}

struct GaugeGuard(Arc<flyff_common::rt::Telemetry>);

impl Drop for GaugeGuard {
    fn drop(&mut self) {
        self.0.sessions.fetch_sub(1, Ordering::Relaxed);
    }
}

struct Task<I> {
    gw: Arc<GatewayShared>,
    id: SessionId,
    sess: GatewaySession,
    shared: Arc<SinkShared>,
    inbound: I,
    ctrl_tx: flume::Sender<Control>,
    ctl_rx: flume::Receiver<SessionCtl>,
    pipeline: InboundPipeline,
    t0: Instant,
    idle_deadline: Instant,
    ping_deadline: Instant,
    state_deadline: Option<Instant>,
    armed_state: Option<GatewayState>,
    offset: OffsetFilter,
    last_server_frame: u64,
    local_cause: Option<LostCause>,
    ping_seq: u64,
    warn_budget: LogBudget,
}

/// Runs one session until it ends. `inbound`/`outbound` are the two halves of the socket.
pub(crate) async fn run_session<I, S>(
    gw: Arc<GatewayShared>,
    id: SessionId,
    peer: IpAddr,
    inbound: I,
    outbound: S,
) where
    I: Stream<Item = Result<WsIn, WsInError>> + Unpin,
    S: Sink<WsOut> + Unpin + Send + 'static,
    S::Error: Display,
{
    let cfg = &gw.cfg;
    let (shared, rx) = SinkShared::new(
        cfg.outbound_queue_frames as usize,
        usize::try_from(cfg.outbound_queue_bytes).unwrap_or(usize::MAX),
    );
    let (ctrl_tx, ctrl_rx) = flume::unbounded();
    let writer = tokio::spawn(writer_task(outbound, rx, ctrl_rx, Arc::clone(&shared)));
    gw.telemetry.sessions.fetch_add(1, Ordering::Relaxed);
    gw.stats.sessions_opened.fetch_add(1, Ordering::Relaxed);
    let _gauge = GaugeGuard(Arc::clone(&gw.telemetry));
    let (_dir_guard, ctl_rx, handle) = gw.directory.register(id, peer);
    handle.meta().set_sink(Arc::clone(&shared));
    let now = Instant::now();
    let limits = Limits {
        max_msg_bytes: cfg.max_client_msg_bytes as usize,
        strict_decode: cfg.strict_decode,
        per_sec: cfg.client_msgs_per_sec,
        burst: cfg.client_msgs_burst,
        kick_factor: cfg.rate_kick_factor,
    };
    let idle = Duration::from_secs(u64::from(cfg.ws_idle_timeout_s.max(1)));
    let ping = ping_interval(cfg.ws_ping_interval_s);
    let sess = GatewaySession::new(
        id,
        peer,
        Arc::clone(&shared),
        Arc::clone(&gw.telemetry),
        handle,
    );
    tracing::debug!(session_id = id.0, %peer, "websocket session opened");
    let mut task = Task {
        id,
        sess,
        shared: Arc::clone(&shared),
        inbound,
        ctrl_tx,
        ctl_rx,
        pipeline: InboundPipeline::new(limits, 0),
        t0: now,
        idle_deadline: now + idle,
        ping_deadline: now + ping,
        state_deadline: None,
        armed_state: None,
        offset: OffsetFilter::default(),
        last_server_frame: 0,
        local_cause: None,
        ping_seq: 0,
        warn_budget: LogBudget::new(),
        gw,
    };
    let end = task.run_loop().await;
    task.finish(end, writer).await;
}

fn ping_interval(secs: u32) -> Duration {
    Duration::from_secs(u64::from(secs.max(1)))
}

impl<I> Task<I>
where
    I: Stream<Item = Result<WsIn, WsInError>> + Unpin,
{
    fn now_ms(&self) -> u64 {
        u64::try_from(self.t0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn idle(&self) -> Duration {
        Duration::from_secs(u64::from(self.gw.cfg.ws_idle_timeout_s.max(1)))
    }

    /// Arms the state deadline for the current state (10 §7.5): hello 20 s, entering 60 s, char-select idle
    /// 900 s; `InWorld` and `Leaving` have none.
    fn arm_state_deadline(&mut self) {
        let cfg = &self.gw.cfg;
        let secs = match self.sess.state() {
            GatewayState::Connected => Some(cfg.timeout_hello_s),
            GatewayState::CharSelect => Some(cfg.timeout_charselect_idle_s),
            GatewayState::Entering => Some(cfg.timeout_entering_s),
            GatewayState::InWorld | GatewayState::Leaving => None,
        };
        self.state_deadline =
            secs.map(|s| Instant::now() + Duration::from_secs(u64::from(s.max(1))));
        self.armed_state = Some(self.sess.state());
    }

    async fn run_loop(&mut self) -> End {
        loop {
            if self.shared.is_closed() {
                return End::Local;
            }
            if self.armed_state != Some(self.sess.state()) {
                let new_state = self.sess.state();
                self.arm_state_deadline();
                if new_state == GatewayState::InWorld {
                    self.on_enter_world().await;
                }
            }
            let (idle_d, ping_d, state_d) =
                (self.idle_deadline, self.ping_deadline, self.state_deadline);
            tokio::select! {
                biased;
                () = self.shared.closed_for_reader() => {
                    return if self.shared.has_write_failed() { End::WriterGone } else { End::Local };
                }
                ctl = self.ctl_rx.recv_async() => {
                    if let Ok(ctl) = ctl {
                        self.on_ctl(ctl).await;
                    }
                }
                item = self.inbound.next() => match item {
                    None | Some(Ok(WsIn::Close)) => return End::Peer,
                    Some(Err(WsInError::TooBig)) => {
                        self.gw.stats.record_dropped(DropReason::Size);
                        self.close_transport(transport::TOO_BIG);
                        self.local_cause.get_or_insert(LostCause::ProtocolError);
                        return End::Local;
                    }
                    Some(Err(WsInError::Closed)) => return End::Peer,
                    Some(Err(WsInError::Protocol)) => return End::ProtocolError,
                    Some(Ok(frame)) => {
                        self.idle_deadline = Instant::now() + self.idle();
                        self.on_frame(frame).await;
                    }
                },
                () = tokio::time::sleep_until(state_d.unwrap_or_else(far_future)), if state_d.is_some() => {
                    self.on_state_timeout().await;
                }
                () = tokio::time::sleep_until(ping_d) => {
                    self.send_ping();
                    self.ping_deadline = Instant::now() + ping_interval(self.gw.cfg.ws_ping_interval_s);
                }
                () = tokio::time::sleep_until(idle_d) => {
                    // No frame of any kind (not even a Pong) within the liveness timeout.
                    self.gw.stats.idle_closes.fetch_add(1, Ordering::Relaxed);
                    self.close_transport(transport::GOING_AWAY);
                    return End::Idle;
                }
            }
        }
    }

    // ---- commands from outside ------------------------------------------------------------------------

    async fn on_ctl(&mut self, ctl: SessionCtl) {
        match ctl {
            SessionCtl::SetState(st) => self.sess.set_state(st),
            SessionCtl::PushFrame(msg) => {
                let _ = self.sess.push_frame(*msg);
            }
            SessionCtl::OpenGate => self.sess.open_gate(),
            SessionCtl::Close(reason) => {
                let cause = match reason {
                    CloseReason::Replaced => LostCause::Replaced,
                    CloseReason::Kicked(r) => LostCause::Kicked(r),
                    CloseReason::SlowConsumer => LostCause::SlowConsumer,
                    CloseReason::ProtocolError => LostCause::ProtocolError,
                    _ => LostCause::PeerClosed,
                };
                self.close_local(reason, cause);
            }
        }
    }

    // ---- closing --------------------------------------------------------------------------------------------

    fn server_frame_stamp(&self) -> u64 {
        self.gw.telemetry.frames_completed.load(Ordering::Acquire)
    }

    /// Closes for `reason`: the `Kick` frame of the §7.7 table (if any), then the close with its code. The
    /// writer sends everything queued before.
    fn close_local(&mut self, reason: CloseReason, cause: LostCause) {
        if self.shared.is_closed() {
            return;
        }
        if let Some(frame) = kick_frame(reason, self.server_frame_stamp()) {
            let _ = self.shared.push_gateway_frame(frame);
        }
        if let CloseReason::Kicked(k) = reason {
            self.gw.stats.record_kick(k);
        }
        self.local_cause.get_or_insert(cause);
        self.shared.close(reason);
    }

    fn kick(&mut self, reason: KickReason, cause: LostCause) {
        self.client_warn(&format!("kicked: {reason:?}"));
        self.close_local(CloseReason::Kicked(reason), cause);
    }

    /// One client-caused WARN line, limited per session to `log.client_warn_per_min` (10 §10.4): the lines
    /// over the budget are counted and summarised once per minute.
    fn client_warn(&mut self, what: &str) {
        let now = self.now_ms();
        let limit = self.gw.client_warn_per_min.load(Ordering::Relaxed);
        let admit = self.warn_budget.admit(now, limit);
        if let Some(n) = admit.summary {
            tracing::info!(
                session_id = self.id.0,
                "suppressed {n} warnings for session {}",
                self.id.0
            );
        }
        if admit.pass {
            tracing::warn!(session_id = self.id.0, peer = %self.sess.peer(), state = self.sess.state().name(), "{what}");
        }
    }

    /// Closes with a transport code and no `Kick` frame (1001, 1009, lobby failure codes).
    fn close_transport(&mut self, code: u16) {
        self.shared.close_with(Closing {
            code,
            reason: None,
            drain: code != transport::GOING_AWAY && code != transport::TOO_BIG,
        });
    }

    fn send_ping(&mut self) {
        self.ping_seq += 1;
        let payload = getrandom::u64().unwrap_or(self.ping_seq).to_be_bytes();
        let _ = self
            .ctrl_tx
            .send(Control::Ping(Bytes::copy_from_slice(&payload)));
        self.gw.stats.pings_sent.fetch_add(1, Ordering::Relaxed);
    }

    // ---- state timeouts ---------------------------------------------------------------------------------------

    async fn on_state_timeout(&mut self) {
        match self.sess.state() {
            GatewayState::Connected | GatewayState::CharSelect => {
                self.kick(KickReason::Timeout, LostCause::IdleTimeout);
            }
            GatewayState::Entering => {
                let gw = Arc::clone(&self.gw);
                gw.lobby.entering_timeout(&mut self.sess).await;
                // The lobby moved the session back (CharSelect arms the idle timer); a lobby that did not
                // change the state gets another full period instead of a busy loop.
                self.arm_state_deadline();
            }
            GatewayState::InWorld | GatewayState::Leaving => {
                self.state_deadline = None;
            }
        }
    }

    // ---- inbound frames --------------------------------------------------------------------------------------

    async fn on_frame(&mut self, frame: WsIn) {
        let verdict = match frame {
            WsIn::Binary(data) => {
                let now = self.now_ms();
                let len = data.len();
                let v = self.pipeline.binary(now, &data);
                if let Verdict::Message(m) = &v {
                    self.gw.stats.record_msg(m.case(), len);
                    self.gw
                        .telemetry
                        .inbound_msgs
                        .fetch_add(1, Ordering::Relaxed);
                    self.gw
                        .telemetry
                        .inbound_bytes
                        .fetch_add(len as u64, Ordering::Relaxed);
                }
                v
            }
            WsIn::Text => self.pipeline.text(),
            WsIn::Ping | WsIn::Pong => return,
            WsIn::Close => return,
        };
        match verdict {
            Verdict::Message(msg) => self.on_message(*msg).await,
            Verdict::Dropped(why) => self.gw.stats.record_dropped(why),
            Verdict::Kick { reason, why } => {
                self.gw.stats.record_dropped(why);
                let cause = match reason {
                    KickReason::RateLimit => LostCause::RateLimited,
                    _ => LostCause::ProtocolError,
                };
                self.kick(reason, cause);
            }
            Verdict::TooBig => {
                self.gw.stats.record_dropped(DropReason::Size);
                self.client_warn("message above the size cap");
                self.close_transport(transport::TOO_BIG);
                self.local_cause.get_or_insert(LostCause::ProtocolError);
            }
        }
    }

    /// Step 7 and 8 of the pipeline: the state table, then the route.
    async fn on_message(&mut self, msg: ClientMsg) {
        match route(self.sess.state(), msg.case()) {
            Route::Hello => {
                if let ClientMsg::Hello(h) = msg {
                    let gw = Arc::clone(&self.gw);
                    let outcome = gw.lobby.hello(&mut self.sess, h).await;
                    self.apply_hello(outcome);
                }
            }
            Route::Lobby => {
                // A lobby request resets the character-select idle timer (ServerTimeReq does not).
                self.arm_state_deadline();
                let gw = Arc::clone(&self.gw);
                let outcome = gw.lobby.char_select(&mut self.sess, msg).await;
                self.apply_lobby(outcome);
            }
            Route::Game => {
                let leaving = matches!(msg, ClientMsg::ReturnToCharSelect(_));
                let input = GameInput::Client {
                    session: self.id,
                    msg,
                };
                // Backpressure lands on this socket only; the game drains with `try_recv`.
                if self.gw.game_tx.send_async(input).await.is_err() {
                    self.close_local(CloseReason::InternalError, LostCause::WriteFailed);
                    return;
                }
                if leaving {
                    self.sess.set_state(GatewayState::Leaving);
                }
            }
            Route::ServerTime => {
                if let ClientMsg::ServerTimeReq(req) = msg {
                    self.answer_server_time(req).await;
                }
            }
            Route::Drop => self.gw.stats.record_dropped(DropReason::State),
            Route::Kick => {
                self.gw.stats.record_dropped(DropReason::State);
                self.kick(KickReason::InvalidState, LostCause::ProtocolError);
            }
        }
    }

    fn apply_hello(&mut self, outcome: HelloOutcome) {
        match outcome {
            HelloOutcome::CharSelect => self.sess.set_state(GatewayState::CharSelect),
            HelloOutcome::InWorld => self.sess.set_state(GatewayState::InWorld),
            // A failure `HelloAck` was pushed: the queued frames are drained, then the close, no `Kick` frame.
            HelloOutcome::Close { code } => self.close_transport(code),
        }
    }

    fn apply_lobby(&mut self, outcome: LobbyOutcome) {
        match outcome {
            LobbyOutcome::Stay => {}
            LobbyOutcome::Entering => self.sess.set_state(GatewayState::Entering),
            LobbyOutcome::Joined => self.sess.set_state(GatewayState::InWorld),
            LobbyOutcome::Close(reason) => self.close_local(reason, LostCause::PeerClosed),
        }
    }

    // ---- ServerTime ----------------------------------------------------------------------------------------------

    /// `ServerTimeReq` → `ServerTime` (10 §7.6). Reached only after every earlier message of this socket has
    /// been pushed into C1 (`send_async` completed: the reader is strictly in order); the reply goes behind the
    /// frames already queued on the sink.
    async fn answer_server_time(&mut self, req: pb::ServerTimeReq) {
        let estimate = estimate_server_frame(&self.gw.telemetry).max(self.last_server_frame);
        self.last_server_frame = estimate;
        let msg = pb::ServerTime {
            client_frame_echo: req.client_frame,
            client_perf_ms_echo: req.client_perf_ms,
            server_frame: estimate,
            server_unix_ms: self.gw.wall.now_ms().0,
        };
        let frame = single_frame(
            pb::ServerMsg {
                obj_id: 0,
                msg: Some(msg.into()),
            },
            estimate,
        );
        let _ = self.shared.push_gateway_frame(frame);
        self.gw
            .stats
            .server_time_replies
            .fetch_add(1, Ordering::Relaxed);
        let sample = i64::try_from(estimate).unwrap_or(i64::MAX) - i64::from(req.client_frame);
        let median = self.offset.push(sample);
        if self.sess.state() == GatewayState::InWorld {
            self.send_offset(median).await;
        }
    }

    /// Entering `InWorld`: the game gets the offset known so far exactly once (10 §4.6).
    async fn on_enter_world(&mut self) {
        self.offset.reset_sent();
        if let Some(m) = self.offset.median() {
            self.send_offset(m).await;
        }
    }

    async fn send_offset(&mut self, offset: i64) {
        if let Some(offset_frames) = self.offset.to_send(offset) {
            let _ = self
                .gw
                .game_tx
                .send_async(GameInput::ClockOffset {
                    session: self.id,
                    offset_frames,
                })
                .await;
        }
    }

    // ---- the end -----------------------------------------------------------------------------------------------

    fn cause(&self, end: End) -> LostCause {
        if let Some(c) = self.local_cause {
            return c;
        }
        match self.shared.closing().and_then(|c| c.reason) {
            Some(CloseReason::Kicked(r)) => LostCause::Kicked(r),
            Some(CloseReason::SlowConsumer) => LostCause::SlowConsumer,
            Some(CloseReason::Replaced) => LostCause::Replaced,
            Some(CloseReason::ProtocolError) => LostCause::ProtocolError,
            Some(
                CloseReason::Logout | CloseReason::ServerShutdown | CloseReason::InternalError,
            )
            | None => match end {
                End::Idle => LostCause::IdleTimeout,
                End::ProtocolError => LostCause::ProtocolError,
                End::WriterGone => LostCause::WriteFailed,
                End::Peer | End::Local => LostCause::PeerClosed,
            },
        }
    }

    /// Waits (bounded) for the lobby's enter task to report where the session ended up, so `SessionLost`
    /// follows `Join` on C1 (rule EW1 of 12 §11.1).
    async fn settle_entering(&mut self) {
        let grace = Duration::from_secs(u64::from(self.gw.cfg.timeout_entering_s.max(1)));
        let deadline = Instant::now() + grace;
        while self.sess.state() == GatewayState::Entering {
            match tokio::time::timeout_at(deadline, self.ctl_rx.recv_async()).await {
                Ok(Ok(SessionCtl::SetState(st))) => self.sess.set_state(st),
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
    }

    async fn finish(mut self, end: End, mut writer: tokio::task::JoinHandle<()>) {
        let cause = self.cause(end);
        tracing::debug!(
            session_id = self.id.0,
            ?cause,
            state = self.sess.state().name(),
            "websocket session ended"
        );
        if self.sess.state() == GatewayState::Entering {
            // the enter task must not send `Join` for a socket that is gone (12 §11.1 step 8)
            self.shared.set_peer_gone();
            self.settle_entering().await;
        }
        let gw = Arc::clone(&self.gw);
        if matches!(
            self.sess.state(),
            GatewayState::InWorld | GatewayState::Leaving
        ) && cause != LostCause::Replaced
        {
            let _ = gw
                .game_tx
                .send_async(GameInput::SessionLost {
                    session: self.id,
                    cause,
                })
                .await;
        }
        gw.lobby.on_session_end(&self.sess, cause);
        if !self.shared.is_closed() {
            // The peer is gone: nothing to drain, nobody to say goodbye to.
            self.shared.close_with(Closing {
                code: close_code(CloseReason::Logout),
                reason: None,
                drain: false,
            });
        }
        if tokio::time::timeout(WRITER_GRACE, &mut writer)
            .await
            .is_err()
        {
            writer.abort();
        }
    }
}

fn far_future() -> Instant {
    Instant::now() + Duration::from_secs(86_400 * 365)
}
