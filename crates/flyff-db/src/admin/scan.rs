//! The admin-read framework (Plan/25 §11.6 "Fairness"): chunking, queueing behind lobby work, the admin reader
//! and the heavy-scan semaphore.
//!
//! # Model
//!
//! An admin read is an [`AdminScan`]: a resumable cursor that does a **bounded** amount of work per
//! [`AdminScan::step`] (at most [`ChunkLimits::rows`] rows or [`ChunkLimits::time`], whichever ends first; the
//! plan's bounds are 500 rows and 50 ms) and says whether it is done. [`PersistHandle::admin_scan`] queues it
//! and returns a [`ScanStream`] that delivers the rows chunk by chunk with a progress counter, can be cancelled
//! (explicitly or by dropping it) and collects into a `Vec` for the one-shot reads.
//!
//! Between two chunks a scan goes to the **back** of the admin queue, so other admin reads interleave with it,
//! and the reader threads serve lobby and game reads first:
//!
//! * `[database] read_connections >= 2`: one **additional** connection and thread is the **admin reader**
//!   (`flyff-persist-ro-admin`). It serves only admin chunks, so a whole-database scan never delays a login;
//!   the `read_connections` other threads serve lobby reads, game reads and backups exactly as before.
//! * `read_connections = 1`: the single reader takes lobby/game work first and runs one admin chunk only when
//!   no such work is waiting (after [`ADMIN_TURN_AFTER`] consecutive lobby commands it takes an admin chunk
//!   anyway, so a busy lobby cannot starve the console). A lobby read therefore waits for at most the chunk that
//!   is running, i.e. [`ChunkLimits::time`].
//!
//! **Heavy scans** ([`AdminScan::heavy`], the whole-database passes: item search, duplicate serials, economy
//! summary) hold the server-wide permit: a second heavy scan is refused with `Busy` while one runs.
//!
//! # Writing a read
//!
//! Implement [`AdminScan`] (or use [`KeysetScan`], the id-ordered walker the simple reads are built from):
//! keep the cursor in `self`, ask SQLite for at most `budget.max_rows()` rows per step (`LIMIT`), check
//! `budget.expired()` per row when the work per row is CPU-heavy, push the results into `out` and return
//! [`Progress`]. A scan must cap its own output (a runaway result is the scan's bug, not the framework's).

use super::model::invalid;
use crate::error::{DbError, DbErrorKind};
use crate::handle::{ClosedSignal, DbStats, PersistHandle};
use rusqlite::Connection;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Upper bound of the rows of one chunk (Plan/25 §11.6).
pub const CHUNK_ROWS_MAX: usize = 500;
/// Upper bound of the time of one chunk (Plan/25 §11.6).
pub const CHUNK_TIME_MAX: Duration = Duration::from_millis(50);
/// With one reader thread, an admin chunk runs after this many consecutive lobby/game commands even when more
/// are waiting (no starvation of the console under a busy lobby).
pub const ADMIN_TURN_AFTER: u32 = 32;
/// At most this many admin reads (scans in flight) are queued; more answer `Busy`.
pub const ADMIN_QUEUE_MAX: usize = 256;

/// The budget of one chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkLimits {
    rows: usize,
    time: Duration,
}

impl ChunkLimits {
    /// 500 rows or 5 ms (a tenth of the plan's time bound): with one reader thread a lobby read waits at most
    /// 5 ms for the chunk that is running, so a login of three sequential reads waits 15 ms in the worst case.
    pub const DEFAULT: ChunkLimits = ChunkLimits {
        rows: CHUNK_ROWS_MAX,
        time: Duration::from_millis(5),
    };

    /// Limits of `rows` and `time`, clamped to `1..=`[`CHUNK_ROWS_MAX`] and `1 ms..=`[`CHUNK_TIME_MAX`].
    pub fn new(rows: usize, time: Duration) -> ChunkLimits {
        ChunkLimits {
            rows: rows.clamp(1, CHUNK_ROWS_MAX),
            time: time.clamp(Duration::from_millis(1), CHUNK_TIME_MAX),
        }
    }

    /// Rows per chunk.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Time per chunk.
    pub fn time(&self) -> Duration {
        self.time
    }
}

impl Default for ChunkLimits {
    fn default() -> Self {
        ChunkLimits::DEFAULT
    }
}

