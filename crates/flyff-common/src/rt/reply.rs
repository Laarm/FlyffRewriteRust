//! One-shot reply channels (`Reply<T>` / `ReplyRx<T>`, Plan/01 §4.3 C7).
//!
//! A [`Reply`] is the sending half carried inside a request; the requester keeps the [`ReplyRx`] and
//! waits synchronously (`recv`) or asynchronously (`recv_async`). `flume::bounded(1)` makes the send
//! non-blocking for the single reply and works across sync and async code without a runtime.

/// Sending half of a one-shot reply (by convention exactly one `send`).
#[derive(Debug)]
pub struct Reply<T>(flume::Sender<T>);

/// Receiving half of a one-shot reply.
#[derive(Debug)]
pub struct ReplyRx<T>(flume::Receiver<T>);

/// Error returned by receive calls when the [`Reply`] was dropped without sending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReplyError {
    /// The sender was dropped without sending a value.
    #[error("reply sender dropped without a value")]
    Disconnected,
    /// `recv_timeout` / `try_recv`: nothing arrived in time.
    #[error("no reply yet")]
    Empty,
}

/// Creates a connected pair.
pub fn reply_pair<T>() -> (Reply<T>, ReplyRx<T>) {
    let (tx, rx) = flume::bounded(1);
    (Reply(tx), ReplyRx(rx))
}

impl<T> Reply<T> {
    /// Sends the reply. Never blocks; returns the value back when the requester has gone away.
    pub fn send(self, value: T) -> Result<(), T> {
        self.0.try_send(value).map_err(|e| match e {
            flume::TrySendError::Full(v) | flume::TrySendError::Disconnected(v) => v,
        })
    }
}

impl<T> ReplyRx<T> {
    /// Blocks the calling thread until the reply arrives.
    pub fn recv(self) -> Result<T, ReplyError> {
        self.0.recv().map_err(|_| ReplyError::Disconnected)
    }

    /// Waits at most `timeout` for the reply.
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Result<T, ReplyError> {
        self.0.recv_timeout(timeout).map_err(|e| match e {
            flume::RecvTimeoutError::Timeout => ReplyError::Empty,
            flume::RecvTimeoutError::Disconnected => ReplyError::Disconnected,
        })
    }

    /// Non-blocking poll.
    pub fn try_recv(&self) -> Result<T, ReplyError> {
        self.0.try_recv().map_err(|e| match e {
            flume::TryRecvError::Empty => ReplyError::Empty,
            flume::TryRecvError::Disconnected => ReplyError::Disconnected,
        })
    }

    /// Awaits the reply (works under any executor).
    pub async fn recv_async(self) -> Result<T, ReplyError> {
        self.0
            .recv_async()
            .await
            .map_err(|_| ReplyError::Disconnected)
    }
}
