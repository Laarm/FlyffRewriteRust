//! What the game thread publishes for `/metrics` and `status` (Plan/10 §12.1): the rolling quantiles of the tick
//! duration, the copy of the movement counters (18 §17), the game-side gauges (objects, layers, snapshots, outbound
//! messages) in [`Telemetry`], and the overrun log (Plan/10 §4.4 step 5.1): which tick phase, world phase, job and
//! input made a tick slow, at most one line per severity per 75 / 15 ticks.

use crate::movement::MoveStats;
use crate::mover::MoverKind;
use crate::persist::save::SaveCounters;
use crate::world::Worlds;
use flyff_common::rt::{Telemetry, TickPhase, WorldPhase};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;

/// The last [`TickWindow::LEN`] tick durations; the 50th and 99th percentile are recomputed every
/// [`TickWindow::EVERY`] ticks (`flyff_tick_p50_seconds`, `flyff_tick_p99_seconds`; reported only: the 5 / 25 ms
/// budget of 01 §12.1 was dropped by the owner on 2026-10-29, the overrun counters at 66.7 / 100 ms are the rule).
#[derive(Debug, Default)]
pub struct TickWindow {
    ring: VecDeque<u64>,
    pushed: u64,
    /// The slowest job and input of the tick being run (reset at the start of every tick, read by the overrun
    /// log after it).
    pub slowest: Slowest,
    /// The rate limit of the overrun log lines.
    pub overrun: OverrunLog,
}

impl TickWindow {
    /// Window length in ticks (one minute at 15 Hz).
    pub const LEN: usize = 900;
    /// Recompute period in ticks.
    pub const EVERY: u64 = 15;

    /// Adds one tick duration (ns). Every [`TickWindow::EVERY`]th call returns the new `(p50, p99)`.
    pub fn push(&mut self, dur_ns: u64) -> Option<(u64, u64)> {
        if self.ring.len() >= Self::LEN {
            self.ring.pop_front();
        }
        self.ring.push_back(dur_ns);
        self.pushed += 1;
        self.pushed
            .is_multiple_of(Self::EVERY)
            .then(|| self.quantiles())
    }

    /// `(p50, p99)` of the window (nearest rank); `(0, 0)` when empty.
    pub fn quantiles(&self) -> (u64, u64) {
        let mut v: Vec<u64> = self.ring.iter().copied().collect();
        v.sort_unstable();
        (nearest_rank(&v, 50), nearest_rank(&v, 99))
    }

    /// Durations held.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// `true` before the first tick ended.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}

/// The nearest-rank percentile `pct` (1..=100) of a sorted slice: the value at rank `ceil(pct * n / 100)`.
pub fn nearest_rank(sorted: &[u64], pct: usize) -> u64 {
    let n = sorted.len();
    if n == 0 {
        return 0;
    }
    let rank = (pct * n).div_ceil(100).clamp(1, n);
    sorted.get(rank - 1).copied().unwrap_or(0)
}

/// One named unit of work and how long it took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlowUnit {
    /// The job name, the proto name of a client message or the kind of an input; empty before any was noted.
    pub name: &'static str,
    /// Its duration (ns).
    pub ns: u64,
}

impl SlowUnit {
    /// Keeps the slower of the current unit and `(name, ns)`.
    pub fn note(&mut self, name: &'static str, ns: u64) {
        if ns > self.ns || self.name.is_empty() {
            *self = SlowUnit { name, ns };
        }
    }
}

/// The slowest job and the slowest input of one tick (the tick and world phases come from
/// `Telemetry::last_phase_ns` / `last_world_ns`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slowest {
    /// The slowest periodic job, wall-clock fire or one-shot (`one_shot`) of phase 3.
    pub job: SlowUnit,
    /// The slowest input of phase 1: the proto name of a client message, else the kind of the input.
    pub input: SlowUnit,
}

/// Which line the overrun log writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverrunLevel {
    /// More than 66.7 ms.
    Warn,
    /// More than 100 ms.
    Error,
}

/// Tick duration (ns) above which a tick counts as a warn-level overrun (`Telemetry::end_tick` counts the same).
pub const OVERRUN_WARN_NS: u64 = 66_700_000;
/// Tick duration (ns) above which a tick counts as an error-level overrun.
pub const OVERRUN_ERROR_NS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, Default)]
struct LineLimit {
    last_tick: Option<u64>,
    suppressed: u32,
}

impl LineLimit {
    fn admit(&mut self, tick: u64, every: u64) -> Option<u32> {
        if self
            .last_tick
            .is_some_and(|l| tick.saturating_sub(l) < every)
        {
            self.suppressed = self.suppressed.saturating_add(1);
            return None;
        }
        self.last_tick = Some(tick);
        Some(std::mem::take(&mut self.suppressed))
    }
}

/// The rate limit of the overrun log (Plan/10 §4.4 step 5.1): at most one warn line per
/// [`OverrunLog::WARN_EVERY_TICKS`] ticks and one error line per [`OverrunLog::ERROR_EVERY_TICKS`]; the next line
/// of a severity reports how many overruns of that severity it suppressed. A plain struct: no clock, no I/O.
#[derive(Clone, Copy, Debug, Default)]
pub struct OverrunLog {
    warn: LineLimit,
    error: LineLimit,
}

