//! `Telemetry`: lock-free counters shared between the game thread, the gateway and the HTTP side
//! (Plan/10 §12.1). The game thread writes the tick fields, the gateway writes the network fields;
//! `/healthz` and `/metrics` only read.
//!
//! All atomics use `Relaxed` except the `tick_start_ns` / `frames_completed` pair (written `Release`,
//! read `Acquire` in reverse order).

use super::clock::MonoClock;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, Ordering};

/// Upper bounds (ns) of the tick-duration histogram buckets; the last bucket is `+Inf`
/// (seconds: 0.001, 0.002, 0.005, 0.010, 0.015, 0.025, 0.033, 0.050, 0.0667, 0.100, 0.250, 0.500, 1.0).
pub const TICK_BUCKET_BOUNDS_NS: [u64; 13] = [
    1_000_000,
    2_000_000,
    5_000_000,
    10_000_000,
    15_000_000,
    25_000_000,
    33_000_000,
    50_000_000,
    66_700_000,
    100_000_000,
    250_000_000,
    500_000_000,
    1_000_000_000,
];

/// Number of histogram buckets including `+Inf`.
pub const HIST_BUCKETS: usize = TICK_BUCKET_BOUNDS_NS.len() + 1;

/// Fixed-bucket duration histogram (non-cumulative counts; the exposition accumulates).
#[derive(Debug)]
pub struct Histogram {
    buckets: [AtomicU64; HIST_BUCKETS],
    sum_ns: AtomicU64,
    count: AtomicU64,
}

/// A consistent-enough copy of a [`Histogram`] for exposition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistogramSnapshot {
    /// Per-bucket (non-cumulative) counts; index `i < 13` has upper bound `TICK_BUCKET_BOUNDS_NS[i]`.
    pub buckets: [u64; HIST_BUCKETS],
    /// Sum of all observations (ns).
    pub sum_ns: u64,
    /// Number of observations.
    pub count: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            sum_ns: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }
}

impl Histogram {
    /// Records one observation.
    pub fn observe_ns(&self, ns: u64) {
        let idx = TICK_BUCKET_BOUNDS_NS
            .iter()
            .position(|&b| ns <= b)
            .unwrap_or(HIST_BUCKETS - 1);
        if let Some(bucket) = self.buckets.get(idx) {
            bucket.fetch_add(1, Ordering::Relaxed);
        }
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Copies the counters.
    pub fn snapshot(&self) -> HistogramSnapshot {
        HistogramSnapshot {
            buckets: std::array::from_fn(|i| {
                self.buckets.get(i).map_or(0, |b| b.load(Ordering::Relaxed))
            }),
            sum_ns: self.sum_ns.load(Ordering::Relaxed),
            count: self.count.load(Ordering::Relaxed),
        }
    }
}

/// Upper bounds (ns) of the buckets of [`FineHistogram`]; the last bucket is `+Inf` (0.05, 0.1, 0.2, 0.3, 0.5, 0.75, 1, 1.5, 2.5, 5 ms).
pub const FINE_BUCKET_BOUNDS_NS: [u64; 10] = [
    50_000, 100_000, 200_000, 300_000, 500_000, 750_000, 1_000_000, 1_500_000, 2_500_000, 5_000_000,
];

/// Number of buckets of [`FineHistogram`] including `+Inf`.
pub const FINE_BUCKETS: usize = FINE_BUCKET_BOUNDS_NS.len() + 1;

/// A duration histogram with sub-millisecond buckets, for costs far below a tick (the admin views' `post` phase, Plan/25 §3.17:
/// 0.3 ms p50 and 1.5 ms p99).
#[derive(Debug)]
pub struct FineHistogram {
    buckets: [AtomicU64; FINE_BUCKETS],
    sum_ns: AtomicU64,
    count: AtomicU64,
}

/// A copy of a [`FineHistogram`] for exposition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FineSnapshot {
    /// Per-bucket (non-cumulative) counts; index `i < 10` has upper bound `FINE_BUCKET_BOUNDS_NS[i]`.
    pub buckets: [u64; FINE_BUCKETS],
    /// Sum of all observations (ns).
    pub sum_ns: u64,
    /// Number of observations.
    pub count: u64,
}

