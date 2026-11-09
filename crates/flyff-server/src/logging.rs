//! Logging (Plan/10 §10, 02 §12): `tracing` with a reloadable `EnvFilter`, a console layer on stderr and (for
//! `serve`) a non-blocking file layer over the size- and age-capped [`SizeRoller`] in `logs/`.
//!
//! * `log.level` is the filter; `RUST_LOG`, when set, wins ([`effective_level`]); [`LogHandles::set_filter`]
//!   swaps it at runtime (console `log <filter>`, config reload) without a restart,
//! * the file layer (`flyff-log` worker thread) is lossy: a stalled disk never blocks the game thread or an
//!   I/O worker, the lines that do not fit the 128 000-line buffer are counted ([`LogHandles::dropped_lines`],
//!   exported as `flyff_log_dropped_lines_total` by the metrics WP),
//! * [`SizeRoller`] rotates `logs/flyff.log` above `log.file_max_mib` and at the first write after UTC
//!   midnight to `flyff-YYYYMMDD-HHMMSS.log`, then deletes rotated files beyond the newest `log.file_keep`
//!   and those older than `log.max_age_days`,
//! * [`LogBudget`] (client-caused WARN limiter) lives in `flyff-common` and is re-exported here.

use crate::config::LogConfig;
use anyhow::Context;
use chrono::{DateTime, Datelike, Timelike, Utc};
use std::fs::{File, OpenOptions};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::layer::{Layered, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry, reload};

pub use flyff_common::log_budget::{Admit, LogBudget};

/// What the process logs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogMode {
    /// `serve`: console + rolling file.
    Serve,
    /// CLI commands: console at `warn` only.
    Cli,
}

/// Name of the live log file inside the log directory.
pub const LOG_FILE: &str = "flyff.log";

/// Buffered lines of the non-blocking file writer (Plan/10 §10.1).
pub const FILE_BUFFER_LINES: usize = 128_000;

/// The filter directive in force: `RUST_LOG` when set and non-empty, else `log.level` (Plan/10 §8.2).
pub fn effective_level<'a>(cfg_level: &'a str, rust_log: Option<&'a str>) -> &'a str {
    rust_log
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(cfg_level)
}

// ---------------------------------------------------------------------------------------------
// SizeRoller
// ---------------------------------------------------------------------------------------------

/// Limits of the rolling file (`[log] file_max_mib`, `file_keep`, `max_age_days`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollerConfig {
    /// Directory of `flyff.log` and the rotated files.
    pub dir: PathBuf,
    /// Rotate when the live file reaches this many bytes.
    pub max_bytes: u64,
    /// Rotated files kept (the newest).
    pub keep: usize,
    /// Rotated files older than this many days (by modification time) are deleted.
    pub max_age_days: u32,
}

impl RollerConfig {
    /// The limits of a `[log]` section writing into `dir`.
    pub fn from_log(cfg: &LogConfig, dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            max_bytes: u64::from(cfg.file_max_mib).saturating_mul(1024 * 1024),
            keep: cfg.file_keep.max(1) as usize,
            max_age_days: cfg.max_age_days.max(1),
        }
    }
}

type ClockFn = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// A `Write` that appends to `flyff.log` and rotates it by size and at UTC midnight (Plan/10 §10.2).
///
/// Rotation renames the live file to `flyff-YYYYMMDD-HHMMSS.log` (UTC time of the rotation; when a file of
/// that name exists the second is advanced until the name is free), reopens a fresh `flyff.log` and prunes.
pub struct SizeRoller {
    cfg: RollerConfig,
    file: Option<File>,
    size: u64,
    /// UTC day (days since the epoch) of the data in the live file.
    day: i64,
    clock: ClockFn,
}

/// Days since the Unix epoch (UTC) of `t`.
fn utc_day(t: SystemTime) -> i64 {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    };
    secs.div_euclid(86_400)
}