/// What one chunk may still spend: handed to [`AdminScan::step`].
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    limits: ChunkLimits,
    started: Instant,
}

impl Budget {
    /// Starts the clock of a chunk.
    pub fn start(limits: ChunkLimits) -> Budget {
        Budget {
            limits,
            started: Instant::now(),
        }
    }

    /// The most rows the chunk may read (use it as the `LIMIT` of the statement).
    pub fn max_rows(&self) -> usize {
        self.limits.rows
    }

    /// `true` once the chunk used its time: stop after the current row and return [`Progress::more`].
    pub fn expired(&self) -> bool {
        self.started.elapsed() >= self.limits.time
    }

    /// Time spent in this chunk so far.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// What a chunk reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// The scan has no more work after this chunk.
    pub done: bool,
    /// Rows (characters, mails, ...) examined by **this** chunk; the stream sums them.
    pub scanned: u64,
    /// Total rows to examine when the scan knows (a progress bar), else `None`.
    pub total: Option<u64>,
}

impl Progress {
    /// A chunk that examined `scanned` rows and wants to be resumed.
    pub fn more(scanned: u64) -> Progress {
        Progress {
            done: false,
            scanned,
            total: None,
        }
    }

    /// The last chunk: it examined `scanned` rows.
    pub fn finished(scanned: u64) -> Progress {
        Progress {
            done: true,
            scanned,
            total: None,
        }
    }

    /// The same progress with a known total.
    #[must_use]
    pub fn with_total(mut self, total: u64) -> Progress {
        self.total = Some(total);
        self
    }
}

/// A resumable read (module docs).
pub trait AdminScan: Send + 'static {
    /// One streamed row.
    type Item: Send + 'static;

    /// `true` for a whole-database pass: it holds the server-wide heavy permit while it runs.
    fn heavy(&self) -> bool {
        false
    }

    /// Does one bounded chunk of work on `conn` (a `query_only` reader connection), appends the rows found to
    /// `out` and reports the progress. Returning `Err` ends the scan (the stream delivers it as
    /// `ScanEvent::Failed`).
    fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        out: &mut Vec<Self::Item>,
    ) -> Result<Progress, DbError>;
}

impl<S: AdminScan + ?Sized> AdminScan for Box<S> {
    type Item = S::Item;

    fn heavy(&self) -> bool {
        (**self).heavy()
    }

    fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        out: &mut Vec<Self::Item>,
    ) -> Result<Progress, DbError> {
        (**self).step(conn, budget, out)
    }
}

/// Maps the items of a scan ([`AdminScanExt::mapped`]).
pub struct Mapped<S, F> {
    inner: S,
    f: F,
}

impl<S, T, F> AdminScan for Mapped<S, F>
where
    S: AdminScan,
    T: Send + 'static,
    F: Fn(S::Item) -> T + Send + 'static,
{
    type Item = T;

    fn heavy(&self) -> bool {
        self.inner.heavy()
    }

    fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        out: &mut Vec<T>,
    ) -> Result<Progress, DbError> {
        let mut raw = Vec::new();
        let progress = self.inner.step(conn, budget, &mut raw)?;
        out.extend(raw.into_iter().map(&self.f));
        Ok(progress)
    }
}

/// Adapters for scans.
pub trait AdminScanExt: AdminScan + Sized {
    /// Applies `f` to every item.
    fn mapped<T, F>(self, f: F) -> Mapped<Self, F>
    where
        T: Send + 'static,
        F: Fn(Self::Item) -> T + Send + 'static,
    {
        Mapped { inner: self, f }
    }
}

impl<S: AdminScan> AdminScanExt for S {}

// ---- the keyset walker ---------------------------------------------------------------------------------------

