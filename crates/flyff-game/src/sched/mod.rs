//! The `Scheduler` (Plan/10 §5): one-shot timers, the periodic job table with the original `CTimeout`
//! offsets, the 1 s systems slot and the wall-clock jobs.
//!
//! The scheduler never calls a job body itself: job functions take `(&mut Services, &mut GameCtx)` and the
//! `GameCtx` contains `&mut Scheduler`, so the runtime (`tick.rs`, phase 3) first asks the scheduler which
//! bodies are due ([`Scheduler::take_due_timers`], [`Scheduler::due_jobs`], [`Scheduler::evaluate_wall`]) and
//! calls them with the split borrow of `Game::split`. The registered table is `registry.rs`.

pub mod registry;
pub mod wall;

use crate::config::GameConfig;
use crate::ctx::{GameCtx, Services};
use crate::live::LiveConfig;
use crate::tick::TickClock;
use flyff_common::config::EventId;
use flyff_common::{SessionId, Tick, UnixMs};
use smallvec::SmallVec;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use wall::{MemWallStore, WallEngine, WallFire, WallJob, WallSpec, WallStore, WallZone};

/// Handle of a one-shot timer (cancel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerId(u64);

/// Placeholder `Timer` enums, one per top-level `flyff-game` module that needs timers (Plan/10 §5.1).
/// Each is empty until its owner WP adds variants; the module's own `Timer` enum then replaces it.
pub mod timers {
    macro_rules! timer_enums {
        ($($module:ident),* $(,)?) => {
            $(
                #[doc = concat!("Timer events of the `", stringify!($module), "` module (empty until its WP adds variants).")]
                pub mod $module {
                    /// No timer events yet.
                    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
                    pub enum Timer {}
                }
            )*
        };
    }
    timer_enums!(
        session, world, combat, skill, buff, item, econ, npc, quest, social, chat, event, persist
    );

    /// Timer events of the `gm` module (the GM tools and the admin console).
    pub mod gm {
        /// A one-shot of the GM tools.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Timer {
            /// Remove a monster the admin console spawned with a lifetime (25 §3.11, `SpawnMonsters.lifetime_s`):
            /// `admin::actions::world::on_timer`.
            DespawnSpawned(flyff_common::ObjId),
        }
    }
}

/// A one-shot timer event: plain data (no closures), so the queue is inspectable and deterministic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerEvent {
    /// `session` module.
    Session(timers::session::Timer),
    /// `world` module.
    World(timers::world::Timer),
    /// `combat` module.
    Combat(timers::combat::Timer),
    /// `skill` module.
    Skill(timers::skill::Timer),
    /// `buff` module.
    Buff(timers::buff::Timer),
    /// `item` module.
    Item(timers::item::Timer),
    /// `econ` module.
    Econ(timers::econ::Timer),
    /// `npc` module.
    Npc(timers::npc::Timer),
    /// `quest` module.
    Quest(timers::quest::Timer),
    /// `social` module.
    Social(timers::social::Timer),
    /// `chat` module.
    Chat(timers::chat::Timer),
    /// `event` module.
    Event(timers::event::Timer),
    /// `gm` module.
    Gm(timers::gm::Timer),
    /// `persist` module.
    Persist(timers::persist::Timer),
}

/// Who runs a timer (Plan/10 §14.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TimerScope(pub Option<SessionId>);

/// A due one-shot: the runtime dispatches it under its scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DueTimer {
    /// Handle (already consumed).
    pub id: TimerId,
    /// What to do.
    pub event: TimerEvent,
    /// `Some(session)`: Player scope for that session; `None`: unscoped.
    pub scope: TimerScope,
}

/// Binary heap of one-shots ordered by `(due tick, sequence number)` with tombstone cancel (Plan/10 §5.1:
/// "cancelling is O(log n)").
#[derive(Debug)]
pub struct OneShots<E> {
    heap: BinaryHeap<Reverse<(u64, u64)>>,
    live: BTreeMap<u64, E>,
    next_seq: u64,
}

impl<E> Default for OneShots<E> {
    fn default() -> Self {
        Self {
            heap: BinaryHeap::new(),
            live: BTreeMap::new(),
            next_seq: 0,
        }
    }
}

