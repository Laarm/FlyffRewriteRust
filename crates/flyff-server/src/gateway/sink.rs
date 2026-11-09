//! The `SessionSink` of one WebSocket (Plan/10 §6.3, WP-NET-01): a bounded, byte-accounted outbound queue that
//! the writer task drains, plus the gate that holds game batches until the lobby has pushed `HelloAck`.
//!
//! One ordered queue carries **every** S→C frame of the socket (gateway frames and game batches), so frames
//! leave in the order they were pushed (G1/G4). The game thread never blocks: [`SessionSink::send_batch`] only
//! appends; when the queue holds more than `net.outbound_queue_frames` frames or `net.outbound_queue_bytes`
//! bytes the session is a slow consumer and the socket is closed with `4000 + REASON_SLOW_CONSUMER` without
//! draining (no `Kick` frame fits).

use bytes::Bytes;
use flyff_common::rt::{CloseReason, KickReason, SessionSink, SinkError};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;

/// How a game sink treats batches (10 §6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkMode {
    /// Batches go straight into the queue (`Join`).
    Open,
    /// Batches are held, under the same limits, until the lobby opens the gate (`Reattach`, 12 RA1–RA3), so the
    /// client always sees `HelloAck` before the first game batch.
    Gated,
}

/// WebSocket close code of a [`CloseReason`] (10 §7.7).
pub fn close_code(reason: CloseReason) -> u16 {
    match reason {
        CloseReason::Logout => 1000,
        CloseReason::Kicked(r) => r.close_code(),
        CloseReason::Replaced => 4005,
        CloseReason::ServerShutdown => 4009,
        CloseReason::ProtocolError => 4001,
        CloseReason::SlowConsumer => 4011,
        CloseReason::InternalError => 4010,
    }
}

/// WebSocket close code of `Kick{r}` (`4000 + r`, 11 §1.5).
pub fn kick_close_code(reason: KickReason) -> u16 {
    reason.close_code()
}

/// Transport close codes (not `CloseReason`s; 10 §7.7 last row).
pub mod transport {
    /// No frame of any kind within `net.ws_idle_timeout_s`.
    pub const GOING_AWAY: u16 = 1001;
    /// A client message above `net.max_client_msg_bytes`.
    pub const TOO_BIG: u16 = 1009;
    /// An unexpected server error.
    pub const INTERNAL: u16 = 1011;
}

/// What the writer sends next to the socket (the control path bypasses the data queue).
#[derive(Debug)]
pub(crate) enum Control {
    /// A WebSocket ping.
    Ping(Bytes),
}

/// Why and how the socket is closing (written once).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closing {
    /// The WebSocket close code.
    pub code: u16,
    /// The reason when it came from a [`CloseReason`] (kept for `LostCause`).
    pub reason: Option<CloseReason>,
    /// `true`: the writer sends every frame queued before the close, then the close frame.
    pub drain: bool,
}

/// State shared by the game's sink, the gateway's own pushes and the writer task.
pub struct SinkShared {
    tx: flume::Sender<Bytes>,
    queued_frames: AtomicUsize,
    queued_bytes: AtomicUsize,
    closed: AtomicBool,
    closing: Mutex<Option<Closing>>,
    close_notify: Notify,
    reader_notify: Notify,
    write_failed: AtomicBool,
    peer_gone: AtomicBool,
    gate: Mutex<Option<VecDeque<Bytes>>>,
    max_frames: usize,
    max_bytes: usize,
}

impl SinkShared {
    /// A queue with the given limits and its receiving end (for the writer).
    pub fn new(max_frames: usize, max_bytes: usize) -> (Arc<SinkShared>, flume::Receiver<Bytes>) {
        let (tx, rx) = flume::unbounded();
        let shared = Arc::new(SinkShared {
            tx,
            queued_frames: AtomicUsize::new(0),
            queued_bytes: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            closing: Mutex::new(None),
            close_notify: Notify::new(),
            reader_notify: Notify::new(),
            write_failed: AtomicBool::new(false),
            peer_gone: AtomicBool::new(false),
            gate: Mutex::new(None),
            max_frames: max_frames.max(1),
            max_bytes: max_bytes.max(1),
        });
        (shared, rx)
    }

