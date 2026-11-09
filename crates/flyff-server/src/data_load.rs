//! Start-up step S6 (B): the VFS index and the game data (Plan/10 §3.1 S6, 14 §2, §5, §8).
//!
//! [`load`] opens the layered case-insensitive VFS over `Resource/` (+ `overrides/`), runs
//! `flyff_data::load_all` (P0-P8, client data included) and returns the immutable data plus a
//! [`DataLoadReport`] with the timings and the diagnostics digest that the start-up log and `flyff check`
//! print. A Fatal diagnostic is [`DataLoadError::Data`] (exit code 2); a missing `Resource/` root is
//! [`DataLoadError::ResourceMissing`] (66).
//!
//! `DataMode::Empty` is for tests that run against a fake `Resource/` (just a `Masquerade.prj`): the VFS is
//! opened leniently and the game data is `GameData::default()`.

use crate::config::{Config, ResolvedPaths};
use flyff_data::load::log::write_diagnostics_file;
use flyff_data::{ClientData, GameData};
use flyff_script::Severity;
use flyff_vfs::{Vfs, VfsError};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// Whether the game data is loaded (the production behaviour) or left empty (tests with a fake `Resource/`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DataMode {
    /// Load `GameData` and `ClientData` from `Resource/`; a Fatal diagnostic stops the start (exit 2).
    #[default]
    Load,
    /// Do not load anything: `GameData::default()`, no client data. The asset framework still runs.
    Empty,
}

/// Why the data could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum DataLoadError {
    /// `Resource/` is missing or has no `Masquerade.prj` (exit 66).
    #[error("the Resource folder is missing: {0}")]
    ResourceMissing(std::path::PathBuf),
    /// A Fatal condition of the loaders, or a VFS I/O error (exit 2).
    #[error("{0}")]
    Data(String),
}

/// Timings and the diagnostics digest of one load.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DataLoadReport {
    /// Files in the VFS index (Resource and overrides).
    pub vfs_files: usize,
    /// Files shadowed or added by `overrides/`.
    pub override_files: usize,
    /// Building the VFS index (ms).
    pub vfs_ms: u64,
    /// `load_all` (ms).
    pub load_ms: u64,
    /// `"Severity:Code"` -> count.
    pub diag_counts: BTreeMap<String, u32>,
    /// Diagnostics of severity Error.
    pub errors: u32,
    /// Diagnostics of severity Warning.
    pub warnings: u32,
    /// The Error diagnostics that are stored, as `code: message (file:line)` (the first 20).
    pub first_errors: Vec<String>,
    /// Loader steps that ran / are still stubs of a later work package.
    pub steps_done: u32,
    /// Steps that are stubs.
    pub steps_stub: u32,
    /// The data fingerprint (`GameData.meta.fingerprint`) as 32 hex digits.
    pub fingerprint: String,
}

/// The result of [`load`].
pub struct LoadedData {
    /// The VFS (shared with `AssetPrep`).
    pub vfs: Arc<Vfs>,
    /// The game data.
    pub game: Arc<GameData>,
    /// The client-only data (`None` in [`DataMode::Empty`]).
    pub client: Option<Arc<ClientData>>,
    /// Timings and counts.
    pub report: DataLoadReport,
}

/// Opens the VFS for `paths` (the layered index of 14 §2, `cp1252_lossy` from `[data]`).
pub fn open_vfs(cfg: &Config, paths: &ResolvedPaths, strict: bool) -> Result<Vfs, DataLoadError> {
    let vfs_cfg = flyff_vfs::VfsConfig::new(&paths.resource).with_overrides(&paths.overrides);
    let opened = if strict {
        Vfs::open(&vfs_cfg)
    } else {
        Vfs::open_lenient(&vfs_cfg)
    };
    let mut vfs = opened.map_err(|e| match e {
        VfsError::MissingRoot(p) => DataLoadError::ResourceMissing(p),
        other => DataLoadError::Data(format!(
            "cannot index {}: {other}",
            paths.resource.display()
        )),
    })?;
    vfs.set_cp1252_lossy(cfg.data.cp1252_lossy);
    Ok(vfs)
}

/// Step S6 (B): VFS index and game data. `diag_log` (when `Some`) receives `data-load.jsonl`.
pub fn load(
    cfg: &Config,
    paths: &ResolvedPaths,
    mode: DataMode,
    diag_log: Option<&Path>,
) -> Result<LoadedData, DataLoadError> {
    let t0 = Instant::now();
    let vfs = open_vfs(cfg, paths, mode == DataMode::Load)?;
    let vfs_ms = elapsed_ms(t0);
    let mut report = DataLoadReport {
        vfs_files: vfs.len(),
        override_files: vfs.override_entries().count(),
        vfs_ms,
        ..DataLoadReport::default()
    };
    if mode == DataMode::Empty {
        tracing::warn!(
            "game data is NOT loaded (test mode): the server runs with an empty GameData"
        );
        return Ok(LoadedData {
            vfs: Arc::new(vfs),
            game: Arc::new(GameData::default()),
            client: None,
            report,
        });
    }
    let t1 = Instant::now();
    let out = flyff_data::load_all(&vfs, &cfg.data_options()).map_err(|e| {
        if let Some(path) = diag_log {
            let _ = write_diagnostics_file(&e.diags, path);
        }
        DataLoadError::Data(e.message)
    })?;
    report.load_ms = elapsed_ms(t1);
    report.diag_counts = out.diags.summary().counts;
    report.errors = out.diags.count_at_least(Severity::Error);
    report.warnings = out
        .diags
        .count_at_least(Severity::Warning)
        .saturating_sub(report.errors);
    report.first_errors = out
        .diags
        .items()
        .iter()
        .filter(|d| d.severity >= Severity::Error)
        .take(20)
        .map(|d| match &d.file {
            Some(f) => format!("{}: {} ({}:{})", d.code, d.message, f, d.line),
            None => format!("{}: {}", d.code, d.message),
        })
        .collect();
    report.steps_done = out
        .steps
        .iter()
        .filter(|s| s.status == flyff_data::load::StepStatus::Done)
        .count() as u32;
    report.steps_stub = out
        .steps
        .iter()
        .filter(|s| s.status == flyff_data::load::StepStatus::Stub)
        .count() as u32;
    report.fingerprint = format!("{:032x}", out.game.meta.fingerprint);
    if let Some(path) = diag_log
        && let Err(e) = write_diagnostics_file(&out.diags, path)
    {
        tracing::warn!(error = %e, file = %path.display(), "cannot write data-load.jsonl");
    }
    Ok(LoadedData {
        vfs: Arc::new(vfs),
        game: Arc::new(out.game),
        client: out.client.map(Arc::new),
        report,
    })
}

/// Logs the report the way the start-up sequence shows it (info line + one warning per stored Error).
pub fn log_report(r: &DataLoadReport, mode: DataMode) {
    if mode == DataMode::Empty {
        return;
    }
    tracing::info!(
        vfs_files = r.vfs_files,
        overrides = r.override_files,
        vfs_ms = r.vfs_ms,
        load_ms = r.load_ms,
        errors = r.errors,
        warnings = r.warnings,
        steps = r.steps_done,
        stubs = r.steps_stub,
        fingerprint = %r.fingerprint,
        "game data loaded"
    );
    if r.override_files > 0 {
        tracing::info!(
            files = r.override_files,
            "overrides/ shadows or adds Resource files"
        );
    }
    for line in &r.first_errors {
        tracing::warn!("data: {line}");
    }
}

fn elapsed_ms(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
}
