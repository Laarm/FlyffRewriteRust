//! The listener and connection handling (Plan/24 §3.1, WP-WEB-01).
//!
//! [`serve`] owns the accept loop of the one HTTP port:
//!
//! 1. sockets get `TCP_NODELAY` and `SO_KEEPALIVE` (60 s idle);
//! 2. accept-level caps: `web.max_tcp_per_ip` (peers inside `web.trusted_proxies` are exempt) and
//!    `web.max_tcp_total` close a connection right after accept without a response, counted in
//!    `flyff_http_conn_rejected_total{reason}`. Every TCP connection counts, WebSocket ones included: the
//!    guard lives in the socket wrapper, so it is released when the socket is dropped, wherever it ended up
//!    (an upgraded connection moves into the gateway's task);
//! 3. HTTP/1.1 through hyper-util's `auto::Builder` (plain HTTP never speaks h2): headers up to 64 KiB; a request
//!    head must complete within 10 s of its first byte (slowloris), an idle keep-alive connection is closed
//!    after 75 s. Both are enforced by a per-connection watchdog over the socket wrapper's activity counters,
//!    because hyper's own `header_read_timeout` also starts while a keep-alive connection waits for its next
//!    request;
//! 4. graceful shutdown: stop accepting, tell every connection to finish its in-flight request, wait at most
//!    10 s, then drop what is left.
//!
//! The router sees the TCP peer as `ConnectInfo<SocketAddr>`; [`TlsConnection`] is the extension a TLS
//! acceptor (WP-WEB-16) adds so that the scheme rule of §3.2 knows the connection is TLS.

use super::cidr::CidrList;
use super::ratelimit::limit_key;
use super::telemetry::{HttpTelemetry, RejectReason};
use axum::Router;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use futures_util::future::BoxFuture;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Marker extension of requests that arrived over a TLS connection (inserted by the TLS acceptor).
#[derive(Clone, Copy, Debug)]
pub struct TlsConnection;

/// Settings of [`serve`].
#[derive(Clone, Debug)]
pub struct ServeOptions {
    /// `web.max_tcp_per_ip`; 0 = unlimited.
    pub max_tcp_per_ip: u32,
    /// `web.max_tcp_total`; 0 = unlimited.
    pub max_tcp_total: u32,
    /// `web.trusted_proxies`: exempt from the per-IP cap.
    pub trusted_proxies: CidrList,
    /// A request head must be complete this long after its first byte.
    pub header_timeout: Duration,
    /// An idle keep-alive connection is closed after this long.
    pub keep_alive_idle: Duration,
    /// Graceful-shutdown wait before connections are dropped.
    pub drain_timeout: Duration,
    /// Largest accepted request head.
    pub max_header_bytes: usize,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            max_tcp_per_ip: 64,
            max_tcp_total: 8192,
            trusted_proxies: CidrList::default(),
            header_timeout: Duration::from_secs(10),
            keep_alive_idle: Duration::from_secs(75),
            drain_timeout: Duration::from_secs(10),
            max_header_bytes: 64 * 1024,
        }
    }
}

/// Counts open connections per IP and in total.
#[derive(Debug, Default)]
pub struct ConnTracker {
    inner: Mutex<TrackerInner>,
}

#[derive(Debug, Default)]
struct TrackerInner {
    total: usize,
    per_ip: HashMap<IpAddr, usize>,
}

impl ConnTracker {
    /// Admits a connection from `peer` or says why not.
    fn admit(
        self: &Arc<Self>,
        peer: IpAddr,
        opts: &ServeOptions,
        telemetry: &Arc<HttpTelemetry>,
    ) -> Result<ConnGuard, RejectReason> {
        let key = limit_key(peer);
        let mut g = self.inner.lock();
        if opts.max_tcp_total > 0 && g.total >= opts.max_tcp_total as usize {
            return Err(RejectReason::Total);
        }
        let exempt = opts.trusted_proxies.contains(peer);
        let count = g.per_ip.get(&key).copied().unwrap_or(0);
        if opts.max_tcp_per_ip > 0 && !exempt && count >= opts.max_tcp_per_ip as usize {
            return Err(RejectReason::PerIp);
        }
        g.total += 1;
        *g.per_ip.entry(key).or_insert(0) += 1;
        telemetry.connection_opened();
        Ok(ConnGuard {
            tracker: Arc::clone(self),
            key,
            telemetry: Arc::clone(telemetry),
        })
    }