impl<E> OneShots<E> {
    /// Schedules `event` for tick `due` (a due tick in the past fires at the next executed tick).
    pub fn schedule(&mut self, due: Tick, event: E) -> TimerId {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.heap.push(Reverse((due.0, seq)));
        self.live.insert(seq, event);
        TimerId(seq)
    }

    /// Cancels a timer; `false` when it already fired or was cancelled.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        self.live.remove(&id.0).is_some()
    }

    /// Removes and returns every timer with `due <= now`, ordered by `(due, sequence)`.
    pub fn pop_due(&mut self, now: Tick) -> Vec<(TimerId, E)> {
        let mut out = Vec::new();
        while let Some(&Reverse((due, seq))) = self.heap.peek() {
            if due > now.0 {
                break;
            }
            self.heap.pop();
            // A cancelled timer left a tombstone: skip it.
            if let Some(event) = self.live.remove(&seq) {
                out.push((TimerId(seq), event));
            }
        }
        out
    }

    /// Pending (not cancelled) timers.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// `true` when nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
}

/// A periodic or slot job body (Plan/10 §4.1.1).
pub type JobFn = fn(&mut Services, &mut GameCtx<'_>);

/// Owning document number (13, 15, 18, 21, 22, …), for logs and metrics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocId(pub u8);

/// When a job runs at all.
#[derive(Clone, Copy)]
pub enum Gate {
    /// Always.
    Always,
    /// While the event flag is non-zero (`events.*`, the `g_eLocal.GetState` checks).
    Event(EventId),
    /// While the startup configuration says so.
    Feature(fn(&GameConfig) -> bool),
}

impl std::fmt::Debug for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Gate::Always => f.write_str("Always"),
            Gate::Event(id) => write!(f, "Event({id:?})"),
            Gate::Feature(_) => f.write_str("Feature(..)"),
        }
    }
}

impl Gate {
    /// Evaluates the gate against the startup and live configuration.
    pub fn is_open(&self, cfg: &GameConfig, live: &LiveConfig) -> bool {
        match self {
            Gate::Always => true,
            Gate::Event(id) => live.event_state(*id) != 0,
            Gate::Feature(f) => f(cfg),
        }
    }
}

/// One row (or item of a row) of the periodic job table of §5.3. Jobs of one row (P3.1, P3.2) share
/// `period` and `phase` and run in registration order.
#[derive(Clone, Copy, Debug)]
pub struct PeriodicJob {
    /// Table row, e.g. `"P3"`.
    pub row: &'static str,
    /// Job name (logs, overrun report, metrics).
    pub name: &'static str,
    /// Owning document.
    pub owner: DocId,
    /// Period in ticks.
    pub period: u64,
    /// Phase: the job runs at ticks `n` with `n % period == phase`.
    pub phase: u64,
    /// Gate.
    pub gate: Gate,
    /// Body.
    pub run: JobFn,
}

impl PeriodicJob {
    /// A job of table row `row` (`name`, owning document `owner`) running at ticks `n` with
    /// `n % period == phase`.
    pub const fn new(
        row: &'static str,
        name: &'static str,
        owner: u8,
        period: u64,
        phase: u64,
        gate: Gate,
        run: JobFn,
    ) -> PeriodicJob {
        PeriodicJob {
            row,
            name,
            owner: DocId(owner),
            period,
            phase,
            gate,
            run,
        }
    }
}

/// One item of the 1 s systems slot (§5.4); `item` is the §5.4 number.
#[derive(Clone, Copy, Debug)]
pub struct SlotItem {
    /// §5.4 item number (1 `game_clock`, 4–16; 17 is the wall-clock evaluation run by the runtime).
    pub item: u8,
    /// Item name.
    pub name: &'static str,
    /// Owning document.
    pub owner: DocId,
    /// Gate.
    pub gate: Gate,
    /// Body.
    pub run: JobFn,
}

impl SlotItem {
    /// Slot item number `item` of §5.4.
    pub const fn new(item: u8, name: &'static str, owner: u8, gate: Gate, run: JobFn) -> SlotItem {
        SlotItem {
            item,
            name,
            owner: DocId(owner),
            gate,
            run,
        }
    }
}

/// The table row whose firing runs the slot items (P2).
pub const SLOT_ROW: &str = "P2";