/// Decodes one row of a [`KeysetScan`]; `Ok(None)` skips an unreadable row (the decoder logs it).
pub type KeysetDecode<T> = fn(&rusqlite::Row<'_>) -> Result<Option<T>, DbError>;

/// A scan that walks one statement in id order with a keyset cursor. The statement must be
/// `SELECT <id>, ... WHERE <id> > ?1 [AND ...] ORDER BY <id> LIMIT ?2` (ascending) or `<id> < ?1 ... ORDER BY
/// <id> DESC LIMIT ?2` (descending); column 0 is the cursor id, `?3...` are the extra parameters in `params`.
/// Each chunk reads at most `budget.max_rows()` rows; the walk ends when the statement runs dry or `max_items`
/// items were produced.
pub struct KeysetScan<T> {
    sql: String,
    params: Vec<rusqlite::types::Value>,
    cursor: i64,
    max_items: Option<usize>,
    produced: usize,
    decode: KeysetDecode<T>,
}

impl<T: Send + 'static> KeysetScan<T> {
    /// An ascending walk from the start of the table (`id > 0`).
    pub fn ascending(
        sql: impl Into<String>,
        params: Vec<rusqlite::types::Value>,
        decode: KeysetDecode<T>,
    ) -> Self {
        Self {
            sql: sql.into(),
            params,
            cursor: 0,
            max_items: None,
            produced: 0,
            decode,
        }
    }

    /// A descending walk from the newest row (`id < i64::MAX`) or from `before` when given.
    pub fn descending(
        sql: impl Into<String>,
        params: Vec<rusqlite::types::Value>,
        before: Option<i64>,
        decode: KeysetDecode<T>,
    ) -> Self {
        Self {
            sql: sql.into(),
            params,
            cursor: before.unwrap_or(i64::MAX),
            max_items: None,
            produced: 0,
            decode,
        }
    }

    /// Ends the walk after `n` produced items (a page).
    #[must_use]
    pub fn limited(mut self, n: usize) -> Self {
        self.max_items = Some(n);
        self
    }
}

impl<T: Send + 'static> AdminScan for KeysetScan<T> {
    type Item = T;

    fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        out: &mut Vec<T>,
    ) -> Result<Progress, DbError> {
        let wanted = match self.max_items {
            Some(max) => max.saturating_sub(self.produced),
            None => usize::MAX,
        };
        if wanted == 0 {
            return Ok(Progress::finished(0));
        }
        let cap = budget.max_rows().min(wanted);
        let mut args: Vec<rusqlite::types::Value> = Vec::with_capacity(2 + self.params.len());
        args.push(rusqlite::types::Value::Integer(self.cursor));
        args.push(rusqlite::types::Value::Integer(
            i64::try_from(cap).unwrap_or(i64::MAX),
        ));
        args.extend(self.params.iter().cloned());
        let mut stmt = conn.prepare_cached(&self.sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(args))?;
        let mut read = 0_usize;
        let mut cut_short = false;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            self.cursor = id;
            read += 1;
            if let Some(item) = (self.decode)(row)? {
                out.push(item);
                self.produced += 1;
            }
            if budget.expired() {
                cut_short = true;
                break;
            }
        }
        let exhausted = !cut_short && read < cap;
        let page_full = self.max_items.is_some_and(|max| self.produced >= max);
        let scanned = u64::try_from(read).unwrap_or(u64::MAX);
        Ok(if exhausted || page_full {
            Progress::finished(scanned)
        } else {
            Progress::more(scanned)
        })
    }
}

/// A scan that runs one closure in one chunk (point reads and small lists that are bounded by construction).
pub struct Once<F, T> {
    f: Option<F>,
    _item: std::marker::PhantomData<fn() -> T>,
}

impl<F, T> Once<F, T>
where
    F: FnOnce(&Connection) -> Result<Vec<T>, DbError> + Send + 'static,
    T: Send + 'static,
{
    /// Wraps `f`.
    pub fn new(f: F) -> Self {
        Self {
            f: Some(f),
            _item: std::marker::PhantomData,
        }
    }
}

impl<F, T> AdminScan for Once<F, T>
where
    F: FnOnce(&Connection) -> Result<Vec<T>, DbError> + Send + 'static,
    T: Send + 'static,
{
    type Item = T;

    fn step(
        &mut self,
        conn: &Connection,
        _budget: &Budget,
        out: &mut Vec<T>,
    ) -> Result<Progress, DbError> {
        let Some(f) = self.f.take() else {
            return Ok(Progress::finished(0));
        };
        let items = f(conn)?;
        let n = u64::try_from(items.len()).unwrap_or(u64::MAX);
        out.extend(items);
        Ok(Progress::finished(n))
    }
}

// ---- the heavy-scan permit -----------------------------------------------------------------------------------

/// The server-wide "one heavy admin scan at a time" gate.
#[derive(Default)]
pub(crate) struct HeavyGate {
    busy: AtomicBool,
}

