//! `logs/persist-failed-YYYY-MM-DD.jsonl` (Plan/13 §16.6, §2.1): commands that failed permanently, one JSON
//! object per line, dumped for manual recovery by an operator.

use crate::backup::utc_compact;
use crate::cmd::DbCmd;
use crate::error::DbError;
use flyff_common::{AccountId, UnixTime};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

/// The accounts that own a snapshot of the command (they become poisoned when the command fails).
pub(super) fn accounts_of(cmd: &DbCmd) -> Vec<AccountId> {
    let mut accounts = BTreeSet::new();
    match cmd {
        DbCmd::Save(snap) => {
            accounts.insert(snap.account_id);
        }
        DbCmd::Atomic(batch) => {
            accounts.extend(batch.snapshots.iter().map(|s| s.account_id));
        }
        DbCmd::ReserveSerialBlock { .. }
        | DbCmd::ReserveSerialBlockBlocking { .. }
        | DbCmd::Flush { .. }
        | DbCmd::Maintenance(_)
        | DbCmd::Shutdown { .. } => {}
    }
    accounts.into_iter().collect()
}

/// Replaces every `password_hash` and TOTP `secret` value in `value` (secrets never go into log files).
fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if key == "password_hash" || key == "secret" {
                    *inner = Value::String("<redacted>".to_owned());
                } else {
                    redact(inner);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

/// The JSON line of a failed command; `None` for commands that carry no data worth dumping.
pub(crate) fn entry(cmd: &DbCmd, error: &DbError, at: UnixTime) -> Option<Value> {
    let (kind, snapshots, ops): (&str, Vec<Value>, Vec<Value>) = match cmd {
        DbCmd::Save(snap) => (
            "Save",
            vec![serde_json::to_value(snap.as_ref()).ok()?],
            Vec::new(),
        ),
        DbCmd::Atomic(batch) => (
            batch.kind.label(),
            batch
                .snapshots
                .iter()
                .filter_map(|s| serde_json::to_value(s).ok())
                .collect(),
            batch
                .ops
                .iter()
                .filter_map(|op| serde_json::to_value(op).ok())
                .collect(),
        ),
        _ => return None,
    };
    let mut line = json!({
        "at": at.0,
        "kind": kind,
        "error": error,
        "accounts": accounts_of(cmd).iter().map(|a| a.0).collect::<Vec<_>>(),
        "snapshots": snapshots,
        "ops": ops,
    });
    redact(&mut line);
    Some(line)
}

/// Appends the failed command to today's `persist-failed` file (best effort: a log failure is logged).
pub(super) fn record(logs_dir: &Path, cmd: &DbCmd, error: &DbError, at: UnixTime) {
    let Some(line) = entry(cmd, error, at) else {
        return;
    };
    let stamp = utc_compact(at);
    let date = format!(
        "{}-{}-{}",
        stamp.get(0..4).unwrap_or("0000"),
        stamp.get(4..6).unwrap_or("00"),
        stamp.get(6..8).unwrap_or("00")
    );
    let path = logs_dir.join(format!("persist-failed-{date}.jsonl"));
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(logs_dir)?;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        writeln!(file, "{line}")
    })();
    match result {
        Ok(()) => tracing::error!(path = %path.display(), error = %error, "failed command dumped"),
        Err(e) => {
            tracing::error!(path = %path.display(), error = %e, "cannot write the failed-command dump")
        }
    }
}
