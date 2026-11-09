//! The scheduler thread `flyff-persist-backup` (Plan/13 §21.1): at every wall-clock multiple of
//! `[backup] interval_min` (by default each full local hour) it asks a reader thread for a `sched` backup
//! (`ReadCmd::Backup`). A scheduled backup is skipped, and counted, when the previous one is still running or the
//! read queue is full. The thread watches the [`BackupClock`], so tests drive it with a fake one.

use super::{BackupClock, BackupStats};
use crate::cmd::{BackupKind, ReadCmd};
use flyff_common::UnixTime;
use flyff_common::rt::reply::ReplyError;
use flyff_common::rt::reply_pair;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// The next multiple of `interval_s` of the *local* clock (UTC offset `offset_s`) strictly after `now`.
pub fn next_boundary(now: UnixTime, interval_s: i64, offset_s: i32) -> UnixTime {
    let interval = interval_s.max(1);
    let local = now.0 + i64::from(offset_s);
    let next_local = (local.div_euclid(interval) + 1) * interval;
    UnixTime(next_local - i64::from(offset_s))
}

/// The running scheduler thread; dropping it (or [`BackupSchedule::stop`]) ends the thread.
pub struct BackupSchedule {
    stop: Option<flume::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl BackupSchedule {
    /// Starts the thread. `interval_min == 0` schedules nothing (`None`).
    pub fn spawn(
        interval_min: u32,
        clock: Arc<dyn BackupClock>,
        reads: flume::Sender<ReadCmd>,
        stats: Arc<BackupStats>,
    ) -> std::io::Result<Option<BackupSchedule>> {
        if interval_min == 0 {
            return Ok(None);
        }
        let (stop_tx, stop_rx) = flume::bounded::<()>(1);
        let thread = std::thread::Builder::new()
            .name("flyff-persist-backup".to_owned())
            .spawn(move || {
                run(
                    i64::from(interval_min) * 60,
                    clock.as_ref(),
                    &reads,
                    &stats,
                    &stop_rx,
                );
            })?;
        Ok(Some(BackupSchedule {
            stop: Some(stop_tx),
            thread: Some(thread),
        }))
    }

    /// Ends the thread and waits for it (a backup that is running finishes first).
    pub fn stop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for BackupSchedule {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    interval_s: i64,
    clock: &dyn BackupClock,
    reads: &flume::Sender<ReadCmd>,
    stats: &BackupStats,
    stop: &flume::Receiver<()>,
) {
    let now = clock.now();
    let mut next = next_boundary(now, interval_s, clock.utc_offset_s(now));
    let poll = clock.poll_interval().max(Duration::from_millis(1));
    loop {
        match stop.recv_timeout(poll) {
            Err(flume::RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(flume::RecvTimeoutError::Disconnected) => return,
        }
        let now = clock.now();
        if now < next {
            continue;
        }
        // schedule the next boundary first: a slow backup never fires twice for one boundary
        next = next_boundary(now, interval_s, clock.utc_offset_s(now));
        if stats.is_running() {
            tracing::warn!("scheduled backup skipped: the previous backup is still running");
            stats.note_skipped();
            continue;
        }
        let (reply, rx) = reply_pair();
        match reads.try_send(ReadCmd::Backup {
            kind: BackupKind::Sched,
            reply,
        }) {
            Ok(()) => {
                // wait for the outcome, but stay responsive to a stop request
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(_) | Err(ReplyError::Disconnected) => break,
                        Err(ReplyError::Empty) => {
                            if stop.is_disconnected() {
                                return;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                tracing::warn!("scheduled backup skipped: the read queue is full or closed");
                stats.note_skipped();
            }
        }
    }
}