    /// Open connections (tests).
    pub fn total(&self) -> usize {
        self.inner.lock().total
    }
}

/// Releases the connection slot when the socket goes away.
#[derive(Debug)]
struct ConnGuard {
    tracker: Arc<ConnTracker>,
    key: IpAddr,
    telemetry: Arc<HttpTelemetry>,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        let mut g = self.tracker.inner.lock();
        g.total = g.total.saturating_sub(1);
        if let Some(n) = g.per_ip.get_mut(&self.key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                g.per_ip.remove(&self.key);
            }
        }
        self.telemetry.connection_closed();
    }
}

/// Activity of one connection, shared by the socket wrapper, the service and the watchdog.
#[derive(Debug)]
struct ConnState {
    base: Instant,
    /// ms since `base` of the last read or write that made progress.
    last_io_ms: AtomicU64,
    /// ms since `base` + 1 of the first byte of a request head that has not completed yet (0 = none).
    head_started: AtomicU64,
    /// Requests the service is still answering.
    inflight: AtomicUsize,
}

impl ConnState {
    fn new() -> Arc<ConnState> {
        Arc::new(ConnState {
            base: Instant::now(),
            last_io_ms: AtomicU64::new(0),
            head_started: AtomicU64::new(0),
            inflight: AtomicUsize::new(0),
        })
    }

    fn now(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn touch(&self) {
        self.last_io_ms.store(self.now(), Ordering::Relaxed);
    }

    fn bytes_read(&self) {
        let now = self.now();
        self.last_io_ms.store(now, Ordering::Relaxed);
        if self.inflight.load(Ordering::Relaxed) == 0 {
            // the first byte of a new request head starts the clock (a no-op while one is running)
            let _ = self.head_started.compare_exchange(
                0,
                now + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    fn request_started(&self) {
        self.head_started.store(0, Ordering::Relaxed);
        self.inflight.fetch_add(1, Ordering::Relaxed);
        self.touch();
    }

    fn request_finished(&self) {
        self.inflight.fetch_sub(1, Ordering::Relaxed);
        self.touch();
    }

    /// `Some(reason)` when the watchdog must close the connection.
    fn expired(&self, opts: &ServeOptions) -> Option<&'static str> {
        if self.inflight.load(Ordering::Relaxed) > 0 {
            return None;
        }
        let now = self.now();
        let head = self.head_started.load(Ordering::Relaxed);
        if head != 0 {
            return (now.saturating_sub(head - 1) > opts.header_timeout.as_millis() as u64)
                .then_some("request head timeout");
        }
        let idle = now.saturating_sub(self.last_io_ms.load(Ordering::Relaxed));
        (idle > opts.keep_alive_idle.as_millis() as u64).then_some("keep-alive idle timeout")
    }
}

/// The accepted socket: counts activity and owns the connection slot.
struct CountedIo {
    inner: TcpStream,
    state: Arc<ConnState>,
    _guard: ConnGuard,
}

impl AsyncRead for CountedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(r, Poll::Ready(Ok(()))) && buf.filled().len() > before {
            self.state.bytes_read();
        }
        r
    }
}

impl AsyncWrite for CountedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if matches!(r, Poll::Ready(Ok(n)) if n > 0) {
            self.state.touch();
        }
        r
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write_vectored(cx, bufs);
        if matches!(r, Poll::Ready(Ok(n)) if n > 0) {
            self.state.touch();
        }
        r
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// The per-connection tower service: adds `ConnectInfo` and tracks in-flight requests.
#[derive(Clone)]
struct ConnService {
    router: Router,
    peer: SocketAddr,
    state: Arc<ConnState>,
}

struct Inflight(Arc<ConnState>);

impl Drop for Inflight {
    fn drop(&mut self) {
        self.0.request_finished();
    }
}

impl tower::Service<Request<Incoming>> for ConnService {
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<Incoming>) -> Self::Future {
        req.extensions_mut().insert(ConnectInfo(self.peer));
        self.state.request_started();
        let guard = Inflight(Arc::clone(&self.state));
        let mut router = self.router.clone();
        Box::pin(async move {
            let r = tower::Service::call(&mut router, req).await;
            drop(guard);
            r
        })
    }
}

