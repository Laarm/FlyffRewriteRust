//! The asset-preparation thread pool (Plan/24 §7.4, Plan/01 §4.2).
//!
//! * `N = runtime.asset_threads` (0 = `max(1, cores - 2)`) std threads named `flyff-asset-<i>`, each
//!   lowering its CPU niceness (`runtime.asset_nice`, default +10) and, best effort, its I/O priority.
//!   Never the tokio blocking pool (D2).
//! * One `Mutex` + `Condvar` guard three FIFO queues. A worker always takes from the highest non-empty
//!   priority (`Startup > Interactive > Prewarm`). **Prewarm jobs are not started while any Interactive
//!   job is queued or running** (Plan/01 §4.5).
//! * A queued job can be *promoted* to a higher queue by id ([`PoolHandle::promote`]); the in-flight
//!   table of `AssetPrep` uses that when an Interactive request asks for a job that is only queued for
//!   Prewarm.
//! * Start-up work runs on a temporary set of un-niced workers ([`AssetPool::start_startup_workers`],
//!   named `flyff-startup-<i>`); while one exists the niced workers leave `Startup` jobs to it.
//! * Every job runs under `catch_unwind`: a panicking builder never takes a worker down.
//!
//! The public surface that existed before the framework (`AssetPool::{start, threads, submit, shutdown}`)
//! is unchanged: `submit` queues an anonymous closure and returns a [`ReplyRx`].

use crate::clock::Clock;
use crate::kinds::Priority;
use crate::util::{lock, panic_message};
use flyff_common::rt::{ReplyRx, reply_pair};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Identifies a queued job for promotion and for dropping at shutdown.
pub type JobId = u64;

/// Errors of the pool.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PoolError {
    /// The pool was shut down; the job was not queued.
    #[error("asset pool is shut down")]
    ShutDown,
    /// A worker thread could not be spawned.
    #[error("cannot spawn asset worker: {0}")]
    Spawn(#[from] std::io::Error),
}

#[derive(Default)]
struct Queues {
    q: [VecDeque<(JobId, Job)>; 3],
    /// Jobs running, by the queue class they were started from.
    running: [usize; 3],
    closed: bool,
    /// Live temporary start-up workers.
    startup_workers: usize,
    /// Live workers of every kind.
    alive: usize,
    next_id: JobId,
}

#[derive(Clone)]
enum Class {
    Niced,
    Startup(Arc<AtomicBool>),
}

impl Queues {
    fn pop(&mut self, class: &Class) -> Option<(JobId, Job, usize)> {
        let take =
            |q: &mut VecDeque<(JobId, Job)>, i: usize| q.pop_front().map(|(id, j)| (id, j, i));
        match class {
            Class::Startup(_) => self.q.first_mut().and_then(|q| take(q, 0)),
            Class::Niced => {
                if self.startup_workers == 0
                    && let Some(x) = self.q.first_mut().and_then(|q| take(q, 0))
                {
                    return Some(x);
                }
                if let Some(x) = self.q.get_mut(1).and_then(|q| take(q, 1)) {
                    return Some(x);
                }
                // Prewarm only while no Interactive job is queued (it is not, here) or running.
                if self.running.get(1).copied().unwrap_or(0) == 0 {
                    return self.q.get_mut(2).and_then(|q| take(q, 2));
                }
                None
            }
        }
    }

    fn queued(&self, i: usize) -> usize {
        self.q.get(i).map_or(0, VecDeque::len)
    }

    fn should_exit(&self, class: &Class) -> bool {
        match class {
            Class::Startup(stop) => {
                (stop.load(Ordering::Acquire) || self.closed) && self.queued(0) == 0
            }
            Class::Niced => {
                self.closed
                    && self.queued(1) == 0
                    && (self.queued(0) == 0 || self.startup_workers > 0)
            }
        }
    }
}

struct Shared {
    queues: Mutex<Queues>,
    ready: Condvar,
    exited: Condvar,
}

/// A cheap, clonable handle to the queues: submit, promote, inspect. It does not own the threads.
#[derive(Clone)]
pub struct PoolHandle {
    shared: Arc<Shared>,
}

impl PoolHandle {
    /// A fresh job id.
    pub fn next_id(&self) -> JobId {
        let mut q = lock(&self.shared.queues);
        q.next_id += 1;
        q.next_id
    }

    /// Queues `job` under `id` at priority `prio`.
    pub fn submit_keyed(
        &self,
        id: JobId,
        prio: Priority,
        job: impl FnOnce() + Send + 'static,
    ) -> Result<(), PoolError> {
        let mut q = lock(&self.shared.queues);
        if q.closed {
            return Err(PoolError::ShutDown);
        }
        if let Some(queue) = q.q.get_mut(prio.index()) {
            queue.push_back((id, Box::new(job)));
        }
        drop(q);
        // Workers differ (niced vs start-up): `notify_one` could wake one that may not take this job.
        self.shared.ready.notify_all();
        Ok(())
    }