impl Default for FineHistogram {
    fn default() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            sum_ns: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }
}

impl FineHistogram {
    /// Records one observation.
    pub fn observe_ns(&self, ns: u64) {
        let idx = FINE_BUCKET_BOUNDS_NS
            .iter()
            .position(|&b| ns <= b)
            .unwrap_or(FINE_BUCKETS - 1);
        if let Some(bucket) = self.buckets.get(idx) {
            bucket.fetch_add(1, Ordering::Relaxed);
        }
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Copies the counters.
    pub fn snapshot(&self) -> FineSnapshot {
        FineSnapshot {
            buckets: std::array::from_fn(|i| {
                self.buckets.get(i).map_or(0, |b| b.load(Ordering::Relaxed))
            }),
            sum_ns: self.sum_ns.load(Ordering::Relaxed),
            count: self.count.load(Ordering::Relaxed),
        }
    }
}

/// Upper bounds (ns) of the buckets of [`PhaseHistogram`]; the last bucket is `+Inf` (50 us, 100 us, 250 us, 500 us,
/// 1, 2, 3, 5, 10, 25, 50, 100 ms). Finer than the tick buckets of Plan/10 §12.2 on purpose: the visibility budget of
/// Plan/60 §12.4 is 3 ms at the p99, so 3 ms has to be a bucket edge, and the world sub-phases cost far less than a
/// tick. [`Histogram`] and [`FineHistogram`] keep their buckets.
pub const PHASE_BUCKET_BOUNDS_NS: [u64; 12] = [
    50_000,
    100_000,
    250_000,
    500_000,
    1_000_000,
    2_000_000,
    3_000_000,
    5_000_000,
    10_000_000,
    25_000_000,
    50_000_000,
    100_000_000,
];

/// Number of buckets of [`PhaseHistogram`] including `+Inf`.
pub const PHASE_BUCKETS: usize = PHASE_BUCKET_BOUNDS_NS.len() + 1;

/// A duration histogram with the [`PHASE_BUCKET_BOUNDS_NS`] bounds (`flyff_world_phase_seconds{phase}`).
#[derive(Debug)]
pub struct PhaseHistogram {
    buckets: [AtomicU64; PHASE_BUCKETS],
    sum_ns: AtomicU64,
    count: AtomicU64,
}

/// A copy of a [`PhaseHistogram`] for exposition.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhaseSnapshot {
    /// Per-bucket (non-cumulative) counts; index `i < 12` has upper bound `PHASE_BUCKET_BOUNDS_NS[i]`.
    pub buckets: [u64; PHASE_BUCKETS],
    /// Sum of all observations (ns).
    pub sum_ns: u64,
    /// Number of observations.
    pub count: u64,
}

impl Default for PhaseHistogram {
    fn default() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            sum_ns: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }
}

impl PhaseHistogram {
    /// The bucket an observation of `ns` falls into (the last index is `+Inf`).
    pub fn bucket_of(ns: u64) -> usize {
        PHASE_BUCKET_BOUNDS_NS
            .iter()
            .position(|&b| ns <= b)
            .unwrap_or(PHASE_BUCKETS - 1)
    }

    /// Records one observation.
    pub fn observe_ns(&self, ns: u64) {
        if let Some(bucket) = self.buckets.get(Self::bucket_of(ns)) {
            bucket.fetch_add(1, Ordering::Relaxed);
        }
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Copies the counters.
    pub fn snapshot(&self) -> PhaseSnapshot {
        PhaseSnapshot {
            buckets: std::array::from_fn(|i| {
                self.buckets.get(i).map_or(0, |b| b.load(Ordering::Relaxed))
            }),
            sum_ns: self.sum_ns.load(Ordering::Relaxed),
            count: self.count.load(Ordering::Relaxed),
        }
    }
}

/// The five top-level phases of a tick (Plan/10 §4.4): the labels of `flyff_tick_phase_seconds_total{phase}` and
/// `flyff_tick_phase_count{phase}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickPhase {
    /// Phase 1: inputs and database replies.
    Input,
    /// Phase 2: the world frame.
    Worlds,
    /// Phase 3: timers, jobs, the save pass.
    Timers,
    /// Phase 4: the outbox flush.
    Flush,
    /// Phase 5: telemetry and the admin views.
    Post,
}

