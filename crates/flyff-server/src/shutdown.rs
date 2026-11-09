//! `ShutdownCoordinator` (Plan/10 §15.1): turns signals, console commands and in-game GM requests
//! into `GameInput::Shutdown`, tracks the countdown rules and implements [`ShutdownHandle`] for the
//! game thread.

use crate::audit::{Actor, AuditFile};
use flyff_common::CharacterId;
use flyff_common::rt::{HealthState, ShutdownHandle, Telemetry};
use flyff_game::GameInput;
use flyff_game::input::{AdminActor, AdminCmd, ShutdownReason};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The whole stop (countdown included) may take `countdown + STOP_GRACE`; past that the process logs an ERROR and
/// exits 1 (Plan/10 §15.2 step 6: the steps after the countdown take at most about 80 s).
pub const STOP_GRACE: Duration = Duration::from_secs(90);

/// Who asked for the shutdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShutdownTrigger {
    /// A POSIX signal number (2 = SIGINT, 15 = SIGTERM).
    Signal(i32),
    /// The stdin console.
    Console,
    /// `/admin/api/shutdown`.
    AdminApi {
        /// Peer address.
        ip: String,
    },
    /// In-game GM command.
    Gm {
        /// Requesting character.
        by: CharacterId,
    },
    /// Tests.
    Test,
}

impl ShutdownTrigger {
    /// The `by` of the `server.shutdown_requested` audit line (Plan/10 §11): `signal:SIGTERM`, `console`,
    /// `admin-api:<ip>`, `gm:<character>`.
    pub fn label(&self) -> String {
        match self {
            ShutdownTrigger::Signal(15) => "signal:SIGTERM".to_owned(),
            ShutdownTrigger::Signal(2) => "signal:SIGINT".to_owned(),
            ShutdownTrigger::Signal(n) => format!("signal:{n}"),
            ShutdownTrigger::Console => "console".to_owned(),
            ShutdownTrigger::AdminApi { ip } => format!("admin-api:{ip}"),
            ShutdownTrigger::Gm { by } => format!("gm:{}", by.0),
            ShutdownTrigger::Test => "test".to_owned(),
        }
    }

    /// The `reason` of the `server.stop` audit line.
    pub fn stop_reason(&self) -> &'static str {
        match self {
            ShutdownTrigger::Signal(_) => "signal",
            ShutdownTrigger::Console => "console",
            ShutdownTrigger::AdminApi { .. } => "admin-api",
            ShutdownTrigger::Gm { .. } => "gm",
            ShutdownTrigger::Test => "test",
        }
    }

    fn reason(&self) -> ShutdownReason {
        match self {
            ShutdownTrigger::Signal(_) => ShutdownReason::Signal,
            ShutdownTrigger::Console => ShutdownReason::Console,
            ShutdownTrigger::AdminApi { .. } => ShutdownReason::AdminApi,
            ShutdownTrigger::Gm { .. } => ShutdownReason::Gm,
            ShutdownTrigger::Test => ShutdownReason::Test,
        }
    }
}

#[derive(Default)]
struct State {
    /// A countdown has been requested (a second SIGINT then means "now").
    requested: bool,
    /// Countdown already at 0: cancel is no longer possible.
    at_zero: bool,
    /// Who asked first (the `reason` of `server.stop`).
    reason: Option<&'static str>,
    /// Players in the world when the countdown started (`players_at_start_of_countdown` of `server.stop`).
    players_at_start: Option<u64>,
    /// When the stop must be complete (`countdown + grace` after the earliest request); the watchdog's deadline.
    hard_deadline: Option<Instant>,
}

struct Inner {
    game_tx: flume::Sender<GameInput>,
    telemetry: Arc<Telemetry>,
    default_countdown_s: std::sync::atomic::AtomicU32,
    state: Mutex<State>,
    audit: Mutex<Option<Arc<AuditFile>>>,
    stop_grace: Mutex<Duration>,
}

/// What `server.stop` reports about the shutdown (Plan/10 §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownSummary {
    /// `signal`, `console`, `admin-api`, `gm` or `test`; `None` when nobody asked (a crash).
    pub reason: Option<&'static str>,
    /// Players in the world when the countdown started.
    pub players_at_start: Option<u64>,
}

/// Cheap-clone coordinator.
#[derive(Clone)]
pub struct ShutdownCoordinator {
    inner: Arc<Inner>,
}