    /// Moves the still-queued job `id` to the back of the higher-priority queue `to`. Returns `false`
    /// when the job is not queued (already running or finished) or already at least as urgent.
    pub fn promote(&self, id: JobId, to: Priority) -> bool {
        let mut q = lock(&self.shared.queues);
        let Some(from) = (to.index() + 1..3).find(|&i| {
            q.q.get(i)
                .is_some_and(|queue| queue.iter().any(|(jid, _)| *jid == id))
        }) else {
            return false;
        };
        let moved = q.q.get_mut(from).and_then(|queue| {
            queue
                .iter()
                .position(|(jid, _)| *jid == id)
                .and_then(|pos| queue.remove(pos))
        });
        let Some(entry) = moved else { return false };
        if let Some(queue) = q.q.get_mut(to.index()) {
            queue.push_back(entry);
        }
        drop(q);
        self.shared.ready.notify_all();
        true
    }

    /// Removes every queued job of priority `prio` and returns their ids (shutdown drops Prewarm).
    pub fn drop_queued(&self, prio: Priority) -> Vec<JobId> {
        let dropped: Vec<(JobId, Job)> = {
            let mut q = lock(&self.shared.queues);
            q.q.get_mut(prio.index())
                .map(|queue| queue.drain(..).collect())
                .unwrap_or_default()
        };
        // The closures (and what they capture) are dropped outside the lock.
        dropped.into_iter().map(|(id, _)| id).collect()
    }

    /// Queued jobs per priority (`[startup, interactive, prewarm]`).
    pub fn depth(&self) -> [usize; 3] {
        let q = lock(&self.shared.queues);
        std::array::from_fn(|i| q.queued(i))
    }

    /// Calls `f` with the queue depths while the queue lock is held, so a value published from `f`
    /// can never be older than a later publication (telemetry gauges).
    pub fn with_depth<R>(&self, f: impl FnOnce([usize; 3]) -> R) -> R {
        let q = lock(&self.shared.queues);
        f(std::array::from_fn(|i| q.queued(i)))
    }

    /// Running jobs per priority class.
    pub fn running(&self) -> [usize; 3] {
        lock(&self.shared.queues).running
    }

    /// `true` once the pool no longer accepts jobs.
    pub fn is_closed(&self) -> bool {
        lock(&self.shared.queues).closed
    }
}

/// A bounded pool of named worker threads running at a lowered scheduling priority.
pub struct AssetPool {
    shared: Arc<Shared>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    threads: usize,
}

impl AssetPool {
    /// Starts `threads` workers (at least 1) named `flyff-asset-N`, each lowering its own nice value
    /// by `nice` (best effort: a failure is logged at debug level and ignored).
    pub fn start(threads: usize, nice: i32) -> Result<AssetPool, PoolError> {
        let shared = Arc::new(Shared {
            queues: Mutex::new(Queues::default()),
            ready: Condvar::new(),
            exited: Condvar::new(),
        });
        let n = threads.max(1);
        let pool = AssetPool {
            shared: Arc::clone(&shared),
            workers: Mutex::new(Vec::with_capacity(n)),
            threads: n,
        };
        for i in 0..n {
            let handle = spawn_worker(&shared, format!("flyff-asset-{i}"), Class::Niced, nice)?;
            lock(&pool.workers).push(handle);
        }
        Ok(pool)
    }

    /// Number of worker threads.
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// A handle for submitting and promoting keyed jobs.
    pub fn handle(&self) -> PoolHandle {
        PoolHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Queues `job` and returns a handle that yields its result.
    pub fn submit<T, F>(&self, prio: Priority, job: F) -> Result<ReplyRx<T>, PoolError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let (tx, rx) = reply_pair();
        let handle = self.handle();
        let id = handle.next_id();
        handle.submit_keyed(id, prio, move || {
            // The requester may have gone away (browser disconnected); the result is then dropped.
            let _ = tx.send(job());
        })?;
        Ok(rx)
    }

