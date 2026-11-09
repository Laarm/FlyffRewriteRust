//! [`AssetPrep`]: the derived-asset framework facade (Plan/24 §7.3).
//!
//! # Life of a request
//!
//! `get(kind, key, prio)` resolves the key through the kind's builder (`route`, `partition_fp`), looks
//! the artifact up in the cache (two `stat` calls) and answers hits immediately. A miss joins the
//! **in-flight table**: the first miss of a job queues it on the pool, every further request for the
//! same job (any key it produces) only adds a waiter, and a higher-priority request *promotes* a job
//! that is still queued at a lower priority. The worker runs the builder under `catch_unwind`, stores
//! every file with the W8 rule through temp files and atomic renames, writes the job marker, answers all
//! waiters and updates progress, telemetry and the report accumulator.
//!
//! The same queue serves `prepare_all` (Startup priority), `load_server_derived` (Startup, on temporary
//! `flyff-startup-*` workers) and the prewarm (Prewarm priority). Nothing here uses tokio: callers wait
//! on a [`ReplyRx`] (`recv`, `recv_timeout` or `recv_async`).

use crate::build;
use crate::cache::{Cache, CachedFile, Durability, GzLookup, Staged, SweepKeep, SweepStats};
use crate::clock::Clock;
use crate::derived::ServerDerived;
use crate::error::{AssetError, AssetErrorKind};
use crate::fp::{FingerprintMode, Fp, FpCtx, file_fp};
use crate::kinds::{
    AssetBuilder, AssetFeatures, AssetKind, BuildCtx, BuiltFile, JobKey, Priority, prewarm_step,
};
use crate::manifest::{BundleIndex, ClientManifest, ManifestParts};
use crate::pool::{AssetPool, JobId, PoolHandle};
use crate::progress::{PrepareProgress, Progress};
use crate::report::{
    CacheReport, KindCacheReport, KindReport, PrepareOptions, PrepareReport, ReportAcc,
};
use crate::telemetry::{AssetTelemetry, JobResult};
use crate::util::{lock, panic_message};
use flyff_common::det::DetMap;
use flyff_common::rt::reply::ReplyError;
use flyff_common::rt::{Reply, ReplyRx, reply_pair};
use flyff_data::GameData;
use flyff_vfs::{Vfs, VfsEntry};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// How long Prewarm stays paused after a cache write error (Plan/24 §7.6).
const PREWARM_PAUSE_MS: u64 = 10 * 60 * 1000;
/// Jobs with at least this many files stage them on several threads when running at Startup priority.
const PARALLEL_STAGE_MIN: usize = 8;
/// Deletions per sweep batch (Plan/24 §8.5).
const SWEEP_BATCH: u64 = 2_000;

/// `assets.gzip_raw`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GzipRawMode {
    /// Prewarm every eligible file and build on request.
    #[default]
    All,
    /// Build only the files that are requested.
    Lazy,
    /// Never build or serve variants.
    Off,
}

impl GzipRawMode {
    /// The config spelling (`all`, `lazy`, `off`).
    pub const fn as_str(self) -> &'static str {
        match self {
            GzipRawMode::All => "all",
            GzipRawMode::Lazy => "lazy",
            GzipRawMode::Off => "off",
        }
    }
}

/// Settings of the framework (the `[runtime]`, `[assets]` and `[paths]` keys of Plan/24 §17).
#[derive(Clone, Debug)]
pub struct AssetConfig {
    /// `paths.cache_dir`, resolved.
    pub cache_dir: PathBuf,
    /// `runtime.asset_threads`; 0 = `max(1, cores - 2)`.
    pub threads: usize,
    /// `runtime.asset_nice`.
    pub nice: i32,
    /// `runtime.startup_threads`; 0 = all cores (the temporary `flyff-startup-*` workers).
    pub startup_threads: usize,
    /// `assets.prewarm`.
    pub prewarm: bool,
    /// `assets.gzip_raw`.
    pub gzip_raw: GzipRawMode,
    /// `assets.disk_budget_mb` in bytes.
    pub disk_budget_bytes: u64,
    /// `assets.fingerprint_mode`.
    pub fingerprint_mode: FingerprintMode,
    /// `assets.packs == "on"`.
    pub packs: bool,
    /// Which kinds are registered (milestone gating).
    pub features: AssetFeatures,
    /// `flyff <version>+<commit>`, written into reports and the manifest.
    pub server_build: String,
}

impl AssetConfig {
    /// The defaults of the plan with `cache_dir` (`threads` auto, nice 10, budget 800 MB, prewarm on).
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            threads: 0,
            nice: 10,
            startup_threads: 0,
            prewarm: true,
            gzip_raw: GzipRawMode::All,
            disk_budget_bytes: 800 * 1024 * 1024,
            fingerprint_mode: FingerprintMode::Stat,
            packs: false,
            features: AssetFeatures::all(),
            server_build: String::new(),
        }
    }

    /// The worker count: `threads`, or `max(1, cores - 2)` when 0 (`runtime.asset_threads`, D2).
    pub fn resolved_threads(&self) -> usize {
        if self.threads > 0 {
            return self.threads;
        }
        cores().saturating_sub(2).max(1)
    }

    /// The start-up pool size: `startup_threads`, or all cores when 0.
    pub fn resolved_startup_threads(&self) -> usize {
        if self.startup_threads > 0 {
            self.startup_threads
        } else {
            cores()
        }
    }
}

fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// The state of the gzip variant of a raw file (`AssetPrep::res_gz`).
#[derive(Clone, Debug)]
pub enum GzState {
    /// A variant exists (serve it with `Content-Encoding: gzip`).
    Present(CachedFile),
    /// Not eligible, switched off, or the ratio was >= 0.9: serve the raw file.
    NotBeneficial,
    /// Not decided yet: serve the raw file and call `request_gz`.
    Unknown,
}

/// What a finished job reports to a [`prepare`](AssetPrep::prepare_all)-style waiter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSummary {
    /// Files the job stored.
    pub files: usize,
    /// Bytes stored (gzip form where applicable).
    pub bytes: u64,
    /// The job was already done and not rebuilt.
    pub cached: bool,
    /// Build time in ms.
    pub ms: u64,
}

type JobResultMsg = (JobKey, Result<JobSummary, AssetError>);

enum Waiter {
    Key {
        key: String,
        prio: Priority,
        reply: Reply<Result<CachedFile, AssetError>>,
    },
    Job {
        prio: Priority,
        tx: flume::Sender<JobResultMsg>,
    },
}

impl Waiter {
    fn interactive(&self) -> bool {
        match self {
            Waiter::Key { prio, .. } | Waiter::Job { prio, .. } => *prio == Priority::Interactive,
        }
    }
}

struct InFlight {
    id: JobId,
    /// The most urgent priority anybody asked for.
    prio: Priority,
    /// The queue the job sits in (or ran from).
    queue_prio: Priority,
    partfp: Fp,
    /// Somebody asked for this artifact (a request, not just the prewarm plan): optional kinds then
    /// build until 100 % of the budget instead of 95 %.
    lazy: bool,
    waiters: Vec<Waiter>,
}

struct FailedJob {
    partfp: Fp,
    error: AssetError,
}

struct Resolved {
    builder: Arc<dyn AssetBuilder>,
    job: JobKey,
    partfp: Fp,
}

enum Fail {
    /// The job was not run (budget, shutdown, paused prewarm): not a builder failure.
    Skip(AssetError),
    /// The builder failed or panicked.
    Build(AssetError),
}

struct Prepared {
    staged: Vec<Staged>,
    keys: Vec<String>,
    bytes: u64,
}

/// A job whose files are committed.
struct Committed {
    files: Vec<(String, CachedFile)>,
    bytes: u64,
    mode: Durability,
}