/// Sets `TCP_NODELAY` and `SO_KEEPALIVE` (60 s idle); failures are not fatal.
fn tune(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
    let _ = rustix::net::sockopt::set_socket_keepalive(stream, true);
    let _ = rustix::net::sockopt::set_tcp_keepidle(stream, Duration::from_secs(60));
}

/// Serves `router` on `listener` until `shutdown` completes; then drains (at most `opts.drain_timeout`) and
/// returns. `telemetry` counts connections and refusals.
pub async fn serve(
    listener: TcpListener,
    router: Router,
    opts: ServeOptions,
    telemetry: Arc<HttpTelemetry>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    let tracker = Arc::new(ConnTracker::default());
    let opts = Arc::new(opts);
    let (graceful_tx, graceful_rx) = watch::channel(false);
    let (abort_tx, abort_rx) = watch::channel(false);
    let live = Arc::new(AtomicUsize::new(0));
    let drained = Arc::new(tokio::sync::Notify::new());
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(x) => x,
                    Err(e) => {
                        // EMFILE and friends: back off instead of spinning
                        tracing::warn!(error = %e, "accept failed");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let guard = match tracker.admit(peer.ip().to_canonical(), &opts, &telemetry) {
                    Ok(g) => g,
                    Err(reason) => {
                        telemetry.conn_rejected(reason);
                        drop(stream);
                        continue;
                    }
                };
                tune(&stream);
                let state = ConnState::new();
                let io = CountedIo { inner: stream, state: Arc::clone(&state), _guard: guard };
                live.fetch_add(1, Ordering::AcqRel);
                let conn = serve_connection(
                    io,
                    peer,
                    router.clone(),
                    state,
                    Arc::clone(&opts),
                    graceful_rx.clone(),
                    abort_rx.clone(),
                );
                let live = Arc::clone(&live);
                let drained = Arc::clone(&drained);
                tokio::spawn(async move {
                    conn.await;
                    if live.fetch_sub(1, Ordering::AcqRel) == 1 {
                        drained.notify_waiters();
                    }
                });
            }
        }
    }
    // No new connections from here on.
    drop(listener);
    let _ = graceful_tx.send(true);
    let deadline = tokio::time::Instant::now() + opts.drain_timeout;
    while live.load(Ordering::Acquire) > 0 {
        let notified = drained.notified();
        tokio::pin!(notified);
        // register before re-checking so a wake-up between the check and the wait is not lost
        notified.as_mut().enable();
        if live.load(Ordering::Acquire) == 0 {
            break;
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            tracing::warn!(
                connections = live.load(Ordering::Acquire),
                "http drain timed out; dropping the remaining connections"
            );
            break;
        }
    }
    let _ = abort_tx.send(true);
}

/// Drives one connection to its end.
async fn serve_connection(
    io: CountedIo,
    peer: SocketAddr,
    router: Router,
    state: Arc<ConnState>,
    opts: Arc<ServeOptions>,
    mut graceful: watch::Receiver<bool>,
    mut abort: watch::Receiver<bool>,
) {
    let service = TowerToHyperService::new(ConnService {
        router,
        peer,
        state: Arc::clone(&state),
    });
    let mut builder = auto::Builder::new(TokioExecutor::new()).http1_only();
    builder
        .http1()
        .timer(TokioTimer::new())
        .max_buf_size(opts.max_header_bytes)
        .keep_alive(true);
    let conn = builder.serve_connection_with_upgrades(TokioIo::new(io), service);
    tokio::pin!(conn);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut graceful_started = false;
    loop {
        tokio::select! {
            r = conn.as_mut() => {
                if let Err(e) = r {
                    tracing::debug!(peer = %peer, error = %e, "connection ended with an error");
                }
                return;
            }
            _ = tick.tick() => {
                if let Some(why) = state.expired(&opts) {
                    tracing::debug!(peer = %peer, "closing connection: {why}");
                    return;
                }
            }
            changed = graceful.changed(), if !graceful_started => {
                if changed.is_ok() && *graceful.borrow() {
                    graceful_started = true;
                    conn.as_mut().graceful_shutdown();
                }
            }
            changed = abort.changed() => {
                if changed.is_ok() && *abort.borrow() {
                    return;
                }
            }
        }
    }
}
