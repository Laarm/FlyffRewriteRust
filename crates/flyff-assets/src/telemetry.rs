//! `AssetTelemetry` (Plan/24 §8.10): lock-free counters the server's `/metrics` renders as
//! `flyff_asset_jobs_total{kind,result}`, `flyff_asset_build_seconds{kind}`,
//! `flyff_asset_queue_depth{priority}`, `flyff_asset_inflight`, `flyff_asset_cache_bytes{kind}` and
//! `flyff_asset_write_errors_total`.

use crate::kinds::AssetKind;
use flyff_common::rt::telemetry::{Histogram, HistogramSnapshot};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The `result` label of `flyff_asset_jobs_total`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobResult {
    /// The builder ran and its files were stored.
    Built,
    /// The job was already done (prepare skipped it).
    Cached,
    /// The builder failed or panicked.
    Failed,
    /// Dropped without running (budget, shutdown, paused prewarm).
    Skipped,
}

impl JobResult {
    /// All results, in label order.
    pub const ALL: [JobResult; 4] = [
        JobResult::Built,
        JobResult::Cached,
        JobResult::Failed,
        JobResult::Skipped,
    ];

    /// Label value.
    pub const fn label(self) -> &'static str {
        match self {
            JobResult::Built => "built",
            JobResult::Cached => "cached",
            JobResult::Failed => "failed",
            JobResult::Skipped => "skipped",
        }
    }

    const fn index(self) -> usize {
        match self {
            JobResult::Built => 0,
            JobResult::Cached => 1,
            JobResult::Failed => 2,
            JobResult::Skipped => 3,
        }
    }
}

/// The counters (atomics, written by the pool workers, read by `/metrics`).
#[derive(Debug)]
pub struct AssetTelemetry {
    jobs: [[AtomicU64; 4]; AssetKind::COUNT],
    build: [Histogram; AssetKind::COUNT],
    queue_depth: [AtomicU64; 3],
    inflight: AtomicU64,
    cache_bytes: [AtomicU64; AssetKind::COUNT + 1],
    write_errors: AtomicU64,
}

impl Default for AssetTelemetry {
    fn default() -> Self {
        Self {
            jobs: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
            build: std::array::from_fn(|_| Histogram::default()),
            queue_depth: std::array::from_fn(|_| AtomicU64::new(0)),
            inflight: AtomicU64::new(0),
            cache_bytes: std::array::from_fn(|_| AtomicU64::new(0)),
            write_errors: AtomicU64::new(0),
        }
    }
}

/// A copy of the counters for exposition.
#[derive(Clone, Debug)]
pub struct TelemetrySnapshot {
    /// `jobs[kind.index()][result]` in [`JobResult::ALL`] order.
    pub jobs: [[u64; 4]; AssetKind::COUNT],
    /// Build durations per kind.
    pub build: Vec<HistogramSnapshot>,
    /// Queued jobs per priority (`startup`, `interactive`, `prewarm`).
    pub queue_depth: [u64; 3],
    /// Distinct jobs queued or running.
    pub inflight: u64,
    /// Cache bytes per kind; the last entry is everything else.
    pub cache_bytes: [u64; AssetKind::COUNT + 1],
    /// Failed cache writes.
    pub write_errors: u64,
}

impl AssetTelemetry {
    /// A shared, zeroed set of counters.
    pub fn new() -> Arc<AssetTelemetry> {
        Arc::new(AssetTelemetry::default())
    }

    /// Counts a finished job.
    pub fn job(&self, kind: AssetKind, result: JobResult) {
        if let Some(c) = self
            .jobs
            .get(kind.index())
            .and_then(|r| r.get(result.index()))
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a build duration.
    pub fn observe_build_ns(&self, kind: AssetKind, ns: u64) {
        if let Some(h) = self.build.get(kind.index()) {
            h.observe_ns(ns);
        }
    }

    pub(crate) fn inflight_add(&self, delta: i64) {
        if delta >= 0 {
            self.inflight
                .fetch_add(delta.unsigned_abs(), Ordering::Relaxed);
        } else {
            let _ = self
                .inflight
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(delta.unsigned_abs()))
                });
        }
    }

    pub(crate) fn set_queue_depth(&self, depth: [usize; 3]) {
        for (slot, d) in self.queue_depth.iter().zip(depth) {
            slot.store(d as u64, Ordering::Relaxed);
        }
    }

    pub(crate) fn sync_cache(&self, cache: &crate::cache::Cache) {
        for k in AssetKind::ALL {
            if let Some(slot) = self.cache_bytes.get(k.index()) {
                slot.store(cache.kind_bytes(k), Ordering::Relaxed);
            }
        }
        if let Some(slot) = self.cache_bytes.get(AssetKind::COUNT) {
            slot.store(cache.misc_bytes(), Ordering::Relaxed);
        }
        self.write_errors
            .store(cache.write_errors(), Ordering::Relaxed);
    }

    /// Copies every counter.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        TelemetrySnapshot {
            jobs: std::array::from_fn(|k| {
                std::array::from_fn(|r| {
                    self.jobs
                        .get(k)
                        .and_then(|row| row.get(r))
                        .map_or(0, |c| c.load(Ordering::Relaxed))
                })
            }),
            build: self.build.iter().map(Histogram::snapshot).collect(),
            queue_depth: std::array::from_fn(|i| {
                self.queue_depth
                    .get(i)
                    .map_or(0, |c| c.load(Ordering::Relaxed))
            }),
            inflight: self.inflight.load(Ordering::Relaxed),
            cache_bytes: std::array::from_fn(|i| {
                self.cache_bytes
                    .get(i)
                    .map_or(0, |c| c.load(Ordering::Relaxed))
            }),
            write_errors: self.write_errors.load(Ordering::Relaxed),
        }
    }
}