struct Inner {
    vfs: Arc<Vfs>,
    data: Arc<GameData>,
    cfg: AssetConfig,
    clock: Arc<dyn Clock>,
    telemetry: Arc<AssetTelemetry>,
    cache: Cache,
    builders: BTreeMap<AssetKind, Arc<dyn AssetBuilder>>,
    parts: Arc<ManifestParts>,
    pool: PoolHandle,
    inflight: Mutex<DetMap<JobKey, InFlight>>,
    failed: Mutex<BTreeMap<JobKey, FailedJob>>,
    progress: Progress,
    acc: Mutex<ReportAcc>,
    cancel: AtomicBool,
    closing: AtomicBool,
    prewarm_paused_until_ms: AtomicU64,
    manifest: RwLock<Arc<ClientManifest>>,
    bundles: RwLock<Arc<BundleIndex>>,
    last_report: Mutex<Option<PrepareReport>>,
    keep_fps: Mutex<BTreeSet<Fp>>,
}

/// The derived-asset framework (cheap to clone; all clones share one pool and one cache).
#[derive(Clone)]
pub struct AssetPrep {
    inner: Arc<Inner>,
    pool: Arc<AssetPool>,
}

impl std::fmt::Debug for AssetPrep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetPrep")
            .field("cache", &self.inner.cache)
            .field("kinds", &self.registered_kinds())
            .finish_non_exhaustive()
    }
}

// ---- Inner --------------------------------------------------------------------------------------

impl Inner {
    /// Publishes the queue depths to the telemetry gauge, atomically with the queue lock.
    fn publish_depth(&self) {
        self.pool.with_depth(|d| self.telemetry.set_queue_depth(d));
    }