/// Which part of phase 3 a job belongs to (§4.4 steps 3.2–3.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobGroup {
    /// 3.2 P1 respawn.
    Respawn,
    /// 3.3 P2 the 1 s systems slot (items in §5.4 order, then the wall-clock evaluation).
    Slot,
    /// 3.4 P3 roll call.
    RollCall,
    /// 3.5 P4–P14 in table order.
    Periodic,
    /// 3.6 per-tick jobs P15, P16, P18, P19.
    PerTick,
}

impl JobGroup {
    /// The group of a table row (`"P11"` → [`JobGroup::Periodic`]).
    pub fn of_row(row: &str) -> JobGroup {
        match row
            .trim_start_matches('P')
            .split('.')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
        {
            Some(1) => JobGroup::Respawn,
            Some(2) => JobGroup::Slot,
            Some(3) => JobGroup::RollCall,
            Some(4..=14) => JobGroup::Periodic,
            _ => JobGroup::PerTick,
        }
    }
}

/// What the runtime has to run for a tick, in order.
#[derive(Clone, Copy, Debug)]
pub enum DueJob {
    /// A periodic row job or a slot item.
    Run {
        /// Phase-3 group.
        group: JobGroup,
        /// Job name.
        name: &'static str,
        /// Body.
        run: JobFn,
    },
    /// Slot item 17: evaluate the wall-clock jobs (`Scheduler::evaluate_wall`).
    Wall,
}

impl DueJob {
    /// The phase-3 group.
    pub fn group(&self) -> JobGroup {
        match self {
            DueJob::Run { group, .. } => *group,
            DueJob::Wall => JobGroup::Slot,
        }
    }

    /// The job name (`"wall_evaluate"` for [`DueJob::Wall`]).
    pub fn name(&self) -> &'static str {
        match self {
            DueJob::Run { name, .. } => name,
            DueJob::Wall => "wall_evaluate",
        }
    }
}

/// How often and when one job ran (the admin console's scheduler page, Plan/25 §9.6). Bookkeeping only: it is not
/// part of the replay digest and no game logic reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JobStats {
    /// Runs by the scheduler and by the console.
    pub runs: u64,
    /// The tick of the last run.
    pub last_tick: Option<Tick>,
    /// The wall clock of the last run (the sample of its tick).
    pub last_unix_ms: Option<UnixMs>,
    /// Runs the console asked for (`AdminCmd::RunJob`).
    pub manual_runs: u64,
    /// The wall clock of the last run the console asked for.
    pub last_manual_unix_ms: Option<UnixMs>,
}

/// Where a job comes from (Plan/10 §5.3-§5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    /// A row of the periodic table (P1, P3..P18).
    Periodic,
    /// An item of the 1 s slot (§5.4), including item 17 (the wall evaluation).
    Slot,
    /// A wall-clock job (§5.5).
    Wall,
}

impl JobKind {
    /// `periodic`, `slot` or `wall`.
    pub fn name(self) -> &'static str {
        match self {
            JobKind::Periodic => "periodic",
            JobKind::Slot => "slot",
            JobKind::Wall => "wall",
        }
    }
}

/// One job as the scheduler page lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobInfo {
    /// The job name (`mail_expire`, `maintenance`).
    pub name: &'static str,
    /// Periodic row, slot item or wall job.
    pub kind: JobKind,
    /// `P8`, `slot 7`, `wall`.
    pub row: String,
    /// The owning document.
    pub owner: DocId,
    /// A readable schedule (`every 60 s`, `daily 05:00`).
    pub schedule: String,
    /// The gate (an event flag, a feature) is open now.
    pub gate_open: bool,
    /// Run statistics.
    pub stats: JobStats,
    /// The next tick the job is due at (periodic rows and slot items).
    pub next_tick: Option<Tick>,
    /// The next calendar occurrence (wall jobs).
    pub next_unix_ms: Option<UnixMs>,
}

/// The body of a job the console may run on demand.
#[derive(Clone, Copy)]
pub enum JobBody {
    /// A periodic row or a slot item.
    Periodic(JobFn),
    /// A wall job (the stamp is the current local time).
    Wall(wall::WallFn),
}