    /// Appends one frame (gateway or game). Under the gate lock so the limits count gated and queued frames
    /// together. `Full` closes the socket as a slow consumer; `Closed` after any close.
    fn enqueue(&self, batch: Bytes, respect_gate: bool) -> Result<(), SinkError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SinkError::Closed);
        }
        let len = batch.len();
        let mut gate = self.gate.lock();
        if self.queued_frames.load(Ordering::Acquire) + 1 > self.max_frames
            || self.queued_bytes.load(Ordering::Acquire) + len > self.max_bytes
        {
            drop(gate);
            self.slow();
            return Err(SinkError::Full);
        }
        self.queued_frames.fetch_add(1, Ordering::AcqRel);
        self.queued_bytes.fetch_add(len, Ordering::AcqRel);
        match gate.as_mut() {
            Some(buf) if respect_gate => {
                buf.push_back(batch);
                Ok(())
            }
            _ => self.tx.send(batch).map_err(|_| SinkError::Closed),
        }
    }

    /// A gateway-originated frame (`HelloAck`, lobby messages, `ServerTime`, `Kick`): always straight into the
    /// queue, never gated, so it precedes every gated game batch (10 §6.3).
    pub fn push_gateway_frame(&self, frame: Bytes) -> Result<(), SinkError> {
        self.enqueue(frame, false)
    }

    /// Switches the gate on (`Reattach`: hold game batches). Idempotent.
    pub fn close_gate(&self) {
        let mut gate = self.gate.lock();
        if gate.is_none() {
            *gate = Some(VecDeque::new());
        }
    }

    /// Moves the gated frames, in order, behind everything already queued (e.g. `HelloAck`); afterwards the sink
    /// is open. Idempotent; a no-op for an open sink.
    pub fn open_gate(&self) {
        let mut gate = self.gate.lock();
        if let Some(buf) = gate.take() {
            for frame in buf {
                // The counters already include gated frames, so the limits held while they waited.
                if self.tx.send(frame).is_err() {
                    break;
                }
            }
        }
    }

    /// `true` while game batches are held.
    pub fn is_gated(&self) -> bool {
        self.gate.lock().is_some()
    }

    /// The writer calls this after each frame it wrote.
    pub(crate) fn frame_written(&self, len: usize) {
        self.queued_frames.fetch_sub(1, Ordering::AcqRel);
        self.queued_bytes.fetch_sub(len, Ordering::AcqRel);
    }

    /// Frames waiting (queued and gated) and their bytes.
    pub fn queued(&self) -> (usize, usize) {
        (
            self.queued_frames.load(Ordering::Acquire),
            self.queued_bytes.load(Ordering::Acquire),
        )
    }

    /// A slow consumer: close at once with `4000 + REASON_SLOW_CONSUMER`, no draining.
    fn slow(&self) {
        self.close_with(Closing {
            code: close_code(CloseReason::SlowConsumer),
            reason: Some(CloseReason::SlowConsumer),
            drain: false,
        });
    }

    /// Records the close (first call wins), refuses further frames and wakes the writer.
    pub fn close_with(&self, closing: Closing) {
        {
            let mut c = self.closing.lock();
            if c.is_none() {
                *c = Some(closing);
            }
        }
        self.closed.store(true, Ordering::Release);
        // A stored permit is not lost when the writer or the reader is busy: it is consumed at its next wait.
        self.close_notify.notify_one();
        self.reader_notify.notify_one();
    }

    /// The writer failed (a write error or timeout): the socket is closing without a goodbye and the reader
    /// must stop (`LostCause::WriteFailed`).
    pub(crate) fn writer_failed(&self) {
        self.write_failed.store(true, Ordering::Release);
        self.close_with(Closing {
            code: 1011,
            reason: None,
            drain: false,
        });
    }

    /// `true` when the writer gave up because the socket failed.
    pub fn has_write_failed(&self) -> bool {
        self.write_failed.load(Ordering::Acquire)
    }

    /// Resolves when the socket is closing (the reader's wake-up; the writer has its own).
    pub(crate) async fn closed_for_reader(&self) {
        self.reader_notify.notified().await;
    }

    /// Closes for a [`CloseReason`]: the frames queued before are written, then the close frame (a slow
    /// consumer closes without draining).
    pub fn close(&self, reason: CloseReason) {
        self.close_with(Closing {
            code: close_code(reason),
            reason: Some(reason),
            drain: reason != CloseReason::SlowConsumer,
        });
    }

    /// The recorded close, if any.
    pub fn closing(&self) -> Option<Closing> {
        *self.closing.lock()
    }

    /// The session task noticed the end of the socket (it is now waiting for a lobby task or reporting the
    /// loss). The enter task of 12 §11.1 checks it before it sends `Join`, so a socket that closed during the
    /// flush wait never gets an avatar.
    pub fn set_peer_gone(&self) {
        self.peer_gone.store(true, Ordering::Release);
    }

    /// `true` once [`SinkShared::set_peer_gone`] was called.
    pub fn peer_gone(&self) -> bool {
        self.peer_gone.load(Ordering::Acquire)
    }

    /// `true` after any close.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Resolves when a close was requested (the writer's wake-up).
    pub(crate) async fn close_requested(&self) {
        self.close_notify.notified().await;
    }
}

/// The game's [`SessionSink`] (10 §6.3): `send_batch` appends to the socket's queue, `close` asks the writer to
/// close after the queued frames.
#[derive(Clone)]
pub struct WsSink {
    shared: Arc<SinkShared>,
}

impl WsSink {
    /// A sink over `shared`. `Gated` switches the gate on, `Open` switches it off (a gated sink that was
    /// dropped without being opened must not hold the next one back).
    pub fn new(shared: Arc<SinkShared>, mode: SinkMode) -> WsSink {
        match mode {
            SinkMode::Open => shared.open_gate(),
            SinkMode::Gated => shared.close_gate(),
        }
        WsSink { shared }
    }

    /// The shared queue state.
    pub fn shared(&self) -> &Arc<SinkShared> {
        &self.shared
    }
}

impl SessionSink for WsSink {
    fn send_batch(&self, batch: Bytes) -> Result<(), SinkError> {
        self.shared.enqueue(batch, true)
    }

    fn close(&self, reason: CloseReason) {
        self.shared.close(reason);
    }
}