    /// Starts `n` un-niced temporary workers named `flyff-startup-N` for start-up work. While the
    /// guard lives the niced workers leave `Startup` jobs to them; dropping or finishing the guard
    /// drains the `Startup` queue and joins the threads.
    pub fn start_startup_workers(&self, n: usize) -> Result<StartupWorkers, PoolError> {
        let n = n.max(1);
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut q = lock(&self.shared.queues);
            if q.closed {
                return Err(PoolError::ShutDown);
            }
            q.startup_workers += n;
        }
        let mut handles = Vec::with_capacity(n);
        for i in 0..n {
            match spawn_worker(
                &self.shared,
                format!("flyff-startup-{i}"),
                Class::Startup(Arc::clone(&stop)),
                0,
            ) {
                Ok(h) => handles.push(h),
                Err(e) => {
                    // The workers that did not start must not keep the niced pool away from Startup jobs.
                    let mut q = lock(&self.shared.queues);
                    q.startup_workers = q.startup_workers.saturating_sub(n - handles.len());
                    drop(q);
                    self.shared.ready.notify_all();
                    if handles.is_empty() {
                        return Err(e.into());
                    }
                    break;
                }
            }
        }
        Ok(StartupWorkers {
            shared: Arc::clone(&self.shared),
            stop,
            handles,
        })
    }

    /// Stops accepting jobs, drops queued prewarm jobs, lets in-flight and queued interactive/startup
    /// jobs finish and joins every worker.
    pub fn shutdown(self) {
        self.close_and_join();
    }

    /// Like [`Self::shutdown`] but waits at most `deadline` (measured with `clock`) for the workers;
    /// on timeout the threads are detached and `false` is returned.
    pub fn shutdown_timeout(&self, deadline: Duration, clock: &dyn Clock) -> bool {
        self.close();
        let limit = clock
            .now_ms()
            .saturating_add(u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX));
        let mut q = lock(&self.shared.queues);
        while q.alive > 0 {
            let now = clock.now_ms();
            if now >= limit {
                break;
            }
            let slice = Duration::from_millis((limit - now).min(25));
            q = match self.shared.exited.wait_timeout(q, slice) {
                Ok((guard, _)) => guard,
                Err(p) => p.into_inner().0,
            };
        }
        let finished = q.alive == 0;
        drop(q);
        if finished {
            self.join_all();
        } else {
            // Detach: the remaining workers exit on their own when their job ends.
            lock(&self.workers).clear();
        }
        finished
    }

    /// Closes the pool to new jobs and removes every queued Prewarm job in one step (no job can be
    /// queued between the two), returning their ids so the owner can answer their waiters.
    pub fn close_and_drain(&self) -> Vec<JobId> {
        let dropped: Vec<(JobId, Job)> = {
            let mut q = lock(&self.shared.queues);
            q.closed = true;
            q.q.get_mut(Priority::Prewarm.index())
                .map(|queue| queue.drain(..).collect())
                .unwrap_or_default()
        };
        self.shared.ready.notify_all();
        // The closures (and what they capture) are dropped outside the lock.
        dropped.into_iter().map(|(id, _)| id).collect()
    }

    fn close(&self) {
        drop(self.close_and_drain());
    }

    fn join_all(&self) {
        let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *lock(&self.workers));
        for h in handles {
            if h.join().is_err() {
                tracing::error!("asset worker panicked");
            }
        }
    }

    fn close_and_join(&self) {
        self.close();
        self.join_all();
    }
}

impl Drop for AssetPool {
    fn drop(&mut self) {
        self.close_and_join();
    }
}

/// The temporary start-up workers (see [`AssetPool::start_startup_workers`]).
pub struct StartupWorkers {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl StartupWorkers {
    /// Number of threads.
    pub fn threads(&self) -> usize {
        self.handles.len()
    }

    /// Lets the workers finish the queued `Startup` jobs and joins them.
    pub fn finish(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.shared.ready.notify_all();
        for h in self.handles.drain(..) {
            if h.join().is_err() {
                tracing::error!("startup worker panicked");
            }
        }
    }
}

impl Drop for StartupWorkers {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

fn spawn_worker(
    shared: &Arc<Shared>,
    name: String,
    class: Class,
    nice: i32,
) -> std::io::Result<JoinHandle<()>> {
    lock(&shared.queues).alive += 1;
    let s = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name(name)
        .spawn(move || worker_loop(&s, &class, nice));
    if spawned.is_err() {
        let mut q = lock(&shared.queues);
        q.alive = q.alive.saturating_sub(1);
    }
    spawned
}

fn worker_loop(shared: &Shared, class: &Class, nice: i32) {
    if matches!(class, Class::Niced) {
        lower_priority(nice);
        crate::sys::lower_io_priority();
    }
    loop {
        let next = {
            let mut q = lock(&shared.queues);
            loop {
                if let Some(x) = q.pop(class) {
                    if let Some(r) = q.running.get_mut(x.2) {
                        *r += 1;
                    }
                    break Some(x);
                }
                if q.should_exit(class) {
                    break None;
                }
                q = match shared.ready.wait(q) {
                    Ok(guard) => guard,
                    Err(p) => p.into_inner(),
                };
            }
        };
        let Some((_id, job, idx)) = next else { break };
        // A panicking builder must not take the worker down (Plan/01 §6.7: `catch_unwind` per job).
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)) {
            tracing::error!(panic = %panic_message(payload.as_ref()), "asset job panicked");
        }
        let mut q = lock(&shared.queues);
        if let Some(r) = q.running.get_mut(idx) {
            *r = r.saturating_sub(1);
        }
        let wake_all = idx == Priority::Interactive.index();
        drop(q);
        if wake_all {
            // Prewarm work may have been waiting for the last Interactive job.
            shared.ready.notify_all();
        }
    }
    let mut q = lock(&shared.queues);
    if matches!(class, Class::Startup(_)) {
        q.startup_workers = q.startup_workers.saturating_sub(1);
    }
    q.alive = q.alive.saturating_sub(1);
    drop(q);
    // Niced workers may now be allowed to take Startup jobs again.
    shared.ready.notify_all();
    shared.exited.notify_all();
}

/// Lowers the calling thread's priority (Linux `nice` is per thread).
fn lower_priority(nice: i32) {
    if nice <= 0 {
        return;
    }
    if let Err(e) = rustix::process::nice(nice) {
        tracing::debug!(error = %e, nice, "could not lower asset worker priority");
    }
}
