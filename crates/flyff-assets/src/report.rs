//! Prepare and cache reports (Plan/24 §8.9, §11.3, §11.4).
//!
//! `cache/prepare-report.json` is rewritten after `flyff prepare` and after each finished prewarm wave:
//!
//! ```json
//! { "format": 1, "server_build": "...", "finished_at": 1760000000,
//!   "kinds": { "png": { "jobs": 1712, "built": 1700, "cached": 12, "failed": 0, "bytes": 51234567, "ms": 48211 } },
//!   "failures": [ { "kind": "dds", "key": "sfx/texture/x.dds", "error": "build: bad DDS magic" } ],
//!   "budget": { "limit_bytes": 838860800, "used_bytes": 712345678 } }
//! ```

use crate::kinds::AssetKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Options of `AssetPrep::prepare_all` (Plan/24 §11.3: `--kind`, `--world`, `--threads`, `--force`,
/// `--no-gzip`, `--report`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareOptions {
    /// Only these kinds (empty = every registered kind).
    pub kinds: Vec<AssetKind>,
    /// Restricts `collision` and the land packs to these world folders (empty = all).
    pub worlds: Vec<String>,
    /// `> 0`: run the jobs on a temporary set of this many un-niced workers; `0`: on the pool that
    /// `AssetPrep::start` created (the CLI sizes it from `--threads`, not niced).
    pub threads: usize,
    /// Delete the current partitions of the selected kinds before building.
    pub force: bool,
    /// Build the `gz` variants (`false` = `--no-gzip`).
    pub gzip: bool,
    /// Where to write the report (`None` = `cache/prepare-report.json`).
    pub report_path: Option<PathBuf>,
}

impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            kinds: Vec::new(),
            worlds: Vec::new(),
            threads: 0,
            force: false,
            gzip: true,
            report_path: None,
        }
    }
}

/// Per-kind counters of a report.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindReport {
    /// Jobs considered.
    pub jobs: u64,
    /// Jobs built in this run.
    pub built: u64,
    /// Jobs that were already done.
    pub cached: u64,
    /// Jobs that failed.
    pub failed: u64,
    /// Bytes stored by the built jobs.
    pub bytes: u64,
    /// Build time summed over the jobs (ms).
    pub ms: u64,
}

/// One failed job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureEntry {
    /// Kind slug.
    pub kind: String,
    /// Job name (the key or source path).
    pub key: String,
    /// `<error kind>: <detail>`.
    pub error: String,
}

/// Budget usage at the end of a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetReport {
    /// `assets.disk_budget_mb` in bytes.
    pub limit_bytes: u64,
    /// Cache bytes.
    pub used_bytes: u64,
}

/// The `prepare-report.json` document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareReport {
    /// Format version, `1`.
    pub format: u32,
    /// `flyff <version>+<commit>`.
    pub server_build: String,
    /// Unix seconds from the injected clock (`0` when the clock has no wall time).
    pub finished_at: u64,
    /// Counters by kind slug.
    pub kinds: BTreeMap<String, KindReport>,
    /// Failed jobs.
    pub failures: Vec<FailureEntry>,
    /// Budget usage.
    pub budget: BudgetReport,
    /// Selected kinds that are not registered at this milestone (`flyff prepare` prints "nothing to
    /// prepare" when this is everything).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
}

impl PrepareReport {
    /// `true` when at least one job failed (`flyff prepare` exit code 1).
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty() || self.kinds.values().any(|k| k.failed > 0)
    }

    /// `flyff prepare`'s exit code for this report: 0 or 1 (2 and 66 are decided by the caller).
    pub fn exit_code(&self) -> i32 {
        i32::from(self.has_failures())
    }

    /// Total jobs over all kinds.
    pub fn total_jobs(&self) -> u64 {
        self.kinds.values().map(|k| k.jobs).sum()
    }

    /// Deterministic pretty JSON (BTreeMap order).
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).unwrap_or_default()
    }
}

/// Accumulates per-kind counters while jobs run (one per wave or prepare run).
#[derive(Default)]
pub(crate) struct ReportAcc {
    pub kinds: BTreeMap<AssetKind, KindReport>,
    pub failures: Vec<FailureEntry>,
    /// The wave contained Prewarm-class jobs (the report is rewritten at the end of such waves).
    pub saw_prewarm: bool,
}

impl ReportAcc {
    pub fn reset(&mut self) {
        *self = ReportAcc::default();
    }

    fn entry(&mut self, kind: AssetKind) -> &mut KindReport {
        self.kinds.entry(kind).or_default()
    }

    pub fn built(&mut self, kind: AssetKind, bytes: u64, ms: u64) {
        let e = self.entry(kind);
        e.jobs += 1;
        e.built += 1;
        e.bytes += bytes;
        e.ms += ms;
    }

    pub fn cached(&mut self, kind: AssetKind) {
        let e = self.entry(kind);
        e.jobs += 1;
        e.cached += 1;
    }

    pub fn failed(&mut self, kind: AssetKind, key: &str, error: &str, ms: u64) {
        let e = self.entry(kind);
        e.jobs += 1;
        e.failed += 1;
        e.ms += ms;
        self.failures.push(FailureEntry {
            kind: kind.slug().to_owned(),
            key: key.to_owned(),
            error: error.to_owned(),
        });
    }

    pub fn skipped(&mut self, kind: AssetKind) {
        self.entry(kind).jobs += 1;
    }

    pub fn to_report(
        &self,
        server_build: &str,
        finished_at: u64,
        limit: u64,
        used: u64,
    ) -> PrepareReport {
        PrepareReport {
            format: 1,
            server_build: server_build.to_owned(),
            finished_at,
            kinds: self
                .kinds
                .iter()
                .map(|(k, v)| (k.slug().to_owned(), v.clone()))
                .collect(),
            failures: self.failures.clone(),
            budget: BudgetReport {
                limit_bytes: limit,
                used_bytes: used,
            },
            disabled: Vec::new(),
        }
    }
}

/// One kind's row in `flyff check` (Plan/24 §11.4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindCacheReport {
    /// Kind slug.
    pub kind: String,
    /// Jobs the builder lists now.
    pub expected_jobs: u64,
    /// Of those, how many are done (marker present; for `gz`, variants present).
    pub present: u64,
    /// Failed jobs: from the last report on disk plus failures of this process.
    pub failed: u64,
    /// Bytes stored for the kind.
    pub bytes: u64,
}

/// The cache part of `flyff check` and `/admin`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheReport {
    /// `stat` or `content`.
    pub fingerprint_mode: String,
    /// One row per registered kind.
    pub kinds: Vec<KindCacheReport>,
    /// Total cache bytes (artifacts, markers, reports).
    pub total_bytes: u64,
    /// The budget.
    pub budget_bytes: u64,
    /// `total_bytes > budget_bytes` (a failure only with `flyff check --expect-reference`).
    pub over_budget: bool,
    /// The last `prepare-report.json`, when one exists.
    pub last_report: Option<PrepareReport>,
}