/// The scheduler (Plan/10 §5.1).
pub struct Scheduler {
    oneshots: OneShots<(TimerEvent, TimerScope)>,
    periodic: Vec<PeriodicJob>,
    slot: Vec<SlotItem>,
    wall: WallEngine,
    trace: Option<Vec<(Tick, &'static str)>>,
    stats: BTreeMap<&'static str, JobStats>,
    guard: crate::thread_guard::GameThreadGuard,
}

impl Scheduler {
    /// A scheduler with an empty table, evaluating wall jobs in `zone` against `store`.
    pub fn new(zone: WallZone, store: Box<dyn WallStore>) -> Scheduler {
        Scheduler {
            oneshots: OneShots::default(),
            periodic: Vec::new(),
            slot: Vec::new(),
            wall: WallEngine::new(zone, store),
            trace: None,
            stats: BTreeMap::new(),
            guard: crate::thread_guard::GameThreadGuard::new(),
        }
    }

    /// A scheduler in `zone` with an in-memory `last_fired` store (tests, and until WP-OPS-07 wires
    /// `server_kv`).
    pub fn with_zone(zone: WallZone) -> Scheduler {
        Scheduler::new(zone, Box::new(MemWallStore::default()))
    }

    /// Schedules a one-shot for tick `due`.
    pub fn at(&mut self, due: Tick, ev: TimerEvent, scope: TimerScope) -> TimerId {
        self.guard.check();
        self.oneshots.schedule(due, (ev, scope))
    }

    /// Schedules a one-shot `delay_ms` of game time after `now` (`Tick::after_ms`: the first tick whose
    /// `game_ms` reaches the deadline, at least the next tick).
    pub fn after_ms(
        &mut self,
        now: &TickClock,
        delay_ms: u64,
        ev: TimerEvent,
        scope: TimerScope,
    ) -> TimerId {
        self.at(now.tick.after_ms(delay_ms), ev, scope)
    }

    /// Cancels a one-shot.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        self.guard.check();
        self.oneshots.cancel(id)
    }

    /// Pending one-shots + registered periodic rows + slot items + wall jobs (metric
    /// `flyff_sched_timers`).
    pub fn len(&self) -> usize {
        self.oneshots.len() + self.periodic.len() + self.slot.len() + self.wall.len()
    }

    /// `true` when nothing is scheduled or registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pending one-shots only.
    pub fn pending_timers(&self) -> usize {
        self.oneshots.len()
    }

    /// Registers a periodic job (Game construction only, in the order of §5.3).
    ///
    /// # Panics
    /// On a zero period, a phase `>= period`, a row whose jobs disagree about `(period, phase)`, or two
    /// period-900 rows with the same phase (the uniqueness rule of §5.3). These are table bugs found at
    /// construction.
    pub fn register_periodic(&mut self, job: PeriodicJob) {
        assert!(
            job.period > 0,
            "periodic job '{}': period must be > 0",
            job.name
        );
        assert!(
            job.phase < job.period,
            "periodic job '{}': phase {} must be < period {}",
            job.name,
            job.phase,
            job.period
        );
        for other in &self.periodic {
            if other.row == job.row {
                assert!(
                    other.period == job.period && other.phase == job.phase,
                    "row {}: '{}' and '{}' must share period and phase",
                    job.row,
                    other.name,
                    job.name
                );
            } else if job.period == 900 && other.period == 900 {
                assert!(
                    other.phase != job.phase,
                    "rows {} and {} both use period 900 phase {} (phases must be unique)",
                    other.row,
                    job.row,
                    job.phase
                );
            }
            assert!(
                other.name != job.name,
                "periodic job '{}' registered twice",
                job.name
            );
        }
        self.periodic.push(job);
    }

    /// Registers an item of the 1 s slot (Game construction only, in the order of §5.4).
    ///
    /// # Panics
    /// When the item number does not increase (the order of §5.4 is normative) or is outside 1–16 and 18
    /// (item 18 is the NPC idle speech of doc 20, WP-NPC-08).
    pub fn register_slot_item(&mut self, item: SlotItem) {
        assert!(
            matches!(item.item, 1..=16 | 18),
            "slot item {}: only items 1..=16 and 18 are registered (17 is the wall evaluation)",
            item.item
        );
        assert!(
            self.slot.last().is_none_or(|l| l.item < item.item),
            "slot item {} must come after item {} (§5.4 order)",
            item.item,
            self.slot.last().map_or(0, |l| l.item)
        );
        self.slot.push(item);
    }