impl ShutdownCoordinator {
    /// Creates a coordinator sending to the game inbox.
    pub fn new(
        game_tx: flume::Sender<GameInput>,
        telemetry: Arc<Telemetry>,
        default_countdown_s: u32,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                game_tx,
                telemetry,
                default_countdown_s: std::sync::atomic::AtomicU32::new(default_countdown_s),
                state: Mutex::new(State::default()),
                audit: Mutex::new(None),
                stop_grace: Mutex::new(STOP_GRACE),
            }),
        }
    }

    /// Replaces the grace period of the stop watchdog (tests).
    pub fn set_stop_grace(&self, grace: Duration) {
        *self.inner.stop_grace.lock() = grace;
    }

    /// The instant by which the stop must be complete, `None` while no shutdown is requested.
    pub fn hard_deadline(&self) -> Option<Instant> {
        self.inner.state.lock().hard_deadline
    }

    /// Emits `server.shutdown_requested` for every trigger and cancel through `audit` (Plan/10 §11).
    pub fn set_audit(&self, audit: Arc<AuditFile>) {
        *self.inner.audit.lock() = Some(audit);
    }

    /// Who asked and how many players were in the world at the start of the countdown.
    pub fn summary(&self) -> ShutdownSummary {
        let st = self.inner.state.lock();
        ShutdownSummary {
            reason: st.reason,
            players_at_start: st.players_at_start,
        }
    }

    fn audit(&self, data: serde_json::Value) {
        if let Some(a) = self.inner.audit.lock().as_ref() {
            a.emit("server.shutdown_requested", &Actor::none(), data);
        }
    }

    /// Requests a graceful shutdown (`None` = `server.shutdown_countdown_s`). A second SIGINT during a
    /// countdown means "now". Returns the countdown that was sent.
    pub fn request(
        &self,
        countdown_s: Option<u32>,
        message: Option<String>,
        by: ShutdownTrigger,
    ) -> u32 {
        let inner = &self.inner;
        let mut countdown =
            countdown_s.unwrap_or_else(|| inner.default_countdown_s.load(Ordering::Relaxed));
        {
            let mut st = inner.state.lock();
            if matches!(by, ShutdownTrigger::Signal(2)) && st.requested {
                countdown = 0;
            }
            if st.players_at_start.is_none() {
                st.players_at_start =
                    Some(inner.telemetry.players_in_world.load(Ordering::Relaxed));
            }
            // the trigger that started the shutdown names it; a later one (a second SIGINT) does not rename it
            st.reason.get_or_insert(by.stop_reason());
            let candidate = Instant::now()
                + Duration::from_secs(u64::from(countdown))
                + *inner.stop_grace.lock();
            st.hard_deadline = Some(st.hard_deadline.map_or(candidate, |d| d.min(candidate)));
            st.requested = true;
            st.at_zero = countdown == 0;
        }
        let health = &inner.telemetry.health;
        if health.state() == HealthState::Running {
            health.set_state(HealthState::ShuttingDown);
        }
        health.accepting_logins.store(false, Ordering::Relaxed);
        tracing::info!(countdown_s = countdown, trigger = ?by, "shutdown requested");
        self.audit(json!({
            "by": by.label(),
            "countdown_s": countdown,
            "message": message,
            "cancelled": false,
        }));
        let input = GameInput::Shutdown {
            countdown_s: countdown,
            reason: by.reason(),
            message,
        };
        if inner
            .game_tx
            .send_timeout(input, Duration::from_secs(2))
            .is_err()
        {
            tracing::error!("game inbox is not accepting the shutdown request");
        }
        countdown
    }

    /// `server.shutdown_countdown_s` after a configuration reload (the default of requests without a countdown).
    pub fn set_default_countdown(&self, seconds: u32) {
        self.inner
            .default_countdown_s
            .store(seconds, Ordering::Relaxed);
    }

    /// Cancels a running countdown; `false` when none runs or it already reached zero.
    pub fn cancel(&self, by: &ShutdownTrigger) -> bool {
        let inner = &self.inner;
        {
            let mut st = inner.state.lock();
            if !st.requested || st.at_zero {
                return false;
            }
            st.requested = false;
            st.players_at_start = None;
            st.reason = None;
            st.hard_deadline = None;
        }
        tracing::info!(trigger = ?by, "shutdown cancelled");
        self.audit(json!({"by": by.label(), "cancelled": true}));
        let (reply, _rx) = flyff_common::rt::reply_pair();
        let sent = inner
            .game_tx
            .send_timeout(
                GameInput::Admin {
                    cmd: AdminCmd::CancelShutdown,
                    actor: AdminActor::Console,
                    reply,
                },
                Duration::from_secs(2),
            )
            .is_ok();
        if sent {
            inner
                .telemetry
                .health
                .accepting_logins
                .store(true, Ordering::Relaxed);
        }
        sent
    }
}

/// Watches a requested shutdown: when it is still not complete at the coordinator's hard deadline (the countdown
/// plus [`STOP_GRACE`]) it runs `on_expire` (the executable logs an ERROR and exits 1) instead of letting a stuck
/// step hang the stop forever. [`StopWatchdog::finish`] ends the watch once the server has stopped.
pub struct StopWatchdog {
    done: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StopWatchdog {
    /// Starts the watch thread; it looks at the deadline every `poll`.
    pub fn spawn(
        coordinator: ShutdownCoordinator,
        poll: Duration,
        on_expire: impl FnOnce() + Send + 'static,
    ) -> StopWatchdog {
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        let thread = std::thread::Builder::new()
            .name("flyff-stop-watchdog".to_owned())
            .spawn(move || {
                while !flag.load(Ordering::Acquire) {
                    if let Some(deadline) = coordinator.hard_deadline()
                        && Instant::now() >= deadline
                    {
                        tracing::error!("the shutdown did not complete in time (countdown + 90 s)");
                        on_expire();
                        return;
                    }
                    std::thread::sleep(poll);
                }
            })
            .ok();
        StopWatchdog { done, thread }
    }

    /// The server has stopped: ends the watch and joins the thread.
    pub fn finish(mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl ShutdownHandle for ShutdownCoordinator {
    fn request(&self, countdown_s: Option<u32>, message: Option<String>, by: CharacterId) {
        let _ =
            ShutdownCoordinator::request(self, countdown_s, message, ShutdownTrigger::Gm { by });
    }

    fn cancel(&self, by: CharacterId) -> bool {
        ShutdownCoordinator::cancel(self, &ShutdownTrigger::Gm { by })
    }
}
