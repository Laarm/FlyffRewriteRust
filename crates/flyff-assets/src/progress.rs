//! Preparation progress (Plan/24 §12): waves, `PrepareProgress`, `progress_version`.
//!
//! A *wave* starts when the job queues go from empty to non-empty and ends when no job is queued or
//! running any more. `total` counts the jobs enqueued in the wave (it can grow while the wave runs),
//! `done` the finished ones (it never decreases inside a wave and never exceeds `total`). Every change
//! bumps `progress_version`, an atomic the gateway polls (`AssetsPreparing` push, §12.4).

use crate::kinds::{AssetKind, Priority};
use crate::util::lock;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// What the pool is busy with, by the highest priority that has work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Start-up jobs (`load_server_derived`, `flyff prepare`).
    Startup,
    /// A browser is waiting for a build.
    Interactive,
    /// Background prewarm.
    Prewarm,
    /// Nothing queued or running.
    #[default]
    Idle,
}

/// The `GET /api/prepare` object and the `/api/bootstrap` `prepare` field (Plan/24 §12.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareProgress {
    /// A wave is running.
    pub active: bool,
    /// The highest priority with work.
    pub phase: Phase,
    /// `floor(done * 100 / total)`; 100 when not active.
    pub percent: u8,
    /// Jobs finished in the current (or last) wave.
    pub done: u64,
    /// Jobs enqueued in the current (or last) wave.
    pub total: u64,
    /// Slug of the kind of the most recently started job of the highest running priority.
    pub kind: String,
    /// Name of that job.
    pub current: String,
    /// Interactive requests waiting for a build right now.
    pub waiting: u32,
}

#[derive(Default)]
struct State {
    queued: [u64; 3],
    running: [u64; 3],
    total: u64,
    done: u64,
    last_started: [Option<(String, String)>; 3],
    waiting: u32,
    active: bool,
}

impl State {
    fn busy(&self) -> u64 {
        self.queued.iter().chain(self.running.iter()).sum()
    }
}

/// The shared progress record.
#[derive(Default)]
pub struct Progress {
    state: Mutex<State>,
    version: AtomicU64,
}

impl Progress {
    /// A fresh, idle record at version 0.
    pub fn new() -> Self {
        Self::default()
    }

    fn bump(&self) {
        self.version.fetch_add(1, Ordering::Release);
    }

    /// The change counter: strictly increases with every change of the snapshot.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// A job entered queue `prio`. Returns `true` when this started a new wave.
    pub fn enqueued(&self, prio: Priority) -> bool {
        let mut s = lock(&self.state);
        let new_wave = s.busy() == 0;
        if new_wave {
            s.total = 0;
            s.done = 0;
            s.active = true;
            s.last_started = Default::default();
        }
        s.total += 1;
        s.queued[prio.index()] += 1;
        drop(s);
        self.bump();
        new_wave
    }

    /// A queued job of class `prio` started running.
    pub fn started(&self, prio: Priority, kind: Option<AssetKind>, name: &str) {
        let mut s = lock(&self.state);
        let i = prio.index();
        s.queued[i] = s.queued[i].saturating_sub(1);
        s.running[i] += 1;
        s.last_started[i] = Some((
            kind.map_or(String::new(), |k| k.slug().to_owned()),
            name.to_owned(),
        ));
        drop(s);
        self.bump();
    }

    /// A running job of class `prio` finished (or a queued one was dropped: use [`Self::dropped`]).
    /// Returns `true` when this ended the wave.
    pub fn finished(&self, prio: Priority) -> bool {
        let mut s = lock(&self.state);
        let i = prio.index();
        s.running[i] = s.running[i].saturating_sub(1);
        s.done = (s.done + 1).min(s.total);
        let ended = s.busy() == 0;
        if ended {
            s.active = false;
            s.waiting = 0;
        }
        drop(s);
        self.bump();
        ended
    }

    /// A queued (never started) job of class `prio` was dropped. Returns `true` when this ended the wave.
    pub fn dropped(&self, prio: Priority) -> bool {
        let mut s = lock(&self.state);
        let i = prio.index();
        s.queued[i] = s.queued[i].saturating_sub(1);
        s.done = (s.done + 1).min(s.total);
        let ended = s.busy() == 0;
        if ended {
            s.active = false;
            s.waiting = 0;
        }
        drop(s);
        self.bump();
        ended
    }

    /// A queued job moved from queue `from` to the higher-priority queue `to`.
    pub fn promoted(&self, from: Priority, to: Priority) {
        let mut s = lock(&self.state);
        let (f, t) = (from.index(), to.index());
        s.queued[f] = s.queued[f].saturating_sub(1);
        s.queued[t] += 1;
        drop(s);
        self.bump();
    }

    /// An Interactive request started waiting.
    pub fn waiter_added(&self) {
        lock(&self.state).waiting += 1;
        self.bump();
    }

    /// `n` Interactive requests were answered.
    pub fn waiters_removed(&self, n: u32) {
        if n == 0 {
            return;
        }
        let mut s = lock(&self.state);
        s.waiting = s.waiting.saturating_sub(n);
        drop(s);
        self.bump();
    }

    /// The current snapshot.
    pub fn snapshot(&self) -> PrepareProgress {
        let s = lock(&self.state);
        let busy_at = |i: usize| s.queued[i] + s.running[i] > 0;
        let phase = if !s.active {
            Phase::Idle
        } else if busy_at(0) {
            Phase::Startup
        } else if busy_at(1) {
            Phase::Interactive
        } else if busy_at(2) {
            Phase::Prewarm
        } else {
            Phase::Idle
        };
        let (kind, current) = (0..3)
            .find(|&i| s.running[i] > 0)
            .and_then(|i| s.last_started[i].clone())
            .unwrap_or_default();
        let percent = if s.active {
            let p = s.done.saturating_mul(100).checked_div(s.total).unwrap_or(0);
            u8::try_from(p.min(100)).unwrap_or(100)
        } else {
            100
        };
        PrepareProgress {
            active: s.active,
            phase,
            percent,
            done: s.done,
            total: s.total,
            kind,
            current,
            waiting: s.waiting,
        }
    }
}
