//! The lobby sweeper (Plan/12 §2, §7.3; WP-AUTH-03): one task at 1 Hz that expires `Claimed` entries whose
//! token grace ran out (removing them once their last save is flushed, else turning them into `Draining`)
//! and prunes the throttle and registration maps.

use super::LobbyCore;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// The period of the sweep.
pub const SWEEP_PERIOD: Duration = Duration::from_secs(1);

impl LobbyCore {
    /// One sweep: token-grace expiry, throttle and registration-window pruning.
    pub fn sweep_once(&self) {
        let actions = self.registry.sweep(&self.persist);
        self.run_actions(actions);
        let now = self.clock.unix().0;
        self.throttle.prune(now);
        self.register_limiter.prune(now);
    }
}

/// The running sweeper task; dropping the handle (or [`SweeperHandle::stop`]) ends it.
pub struct SweeperHandle {
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl SweeperHandle {
    /// Stops the task and waits for it.
    pub async fn stop(mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for SweeperHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Starts the 1 Hz sweeper on `rt`. The task keeps only a weak reference to the core and ends when it is gone.
pub fn spawn(core: &Arc<LobbyCore>, rt: &tokio::runtime::Handle) -> SweeperHandle {
    let (stop, mut stopped) = watch::channel(false);
    let core = Arc::downgrade(core);
    let task = rt.spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_PERIOD);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let Some(core) = core.upgrade() else { break };
                    core.sweep_once();
                }
                changed = stopped.changed() => {
                    if changed.is_err() || *stopped.borrow() {
                        break;
                    }
                }
            }
        }
    });
    SweeperHandle {
        stop,
        task: Some(task),
    }
}
