//! Backup rotation (Plan/13 §21.2): grandfather-father-son for the `sched` kind, "newest N" for `premigrate`,
//! `prerestore` and `shutdown`, never for `manual` (and `replay` files are not in `backups/` at all).
//!
//! The union kept by the GFS rule is
//!
//! * (a) the newest `keep_hourly` (24) scheduled backups,
//! * (b) for each of the newest `keep_daily` (7) **local calendar days** that have backups, the **oldest** backup of
//!   that day,
//! * (c) for each of the newest `keep_weekly` (4) **ISO weeks** that have backups, the oldest backup of that week.
//!
//! Every other `sched` file is deleted. Rotation never deletes the file that was just written and ignores files
//! whose names it does not recognise.

use crate::calendar;
use crate::config::BackupConfig;
use crate::error::DbError;
use flyff_common::UnixTime;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One recognised backup file (`flyff-YYYYMMDD-HHMMSS-<kind>.db`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupFile {
    /// File name.
    pub name: String,
    /// Full path.
    pub path: PathBuf,
    /// The UTC time in the name.
    pub at: UnixTime,
    /// The kind label (`sched`, `manual`, ...).
    pub kind: String,
}

/// Parses `flyff-YYYYMMDD-HHMMSS-<kind>.db` into the time and the kind label.
pub fn parse_backup_name(name: &str) -> Option<(UnixTime, &str)> {
    let rest = name.strip_prefix("flyff-")?.strip_suffix(".db")?;
    let (stamp, kind) = rest.split_at_checked(15)?;
    let kind = kind.strip_prefix('-')?;
    if kind.is_empty() || !kind.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    Some((calendar::parse_compact(stamp)?, kind))
}

/// The recognised backup files of `dir`, oldest first (a missing directory is empty).
pub fn list_backups(dir: &Path) -> Result<Vec<BackupFile>, DbError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut files: Vec<BackupFile> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let (at, kind) = parse_backup_name(&name)?;
            let kind = kind.to_owned();
            Some(BackupFile {
                path: e.path(),
                name,
                at,
                kind,
            })
        })
        .collect();
    files.sort_by(|a, b| (a.at, &a.name).cmp(&(b.at, &b.name)));
    Ok(files)
}

/// The names the GFS rule keeps out of `files` (all of one kind, any order). `offset` gives the local UTC
/// offset at an instant (the day and week boundaries are local).
pub fn gfs_keep<'a>(
    files: &'a [BackupFile],
    keep_hourly: u32,
    keep_daily: u32,
    keep_weekly: u32,
    offset: &dyn Fn(UnixTime) -> i32,
) -> BTreeSet<&'a str> {
    let mut by_age: Vec<&BackupFile> = files.iter().collect();
    by_age.sort_by(|a, b| (b.at, &b.name).cmp(&(a.at, &a.name))); // newest first
    let mut keep: BTreeSet<&str> = BTreeSet::new();
    // (a)
    for f in by_age
        .iter()
        .take(usize::try_from(keep_hourly).unwrap_or(usize::MAX))
    {
        keep.insert(&f.name);
    }
    // (b) and (c): the oldest backup of every day / ISO week, then the newest buckets
    let mut days: BTreeMap<i64, &BackupFile> = BTreeMap::new();
    let mut weeks: BTreeMap<(i64, u32), &BackupFile> = BTreeMap::new();
    for f in by_age.iter().rev() {
        // iterating oldest -> newest: the first file seen of a bucket is its oldest
        let day = calendar::local_day(f.at, offset(f.at));
        days.entry(day).or_insert(f);
        weeks.entry(calendar::iso_week(day)).or_insert(f);
    }
    for f in days
        .values()
        .rev()
        .take(usize::try_from(keep_daily).unwrap_or(usize::MAX))
    {
        keep.insert(&f.name);
    }
    for f in weeks
        .values()
        .rev()
        .take(usize::try_from(keep_weekly).unwrap_or(usize::MAX))
    {
        keep.insert(&f.name);
    }
    keep
}

/// Rotates the backups of `kind_label` in `dir` after `just_written` was created. Returns the names deleted.
///
/// * `sched`: the GFS rule above;
/// * `premigrate`, `prerestore`, `shutdown`: the newest `keep_premigrate` / `keep_prerestore` / `keep_shutdown`
///   (at least one);
/// * anything else (`manual`): nothing is deleted.
pub fn rotate(
    dir: &Path,
    cfg: &BackupConfig,
    kind_label: &str,
    just_written: &Path,
    offset: &dyn Fn(UnixTime) -> i32,
) -> Result<Vec<String>, DbError> {
    let all = list_backups(dir)?;
    let of_kind: Vec<BackupFile> = all.into_iter().filter(|f| f.kind == kind_label).collect();
    let protected = just_written
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned);
    let doomed: Vec<&BackupFile> = match kind_label {
        "sched" => {
            let keep = gfs_keep(
                &of_kind,
                cfg.keep_hourly,
                cfg.keep_daily,
                cfg.keep_weekly,
                offset,
            );
            of_kind
                .iter()
                .filter(|f| !keep.contains(f.name.as_str()))
                .collect()
        }
        "premigrate" | "prerestore" | "shutdown" => {
            let n = match kind_label {
                "premigrate" => cfg.keep_premigrate,
                "prerestore" => cfg.keep_prerestore,
                _ => cfg.keep_shutdown,
            };
            let keep = usize::try_from(n).unwrap_or(usize::MAX).max(1);
            let excess = of_kind.len().saturating_sub(keep);
            of_kind.iter().take(excess).collect() // oldest first
        }
        _ => Vec::new(),
    };
    let mut deleted = Vec::new();
    for f in doomed {
        if protected.as_deref() == Some(f.name.as_str()) {
            continue;
        }
        match std::fs::remove_file(&f.path) {
            Ok(()) => deleted.push(f.name.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(deleted)
}