impl TickPhase {
    /// Every phase in tick order (the index of [`Telemetry::phase_ns`] and [`Telemetry::last_phase_ns`]).
    pub const ALL: [TickPhase; 5] = [
        TickPhase::Input,
        TickPhase::Worlds,
        TickPhase::Timers,
        TickPhase::Flush,
        TickPhase::Post,
    ];

    /// The metric label.
    pub const fn name(self) -> &'static str {
        match self {
            TickPhase::Input => "input",
            TickPhase::Worlds => "worlds",
            TickPhase::Timers => "timers",
            TickPhase::Flush => "flush",
            TickPhase::Post => "post",
        }
    }

    /// Position in [`TickPhase::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The sub-phases of the per-layer world frame (Plan/10 §4.4, 15 §5.3) plus the layer GC: the labels of
/// `flyff_world_phase_seconds{phase}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldPhase {
    /// `_add`.
    Add,
    /// `_process`.
    Process,
    /// `_modifylink`.
    ModifyLink,
    /// `_OnDie`.
    OnDie,
    /// `_replace`.
    Replace,
    /// `_delete`.
    Delete,
    /// The layer garbage collection after the last layer of a world.
    LayerGc,
}

impl WorldPhase {
    /// Every phase in frame order (the index of [`Telemetry::world_phase_hist`] and [`Telemetry::last_world_ns`]).
    pub const ALL: [WorldPhase; 7] = [
        WorldPhase::Add,
        WorldPhase::Process,
        WorldPhase::ModifyLink,
        WorldPhase::OnDie,
        WorldPhase::Replace,
        WorldPhase::Delete,
        WorldPhase::LayerGc,
    ];

    /// The metric label.
    pub const fn name(self) -> &'static str {
        match self {
            WorldPhase::Add => "_add",
            WorldPhase::Process => "_process",
            WorldPhase::ModifyLink => "_modifylink",
            WorldPhase::OnDie => "_on_die",
            WorldPhase::Replace => "_replace",
            WorldPhase::Delete => "_delete",
            WorldPhase::LayerGc => "layer_gc",
        }
    }

    /// Position in [`WorldPhase::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The kinds of input the game thread dispatches: `flyff_game_inputs_total{kind}` (the variants of `GameInput`
/// plus the persistence replies of C6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    /// `GameInput::Join`.
    Join,
    /// `GameInput::Reattach`.
    Reattach,
    /// `GameInput::Client`.
    Client,
    /// `GameInput::SessionLost`.
    SessionLost,
    /// `GameInput::KickAccount`.
    KickAccount,
    /// `GameInput::CreateCharacter`.
    CreateCharacter,
    /// `GameInput::DeleteCharacter`.
    DeleteCharacter,
    /// `GameInput::Admin`.
    Admin,
    /// `GameInput::AdminView`.
    AdminView,
    /// `GameInput::ReloadLive`.
    ReloadLive,
    /// `GameInput::Shutdown`.
    Shutdown,
    /// `GameInput::ClockOffset`.
    ClockOffset,
    /// `GameInput::Test` (feature `test-hooks`).
    Test,
    /// A `DbReply` of C6.
    DbReply,
}

impl InputKind {
    /// Every kind (the index of [`Telemetry::inputs`]).
    pub const ALL: [InputKind; 14] = [
        InputKind::Join,
        InputKind::Reattach,
        InputKind::Client,
        InputKind::SessionLost,
        InputKind::KickAccount,
        InputKind::CreateCharacter,
        InputKind::DeleteCharacter,
        InputKind::Admin,
        InputKind::AdminView,
        InputKind::ReloadLive,
        InputKind::Shutdown,
        InputKind::ClockOffset,
        InputKind::Test,
        InputKind::DbReply,
    ];