    fn fp_ctx(&self) -> FpCtx<'_> {
        FpCtx {
            vfs: &self.vfs,
            data: &self.data,
            mode: self.cfg.fingerprint_mode,
        }
    }

    fn build_ctx(&self) -> BuildCtx<'_> {
        BuildCtx::new(
            &self.vfs,
            &self.data,
            &self.parts,
            &self.cancel,
            self.cfg.fingerprint_mode,
        )
    }

    fn builder(&self, kind: AssetKind) -> Result<&Arc<dyn AssetBuilder>, AssetError> {
        self.builders
            .get(&kind)
            .ok_or_else(|| AssetError::disabled(format!("asset kind {kind} is not registered")))
    }

    /// The fingerprint that names the artifacts of `job` (a partition fingerprint; for `gz` the file
    /// fingerprint).
    fn job_partfp(&self, job: &JobKey) -> Option<Fp> {
        if job.kind == AssetKind::Gzip {
            return self.vfs.resolve(&job.name).map(file_fp);
        }
        self.builders
            .get(&job.kind)
            .and_then(|b| b.partition_fp(&self.fp_ctx(), &job.partition))
    }

    fn resolve(&self, kind: AssetKind, key: &str) -> Result<Resolved, AssetError> {
        let builder = Arc::clone(self.builder(kind)?);
        let ctx = self.fp_ctx();
        let route = builder.route(&ctx, key)?;
        let partfp = if kind == AssetKind::Gzip {
            self.vfs
                .resolve(&route.job.name)
                .map(file_fp)
                .ok_or_else(|| AssetError::not_found(format!("no such file: {}", route.job.name)))?
        } else {
            builder
                .partition_fp(&ctx, &route.partition)
                .ok_or_else(|| {
                    AssetError::not_found(format!("unknown {kind} partition {:?}", route.partition))
                })?
        };
        Ok(Resolved {
            builder,
            job: route.job,
            partfp,
        })
    }

    fn lookup_resolved(&self, r: &Resolved, key: &str) -> Result<Option<CachedFile>, AssetError> {
        if r.job.kind == AssetKind::Gzip {
            let ct = r
                .builder
                .content_type(&format!("{}.{}.gz", r.job.name, r.partfp));
            return match self.cache.lookup_res_gz(&r.job.name, r.partfp, ct) {
                GzLookup::Present(cf) => Ok(Some(cf)),
                GzLookup::NotBeneficial => Err(AssetError::not_convertible(format!(
                    "{}: gzip is not beneficial",
                    r.job.name
                ))),
                GzLookup::Unknown => Ok(None),
            };
        }
        self.cache
            .lookup(r.job.kind, r.partfp, key, r.builder.content_type(key))
    }

    fn failed_error(&self, job: &JobKey, partfp: Fp) -> Option<AssetError> {
        let failed = lock(&self.failed);
        failed
            .get(job)
            .filter(|f| f.partfp == partfp)
            .map(|f| f.error.clone())
    }

    /// Optional kinds (`gz`, packs) are refused over budget: planned Prewarm and Startup work at
    /// 95 %, jobs somebody asked for (`lazy`, Interactive) at 100 % (Plan/24 §8.7).
    fn check_budget(&self, kind: AssetKind, prio: Priority, lazy: bool) -> Result<(), AssetError> {
        if kind.required() {
            return Ok(());
        }
        let ok = if lazy || prio == Priority::Interactive {
            self.cache.below_budget()
        } else {
            self.cache.below_prewarm_limit()
        };
        if ok {
            Ok(())
        } else {
            Err(AssetError::new(
                AssetErrorKind::OverBudget,
                format!(
                    "cache is at {} % of its {} byte budget",
                    self.cache.used_percent(),
                    self.cache.budget_bytes()
                ),
            ))
        }
    }

    fn job_done(&self, job: &JobKey, partfp: Fp) -> bool {
        if job.kind == AssetKind::Gzip {
            return matches!(
                self.cache
                    .lookup_res_gz(&job.name, partfp, "application/octet-stream"),
                GzLookup::Present(_) | GzLookup::NotBeneficial
            );
        }
        self.cache.job_done(job.kind, partfp, &job.name)
    }

    // ---- get ----------------------------------------------------------------------------------

    fn get(
        self: &Arc<Self>,
        kind: AssetKind,
        key: &str,
        prio: Priority,
        reply: Reply<Result<CachedFile, AssetError>>,
    ) {
        let step = (|| -> Result<Result<CachedFile, Resolved>, AssetError> {
            let r = self.resolve(kind, key)?;
            if let Some(cf) = self.lookup_resolved(&r, key)? {
                return Ok(Ok(cf));
            }
            if self.closing.load(Ordering::Acquire) {
                return Err(AssetError::shutting_down());
            }
            if let Some(e) = self.failed_error(&r.job, r.partfp) {
                return Err(e);
            }
            self.check_budget(kind, prio, true)?;
            Ok(Err(r))
        })();
        match step {
            Err(e) => {
                let _ = reply.send(Err(e));
            }
            Ok(Ok(cf)) => {
                let _ = reply.send(Ok(cf));
            }
            Ok(Err(r)) => {
                let waiter = Waiter::Key {
                    key: key.to_owned(),
                    prio,
                    reply,
                };
                self.enqueue(r.job.clone(), r.partfp, prio, waiter, Some((&r, key)));
            }
        }
    }

    /// Adds `waiter` to the in-flight entry of `job`, creating and queueing it when absent.
    /// `recheck`: the key to look up once more under the table lock (a job that finished between the
    /// caller's miss and this call must not be built twice).
    fn enqueue(
        self: &Arc<Self>,
        job: JobKey,
        partfp: Fp,
        prio: Priority,
        waiter: Waiter,
        recheck: Option<(&Resolved, &str)>,
    ) {
        let interactive = waiter.interactive();
        // A request for a key is somebody asking; `prepare`/start-up jobs are planned work.
        let requested = matches!(waiter, Waiter::Key { .. });
        let mut table = lock(&self.inflight);
        if let Some(entry) = table.get_mut(&job) {
            entry.lazy |= requested;
            entry.waiters.push(waiter);
            if interactive {
                self.progress.waiter_added();
            }
            if prio < entry.prio {
                entry.prio = prio;
                if prio < entry.queue_prio && self.pool.promote(entry.id, prio) {
                    self.progress.promoted(entry.queue_prio, prio);
                    entry.queue_prio = prio;
                }
            }
            return;
        }
        // The job may have finished (or failed) between the caller's miss and this lock: both
        // outcomes are visible before its in-flight entry disappears, so look again here.
        if let (Some((r, key)), Waiter::Key { .. }) = (recheck, &waiter) {
            let answer = match self.lookup_resolved(r, key) {
                Ok(Some(cf)) => Some(Ok(cf)),
                Ok(None) => self.failed_error(&job, partfp).map(Err),
                Err(e) => Some(Err(e)),
            };
            if let Some(answer) = answer {
                drop(table);
                if let Waiter::Key { reply, .. } = waiter {
                    let _ = reply.send(answer);
                }
                return;
            }
        }
        let id = self.pool.next_id();
        table.insert(
            job.clone(),
            InFlight {
                id,
                prio,
                queue_prio: prio,
                partfp,
                lazy: requested,
                waiters: vec![waiter],
            },
        );
        if interactive {
            self.progress.waiter_added();
        }
        self.start_job(&mut table, job, id, prio);
    }

    /// Like `enqueue` for fire-and-forget jobs: no waiter. `lazy` = a request (`request_gz`), as
    /// opposed to the prewarm plan.
    fn enqueue_detached(self: &Arc<Self>, job: JobKey, partfp: Fp, prio: Priority, lazy: bool) {
        let mut table = lock(&self.inflight);
        if let Some(entry) = table.get_mut(&job) {
            entry.lazy |= lazy;
            return;
        }
        // Finished or failed since the caller looked: do not run it again.
        if self.failed_error(&job, partfp).is_some() || self.job_done(&job, partfp) {
            return;
        }
        let id = self.pool.next_id();
        table.insert(
            job.clone(),
            InFlight {
                id,
                prio,
                queue_prio: prio,
                partfp,
                lazy,
                waiters: Vec::new(),
            },
        );
        self.start_job(&mut table, job, id, prio);
    }

    /// Registers the job with progress and telemetry and submits it. The entry is already in `table`.
    fn start_job(
        self: &Arc<Self>,
        table: &mut DetMap<JobKey, InFlight>,
        job: JobKey,
        id: JobId,
        prio: Priority,
    ) {
        if self.progress.enqueued(prio) {
            lock(&self.acc).reset();
        }
        if prio == Priority::Prewarm {
            lock(&self.acc).saw_prewarm = true;
        }
        self.telemetry.inflight_add(1);
        let this = Arc::clone(self);
        let jk = job.clone();
        if self
            .pool
            .submit_keyed(id, prio, move || this.run_job(id, &jk))
            .is_err()
        {
            // The pool is closed: fail the waiters we just registered.
            if let Some(entry) = table.shift_remove(&job) {
                self.progress.dropped(entry.queue_prio);
                self.telemetry.inflight_add(-1);
                self.resolve_error(&job, entry.waiters, &AssetError::shutting_down());
            }
        }
        self.publish_depth();
    }

    // ---- running a job ------------------------------------------------------------------------

    fn run_job(self: &Arc<Self>, _id: JobId, job: &JobKey) {
        let (queue_prio, partfp, lazy) = {
            let table = lock(&self.inflight);
            match table.get(job) {
                Some(e) => (e.queue_prio, e.partfp, e.lazy),
                None => return,
            }
        };
        self.progress.started(queue_prio, Some(job.kind), &job.name);
        self.publish_depth();
        let t0 = self.clock.now_ms();
        let outcome = self.execute(job, partfp, queue_prio, lazy);
        let outcome = match outcome {
            Ok(prepared) => Ok(self.commit(job, partfp, queue_prio, prepared)),
            Err(f) => Err(f),
        };
        let ms = self.clock.now_ms().saturating_sub(t0);
        self.finish(job, partfp, queue_prio, outcome, ms);
    }

    /// Build and stage (no rename yet).
    fn execute(
        &self,
        job: &JobKey,
        partfp: Fp,
        qprio: Priority,
        lazy: bool,
    ) -> Result<Prepared, Fail> {
        let closing = self.closing.load(Ordering::Acquire);
        if closing && qprio == Priority::Prewarm {
            return Err(Fail::Skip(AssetError::shutting_down()));
        }
        if qprio == Priority::Prewarm && self.prewarm_paused() {
            return Err(Fail::Skip(AssetError::new(
                AssetErrorKind::Io,
                "prewarm is paused after a cache write error",
            )));
        }
        self.check_budget(job.kind, qprio, lazy)
            .map_err(Fail::Skip)?;
        let builder = self.builder(job.kind).map_err(Fail::Skip)?;

        if job.kind == AssetKind::Gzip && self.job_done(job, partfp) {
            // Built by another request or process since this job was queued.
            return Ok(Prepared {
                staged: Vec::new(),
                keys: Vec::new(),
                bytes: 0,
            });
        }

        let ctx = self.build_ctx();
        let built =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.build(&ctx, job)));
        let files = match built {
            Ok(Ok(files)) => files,
            Ok(Err(e)) if e.kind == AssetErrorKind::ShuttingDown => return Err(Fail::Skip(e)),
            Ok(Err(e)) => return Err(Fail::Build(e)),
            Err(payload) => {
                let msg = panic_message(payload.as_ref());
                tracing::error!(job = %job, panic = %msg, "asset builder panicked");
                return Err(Fail::Build(AssetError::build(format!(
                    "{job}: builder panicked: {msg}"
                ))));
            }
        };

        let mut seen = BTreeSet::new();
        let mut keys = Vec::with_capacity(files.len());
        for f in &files {
            if !seen.insert(f.key.as_str()) {
                return Err(Fail::Build(AssetError::build(format!(
                    "{job}: builder returned the key {:?} twice",
                    f.key
                ))));
            }
            keys.push(f.key.clone());
        }
        drop(seen);
        // Start-up jobs (a whole world's chunks) are compressed and written by several threads; the
        // bounded niced pool stays one thread per job.
        let threads = if qprio == Priority::Startup && files.len() >= PARALLEL_STAGE_MIN {
            self.cfg.resolved_startup_threads()
        } else {
            1
        };
        let raw_len_of_source = (job.kind == AssetKind::Gzip)
            .then(|| self.vfs.resolve(&job.name).map(|e| e.size))
            .flatten();
        let results = stage_files(&self.cache, job.kind, partfp, files, threads);
        let mut staged = Vec::with_capacity(results.len() + 1);
        for r in results {
            let mut s = r.map_err(|e| {
                Fail::Build(AssetError::build(format!(
                    "{job}: builder returned a bad key: {e}"
                )))
            })?;
            if let Some(size) = raw_len_of_source {
                s.set_raw_len(size);
            }
            staged.push(s);
        }
        let mut bytes = 0;
        if job.kind != AssetKind::Gzip && !staged.iter().any(Staged::write_failed) {
            // The marker comes last: it is renamed after the artifacts it vouches for.
            match self.cache.stage_marker(job.kind, partfp, &job.name) {
                Ok(m) => staged.push(m),
                Err(e) => {
                    return Err(Fail::Build(AssetError::build(format!(
                        "{job}: job name is not a valid marker name: {e}"
                    ))));
                }
            }
        }
        for s in &staged {
            bytes += s.stored_len();
        }
        Ok(Prepared {
            staged,
            keys,
            bytes,
        })
    }

    fn commit(&self, job: &JobKey, partfp: Fp, qprio: Priority, prepared: Prepared) -> Committed {
        let Prepared {
            staged,
            keys,
            bytes,
        } = prepared;
        let mode = {
            let table = lock(&self.inflight);
            let nobody_waits = table.get(job).is_none_or(|e| e.waiters.is_empty());
            if qprio == Priority::Prewarm && nobody_waits {
                Durability::Deferred
            } else {
                Durability::Sync
            }
        };
        let committed = self.cache.commit_all(staged, mode);
        if committed.iter().any(|c| c.mem.is_some()) {
            let until = self.clock.now_ms().saturating_add(PREWARM_PAUSE_MS);
            self.prewarm_paused_until_ms.store(until, Ordering::Relaxed);
        }
        let _ = partfp;
        let files = keys.into_iter().zip(committed).collect();
        Committed { files, bytes, mode }
    }

    fn prewarm_paused(&self) -> bool {
        let until = self.prewarm_paused_until_ms.load(Ordering::Relaxed);
        until != 0 && self.clock.now_ms() < until
    }

    fn finish(
        self: &Arc<Self>,
        job: &JobKey,
        partfp: Fp,
        qprio: Priority,
        outcome: Result<Committed, Fail>,
        ms: u64,
    ) {
        // A failure is recorded BEFORE the in-flight entry disappears: a request that arrives in
        // between must find either the entry or the remembered error, never neither (it would run
        // the failing builder a second time).
        match &outcome {
            Err(Fail::Build(e)) => {
                lock(&self.failed).insert(
                    job.clone(),
                    FailedJob {
                        partfp,
                        error: e.clone(),
                    },
                );
            }
            Ok(_) => {
                lock(&self.failed).remove(job);
            }
            Err(Fail::Skip(_)) => {}
        }
        let waiters = {
            let mut table = lock(&self.inflight);
            table
                .shift_remove(job)
                .map(|e| e.waiters)
                .unwrap_or_default()
        };
        self.telemetry.inflight_add(-1);
        let interactive = waiters.iter().filter(|w| w.interactive()).count();

        // Progress is settled before the waiters are answered, so a caller that has its reply (or the
        // last reply of a `prepare` run) never reads a stale "still active" snapshot.
        let settle = |this: &Self| -> bool {
            this.progress
                .waiters_removed(u32::try_from(interactive).unwrap_or(u32::MAX));
            let ended = this.progress.finished(qprio);
            this.publish_depth();
            this.telemetry.sync_cache(&this.cache);
            ended
        };

        let ended = match outcome {
            Ok(Committed { files, bytes, mode }) => {
                if mode == Durability::Deferred && !waiters.is_empty() {
                    self.cache.flush_pending();
                }
                lock(&self.acc).built(job.kind, bytes, ms);
                self.telemetry.job(job.kind, JobResult::Built);
                self.telemetry
                    .observe_build_ns(job.kind, ms.saturating_mul(1_000_000));
                let summary = JobSummary {
                    files: files.len(),
                    bytes,
                    cached: false,
                    ms,
                };
                let ended = settle(self);
                for w in waiters {
                    self.resolve_ok(job, partfp, w, &files, &summary);
                }
                ended
            }
            Err(Fail::Skip(e)) => {
                lock(&self.acc).skipped(job.kind);
                self.telemetry.job(job.kind, JobResult::Skipped);
                let ended = settle(self);
                self.resolve_error(job, waiters, &e);
                ended
            }
            Err(Fail::Build(e)) => {
                tracing::warn!(job = %job, error = %e, "asset job failed");
                lock(&self.acc).failed(job.kind, &job.name, &e.to_string(), ms);
                self.telemetry.job(job.kind, JobResult::Failed);
                let ended = settle(self);
                self.resolve_error(job, waiters, &e);
                ended
            }
        };
        if ended {
            self.on_wave_end();
        }
    }

    fn resolve_ok(
        &self,
        job: &JobKey,
        partfp: Fp,
        waiter: Waiter,
        files: &[(String, CachedFile)],
        summary: &JobSummary,
    ) {
        match waiter {
            Waiter::Job { tx, .. } => {
                let _ = tx.send((job.clone(), Ok(summary.clone())));
            }
            Waiter::Key { key, reply, .. } => {
                let result = if job.kind == AssetKind::Gzip {
                    self.gz_result(job, partfp, files)
                } else {
                    files
                        .iter()
                        .find(|(k, _)| *k == key)
                        .map(|(_, cf)| cf.clone())
                        .ok_or_else(|| {
                            AssetError::not_found(format!("job {job} did not produce {key:?}"))
                        })
                };
                let _ = reply.send(result);
            }
        }
    }

    fn gz_result(
        &self,
        job: &JobKey,
        partfp: Fp,
        files: &[(String, CachedFile)],
    ) -> Result<CachedFile, AssetError> {
        if let Some((_, cf)) = files.iter().find(|(k, _)| k.ends_with(".gz")) {
            return Ok(cf.clone());
        }
        match self
            .cache
            .lookup_res_gz(&job.name, partfp, "application/octet-stream")
        {
            GzLookup::Present(cf) => Ok(cf),
            GzLookup::NotBeneficial => Err(AssetError::not_convertible(format!(
                "{}: gzip is not beneficial",
                job.name
            ))),
            GzLookup::Unknown => Err(AssetError::not_found(format!(
                "{}: no gzip variant was produced",
                job.name
            ))),
        }
    }

    fn resolve_error(&self, job: &JobKey, waiters: Vec<Waiter>, e: &AssetError) {
        for w in waiters {
            match w {
                Waiter::Key { reply, .. } => {
                    let _ = reply.send(Err(e.clone()));
                }
                Waiter::Job { tx, .. } => {
                    let _ = tx.send((job.clone(), Err(e.clone())));
                }
            }
        }
    }

    /// The end of a wave: make deferred writes durable and, after a wave that ran Prewarm jobs,
    /// rewrite `prepare-report.json` (Plan/24 §8.9).
    fn on_wave_end(&self) {
        self.cache.flush_pending();
        self.telemetry.sync_cache(&self.cache);
        let report = {
            let acc = lock(&self.acc);
            // A wave that built or failed nothing (everything was done already) must not replace the
            // report of the last real wave, which `flyff check` reads its failure counts from.
            let worked = acc.kinds.values().any(|k| k.built > 0 || k.failed > 0);
            (acc.saw_prewarm && worked).then(|| {
                acc.to_report(
                    &self.cfg.server_build,
                    self.clock.unix_s(),
                    self.cache.budget_bytes(),
                    self.cache.used_bytes(),
                )
            })
        };
        if let Some(r) = report {
            self.write_report(&r, None);
        }
    }

    fn write_report(&self, report: &PrepareReport, path: Option<&Path>) {
        let bytes = report.to_json();
        let res = match path {
            None => self
                .cache
                .write_atomic("prepare-report.json", &bytes, false),
            Some(p) => write_file_atomic(p, &bytes),
        };
        if let Err(e) = res {
            tracing::warn!(error = %e, "could not write the prepare report");
        }
        *lock(&self.last_report) = Some(report.clone());
    }

    /// Answers the waiters of jobs that were dropped from the queue at shutdown.
    fn fail_dropped(&self, ids: &[JobId]) {
        if ids.is_empty() {
            return;
        }
        let dropped: Vec<(JobKey, InFlight)> = {
            let mut table = lock(&self.inflight);
            let keys: Vec<JobKey> = table
                .iter()
                .filter(|(_, e)| ids.contains(&e.id))
                .map(|(k, _)| k.clone())
                .collect();
            keys.into_iter()
                .filter_map(|k| table.shift_remove(&k).map(|e| (k, e)))
                .collect()
        };
        let err = AssetError::shutting_down();
        for (job, entry) in dropped {
            self.telemetry.inflight_add(-1);
            self.telemetry.job(job.kind, JobResult::Skipped);
            let interactive = entry.waiters.iter().filter(|w| w.interactive()).count();
            self.resolve_error(&job, entry.waiters, &err);
            self.progress
                .waiters_removed(u32::try_from(interactive).unwrap_or(u32::MAX));
            self.progress.dropped(entry.queue_prio);
        }
    }

    // ---- sweep and prewarm --------------------------------------------------------------------

    fn current_partitions(&self) -> BTreeMap<AssetKind, BTreeSet<Fp>> {
        let ctx = self.fp_ctx();
        let mut out = BTreeMap::new();
        for (kind, b) in &self.builders {
            if *kind == AssetKind::Gzip {
                continue;
            }
            let mut fps = BTreeSet::new();
            let mut seen = BTreeSet::new();
            for job in b.jobs(&ctx) {
                if seen.insert(job.partition.clone())
                    && let Some(fp) = b.partition_fp(&ctx, &job.partition)
                {
                    fps.insert(fp);
                }
            }
            out.insert(*kind, fps);
        }
        out
    }

    fn sweep(&self) -> SweepStats {
        let partitions = self.current_partitions();
        let mut keep_fps: BTreeSet<Fp> = partitions.values().flatten().copied().collect();
        keep_fps.extend(lock(&self.keep_fps).iter().copied());
        keep_fps.extend(self.parts.kept_fingerprints());
        let vfs = &self.vfs;
        let gz_current = |path: &str, fp: Fp| {
            vfs.resolve(path).is_some_and(|e| {
                file_fp(e) == fp && build::gz::is_eligible(e.path.as_str(), e.size)
            })
        };
        let keep = SweepKeep {
            partitions,
            gz_current: self
                .builders
                .contains_key(&AssetKind::Gzip)
                .then_some(&gz_current),
            keep_fps,
        };
        let mut total = SweepStats::default();
        loop {
            let st = self.cache.sweep_step(&keep, SWEEP_BATCH);
            total.absorb(st);
            if st.done || self.closing.load(Ordering::Acquire) {
                break;
            }
        }
        self.telemetry.sync_cache(&self.cache);
        total
    }

    fn plan_prewarm(self: &Arc<Self>) {
        self.progress
            .started(Priority::Prewarm, None, "prewarm plan");
        let st = self.sweep();
        if st.deleted_files > 0 {
            tracing::info!(
                files = st.deleted_files,
                bytes = st.deleted_bytes,
                "stale asset cache swept"
            );
        }
        let ctx = self.fp_ctx();
        let mut planned: Vec<(crate::kinds::PrewarmStep, JobKey)> = Vec::new();
        for (kind, b) in &self.builders {
            let skip = kind.startup()
                || (*kind == AssetKind::Gzip && self.cfg.gzip_raw != GzipRawMode::All)
                || (matches!(kind, AssetKind::LandPack | AssetKind::SkeletonPack)
                    && !self.cfg.packs);
            if skip {
                continue;
            }
            for job in b.jobs(&ctx) {
                if let Some(step) = prewarm_step(&job) {
                    planned.push((step, job));
                }
            }
        }
        planned.sort_by_key(|(step, _)| *step); // stable: the builders' own order inside a step
        for (_, job) in planned {
            if self.closing.load(Ordering::Acquire) {
                break;
            }
            if !job.kind.required() && !self.cache.below_prewarm_limit() {
                continue;
            }
            let Some(partfp) = self.job_partfp(&job) else {
                continue;
            };
            if self.job_done(&job, partfp) || self.failed_error(&job, partfp).is_some() {
                continue;
            }
            self.enqueue_detached(job, partfp, Priority::Prewarm, false);
        }
        let ended = self.progress.finished(Priority::Prewarm);
        self.publish_depth();
        if ended {
            self.on_wave_end();
        }
    }

    // ---- batch runs (prepare, start-up) -------------------------------------------------------

    /// Queues `jobs` at `prio`, skipping the ones that are already done, and waits for all of them.
    /// `each` is called after every finished job.
    fn run_jobs(
        self: &Arc<Self>,
        jobs: Vec<JobKey>,
        prio: Priority,
        each: &mut dyn FnMut(&JobKey, &Result<JobSummary, AssetError>),
    ) -> Vec<JobResultMsg> {
        let (tx, rx) = flume::unbounded::<JobResultMsg>();
        let mut results = Vec::with_capacity(jobs.len());
        let mut pending = 0usize;
        for job in jobs {
            let Some(partfp) = self.job_partfp(&job) else {
                let e = AssetError::not_found(format!("unknown partition for job {job}"));
                each(&job, &Err(e.clone()));
                results.push((job, Err(e)));
                continue;
            };
            if self.job_done(&job, partfp) {
                let r = Ok(JobSummary {
                    files: 0,
                    bytes: 0,
                    cached: true,
                    ms: 0,
                });
                self.telemetry.job(job.kind, JobResult::Cached);
                each(&job, &r);
                results.push((job, r));
                continue;
            }
            if self.closing.load(Ordering::Acquire) {
                let r = Err(AssetError::shutting_down());
                each(&job, &r);
                results.push((job, r));
                continue;
            }
            pending += 1;
            self.enqueue(
                job,
                partfp,
                prio,
                Waiter::Job {
                    prio,
                    tx: tx.clone(),
                },
                None,
            );
        }
        drop(tx);
        for _ in 0..pending {
            let Ok((job, r)) = rx.recv() else { break };
            each(&job, &r);
            results.push((job, r));
        }
        results
    }
}

