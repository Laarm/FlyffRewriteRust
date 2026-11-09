//! The post-backup hook (Plan/13 §21.3): `[backup] hook = ["/usr/local/bin/offsite.sh", ...]` runs after every
//! successful backup as `argv... <backup_path> <kind>` with `FLYFF_BACKUP_PATH`, `FLYFF_BACKUP_KIND` and
//! `FLYFF_DB_PATH` in its environment, on its own thread, killed (with its whole process group) after
//! `hook_timeout_s`. A non-zero exit, a timeout or a hook that cannot be started is logged and counted
//! (`flyff_db_backup_hook_failures_total`) and never blocks or fails the backup. At most one hook process runs at
//! a time: a backup that finishes while the previous hook still runs does not start another (it is skipped and
//! logged).

use super::BackupStats;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Starts hooks, one at a time.
#[derive(Clone, Debug, Default)]
pub struct HookRunner {
    busy: Arc<AtomicBool>,
}

impl HookRunner {
    /// `true` while a hook process runs.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// Waits until no hook runs, at most `timeout`; `true` when idle.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.is_busy() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    /// Starts the hook for the backup `path` of `kind` on its own thread. Returns `false` (and starts nothing)
    /// when `argv` is empty or the previous hook still runs.
    pub fn spawn(
        &self,
        argv: &[String],
        timeout: Duration,
        path: &Path,
        kind: &str,
        db_path: &Path,
        stats: &Arc<BackupStats>,
    ) -> bool {
        let Some((program, args)) = argv.split_first() else {
            return false;
        };
        if self.busy.swap(true, Ordering::AcqRel) {
            tracing::warn!(
                path = %path.display(),
                "the previous backup hook is still running: not starting another"
            );
            return false;
        }
        let mut cmd = Command::new(program);
        cmd.args(args)
            .arg(path)
            .arg(kind)
            .env("FLYFF_BACKUP_PATH", path)
            .env("FLYFF_BACKUP_KIND", kind)
            .env("FLYFF_DB_PATH", db_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            // its own process group, so a timeout kills the script and what it started
            .process_group(0);
        let busy = Arc::clone(&self.busy);
        let stats = Arc::clone(stats);
        let program = program.clone();
        let spawned = std::thread::Builder::new()
            .name("flyff-backup-hook".to_owned())
            .spawn({
                let busy = Arc::clone(&busy);
                let stats = Arc::clone(&stats);
                move || {
                    stats.note_hook_started();
                    if !run_hook(cmd, &program, timeout) {
                        stats.note_hook_failed();
                    }
                    busy.store(false, Ordering::Release);
                }
            });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "cannot start the backup hook thread");
            stats.note_hook_failed();
            busy.store(false, Ordering::Release);
            return false;
        }
        true
    }
}

/// Runs the command to completion or the timeout; `true` when it exited with status 0.
fn run_hook(mut cmd: Command, program: &str, timeout: Duration) -> bool {
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(hook = program, error = %e, "the backup hook could not be started");
            return false;
        }
    };
    // Keep the first 4 KiB of stderr for the log; drain the rest so the hook never blocks on a full pipe.
    let (err_tx, err_rx) = flume::bounded::<String>(1);
    if let Some(mut pipe) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut chunk = [0_u8; 1024];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                if kept.len() < 4096 {
                    kept.extend_from_slice(chunk.get(..n).unwrap_or(&[]));
                }
            }
            let _ = err_tx.send(String::from_utf8_lossy(&kept).trim().to_owned());
        });
    }
    // A grandchild that keeps the pipe open must not hold the hook thread: wait briefly for the text only.
    let stderr_text = || {
        err_rx
            .recv_timeout(Duration::from_millis(300))
            .unwrap_or_default()
    };
    let deadline = Instant::now() + timeout;
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => break Err("timeout"),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break Err("wait failed"),
        }
    };
    match outcome {
        Ok(status) if status.success() => true,
        Ok(status) => {
            tracing::error!(hook = program, %status, stderr = %stderr_text(), "the backup hook failed");
            false
        }
        Err(why) => {
            kill_group(&child);
            let _ = child.kill();
            let _ = child.wait();
            tracing::error!(hook = program, why, stderr = %stderr_text(), "the backup hook was killed");
            false
        }
    }
}

/// SIGKILL to the hook's process group.
fn kill_group(child: &std::process::Child) {
    if let Some(pid) = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
}