    /// The metric label (snake case of the variant).
    pub const fn name(self) -> &'static str {
        match self {
            InputKind::Join => "join",
            InputKind::Reattach => "reattach",
            InputKind::Client => "client",
            InputKind::SessionLost => "session_lost",
            InputKind::KickAccount => "kick_account",
            InputKind::CreateCharacter => "create_character",
            InputKind::DeleteCharacter => "delete_character",
            InputKind::Admin => "admin",
            InputKind::AdminView => "admin_view",
            InputKind::ReloadLive => "reload_live",
            InputKind::Shutdown => "shutdown",
            InputKind::ClockOffset => "clock_offset",
            InputKind::Test => "test",
            InputKind::DbReply => "db_reply",
        }
    }

    /// Position in [`InputKind::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The labels of [`Telemetry::objects`] (`flyff_objects{kind}`).
pub const OBJECT_KINDS: [&str; 5] = ["player", "monster", "npc", "item", "ctrl"];

/// The labels of [`Telemetry::saves`] (`flyff_saves_total{reason}`): `logout` counts every final snapshot
/// (logout, return to character select, shutdown), `shutdown` is the subset taken by the shutdown countdown.
pub const SAVE_REASONS: [&str; 5] = ["periodic", "urgent", "world_change", "logout", "shutdown"];

/// A `(sum, count)` pair of counters (`phase_ns` of Plan/10 §12.1).
#[derive(Debug, Default)]
pub struct Counter2 {
    /// Sum (ns).
    pub sum: AtomicU64,
    /// Number of additions.
    pub count: AtomicU64,
}

/// Process health state published by the runtime.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthState {
    /// Startup in progress.
    Starting = 0,
    /// Serving.
    Running = 1,
    /// Graceful shutdown in progress (HTTP still serves).
    ShuttingDown = 2,
    /// An unscoped panic ended the game thread.
    Panicked = 3,
    /// Stopped.
    Stopped = 4,
}

impl HealthState {
    /// Decodes the stored byte (unknown values read as `Stopped`).
    pub const fn from_u8(v: u8) -> HealthState {
        match v {
            0 => HealthState::Starting,
            1 => HealthState::Running,
            2 => HealthState::ShuttingDown,
            3 => HealthState::Panicked,
            _ => HealthState::Stopped,
        }
    }

    /// Lower-case name used in the `/healthz` body.
    pub const fn name(self) -> &'static str {
        match self {
            HealthState::Starting => "starting",
            HealthState::Running => "running",
            HealthState::ShuttingDown => "shutting_down",
            HealthState::Panicked => "panicked",
            HealthState::Stopped => "stopped",
        }
    }
}

/// Health block of the [`Telemetry`].
#[derive(Debug)]
pub struct Health {
    state: AtomicU8,
    /// Whether the lobby accepts new logins.
    pub accepting_logins: AtomicBool,
    /// The persistence writer is in transient-retry mode.
    pub db_degraded: AtomicBool,
    /// Unix seconds of the shutdown deadline, `0` = none.
    pub shutdown_deadline_unix_s: AtomicI64,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(HealthState::Starting as u8),
            accepting_logins: AtomicBool::new(false),
            db_degraded: AtomicBool::new(false),
            shutdown_deadline_unix_s: AtomicI64::new(0),
        }
    }
}

impl Health {
    /// Current state.
    pub fn state(&self) -> HealthState {
        HealthState::from_u8(self.state.load(Ordering::Relaxed))
    }

    /// Publishes a new state.
    pub fn set_state(&self, s: HealthState) {
        self.state.store(s as u8, Ordering::Relaxed);
    }
}

/// The movement counters of Plan/18 §17, published by the game thread after every tick (`flyff_move_*`,
/// `flyff_npc_*`, `flyff_keyframes_total`, `flyff_sim_frames_total`, `flyff_movers_awake`). The game owns the
/// plain counters (`MoveStats`); these atomics are the copy `/metrics` reads.
#[derive(Debug)]
pub struct MovementCounters {
    /// Accepted `PlayerMoveState`s by `MoveReason` number (0..=5).
    pub states: [AtomicU64; 6],
    /// Ignored movement messages by reason (the order of `IgnoreReason::ALL`).
    pub ignored: [AtomicU64; 9],
    /// `DestPosReq` accepted.
    pub dest_pos_reqs: AtomicU64,
    /// `SetDestObjReq` accepted.
    pub dest_obj_reqs: AtomicU64,
    /// `MotionReq` accepted / rejected.
    pub motion_reqs: [AtomicU64; 2],
    /// NPC intents captured by kind (12 kinds).
    pub intents: [AtomicU64; 12],
    /// Keyframes: periodic, settle, dev.
    pub keyframes: [AtomicU64; 3],
    /// NPC corrections.
    pub corrections: AtomicU64,
    /// `step_frame` calls of the frame loop.
    pub sim_frames: AtomicU64,
    /// Sim events dispatched at the barriers.
    pub events: AtomicU64,
    /// Awake movers of the last processed layer pass (gauge).
    pub awake: AtomicU64,
    /// Frames that reset a non-finite position or velocity.
    pub sanitised: AtomicU64,
}