/// `flyff-YYYYMMDD-HHMMSS.log` for the UTC time `t`.
pub fn rotated_name(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let dt: DateTime<Utc> = DateTime::from_timestamp(secs, 0).unwrap_or(DateTime::UNIX_EPOCH);
    format!(
        "flyff-{:04}{:02}{:02}-{:02}{:02}{:02}.log",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

/// `true` for `flyff-YYYYMMDD-HHMMSS.log`.
pub fn is_rotated_name(name: &str) -> bool {
    let Some(mid) = name
        .strip_prefix("flyff-")
        .and_then(|r| r.strip_suffix(".log"))
    else {
        return false;
    };
    let b = mid.as_bytes();
    b.len() == 15
        && b.get(8) == Some(&b'-')
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 8 || c.is_ascii_digit())
}

impl SizeRoller {
    /// Opens (appending) `<dir>/flyff.log`, creating the directory.
    pub fn open(cfg: RollerConfig) -> std::io::Result<SizeRoller> {
        Self::open_with_clock(cfg, Arc::new(SystemTime::now))
    }

    /// [`SizeRoller::open`] with an injected clock (tests).
    pub fn open_with_clock(cfg: RollerConfig, clock: ClockFn) -> std::io::Result<SizeRoller> {
        std::fs::create_dir_all(&cfg.dir)?;
        let path = cfg.dir.join(LOG_FILE);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let meta = file.metadata()?;
        // The data in an existing file belongs to the day it was last written.
        let day = meta.modified().map_or_else(|_| utc_day(clock()), utc_day);
        Ok(SizeRoller {
            size: meta.len(),
            day,
            file: Some(file),
            cfg,
            clock,
        })
    }

    fn live_path(&self) -> PathBuf {
        self.cfg.dir.join(LOG_FILE)
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        // Close the live file first so the rename works on every platform.
        self.file = None;
        let now = (self.clock)();
        let mut at = now;
        let target = loop {
            let candidate = self.cfg.dir.join(rotated_name(at));
            if !candidate.exists() {
                break candidate;
            }
            at += Duration::from_secs(1);
        };
        let live = self.live_path();
        let renamed = if live.exists() {
            std::fs::rename(&live, &target)
        } else {
            Ok(())
        };
        // Reopen even when the rename failed, so logging continues into the old file.
        self.file = Some(OpenOptions::new().create(true).append(true).open(&live)?);
        if renamed.is_ok() {
            self.size = 0;
        }
        self.day = utc_day(now);
        self.prune();
        renamed
    }

    /// Deletes rotated files beyond the newest `keep` and those older than `max_age_days`.
    fn prune(&self) {
        let Ok(rd) = std::fs::read_dir(&self.cfg.dir) else {
            return;
        };
        let now = (self.clock)();
        let max_age = Duration::from_secs(u64::from(self.cfg.max_age_days) * 86_400);
        let mut rotated: Vec<(String, PathBuf, Option<SystemTime>)> = rd
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                is_rotated_name(&name).then(|| {
                    let mtime = e.metadata().and_then(|m| m.modified()).ok();
                    (name, e.path(), mtime)
                })
            })
            .collect();
        // The timestamp in the name sorts chronologically.
        rotated.sort_by(|a, b| b.0.cmp(&a.0));
        for (i, (_, path, mtime)) in rotated.iter().enumerate() {
            let too_many = i >= self.cfg.keep;
            let too_old = mtime
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age > max_age);
            if too_many || too_old {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn file(&mut self) -> std::io::Result<&mut File> {
        if self.file.is_none() {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.live_path())?,
            );
        }
        self.file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("log file is not open"))
    }
}

impl Write for SizeRoller {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let today = utc_day((self.clock)());
        if self.size > 0 && (self.size >= self.cfg.max_bytes || today != self.day) {
            // A failed rotation must not lose the line: keep appending to the current file.
            if let Err(e) = self.rotate() {
                let _ = writeln!(std::io::stderr(), "log rotation failed: {e}");
            }
        }
        if self.size == 0 {
            self.day = today;
        }
        let n = self.file()?.write(buf)?;
        self.size = self.size.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(f) => f.flush(),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// subscriber
// ---------------------------------------------------------------------------------------------

type FilterHandle = reload::Handle<EnvFilter, Registry>;
type Base = Layered<reload::Layer<EnvFilter, Registry>, Registry>;
type BoxedLayer = Box<dyn Layer<Base> + Send + Sync>;

/// Keeps the logging machinery alive; dropping it flushes the file writer.
pub struct LogHandles {
    filter: Option<FilterHandle>,
    _file_guard: Option<WorkerGuard>,
    dropped: Option<ErrorCounter>,
}

impl LogHandles {
    /// Replaces the active filter (console `log <filter>`, config reload; not persisted).
    pub fn set_filter(&self, directive: &str) -> anyhow::Result<()> {
        let filter = EnvFilter::try_new(directive)
            .with_context(|| format!("invalid filter '{directive}'"))?;
        match &self.filter {
            Some(h) => h.reload(filter).context("reload filter"),
            None => anyhow::bail!("logging is not active in this process"),
        }
    }