    /// Registers a wall-clock job (Game construction only).
    ///
    /// # Panics
    /// On an empty or invalid spec, a duplicate name, or `RunOnceAtStart` with a minute spec.
    pub fn register_wall(&mut self, job: WallJob) {
        self.wall.register(job);
    }

    /// The next calendar occurrence of a wall job (`None` before the first evaluation, for minute-only
    /// jobs and for unknown names).
    pub fn next_fire(&self, name: &str) -> Option<UnixMs> {
        self.wall.next_fire(name)
    }

    /// Replaces the specs of a wall job at runtime (22: `/lua rr`); the next occurrence is recomputed at
    /// the next evaluation. Invalid specs are ignored with a warning.
    pub fn respec_wall(&mut self, name: &str, specs: SmallVec<[WallSpec; 2]>) {
        self.wall.respec(name, specs);
    }

    /// The server time zone.
    pub fn zone(&self) -> &WallZone {
        self.wall.zone()
    }

    /// Phase 3.1: removes the one-shots due at `now`, ordered by `(due tick, sequence)`.
    pub fn take_due_timers(&mut self, now: Tick) -> Vec<DueTimer> {
        self.guard.check();
        self.oneshots
            .pop_due(now)
            .into_iter()
            .map(|(id, (event, scope))| DueTimer { id, event, scope })
            .collect()
    }

    /// Phases 3.2–3.6: the periodic jobs due at `tick` in execution order (table order; P2 expands to the
    /// open slot items in §5.4 order followed by the wall-clock evaluation, item 17). Closed gates skip
    /// the job.
    pub fn due_jobs(&self, tick: Tick, cfg: &GameConfig, live: &LiveConfig) -> Vec<DueJob> {
        let mut out = Vec::new();
        for job in &self.periodic {
            if tick.0 % job.period != job.phase || !job.gate.is_open(cfg, live) {
                continue;
            }
            if job.row == SLOT_ROW {
                for item in &self.slot {
                    if item.gate.is_open(cfg, live) {
                        out.push(DueJob::Run {
                            group: JobGroup::Slot,
                            name: item.name,
                            run: item.run,
                        });
                    }
                }
                out.push(DueJob::Wall);
            } else {
                out.push(DueJob::Run {
                    group: JobGroup::of_row(job.row),
                    name: job.name,
                    run: job.run,
                });
            }
        }
        out
    }

    /// Slot item 17: evaluates the wall-clock jobs at `clock.unix_ms` and returns the bodies to call, in
    /// registration order.
    pub fn evaluate_wall(&mut self, clock: &TickClock) -> SmallVec<[WallFire; 4]> {
        self.guard.check();
        self.wall.evaluate(clock)
    }

    /// Starts recording `(tick, job name)` of every executed job (tests, `TestWorld`).
    pub fn enable_trace(&mut self) {
        self.trace.get_or_insert_with(Vec::new);
    }

    /// Records one executed job when tracing is on.
    pub fn note_run(&mut self, tick: Tick, name: &'static str) {
        if let Some(t) = self.trace.as_mut() {
            t.push((tick, name));
        }
    }