impl Default for MovementCounters {
    fn default() -> Self {
        Self {
            states: std::array::from_fn(|_| AtomicU64::new(0)),
            ignored: std::array::from_fn(|_| AtomicU64::new(0)),
            dest_pos_reqs: AtomicU64::new(0),
            dest_obj_reqs: AtomicU64::new(0),
            motion_reqs: std::array::from_fn(|_| AtomicU64::new(0)),
            intents: std::array::from_fn(|_| AtomicU64::new(0)),
            keyframes: std::array::from_fn(|_| AtomicU64::new(0)),
            corrections: AtomicU64::new(0),
            sim_frames: AtomicU64::new(0),
            events: AtomicU64::new(0),
            awake: AtomicU64::new(0),
            sanitised: AtomicU64::new(0),
        }
    }
}

/// Counters shared across threads (Plan/10 §12.1; the catalogue grows with later WPs).
#[derive(Debug)]
pub struct Telemetry {
    clock: MonoClock,
    // ---- tick (written by flyff-game only) ----
    /// Index of the last started tick.
    pub tick: AtomicU64,
    /// Simulation frames completed (written `Release` after `tick_start_ns`).
    pub frames_completed: AtomicU64,
    /// Start of the last tick, ns on the [`MonoClock`].
    pub tick_start_ns: AtomicU64,
    /// End of the last tick, ns on the [`MonoClock`]; `0` before the first tick ends.
    pub last_tick_end_ns: AtomicU64,
    /// The ticks are run by hand (`TickDriverKind::Manual`, tests): time does not pass between ticks, so the
    /// gateway's `ServerTime` estimate is the frame count of the last tick without the elapsed-time part.
    pub manual_ticks: AtomicBool,
    /// Tick duration histogram.
    pub tick_hist: Histogram,
    /// Rolling median of the last 900 tick durations (ns), recomputed every 15 ticks (10 §12.1).
    pub tick_p50_ns: AtomicU64,
    /// Rolling 99th percentile of the last 900 tick durations (ns), recomputed every 15 ticks.
    pub tick_p99_ns: AtomicU64,
    /// The movement pipeline's counters (doc 18 §17), copied here by the game thread each tick.
    pub movement: MovementCounters,
    /// Time per top-level phase of a tick: `(sum_ns, count)` by [`TickPhase`] (`flyff_tick_phase_*`).
    pub phase_ns: [Counter2; 5],
    /// The duration of each top-level phase in the last finished tick (ns; the overrun log reads it).
    pub last_phase_ns: [AtomicU64; 5],
    /// Duration of each world sub-phase per tick, by [`WorldPhase`]: observed once per tick for every phase, also
    /// with 0 (`flyff_world_phase_seconds{phase}`).
    pub world_phase_hist: [PhaseHistogram; 7],
    /// The sum of each world sub-phase over all layers in the last finished tick (ns; the overrun log reads it).
    pub last_world_ns: [AtomicU64; 7],
    /// Per-tick accumulators of [`Telemetry::record_phase`], drained by [`Telemetry::end_tick`].
    world_slots: [AtomicU64; 7],
    /// Inputs dispatched, by [`InputKind`] (`flyff_game_inputs_total{kind}`).
    pub inputs: [AtomicU64; 14],
    /// Live objects by [`OBJECT_KINDS`] (recounted every 15th tick).
    pub objects: [AtomicU64; 5],
    /// Live layers over all worlds.
    pub layers: AtomicU64,
    /// Snapshots handed to persistence, by [`SAVE_REASONS`] (`flyff_saves_total{reason}`).
    pub saves: [AtomicU64; 5],
    /// Messages appended to the session queues (`flyff_outbound_msgs_total`).
    pub out_msgs: AtomicU64,
    /// Ticks slower than 66.7 ms.
    pub overruns_warn: AtomicU64,
    /// Ticks slower than 100 ms.
    pub overruns_error: AtomicU64,
    /// Late-tick re-anchors (KEEP-QUIRK, ADR-002).
    pub resyncs: AtomicU64,
    /// Lateness of the last tick start versus its deadline (ns).
    pub lag_ns: AtomicU64,
    /// Game inbox depth sampled at the start of a tick.
    pub inbox_depth: AtomicU64,
    /// Players in the world.
    pub players_in_world: AtomicU64,
    /// Players lingering without a socket.
    pub players_lingering: AtomicU64,
    /// Pending one-shots + registered scheduler rows and wall jobs (`flyff_sched_timers`, Plan/10 §5.1).
    pub sched_timers: AtomicU64,
    /// Entities the admin map collector visited (cells count 1/16; `flyff_admin_map_visits_total`, Plan/25 §11.9);
    /// written by the game thread's collector.
    pub admin_map_visits: AtomicU64,
    /// The time of the admin views' `post` phase per tick that had a connection (`flyff_admin_views_seconds`, Plan/25 §3.17);
    /// written by the game thread.
    pub admin_views_hist: FineHistogram,
    // ---- gateway (written by flyff-server) ----
    /// Open WebSocket sessions.
    pub sessions: AtomicU64,
    /// Inbound client messages.
    pub inbound_msgs: AtomicU64,
    /// Inbound client bytes.
    pub inbound_bytes: AtomicU64,
    // ---- outbox ----
    /// Batches handed to sinks.
    pub out_batches: AtomicU64,
    /// Bytes handed to sinks.
    pub out_bytes: AtomicU64,
    /// Sessions lost to a full outbound queue.
    pub slow_consumers: AtomicU64,
    /// Panics: `[player, multiparty, unscoped, task]`.
    pub panics: [AtomicU64; 4],
    /// Persistence queue depth gauge.
    pub persist_queue_depth: AtomicU64,
    // ---- health ----
    /// Health block read by `/healthz`.
    pub health: Health,
    /// Duration of start-up steps S1..S12 (ns).
    pub startup_ns: [AtomicU64; 12],
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    /// Creates zeroed telemetry; the monotonic clock starts now.
    pub fn new() -> Self {
        Self {
            clock: MonoClock::start(),
            tick: AtomicU64::new(0),
            frames_completed: AtomicU64::new(0),
            tick_start_ns: AtomicU64::new(0),
            last_tick_end_ns: AtomicU64::new(0),
            manual_ticks: AtomicBool::new(false),
            tick_hist: Histogram::default(),
            tick_p50_ns: AtomicU64::new(0),
            tick_p99_ns: AtomicU64::new(0),
            movement: MovementCounters::default(),
            phase_ns: std::array::from_fn(|_| Counter2::default()),
            last_phase_ns: std::array::from_fn(|_| AtomicU64::new(0)),
            world_phase_hist: std::array::from_fn(|_| PhaseHistogram::default()),
            last_world_ns: std::array::from_fn(|_| AtomicU64::new(0)),
            world_slots: std::array::from_fn(|_| AtomicU64::new(0)),
            inputs: std::array::from_fn(|_| AtomicU64::new(0)),
            objects: std::array::from_fn(|_| AtomicU64::new(0)),
            layers: AtomicU64::new(0),
            saves: std::array::from_fn(|_| AtomicU64::new(0)),
            out_msgs: AtomicU64::new(0),
            overruns_warn: AtomicU64::new(0),
            overruns_error: AtomicU64::new(0),
            resyncs: AtomicU64::new(0),
            lag_ns: AtomicU64::new(0),
            inbox_depth: AtomicU64::new(0),
            players_in_world: AtomicU64::new(0),
            players_lingering: AtomicU64::new(0),
            sched_timers: AtomicU64::new(0),
            admin_map_visits: AtomicU64::new(0),
            admin_views_hist: FineHistogram::default(),
            sessions: AtomicU64::new(0),
            inbound_msgs: AtomicU64::new(0),
            inbound_bytes: AtomicU64::new(0),
            out_batches: AtomicU64::new(0),
            out_bytes: AtomicU64::new(0),
            slow_consumers: AtomicU64::new(0),
            panics: std::array::from_fn(|_| AtomicU64::new(0)),
            persist_queue_depth: AtomicU64::new(0),
            health: Health::default(),
            startup_ns: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// Nanoseconds on this telemetry's monotonic clock.
    pub fn now_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    /// Marks the start of tick `n` (game thread) and publishes the clock pair of Plan/10 §4.4/§12.1:
    /// `tick_start_ns` first, then `frames_completed = 4n` (both `Release`; the gateway's `ServerTime`
    /// estimate reads `frames_completed` first, then `tick_start_ns`, both `Acquire`).
    pub fn begin_tick(&self, n: u64, lag_ns: u64) {
        self.lag_ns.store(lag_ns, Ordering::Relaxed);
        self.tick.store(n, Ordering::Relaxed);
        self.tick_start_ns.store(self.now_ns(), Ordering::Release);
        self.frames_completed
            .store(n.saturating_mul(4), Ordering::Release);
    }

    /// Marks the end of the current tick (game thread): histogram, overrun counters, heartbeat. Returns the
    /// duration of the tick in nanoseconds (the input of the rolling quantiles).
    pub fn end_tick(&self) -> u64 {
        let end = self.now_ns();
        let start = self.tick_start_ns.load(Ordering::Acquire);
        let dur = end.saturating_sub(start);
        self.tick_hist.observe_ns(dur);
        // every world sub-phase is observed once per tick, a silent one with 0: its `_count` rises by exactly the
        // number of executed ticks (WP-OPS-10)
        for ((slot, hist), last) in self
            .world_slots
            .iter()
            .zip(&self.world_phase_hist)
            .zip(&self.last_world_ns)
        {
            let ns = slot.swap(0, Ordering::Relaxed);
            hist.observe_ns(ns);
            last.store(ns, Ordering::Relaxed);
        }
        if dur > 100_000_000 {
            self.overruns_error.fetch_add(1, Ordering::Relaxed);
        } else if dur > 66_700_000 {
            self.overruns_warn.fetch_add(1, Ordering::Relaxed);
        }
        self.last_tick_end_ns.store(end.max(1), Ordering::Relaxed);
        dur
    }

    /// Adds the duration of one top-level phase of the current tick (game thread; once per phase and tick).
    pub fn record_tick_phase(&self, phase: TickPhase, ns: u64) {
        if let Some(c) = self.phase_ns.get(phase.index()) {
            c.sum.fetch_add(ns, Ordering::Relaxed);
            c.count.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(l) = self.last_phase_ns.get(phase.index()) {
            l.store(ns, Ordering::Relaxed);
        }
    }

    /// Adds `ns` to the world sub-phase `phase` of the current tick (lock-free; called once per layer, so the tick
    /// sums over worlds and layers). [`Telemetry::end_tick`] observes the sum and starts the next tick at 0.
    pub fn record_phase(&self, phase: WorldPhase, ns: u64) {
        if let Some(slot) = self.world_slots.get(phase.index()) {
            slot.fetch_add(ns, Ordering::Relaxed);
        }
    }

    /// Counts one dispatched input.
    pub fn count_input(&self, kind: InputKind) {
        if let Some(c) = self.inputs.get(kind.index()) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Age of the last completed tick in milliseconds, `None` before the first tick ended.
    pub fn last_tick_age_ms(&self) -> Option<u64> {
        let end = self.last_tick_end_ns.load(Ordering::Relaxed);
        if end == 0 {
            None
        } else {
            Some(self.now_ns().saturating_sub(end) / 1_000_000)
        }
    }

    /// `/healthz` rule (Plan/10 §12.3): state `Running` or `ShuttingDown` and the last tick ended
    /// at most 2 s ago.
    pub fn is_healthy(&self) -> bool {
        let state_ok = matches!(
            self.health.state(),
            HealthState::Running | HealthState::ShuttingDown
        );
        state_ok && self.last_tick_age_ms().is_some_and(|age| age <= 2_000)
    }
}