    /// Lines the file writer dropped because its buffer was full (`flyff_log_dropped_lines_total`).
    pub fn dropped_lines(&self) -> u64 {
        self.dropped
            .as_ref()
            .map_or(0, |c| c.dropped_lines() as u64)
    }

    /// Handles for a process that installed no subscriber (tests, a second `start()`).
    pub fn inactive() -> Self {
        Self {
            filter: None,
            _file_guard: None,
            dropped: None,
        }
    }

    /// Handles over an existing filter handle (tests that build their own subscriber).
    pub fn from_parts(
        filter: FilterHandle,
        guard: Option<WorkerGuard>,
        dropped: Option<ErrorCounter>,
    ) -> Self {
        Self {
            filter: Some(filter),
            _file_guard: guard,
            dropped,
        }
    }
}

/// A lossy non-blocking writer over `writer` on a thread named `flyff-log`: when the buffer of `lines`
/// entries is full new lines are dropped (and counted) instead of blocking the caller.
pub fn lossy_writer<W: Write + Send + 'static>(
    writer: W,
    lines: usize,
) -> (NonBlocking, WorkerGuard, ErrorCounter) {
    let (nb, guard) = NonBlockingBuilder::default()
        .buffered_lines_limit(lines)
        .lossy(true)
        .thread_name("flyff-log")
        .finish(writer);
    let counter = nb.error_counter();
    (nb, guard, counter)
}

/// The text or JSON file layer over a non-blocking writer.
fn file_layer(writer: NonBlocking, json: bool) -> BoxedLayer {
    if json {
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(writer)
            .with_ansi(false)
            .with_target(true)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_writer(writer)
            .with_ansi(false)
            .with_target(true)
            .boxed()
    }
}

/// A reloadable filter layer for `directive` and its handle.
pub fn filter_layer(
    directive: &str,
) -> anyhow::Result<(reload::Layer<EnvFilter, Registry>, FilterHandle)> {
    let filter = EnvFilter::try_new(directive)
        .with_context(|| format!("invalid log filter '{directive}'"))?;
    Ok(reload::Layer::new(filter))
}

/// Installs the global subscriber. `logs_dir` is the resolved `log.dir`. A second call in the same process
/// (tests) is a no-op and returns inactive handles instead of failing.
pub fn init(cfg: &LogConfig, logs_dir: &Path, mode: LogMode) -> anyhow::Result<LogHandles> {
    let rust_log = std::env::var("RUST_LOG").ok();
    let level = match mode {
        LogMode::Serve => effective_level(&cfg.level, rust_log.as_deref()).to_owned(),
        LogMode::Cli => "warn".to_owned(),
    };
    let (filter_layer, handle) = filter_layer(&level)?;

    let mut layers: Vec<BoxedLayer> = Vec::new();
    let ansi = cfg.console == "auto" && std::io::stderr().is_terminal();
    if cfg.console != "off" || mode == LogMode::Cli {
        layers.push(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(ansi)
                .with_target(true)
                .boxed(),
        );
    }

    if mode == LogMode::Serve {
        // the admin console's log tail (Plan/25 §9.8): the last 10,000 lines in memory, behind the same filter
        layers.push(crate::admin::logs::global().layer().boxed());
    }

    let mut guard = None;
    let mut dropped = None;
    if mode == LogMode::Serve {
        let roller = SizeRoller::open(RollerConfig::from_log(cfg, logs_dir))
            .with_context(|| format!("open {}", logs_dir.join(LOG_FILE).display()))?;
        let (writer, g, counter) = lossy_writer(roller, FILE_BUFFER_LINES);
        guard = Some(g);
        dropped = Some(counter);
        layers.push(file_layer(writer, cfg.format == "json"));
    }

    let installed = Registry::default()
        .with(filter_layer)
        .with(layers)
        .try_init()
        .is_ok();
    if installed {
        Ok(LogHandles {
            filter: Some(handle),
            _file_guard: guard,
            dropped,
        })
    } else {
        Ok(LogHandles::inactive())
    }
}