    /// The recorded executions (empty unless [`Scheduler::enable_trace`] was called).
    pub fn trace(&self) -> &[(Tick, &'static str)] {
        self.trace.as_deref().unwrap_or(&[])
    }

    /// The registered periodic rows (inspection, `/admin/api/status`).
    pub fn periodic_jobs(&self) -> &[PeriodicJob] {
        &self.periodic
    }

    /// Counts one run of job `name` at `tick` (`unix_ms` is the wall-clock sample of the tick). `manual` is a run the
    /// admin console asked for. Called by the runtime after each job body (Plan/25 §9.6).
    pub fn record_run(&mut self, tick: Tick, unix_ms: UnixMs, name: &'static str, manual: bool) {
        let s = self.stats.entry(name).or_default();
        s.runs += 1;
        s.last_tick = Some(tick);
        s.last_unix_ms = Some(unix_ms);
        if manual {
            s.manual_runs += 1;
            s.last_manual_unix_ms = Some(unix_ms);
        }
    }

    /// The run statistics of one job (`None` for a name that is not registered).
    pub fn job_stats(&self, name: &str) -> Option<JobStats> {
        let known = self.periodic.iter().any(|j| j.name == name)
            || self.slot.iter().any(|j| j.name == name)
            || self.wall.has(name)
            || name == WALL_EVALUATE;
        known.then(|| {
            self.stats
                .iter()
                .find(|(n, _)| **n == name)
                .map(|(_, s)| *s)
                .unwrap_or_default()
        })
    }

    /// The body of job `name`, for the jobs the console may run on demand (the caller decides which).
    pub fn job_body(&self, name: &str) -> Option<JobBody> {
        if let Some(j) = self
            .periodic
            .iter()
            .find(|j| j.name == name && j.row != SLOT_ROW)
        {
            return Some(JobBody::Periodic(j.run));
        }
        if let Some(j) = self.slot.iter().find(|j| j.name == name) {
            return Some(JobBody::Periodic(j.run));
        }
        self.wall.body(name).map(JobBody::Wall)
    }

    /// Every registered job with its schedule, gate, statistics and next run, in table order: periodic rows, slot
    /// items (with item 17, the wall evaluation), wall jobs (`now` is the current tick).
    pub fn jobs(&self, cfg: &GameConfig, live: &LiveConfig, now: Tick) -> Vec<JobInfo> {
        let stats = |name: &str| self.job_stats(name).unwrap_or_default();
        let mut out = Vec::new();
        for j in &self.periodic {
            if j.row == SLOT_ROW {
                continue;
            }
            out.push(JobInfo {
                name: j.name,
                kind: JobKind::Periodic,
                row: j.row.to_owned(),
                owner: j.owner,
                schedule: describe_period(j.period, j.phase),
                gate_open: j.gate.is_open(cfg, live),
                stats: stats(j.name),
                next_tick: Some(next_due(now, j.period, j.phase)),
                next_unix_ms: None,
            });
        }
        let slot_period = self
            .periodic
            .iter()
            .find(|j| j.row == SLOT_ROW)
            .map_or((15, 0), |j| (j.period, j.phase));
        for it in &self.slot {
            out.push(JobInfo {
                name: it.name,
                kind: JobKind::Slot,
                row: format!("slot {}", it.item),
                owner: it.owner,
                schedule: format!(
                    "{}, item {}",
                    describe_period(slot_period.0, slot_period.1),
                    it.item
                ),
                gate_open: it.gate.is_open(cfg, live),
                stats: stats(it.name),
                next_tick: Some(next_due(now, slot_period.0, slot_period.1)),
                next_unix_ms: None,
            });
        }
        out.push(JobInfo {
            name: WALL_EVALUATE,
            kind: JobKind::Slot,
            row: "slot 17".to_owned(),
            owner: DocId(10),
            schedule: format!("{}, item 17", describe_period(slot_period.0, slot_period.1)),
            gate_open: true,
            stats: stats(WALL_EVALUATE),
            next_tick: Some(next_due(now, slot_period.0, slot_period.1)),
            next_unix_ms: None,
        });
        for (name, owner, specs) in self.wall.describe() {
            out.push(JobInfo {
                name,
                kind: JobKind::Wall,
                row: "wall".to_owned(),
                owner,
                schedule: specs,
                gate_open: true,
                stats: stats(name),
                next_tick: None,
                next_unix_ms: self.wall.next_fire(name),
            });
        }
        out
    }

    /// The registered slot items (inspection).
    pub fn slot_items(&self) -> &[SlotItem] {
        &self.slot
    }
}

/// The name under which the wall-clock evaluation (slot item 17) is counted and listed.
pub const WALL_EVALUATE: &str = "wall_evaluate";

/// The next tick at or after `now` with `tick % period == phase`.
fn next_due(now: Tick, period: u64, phase: u64) -> Tick {
    let period = period.max(1);
    let r = now.0 % period;
    Tick(if r <= phase {
        now.0 - r + phase
    } else {
        now.0 - r + period + phase
    })
}

/// `every 60 s (phase 301 ticks)`, `every tick`, `every 5 s`.
fn describe_period(period: u64, phase: u64) -> String {
    if period <= 1 {
        return "every tick".to_owned();
    }
    let per_s = Tick::PER_SECOND;
    let secs = if period.is_multiple_of(per_s) {
        format!("{} s", period / per_s)
    } else {
        format!("{period} ticks")
    };
    format!("every {secs} (phase {phase})")
}