/// Compresses and writes the files of one job to `tmp/`, in order, on up to `threads` threads
/// (round-robin partition; the result keeps the input order).
fn stage_files(
    cache: &Cache,
    kind: AssetKind,
    partfp: Fp,
    files: Vec<BuiltFile>,
    threads: usize,
) -> Vec<Result<Staged, AssetError>> {
    let n = files.len();
    let threads = threads.clamp(1, n.max(1));
    if threads == 1 {
        return files
            .into_iter()
            .map(|f| cache.stage_built(kind, partfp, f))
            .collect();
    }
    let mut lanes: Vec<Vec<(usize, BuiltFile)>> = (0..threads).map(|_| Vec::new()).collect();
    for (i, f) in files.into_iter().enumerate() {
        if let Some(lane) = lanes.get_mut(i % threads) {
            lane.push((i, f));
        }
    }
    let mut out: Vec<Option<Result<Staged, AssetError>>> = (0..n).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = lanes
            .into_iter()
            .map(|lane| {
                scope.spawn(move || {
                    lane.into_iter()
                        .map(|(i, f)| (i, cache.stage_built(kind, partfp, f)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            match h.join() {
                Ok(done) => {
                    for (i, r) in done {
                        if let Some(slot) = out.get_mut(i) {
                            *slot = Some(r);
                        }
                    }
                }
                Err(_) => tracing::error!("asset staging thread panicked"),
            }
        }
    });
    out.into_iter()
        .map(|r| r.unwrap_or_else(|| Err(AssetError::build("a staging thread failed"))))
        .collect()
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            AssetError::io(path.display(), &e)
        })
}

// ---- AssetPrep ----------------------------------------------------------------------------------

impl AssetPrep {
    /// Starts the framework with the builders compiled into this crate ([`build::default_builders`])
    /// that `cfg.features` enables (Plan/24 §11.1 steps 1-4): opens the cache, registers the builders,
    /// runs their `on_start` hooks and starts the niced pool.
    pub fn start(
        vfs: Arc<Vfs>,
        data: Arc<GameData>,
        cfg: AssetConfig,
        clock: Arc<dyn Clock>,
        telemetry: Arc<AssetTelemetry>,
    ) -> Result<AssetPrep, AssetError> {
        Self::start_with(vfs, data, cfg, clock, telemetry, |parts| {
            build::default_builders(parts)
        })
    }

    /// Like [`Self::start`] with an explicit builder list: `make_builders` receives the shared
    /// [`ManifestParts`] and returns the builders to register (tests pass `EchoBuilder`s; the server
    /// adds the builders other crates own, such as the data-bundle exporter). Builders of kinds that
    /// `cfg.features` disables are dropped; two builders of one kind are an error.
    pub fn start_with(
        vfs: Arc<Vfs>,
        data: Arc<GameData>,
        cfg: AssetConfig,
        clock: Arc<dyn Clock>,
        telemetry: Arc<AssetTelemetry>,
        make_builders: impl FnOnce(&Arc<ManifestParts>) -> Vec<Arc<dyn AssetBuilder>>,
    ) -> Result<AssetPrep, AssetError> {
        let cache = Cache::open(&cfg.cache_dir, cfg.disk_budget_bytes, Arc::clone(&clock))?;
        let parts = Arc::new(ManifestParts::new());
        let mut builders: BTreeMap<AssetKind, Arc<dyn AssetBuilder>> = BTreeMap::new();
        for b in make_builders(&parts) {
            let kind = b.kind();
            if !cfg.features.contains(kind) {
                tracing::debug!(%kind, "asset kind not enabled at this milestone");
                continue;
            }
            if builders.insert(kind, b).is_some() {
                return Err(AssetError::build(format!(
                    "two builders registered for asset kind {kind}"
                )));
            }
        }
        if cfg.fingerprint_mode == FingerprintMode::Content {
            tracing::warn!(
                "assets.fingerprint_mode = \"content\" is not implemented yet (WP-WEB-23); using stat fingerprints"
            );
        }
        let pool = AssetPool::start(cfg.resolved_threads(), cfg.nice)
            .map_err(|e| AssetError::new(AssetErrorKind::Io, format!("asset pool: {e}")))?;
        let last_report = cache_report_file(&cache);
        let manifest = ClientManifest::empty(&cfg.server_build);
        let inner = Arc::new(Inner {
            vfs,
            data,
            clock,
            telemetry,
            cache,
            builders,
            parts,
            pool: pool.handle(),
            inflight: Mutex::new(DetMap::default()),
            failed: Mutex::new(BTreeMap::new()),
            progress: Progress::new(),
            acc: Mutex::new(ReportAcc::default()),
            cancel: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            prewarm_paused_until_ms: AtomicU64::new(0),
            manifest: RwLock::new(Arc::new(manifest)),
            bundles: RwLock::new(Arc::new(BundleIndex::default())),
            last_report: Mutex::new(last_report),
            keep_fps: Mutex::new(BTreeSet::new()),
            cfg,
        });
        let prep = AssetPrep {
            inner,
            pool: Arc::new(pool),
        };
        // Step 4: the builders' start-up hooks, in kind order.
        let ctx = prep.inner.build_ctx();
        for (kind, b) in &prep.inner.builders {
            if let Err(e) = b.on_start(&ctx, &prep.inner.cache) {
                if kind.startup() {
                    return Err(AssetError::build(format!("start-up of {kind} failed: {e}")));
                }
                tracing::warn!(%kind, error = %e, "asset kind start-up hook failed");
            }
        }
        prep.inner.telemetry.sync_cache(&prep.inner.cache);
        Ok(prep)
    }

    /// The registered kinds, in [`AssetKind::ALL`] order.
    pub fn registered_kinds(&self) -> Vec<AssetKind> {
        self.inner.builders.keys().copied().collect()
    }

    /// The builder of `kind`.
    pub fn builder(&self, kind: AssetKind) -> Option<Arc<dyn AssetBuilder>> {
        self.inner.builders.get(&kind).cloned()
    }

    /// The configuration.
    pub fn config(&self) -> &AssetConfig {
        &self.inner.cfg
    }

    /// The resource index.
    pub fn vfs(&self) -> &Arc<Vfs> {
        &self.inner.vfs
    }

    /// The game data.
    pub fn data(&self) -> &Arc<GameData> {
        &self.inner.data
    }

    /// The cache directory handle (budget counters, atomic writes for `index/` files).
    pub fn cache(&self) -> &Cache {
        &self.inner.cache
    }

    /// The telemetry counters (the gauges are refreshed by the pool workers as jobs move; use
    /// [`Self::telemetry_snapshot`] for an exact reading).
    pub fn telemetry(&self) -> &Arc<AssetTelemetry> {
        &self.inner.telemetry
    }

    /// A snapshot after refreshing the queue-depth and cache-size gauges from the live state: what
    /// `/metrics` should render.
    pub fn telemetry_snapshot(&self) -> crate::telemetry::TelemetrySnapshot {
        let i = &self.inner;
        i.publish_depth();
        i.telemetry.sync_cache(&i.cache);
        i.telemetry.snapshot()
    }

    /// The manifest parts builders contribute to.
    pub fn manifest_parts(&self) -> &Arc<ManifestParts> {
        &self.inner.parts
    }

    /// Fingerprint helpers over the framework's VFS and game data.
    pub fn fp_ctx(&self) -> FpCtx<'_> {
        self.inner.fp_ctx()
    }

    /// A [`BuildCtx`] for calling a builder's functions directly (not cancelled by `shutdown`).
    pub fn build_ctx(&self) -> BuildCtx<'_> {
        self.inner.build_ctx()
    }

    /// The pool worker count.
    pub fn threads(&self) -> usize {
        self.pool.threads()
    }

    /// Keeps files in `server/` and `index/` whose names embed `fp` when sweeping (for builders whose
    /// index file is named after a fingerprint that is not a partition fingerprint).
    pub fn keep_fingerprint(&self, fp: Fp) {
        lock(&self.inner.keep_fps).insert(fp);
    }

    /// The client manifest ([`ClientManifest::empty`] until [`Self::set_manifest`], WP-WEB-05/06).
    pub fn manifest(&self) -> Arc<ClientManifest> {
        match self.inner.manifest.read() {
            Ok(g) => Arc::clone(&g),
            Err(p) => Arc::clone(&p.into_inner()),
        }
    }

    /// Installs the assembled manifest (once, after `load_server_derived`).
    pub fn set_manifest(&self, m: Arc<ClientManifest>) {
        match self.inner.manifest.write() {
            Ok(mut g) => *g = m,
            Err(p) => *p.into_inner() = m,
        }
    }

    /// The `/data` bundle lookup table (empty until [`Self::set_bundles`], WP-WEB-05).
    pub fn bundles(&self) -> Arc<BundleIndex> {
        match self.inner.bundles.read() {
            Ok(g) => Arc::clone(&g),
            Err(p) => Arc::clone(&p.into_inner()),
        }
    }

    /// Installs the bundle lookup table.
    pub fn set_bundles(&self, b: Arc<BundleIndex>) {
        match self.inner.bundles.write() {
            Ok(mut g) => *g = b,
            Err(p) => *p.into_inner() = b,
        }
    }

    /// The current fingerprint of `partition` of `kind`; `None` for an unknown partition or an
    /// unregistered kind.
    pub fn partition_fp(&self, kind: AssetKind, partition: &str) -> Option<Fp> {
        self.inner
            .builders
            .get(&kind)
            .and_then(|b| b.partition_fp(&self.inner.fp_ctx(), partition))
    }

    /// Cache lookup without building: path computation plus up to two `stat` calls. `Ok(None)` is a
    /// miss; unregistered kinds are `Disabled`, malformed keys `BadKey`.
    pub fn lookup(&self, kind: AssetKind, key: &str) -> Result<Option<CachedFile>, AssetError> {
        let r = self.inner.resolve(kind, key)?;
        self.inner.lookup_resolved(&r, key)
    }

    /// The artifact for `key`, building it if needed (C8). Never blocks: wait on the result with
    /// `recv`, `recv_timeout` or `recv_async`. Concurrent requests for the same job share one build; a
    /// request with a higher priority promotes a queued job.
    pub fn get(
        &self,
        kind: AssetKind,
        key: &str,
        prio: Priority,
    ) -> ReplyRx<Result<CachedFile, AssetError>> {
        let (tx, rx) = reply_pair();
        self.inner.get(kind, key, prio, tx);
        rx
    }

    /// [`Self::get`] and wait up to `timeout` (`Timeout` on expiry; the job keeps running).
    pub fn get_blocking(
        &self,
        kind: AssetKind,
        key: &str,
        prio: Priority,
        timeout: Duration,
    ) -> Result<CachedFile, AssetError> {
        match self.get(kind, key, prio).recv_timeout(timeout) {
            Ok(r) => r,
            Err(ReplyError::Empty) => Err(AssetError::new(
                AssetErrorKind::Timeout,
                format!("{kind} {key:?} is still building"),
            )),
            Err(ReplyError::Disconnected) => Err(AssetError::shutting_down()),
        }
    }

    /// The gzip variant of a raw file (§5.3 step 6). Anything that cannot have a variant (not
    /// eligible, `assets.gzip_raw = "off"`, `gz` not registered) is `NotBeneficial`.
    pub fn res_gz(&self, e: &VfsEntry) -> GzState {
        let i = &self.inner;
        if i.cfg.gzip_raw == GzipRawMode::Off
            || !i.builders.contains_key(&AssetKind::Gzip)
            || !build::gz::is_eligible(e.path.as_str(), e.size)
        {
            return GzState::NotBeneficial;
        }
        let ct = crate::kinds::content_type_by_ext(e.path.as_str());
        match i.cache.lookup_res_gz(e.path.as_str(), file_fp(e), ct) {
            GzLookup::Present(cf) => GzState::Present(cf),
            GzLookup::NotBeneficial => GzState::NotBeneficial,
            GzLookup::Unknown => GzState::Unknown,
        }
    }

    /// Queues the gzip variant of `e` at Prewarm priority (de-duplicated, fire and forget). A no-op
    /// when the file is not eligible, the mode is `off`, the variant already exists or the cache is
    /// at its budget.
    pub fn request_gz(&self, e: &VfsEntry) {
        let i = &self.inner;
        if i.cfg.gzip_raw == GzipRawMode::Off
            || !i.builders.contains_key(&AssetKind::Gzip)
            || !build::gz::is_eligible(e.path.as_str(), e.size)
            || !i.cache.below_budget()
            || i.closing.load(Ordering::Acquire)
        {
            return;
        }
        let fp = file_fp(e);
        if !matches!(
            i.cache
                .lookup_res_gz(e.path.as_str(), fp, "application/octet-stream"),
            GzLookup::Unknown
        ) {
            return;
        }
        if i.failed_error(
            &JobKey::new(AssetKind::Gzip, e.path.parent(), e.path.as_str()),
            fp,
        )
        .is_some()
        {
            return;
        }
        let job = JobKey::new(AssetKind::Gzip, e.path.parent(), e.path.as_str());
        i.enqueue_detached(job, fp, Priority::Prewarm, true);
    }

    /// Starts the background prewarm (Plan/24 §7.5): one Prewarm job that sweeps the cache (P1) and
    /// then queues the jobs of steps P2-P8 in order. A no-op unless `assets.prewarm`. Call it after
    /// the listener is bound.
    pub fn prewarm(&self) {
        let i = &self.inner;
        if !i.cfg.prewarm || i.closing.load(Ordering::Acquire) {
            return;
        }
        let id = i.pool.next_id();
        if i.progress.enqueued(Priority::Prewarm) {
            lock(&i.acc).reset();
        }
        lock(&i.acc).saw_prewarm = true;
        let this = Arc::clone(i);
        if i.pool
            .submit_keyed(id, Priority::Prewarm, move || this.plan_prewarm())
            .is_err()
        {
            i.progress.dropped(Priority::Prewarm);
        }
    }

    /// Queues the stale sweep (P1) as a Prewarm job, whether or not `assets.prewarm` is on: for
    /// servers that run without prewarm but still want stale partitions and variants removed after a
    /// start. `prewarm()` already includes it.
    pub fn sweep_in_background(&self) {
        let i = &self.inner;
        if i.closing.load(Ordering::Acquire) {
            return;
        }
        let id = i.pool.next_id();
        if i.progress.enqueued(Priority::Prewarm) {
            lock(&i.acc).reset();
        }
        let this = Arc::clone(i);
        let submitted = i.pool.submit_keyed(id, Priority::Prewarm, move || {
            this.progress
                .started(Priority::Prewarm, None, "cache sweep");
            let st = this.sweep();
            if st.deleted_files > 0 {
                tracing::info!(
                    files = st.deleted_files,
                    bytes = st.deleted_bytes,
                    "stale asset cache swept"
                );
            }
            if this.progress.finished(Priority::Prewarm) {
                this.on_wave_end();
            }
        });
        if submitted.is_err() {
            i.progress.dropped(Priority::Prewarm);
        }
    }

    /// The stale sweep of §8.5 run to completion on the calling thread: removes partitions that are
    /// not current, unknown kind directories, stale `res-gz` variants and stale fingerprinted files.
    pub fn sweep(&self) -> SweepStats {
        self.inner.sweep()
    }

    /// The progress record of the current (or last) wave (`GET /api/prepare`).
    pub fn progress(&self) -> PrepareProgress {
        self.inner.progress.snapshot()
    }

    /// Increases with every change of [`Self::progress`].
    pub fn progress_version(&self) -> u64 {
        self.inner.progress.version()
    }

    /// Runs the jobs of the start-up kinds in dependency order (Plan/24 §11.1 step 5: bundles, then
    /// motion table + bounds, then collision, then ctrl models + rigs) on temporary un-niced
    /// `flyff-startup-*` workers, then calls each builder's `load_server` hook. A failure of
    /// `data-bundles`, `motion-table` or `model-bounds` is fatal and names the job; collision, ctrl
    /// model and rig failures are logged and skipped. With no start-up kind registered the result is
    /// empty.
    pub fn load_server_derived(&self) -> Result<ServerDerived, AssetError> {
        let i = &self.inner;
        let mut derived = ServerDerived::default();
        if !AssetKind::ALL
            .iter()
            .any(|k| k.startup() && i.builders.contains_key(k))
        {
            return Ok(derived);
        }
        let workers = self
            .pool
            .start_startup_workers(i.cfg.resolved_startup_threads())
            .map_err(|_| AssetError::shutting_down())?;
        let ctx = i.fp_ctx();
        let outcome = (|| -> Result<(), AssetError> {
            for stage in 0..=3u8 {
                let kinds: Vec<AssetKind> = AssetKind::ALL
                    .iter()
                    .copied()
                    .filter(|k| k.startup_stage() == Some(stage) && i.builders.contains_key(k))
                    .collect();
                let jobs: Vec<JobKey> = kinds
                    .iter()
                    .filter_map(|k| i.builders.get(k))
                    .flat_map(|b| b.jobs(&ctx))
                    .collect();
                for (job, r) in i.run_jobs(jobs, Priority::Startup, &mut |_, _| {}) {
                    if let Err(e) = r {
                        let fatal = matches!(
                            job.kind,
                            AssetKind::DataBundle | AssetKind::MotionTable | AssetKind::ModelBounds
                        );
                        if fatal {
                            return Err(AssetError::build(format!(
                                "required asset {job} could not be prepared: {e}"
                            )));
                        }
                        tracing::error!(%job, error = %e, "start-up asset job failed");
                    }
                }
                for kind in kinds {
                    let Some(b) = i.builders.get(&kind) else {
                        continue;
                    };
                    match b.load_server(self, &mut derived) {
                        Ok(()) => {
                            derived.loaded.insert(kind);
                        }
                        Err(e)
                            if matches!(
                                kind,
                                AssetKind::DataBundle
                                    | AssetKind::MotionTable
                                    | AssetKind::ModelBounds
                            ) =>
                        {
                            return Err(AssetError::build(format!("loading {kind} failed: {e}")));
                        }
                        Err(e) => {
                            tracing::error!(%kind, error = %e, "loading derived server data failed")
                        }
                    }
                }
            }
            Ok(())
        })();
        workers.finish();
        i.cache.flush_pending();
        i.telemetry.sync_cache(&i.cache);
        outcome.map(|()| derived)
    }

    /// `flyff prepare` (Plan/24 §11.3): sweeps, optionally removes the selected kinds' current
    /// partitions (`force`), builds every missing job of the selected registered kinds in dependency
    /// order at Startup priority and writes the report (`opts.report_path` or
    /// `cache/prepare-report.json`). `progress` is called after every finished job. Kinds that are not
    /// registered are listed in `PrepareReport::disabled`. Already finished jobs count as `cached`.
    pub fn prepare_all(
        &self,
        opts: &PrepareOptions,
        progress: &dyn Fn(&PrepareProgress),
    ) -> PrepareReport {
        let i = &self.inner;
        let mut disabled = Vec::new();
        let mut selected: Vec<AssetKind> = Vec::new();
        if opts.kinds.is_empty() {
            for k in AssetKind::ALL {
                if !i.builders.contains_key(&k) {
                    continue;
                }
                let skip = (k == AssetKind::Gzip
                    && (!opts.gzip || i.cfg.gzip_raw == GzipRawMode::Off))
                    || (matches!(k, AssetKind::LandPack | AssetKind::SkeletonPack) && !i.cfg.packs);
                if !skip {
                    selected.push(k);
                }
            }
        } else {
            for k in AssetKind::ALL {
                if opts.kinds.contains(&k) {
                    if i.builders.contains_key(&k) {
                        if !(k == AssetKind::Gzip && !opts.gzip) {
                            selected.push(k);
                        }
                    } else {
                        disabled.push(k.slug().to_owned());
                    }
                }
            }
        }

        let st = i.sweep();
        if st.deleted_files > 0 {
            tracing::info!(
                files = st.deleted_files,
                bytes = st.deleted_bytes,
                "stale asset cache swept"
            );
        }
        let ctx = i.fp_ctx();
        let mut jobs_by_kind: BTreeMap<AssetKind, Vec<JobKey>> = BTreeMap::new();
        for k in &selected {
            let Some(b) = i.builders.get(k) else { continue };
            let mut jobs = b.jobs(&ctx);
            if !opts.worlds.is_empty()
                && matches!(k, AssetKind::CollisionChunk | AssetKind::LandPack)
            {
                jobs.retain(|j| {
                    opts.worlds
                        .iter()
                        .any(|w| w.eq_ignore_ascii_case(&j.partition))
                });
            }
            jobs_by_kind.insert(*k, jobs);
        }
        if opts.force {
            for (k, jobs) in &jobs_by_kind {
                if *k == AssetKind::Gzip {
                    i.cache.remove_partition(*k, Fp::ZERO);
                    continue;
                }
                let parts: BTreeSet<Fp> = jobs.iter().filter_map(|j| i.job_partfp(j)).collect();
                for fp in parts {
                    i.cache.remove_partition(*k, fp);
                }
            }
            i.cache.clear_positive();
        }

        let workers = (opts.threads > 0)
            .then(|| self.pool.start_startup_workers(opts.threads).ok())
            .flatten();
        let mut acc = ReportAcc::default();
        // Dependency order: start-up stages first, client-only kinds last (stage 4).
        for stage in 0..=4u8 {
            let jobs: Vec<JobKey> = selected
                .iter()
                .filter(|k| k.startup_stage().unwrap_or(4) == stage)
                .filter_map(|k| jobs_by_kind.remove(k))
                .flatten()
                .collect();
            if jobs.is_empty() {
                continue;
            }
            progress(&i.progress.snapshot());
            i.run_jobs(jobs, Priority::Startup, &mut |job, r| {
                match r {
                    Ok(s) if s.cached => acc.cached(job.kind),
                    Ok(s) => acc.built(job.kind, s.bytes, s.ms),
                    Err(e)
                        if e.kind == AssetErrorKind::OverBudget
                            || e.kind == AssetErrorKind::ShuttingDown =>
                    {
                        acc.skipped(job.kind);
                    }
                    Err(e) => acc.failed(job.kind, &job.name, &e.to_string(), 0),
                }
                progress(&i.progress.snapshot());
            });
        }
        if let Some(w) = workers {
            w.finish();
        }
        i.cache.flush_pending();
        // Nothing of ours is running now: correct any drift from another process sharing the cache.
        i.cache.recount_usage();
        i.telemetry.sync_cache(&i.cache);
        let mut report = acc.to_report(
            &i.cfg.server_build,
            i.clock.unix_s(),
            i.cache.budget_bytes(),
            i.cache.used_bytes(),
        );
        report.disabled = disabled;
        i.write_report(&report, opts.report_path.as_deref());
        progress(&i.progress.snapshot());
        report
    }

    /// [`Self::prepare_all`] for one kind; returns its counters (`None` when the kind is not
    /// registered).
    pub fn prepare_kind(
        &self,
        kind: AssetKind,
        opts: &PrepareOptions,
        progress: &dyn Fn(&PrepareProgress),
    ) -> Option<KindReport> {
        let mut o = opts.clone();
        o.kinds = vec![kind];
        let report = self.prepare_all(&o, progress);
        report.kinds.get(kind.slug()).cloned()
    }

    /// The cache part of `flyff check` (Plan/24 §11.4): per registered kind the jobs the builder
    /// lists now, how many are done, how many failed in the last run, and the bytes.
    pub fn cache_report(&self) -> CacheReport {
        let i = &self.inner;
        i.cache.flush_pending();
        let ctx = i.fp_ctx();
        let last = lock(&i.last_report).clone();
        let mut kinds = Vec::new();
        for (kind, b) in &i.builders {
            let jobs = b.jobs(&ctx);
            let present = if *kind == AssetKind::Gzip {
                jobs.iter()
                    .filter(|j| i.job_partfp(j).is_some_and(|fp| i.job_done(j, fp)))
                    .count() as u64
            } else {
                jobs.iter()
                    .filter(|j| {
                        i.job_partfp(j)
                            .is_some_and(|fp| i.cache.job_done(*kind, fp, &j.name))
                    })
                    .count() as u64
            };
            let failed_now = lock(&i.failed).keys().filter(|j| j.kind == *kind).count() as u64;
            let failed_last = last
                .as_ref()
                .and_then(|r| r.kinds.get(kind.slug()))
                .map_or(0, |k| k.failed);
            kinds.push(KindCacheReport {
                kind: kind.slug().to_owned(),
                expected_jobs: jobs.len() as u64,
                present,
                failed: failed_now.max(failed_last),
                bytes: i.cache.kind_bytes(*kind),
            });
        }
        CacheReport {
            fingerprint_mode: match i.cfg.fingerprint_mode {
                FingerprintMode::Stat => "stat",
                FingerprintMode::Content => "content",
            }
            .to_owned(),
            kinds,
            total_bytes: i.cache.used_bytes(),
            budget_bytes: i.cache.budget_bytes(),
            over_budget: i.cache.used_bytes() > i.cache.budget_bytes(),
            last_report: last,
        }
    }

    /// Graceful stop (01 §6.10 step 5): refuses new work, drops queued Prewarm jobs (their waiters get
    /// `ShuttingDown`), lets running jobs finish their atomic write (builders see
    /// `BuildCtx::cancelled`), makes deferred writes durable and joins the pool within `deadline`.
    /// Returns `true` when every worker finished in time.
    pub fn shutdown(&self, deadline: Duration) -> bool {
        let i = &self.inner;
        i.closing.store(true, Ordering::Release);
        i.cancel.store(true, Ordering::Release);
        // Closing and draining are one step: a job that raced with the shutdown is either rejected by
        // `submit_keyed` (and answered there) or is in `ids` here, never silently lost.
        let ids = self.pool.close_and_drain();
        i.fail_dropped(&ids);
        let finished = self.pool.shutdown_timeout(deadline, &*i.clock);
        i.cache.flush_pending();
        i.telemetry.sync_cache(&i.cache);
        finished
    }
}

fn cache_report_file(cache: &Cache) -> Option<PrepareReport> {
    let bytes = std::fs::read(cache.path_of("prepare-report.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}