impl OverrunLog {
    /// Minimum distance between two warn lines (5 s).
    pub const WARN_EVERY_TICKS: u64 = 75;
    /// Minimum distance between two error lines (1 s).
    pub const ERROR_EVERY_TICKS: u64 = 15;

    /// Decides whether the tick `tick` that took `dur_ns` is logged: `Some((level, suppressed))` when a line is due
    /// (`suppressed` = overruns of that level swallowed since the previous line), `None` for a tick within the
    /// limit or below the thresholds.
    pub fn check(&mut self, tick: u64, dur_ns: u64) -> Option<(OverrunLevel, u32)> {
        if dur_ns > OVERRUN_ERROR_NS {
            self.error
                .admit(tick, Self::ERROR_EVERY_TICKS)
                .map(|n| (OverrunLevel::Error, n))
        } else if dur_ns > OVERRUN_WARN_NS {
            self.warn
                .admit(tick, Self::WARN_EVERY_TICKS)
                .map(|n| (OverrunLevel::Warn, n))
        } else {
            None
        }
    }
}

/// The unit with the largest value among the `names` / `values` pairs: `(name, ns)`.
fn slowest_of<'a>(
    names: impl Iterator<Item = &'static str>,
    values: impl Iterator<Item = &'a std::sync::atomic::AtomicU64>,
) -> (&'static str, u64) {
    let mut best = ("", 0u64);
    for (name, v) in names.zip(values) {
        let ns = v.load(Ordering::Relaxed);
        if ns > best.1 {
            best = (name, ns);
        }
    }
    best
}

/// The tick phase that took longest in the last finished tick (`("", 0)` when none ran).
pub fn slowest_tick_phase(t: &Telemetry) -> (&'static str, u64) {
    slowest_of(
        TickPhase::ALL.iter().map(|p| p.name()),
        t.last_phase_ns.iter(),
    )
}

/// The world sub-phase that took longest, summed over the layers, in the last finished tick.
pub fn slowest_world_phase(t: &Telemetry) -> (&'static str, u64) {
    slowest_of(
        WorldPhase::ALL.iter().map(|p| p.name()),
        t.last_world_ns.iter(),
    )
}

/// How often the object gauges are recounted (every 15th tick = once per second).
pub const OBJECT_RECOUNT_EVERY: u64 = 15;

/// Publishes the game-side gauges of Plan/10 §12.2 after a tick: the live layers, the snapshots taken by reason,
/// the messages appended to the outbound queues (every tick) and the live objects by kind (on the first tick and
/// every [`OBJECT_RECOUNT_EVERY`]th: a pass over the object store).
pub fn publish_game(
    t: &Telemetry,
    tick: u64,
    worlds: &Worlds,
    out_pushes: u64,
    saves: &SaveCounters,
) {
    t.out_msgs.store(out_pushes, Ordering::Relaxed);
    let layers: usize = worlds
        .world_ids()
        .filter_map(|w| worlds.world(w))
        .map(|w| w.layers.len())
        .sum();
    t.layers.store(layers as u64, Ordering::Relaxed);
    for (dst, v) in t.saves.iter().zip([
        saves.periodic,
        saves.urgent,
        saves.world_change,
        saves.logout,
        saves.shutdown,
    ]) {
        dst.store(v, Ordering::Relaxed);
    }
    if tick.is_multiple_of(OBJECT_RECOUNT_EVERY) {
        let store = worlds.store();
        let [mut players, mut monsters, mut npcs] = [0u64; 3];
        for m in store.movers.values() {
            match m.kind {
                MoverKind::Player(_) => players += 1,
                MoverKind::Monster(_) => monsters += 1,
                MoverKind::Npc(_) => npcs += 1,
            }
        }
        for (dst, v) in t.objects.iter().zip([
            players,
            monsters,
            npcs,
            store.items.len() as u64,
            store.ctrls.len() as u64,
        ]) {
            dst.store(v, Ordering::Relaxed);
        }
    }
}

/// Copies the movement counters into the shared telemetry (the game thread's counters only grow, so a plain
/// store of the current value is the whole protocol).
pub fn publish_movement(t: &Telemetry, s: &MoveStats) {
    let m = &t.movement;
    let put = |dst: &std::sync::atomic::AtomicU64, v: u64| dst.store(v, Ordering::Relaxed);
    for (d, v) in m.states.iter().zip(&s.states) {
        put(d, *v);
    }
    for (d, v) in m.ignored.iter().zip(&s.ignored) {
        put(d, *v);
    }
    put(&m.dest_pos_reqs, s.dest_pos_reqs);
    put(&m.dest_obj_reqs, s.dest_obj_reqs);
    for (d, v) in m.motion_reqs.iter().zip(&s.motion_reqs) {
        put(d, *v);
    }
    for (d, v) in m.intents.iter().zip(&s.intents) {
        put(d, *v);
    }
    for (d, v) in m.keyframes.iter().zip(&s.keyframes) {
        put(d, *v);
    }
    put(&m.corrections, s.corrections);
    put(&m.sim_frames, s.sim_frames);
    put(&m.events, s.events);
    put(&m.awake, s.awake);
    put(&m.sanitised, s.sanitised);
}