impl HeavyGate {
    pub(crate) fn try_acquire(self: &Arc<Self>) -> Option<HeavyPermit> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| HeavyPermit(Arc::clone(self)))
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

/// Held by a running heavy scan; released when the scan finishes, fails, is cancelled or is dropped.
pub(crate) struct HeavyPermit(Arc<HeavyGate>);

impl Drop for HeavyPermit {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

// ---- jobs on the reader threads ------------------------------------------------------------------------------

/// What the reader thread passes to a job: the way to re-queue itself, the budget and the counters.
pub(crate) struct AdminCtx<'a> {
    pub(crate) requeue: &'a dyn Fn(AdminJob),
    pub(crate) limits: ChunkLimits,
    pub(crate) stats: &'a DbStats,
}

/// One queued chunk of an admin scan.
pub(crate) type AdminJob = Box<dyn FnOnce(&Connection, &AdminCtx<'_>) + Send>;

/// What a [`ScanStream`] delivers.
#[derive(Debug)]
pub enum ScanEvent<T> {
    /// Rows found by a chunk, with the progress so far (`scanned` is cumulative).
    Rows {
        /// The rows (possibly none: a progress tick).
        items: Vec<T>,
        /// Rows examined so far.
        scanned: u64,
        /// Total rows to examine, when the scan knows.
        total: Option<u64>,
    },
    /// The scan ended; no event follows.
    Done {
        /// Rows examined in total.
        scanned: u64,
    },
    /// The scan failed (or the persistence layer shut down); no event follows.
    Failed(DbError),
}

/// The heavy permit of a scan, shared between the running scan and its stream so that cancelling (or dropping)
/// the stream frees it **at once** (the scan itself notices at its next chunk boundary).
type SharedPermit = Arc<std::sync::Mutex<Option<HeavyPermit>>>;

fn release(permit: &SharedPermit) {
    let taken = permit
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    drop(taken);
}

struct ScanRun<S: AdminScan> {
    scan: S,
    tx: flume::Sender<ScanEvent<S::Item>>,
    cancel: Arc<AtomicBool>,
    permit: SharedPermit,
    scanned: u64,
}

impl<S: AdminScan> ScanRun<S> {
    /// Ends the run: the scan and the heavy permit are released **before** the final event is delivered, so a
    /// consumer that starts the next heavy scan the moment it sees `Done` / `Failed` is never refused with `Busy`
    /// because of the scan that just ended.
    fn end(self, event: ScanEvent<S::Item>) {
        let ScanRun {
            scan, tx, permit, ..
        } = self;
        release(&permit);
        drop(scan);
        let _ = tx.send(event);
    }
}

fn run_chunk<S: AdminScan>(mut run: Box<ScanRun<S>>, conn: &Connection, ctx: &AdminCtx<'_>) {
    if run.cancel.load(Ordering::Acquire) || run.tx.is_disconnected() {
        ctx.stats
            .admin_scans_cancelled
            .fetch_add(1, Ordering::Relaxed);
        return;
    }
    let budget = Budget::start(ctx.limits);
    let mut items = Vec::new();
    let stepped = catch_unwind(AssertUnwindSafe(|| {
        run.scan.step(conn, &budget, &mut items)
    }));
    ctx.stats.admin_chunks.fetch_add(1, Ordering::Relaxed);
    let progress = match stepped {
        Ok(Ok(progress)) => progress,
        Ok(Err(e)) => {
            ctx.stats.admin_scans_failed.fetch_add(1, Ordering::Relaxed);
            (*run).end(ScanEvent::Failed(e));
            return;
        }
        Err(_) => {
            tracing::error!("an admin scan panicked in step()");
            ctx.stats.admin_scans_failed.fetch_add(1, Ordering::Relaxed);
            (*run).end(ScanEvent::Failed(DbError::new(
                DbErrorKind::Bug,
                "the admin scan panicked",
            )));
            return;
        }
    };
    run.scanned = run.scanned.saturating_add(progress.scanned);
    ctx.stats
        .admin_rows
        .fetch_add(progress.scanned, Ordering::Relaxed);
    if !items.is_empty() || !progress.done {
        let _ = run.tx.send(ScanEvent::Rows {
            items,
            scanned: run.scanned,
            total: progress.total,
        });
    }
    if progress.done {
        ctx.stats.admin_scans.fetch_add(1, Ordering::Relaxed);
        let scanned = run.scanned;
        (*run).end(ScanEvent::Done { scanned });
        return;
    }
    // Behind every other admin read that is already queued (and, on the shared reader, behind lobby work).
    (ctx.requeue)(Box::new(move |c, x| run_chunk(run, c, x)));
}

// ---- the stream ------------------------------------------------------------------------------------------------

/// The consumer end of an admin scan. Dropping it cancels the scan (the next chunk does not run) and frees its
/// heavy permit at once.
pub struct ScanStream<T> {
    rx: flume::Receiver<ScanEvent<T>>,
    cancel: Arc<AtomicBool>,
    permit: SharedPermit,
    closed: Option<ClosedSignal>,
    finished: bool,
}

/// Result of waiting for the next event.
enum Waited<T> {
    Event(ScanEvent<T>),
    Gone,
    TimedOut,
}

impl<T: Send + 'static> ScanStream<T> {
    /// Asks the scan to stop at its next chunk boundary and frees its heavy permit **at once** (a new heavy scan
    /// may start immediately; it overlaps the cancelled one for at most the chunk that is running). The events
    /// already produced can still be received; a cancelled scan sends no final event, it just ends.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        release(&self.permit);
    }

    /// `true` once the final event (`Done` or `Failed`) was received.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn finish_with(&mut self, event: Waited<T>) -> Option<ScanEvent<T>> {
        match event {
            Waited::Event(e) => {
                if matches!(e, ScanEvent::Done { .. } | ScanEvent::Failed(_)) {
                    self.finished = true;
                }
                Some(e)
            }
            Waited::Gone => {
                self.finished = true;
                // Cancelled by us: end quietly; otherwise the reader threads are gone.
                if self.cancel.load(Ordering::Acquire) {
                    None
                } else {
                    Some(ScanEvent::Failed(DbError::shutting_down()))
                }
            }
            Waited::TimedOut => None,
        }
    }

    fn wait(&self, deadline: Option<Instant>) -> Waited<T> {
        const POLL: Duration = Duration::from_millis(25);
        loop {
            let slice = match deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Waited::TimedOut;
                    }
                    left.min(POLL)
                }
                None => POLL,
            };
            match self.rx.recv_timeout(slice) {
                Ok(event) => return Waited::Event(event),
                Err(flume::RecvTimeoutError::Disconnected) => return Waited::Gone,
                Err(flume::RecvTimeoutError::Timeout) => {
                    if self
                        .closed
                        .as_ref()
                        .is_some_and(flume::Receiver::is_disconnected)
                    {
                        return match self.rx.try_recv() {
                            Ok(event) => Waited::Event(event),
                            Err(_) => Waited::Gone,
                        };
                    }
                }
            }
        }
    }

    /// The next event, waiting as long as it takes; `None` after the final event.
    pub fn recv_blocking(&mut self) -> Option<ScanEvent<T>> {
        if self.finished {
            return None;
        }
        let waited = self.wait(None);
        self.finish_with(waited)
    }

    /// Like [`ScanStream::recv_blocking`] but gives up after `timeout`: `Err(Timeout)`.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<ScanEvent<T>>, DbError> {
        if self.finished {
            return Ok(None);
        }
        match self.wait(Some(Instant::now() + timeout)) {
            Waited::TimedOut => Err(DbError::new(
                DbErrorKind::Timeout,
                "no admin scan event in time",
            )),
            other => Ok(self.finish_with(other)),
        }
    }

    /// The next event; `None` after the final event.
    pub async fn recv(&mut self) -> Option<ScanEvent<T>> {
        use std::future::Future;
        use std::task::Poll;
        if self.finished {
            return None;
        }
        let waited = {
            let mut next = std::pin::pin!(self.rx.recv_async());
            let mut gone = self.closed.as_ref().map(|c| Box::pin(c.recv_async()));
            std::future::poll_fn(|cx| {
                if let Poll::Ready(v) = next.as_mut().poll(cx) {
                    return Poll::Ready(match v {
                        Ok(event) => Waited::Event(event),
                        Err(_) => Waited::Gone,
                    });
                }
                if let Some(signal) = gone.as_mut()
                    && signal.as_mut().poll(cx).is_ready()
                {
                    // A final event may have raced the shutdown.
                    return Poll::Ready(match self.rx.try_recv() {
                        Ok(event) => Waited::Event(event),
                        Err(_) => Waited::Gone,
                    });
                }
                Poll::Pending
            })
            .await
        };
        self.finish_with(waited)
    }

    /// Receives until the end and returns every row (`Err` for a failed or interrupted scan).
    pub fn collect_blocking(mut self) -> Result<Vec<T>, DbError> {
        let mut all = Vec::new();
        while let Some(event) = self.recv_blocking() {
            match event {
                ScanEvent::Rows { items, .. } => all.extend(items),
                ScanEvent::Done { .. } => return Ok(all),
                ScanEvent::Failed(e) => return Err(e),
            }
        }
        Err(DbError::new(
            DbErrorKind::Bug,
            "the admin scan ended without a result",
        ))
    }

    /// Async variant of [`ScanStream::collect_blocking`].
    pub async fn collect(mut self) -> Result<Vec<T>, DbError> {
        let mut all = Vec::new();
        while let Some(event) = self.recv().await {
            match event {
                ScanEvent::Rows { items, .. } => all.extend(items),
                ScanEvent::Done { .. } => return Ok(all),
                ScanEvent::Failed(e) => return Err(e),
            }
        }
        Err(DbError::new(
            DbErrorKind::Bug,
            "the admin scan ended without a result",
        ))
    }
}

impl<T> Drop for ScanStream<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        release(&self.permit);
    }
}

impl PersistHandle {
    /// Queues `scan` on the admin reader and returns its stream (module docs). `Busy` when the scan is heavy and
    /// another heavy scan runs, or when [`ADMIN_QUEUE_MAX`] scans are in flight; `ShuttingDown` for a detached
    /// handle or after the reader threads ended.
    pub fn admin_scan<S: AdminScan>(&self, scan: S) -> Result<ScanStream<S::Item>, DbError> {
        let Some(admin_tx) = &self.admin_tx else {
            return Err(DbError::shutting_down());
        };
        if admin_tx.len() >= ADMIN_QUEUE_MAX {
            return Err(DbError::new(
                DbErrorKind::Busy,
                "too many admin reads are queued",
            ));
        }
        let permit = if scan.heavy() {
            match self.heavy.try_acquire() {
                Some(permit) => Some(permit),
                None => {
                    self.stats
                        .admin_scans_rejected
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(DbError::new(
                        DbErrorKind::Busy,
                        "another heavy admin scan is running",
                    ));
                }
            }
        } else {
            None
        };
        let (tx, rx) = flume::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let permit: SharedPermit = Arc::new(std::sync::Mutex::new(permit));
        let run = Box::new(ScanRun {
            scan,
            tx,
            cancel: Arc::clone(&cancel),
            permit: Arc::clone(&permit),
            scanned: 0,
        });
        admin_tx
            .send(Box::new(move |c, x| run_chunk(run, c, x)))
            .map_err(|_| DbError::shutting_down())?;
        Ok(ScanStream {
            rx,
            cancel,
            permit,
            closed: self.readers_closed.clone(),
            finished: false,
        })
    }

    /// `true` while a heavy admin scan runs (the console shows it; a second one would be refused).
    pub fn admin_heavy_scan_running(&self) -> bool {
        self.heavy.is_busy()
    }

    /// Runs `scan` to the end and returns its rows (`await`).
    pub async fn admin_collect<S: AdminScan>(&self, scan: S) -> Result<Vec<S::Item>, DbError> {
        self.admin_scan(scan)?.collect().await
    }

    /// Blocking variant of [`PersistHandle::admin_collect`] for threads outside the async runtime.
    pub fn admin_collect_blocking<S: AdminScan>(&self, scan: S) -> Result<Vec<S::Item>, DbError> {
        self.admin_scan(scan)?.collect_blocking()
    }
}

/// Refuses a page size outside `1..=`[`CHUNK_ROWS_MAX`] (the paged reads never return more than one chunk).
pub(crate) fn page_limit(limit: u32) -> Result<usize, DbError> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    if (1..=CHUNK_ROWS_MAX).contains(&limit) {
        Ok(limit)
    } else {
        Err(invalid(format!(
            "a page holds 1..={CHUNK_ROWS_MAX} rows, not {limit}"
        )))
    }
}
