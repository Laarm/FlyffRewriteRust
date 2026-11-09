//! The game thread and its 15 Hz loop (Plan/10 §4, 01 §6.3, ADR-002).
//!
//! Deadline math is integer-only (`n · 10⁹ / 15` ns from an anchor, no drift, [`DeadlineGrid`]). A tick that
//! is late by more than one period runs once and **re-anchors** (KEEP-QUIRK of the original `CTimeout`, no
//! catch-up burst). Phases follow Plan/10 §4.4: 1 drain inputs, 2 worlds, 3 timers (one-shots, respawn, the
//! 1 s slot, roll call, periodic and per-tick jobs, shutdown step, save scheduler), 4 flush, 5 post
//! (telemetry). The world phase and the save scheduler are empty hooks until their WPs land.

// The pacing driver is the only code that reads a monotonic clock and sleeps (Plan/10 §4.3), and
// `WallClockCell::now_ms` is the only wall-clock sample (recorded for replay, D2). Game logic never calls
// them.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use crate::chat::ChatData;
use crate::config::GameConfig;
use crate::ctx::{Services, Worlds};
use crate::dispatch::DispatchStats;
use crate::handlers::Handlers;
use crate::input::{GameInput, NetEvent, StatusSnapshot};
use crate::live::LiveConfig;
use crate::outbox::Outbox;
use crate::panic::{HandlerScope, PanicInfo, catch};
use crate::rng::RngSet;
use crate::sched::registry;
use crate::sched::wall::WallZone;
use crate::sched::{DueJob, DueTimer, JobGroup, Scheduler, TimerEvent};
use crate::session::{LeaveTrigger, SessionTable};
use crate::stats_pub::{OverrunLevel, Slowest};
use crate::tickets::{FlushPurpose, Pending, TicketRegistry};
use crate::world::{PhaseEnv, SpawnCacheStats, SpawnWriter, WorldInit};
use flyff_common::rt::telemetry::HealthState;
use flyff_common::rt::{CloseReason, Reply, ShutdownHandle, Telemetry, TickPhase, reply_pair};
use flyff_common::{CharacterId, GameMs, ObjId, SessionId, SimFrame, Tick, UnixMs};
use flyff_data::GameData;
use flyff_db::{AuditLog, DbReply, PersistHandle, StartupState, Ticket};
use flyff_lua::{LuaHost, LuaInit};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Nanoseconds in one tick period (66.666… ms, truncated; deadlines use the exact fraction).
const PERIOD_NS: u64 = 1_000_000_000 / 15;

/// A wall-clock instant broken down in the server time zone (Plan/10 §4.2). Produced only by
/// [`WallZone::local`](crate::sched::wall::WallZone::local), never by game code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalStamp {
    /// Calendar year.
    pub year: i16,
    /// 1–12.
    pub month: u8,
    /// 1–31.
    pub day: u8,
    /// 0–23.
    pub hour: u8,
    /// 0–59.
    pub minute: u8,
    /// 0–59.
    pub second: u8,
    /// 0 = Sunday … 6 = Saturday (`%w`).
    pub weekday: u8,
    /// Offset of the zone from UTC at this instant, seconds east.
    pub utc_offset_s: i32,
    /// The instant this stamp describes (Unix seconds).
    pub unix_s: i64,
}

impl LocalStamp {
    /// `HHMM` (2100 = 21:00), for `WallSpec::Window`.
    pub fn hm(&self) -> u16 {
        u16::from(self.hour) * 100 + u16::from(self.minute)
    }
}

/// The clock values every handler of one tick sees (Plan/10 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickClock {
    /// Current tick `n`.
    pub tick: Tick,
    /// `floor(n · 1000 / 15)`: the only "now" for in-game timers (PL-012).
    pub game_ms: GameMs,
    /// First simulation frame of the tick (`4n`).
    pub frame0: SimFrame,
    /// Wall clock sampled once at the start of the tick (recorded for replay).
    pub unix_ms: UnixMs,
    /// `unix_ms` broken down in `server.timezone`, computed once per tick.
    pub local: LocalStamp,
}

impl TickClock {
    /// Builds the clock of tick `tick` with the sampled wall-clock time, broken down in `zone`.
    pub fn new(tick: Tick, unix_ms: UnixMs, zone: &WallZone) -> Self {
        Self {
            tick,
            game_ms: tick.game_ms(),
            frame0: tick.first_frame(),
            unix_ms,
            local: zone.local(unix_ms),
        }
    }

    /// [`TickClock::new`] in UTC.
    pub fn utc(tick: Tick, unix_ms: UnixMs) -> Self {
        Self::new(tick, unix_ms, &WallZone::utc())
    }
}

/// Source of the per-tick wall-clock sample (Plan/10 §2.4).
#[derive(Clone, Debug, Default)]
pub enum WallClockSource {
    /// `SystemTime::now()`.
    #[default]
    System,
    /// System clock plus a fixed offset in milliseconds.
    Offset(i64),
    /// Moves only by [`WallClockCell::set_manual_ms`]; starts frozen at the current system time.
    Manual,
    /// [`WallClockSource::Manual`] starting at this instant (tests).
    Fixed(UnixMs),
    /// An existing cell shared with the server handle and the gateway (`ServerHandle::set_wall_ms`,
    /// `ServerTime.server_unix_ms`).
    Cell(WallClockCell),
}

const MODE_SYSTEM: u8 = 0;
const MODE_OFFSET: u8 = 1;
const MODE_MANUAL: u8 = 2;

#[derive(Debug)]
struct WallCellInner {
    mode: AtomicU8,
    /// Offset in ms (`MODE_OFFSET`) or the frozen instant (`MODE_MANUAL`).
    arg: AtomicI64,
    /// The value of the last per-tick sample.
    last: AtomicI64,
}

/// The shared wall-clock cell behind the single per-tick sample (Plan/10 §2.4, §5.5 rule 8). The game
/// thread reads it once at the start of every tick; the gateway reads [`WallClockCell::now_ms`] for
/// `ServerTime.server_unix_ms`. Changes made through the cell take effect at the next tick's sample, never
/// inside a tick.
#[derive(Clone, Debug)]
pub struct WallClockCell(Arc<WallCellInner>);

fn system_ms() -> i64 {
    SystemTime::now() // det-ok: the one wall-clock sample per tick (recorded for replay)
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl WallClockCell {
    /// A cell for `src`.
    pub fn new(src: WallClockSource) -> WallClockCell {
        let (mode, arg) = match src {
            WallClockSource::System => (MODE_SYSTEM, 0),
            WallClockSource::Offset(ms) => (MODE_OFFSET, ms),
            WallClockSource::Manual => (MODE_MANUAL, system_ms()),
            WallClockSource::Fixed(t) => (MODE_MANUAL, t.0),
            WallClockSource::Cell(cell) => return cell,
        };
        WallClockCell(Arc::new(WallCellInner {
            mode: AtomicU8::new(mode),
            arg: AtomicI64::new(arg),
            last: AtomicI64::new(0),
        }))
    }

    /// Switches the source. Switching to `Manual` freezes the last sampled value (or the current system
    /// time before the first sample); `Fixed(t)` freezes `t`. Takes effect at the next tick's sample.
    pub fn set_source(&self, src: WallClockSource) {
        let inner = &self.0;
        match src {
            WallClockSource::System => inner.mode.store(MODE_SYSTEM, Ordering::Release),
            WallClockSource::Offset(ms) => {
                inner.arg.store(ms, Ordering::Release);
                inner.mode.store(MODE_OFFSET, Ordering::Release);
            }
            WallClockSource::Manual => {
                let frozen = match inner.last.load(Ordering::Acquire) {
                    0 => self.now_ms().0,
                    last => last,
                };
                inner.arg.store(frozen, Ordering::Release);
                inner.mode.store(MODE_MANUAL, Ordering::Release);
            }
            WallClockSource::Fixed(t) => {
                inner.arg.store(t.0, Ordering::Release);
                inner.mode.store(MODE_MANUAL, Ordering::Release);
            }
            WallClockSource::Cell(_) => {}
        }
    }

    /// Sets the frozen instant of a `Manual` cell; ignored (returns `false`) in any other mode.
    pub fn set_manual_ms(&self, t: UnixMs) -> bool {
        if self.0.mode.load(Ordering::Acquire) == MODE_MANUAL {
            self.0.arg.store(t.0, Ordering::Release);
            true
        } else {
            false
        }
    }

    /// The current value of the source (readable from any thread).
    pub fn now_ms(&self) -> UnixMs {
        let arg = self.0.arg.load(Ordering::Acquire);
        match self.0.mode.load(Ordering::Acquire) {
            MODE_OFFSET => UnixMs(system_ms().saturating_add(arg)),
            MODE_MANUAL => UnixMs(arg),
            _ => UnixMs(system_ms()),
        }
    }

    /// The per-tick sample: [`WallClockCell::now_ms`], remembered as the last sample. Called once per tick
    /// by the game thread only.
    fn sample(&self) -> UnixMs {
        let now = self.now_ms();
        self.0.last.store(now.0, Ordering::Release);
        now
    }

    /// The value of the last per-tick sample (`UnixMs(0)` before the first tick).
    pub fn last_sample(&self) -> UnixMs {
        UnixMs(self.0.last.load(Ordering::Acquire))
    }
}

/// The pure deadline arithmetic of the tick loop (Plan/10 §4.3): no clock, no sleep, testable with fake
/// times. Deadlines are `anchor + (n − anchor_tick) · 10⁹ / 15` ns in integers (no drift); a tick late by
/// more than one period runs once and re-anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadlineGrid {
    anchor_ns: u64,
    anchor_tick: u64,
}

/// What the loop does before tick `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// Sleep this many nanoseconds, then run.
    Sleep(u64),
    /// Run now.
    Run {
        /// How late the tick is (0 when it was on time).
        lag_ns: u64,
        /// The grid was re-anchored (more than one period late): count `flyff_tick_resyncs_total`.
        resync: bool,
    },
}

impl DeadlineGrid {
    /// A grid whose tick `first_tick` is due at `now_ns`.
    pub fn new(now_ns: u64, first_tick: u64) -> Self {
        Self {
            anchor_ns: now_ns,
            anchor_tick: first_tick,
        }
    }

    /// The deadline of tick `n` in nanoseconds on the loop's own clock.
    pub fn deadline_ns(&self, n: u64) -> u64 {
        let ticks = n.saturating_sub(self.anchor_tick);
        self.anchor_ns
            .saturating_add(ticks.saturating_mul(1_000_000_000) / 15)
    }

    /// Phase 0 for tick `n` at time `now_ns`: sleep until the deadline, run late ticks immediately
    /// (staying on the grid when less than one period late: short delays are compensated) and re-anchor
    /// after a longer stall (KEEP-QUIRK, R2: run once, no catch-up burst).
    pub fn wait(&mut self, n: u64, now_ns: u64) -> Wait {
        let deadline = self.deadline_ns(n);
        if now_ns < deadline {
            return Wait::Sleep(deadline - now_ns);
        }
        let lag_ns = now_ns - deadline;
        if lag_ns > PERIOD_NS {
            self.anchor_ns = now_ns;
            self.anchor_tick = n;
            Wait::Run {
                lag_ns,
                resync: true,
            }
        } else {
            Wait::Run {
                lag_ns,
                resync: false,
            }
        }
    }
}

/// How the loop is paced (Plan/10 §4.1 `TickDriverKind`; the channel-carrying form is [`TickDriver`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickDriverKind {
    /// Real time at 15 Hz.
    RealTime,
    /// Tests, `TestWorld` and replay step ticks explicitly.
    Manual,
}

/// A hook of the tick (Plan/10 §4.4), recorded in the call log of [`Game::record_hooks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hook {
    /// Phase 1: drain C1 and C6.
    Input,
    /// Phase 2: worlds (doc 15).
    Worlds,
    /// 3.1 one-shot timers.
    OneShots,
    /// 3.2 P1 respawn.
    Respawn,
    /// 3.3 P2 the 1 s slot incl. the wall evaluation.
    Slot,
    /// 3.4 P3 roll call.
    RollCall,
    /// 3.5 P4–P14.
    Periodic,
    /// 3.6 per-tick jobs.
    PerTick,
    /// 3.7 shutdown step.
    ShutdownStep,
    /// 3.8 SaveScheduler pass (WP-DB-07).
    SaveScheduler,
    /// Phase 4: flush.
    Flush,
    /// Phase 5: post (telemetry gauges).
    Post,
}

/// A request to run `ticks` ticks under [`TickDriver::Manual`].
pub struct StepRequest {
    /// Number of ticks to run (`0` completes immediately).
    pub ticks: u32,
    /// Completed after the last of the requested ticks.
    pub done: Reply<()>,
}

/// How the loop is paced.
pub enum TickDriver {
    /// Real time at 15 Hz.
    RealTime,
    /// Tests step ticks explicitly through [`StepRequest`]s.
    Manual(flume::Receiver<StepRequest>),
}

/// Why the game thread could not start.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("game start failed: {0}")]
pub struct GameStartError(pub String);

/// How the game loop ended (Plan/01 §5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameExit {
    /// Graceful shutdown completed.
    Shutdown,
    /// An unscoped panic ended the loop (exit code 70).
    Panicked,
    /// The persistence writer died (exit 70) or reported corruption (exit 74).
    PersistenceFailed {
        /// The writer stopped on a corrupt database.
        corrupt: bool,
    },
    /// The game thread could not be built (reported through `GameInit.ready`).
    StartFailed,
}

/// Everything the game thread needs (Plan/01 §5.5 `GameInit`; `data`/`derived` follow with the data
/// WPs).
pub struct GameInit {
    /// Tick-loop configuration.
    pub cfg: GameConfig,
    /// Persistence handle (C4).
    pub persist: PersistHandle,
    /// Events to the lobby/gateway (C3).
    pub net_events: flume::Sender<NetEvent>,
    /// Persistence completions (C6).
    pub db_replies: flume::Receiver<DbReply>,
    /// Initial item-serial blocks.
    pub serial_blocks: [std::ops::Range<u64>; 2],
    /// Audit emitter.
    pub audit: AuditLog,
    /// Shared counters.
    pub telemetry: Arc<Telemetry>,
    /// Lua description; the VM is built on the game thread.
    pub lua: LuaInit,
    /// State loaded at boot.
    pub startup: StartupState,
    /// RNG seed.
    pub seed: u64,
    /// Server-side chat tables.
    pub chat_data: ChatData,
    /// In-game `/shutdown` entry point.
    pub shutdown: Box<dyn ShutdownHandle>,
    /// Pacing.
    pub driver: TickDriver,
    /// Wall-clock sample source.
    pub wall_clock: WallClockSource,
    /// Receives `Ok(())` once the game is built (before the first tick), or the failure.
    pub ready: Option<Reply<Result<(), GameStartError>>>,
    /// The plug points of the input dispatch (session rules, client router, admin handler; 10 §4.6). The
    /// default installs the M1 stubs; the lobby and world WPs replace the parts they implement.
    pub handlers: Handlers,
    /// The loaded game data and the world catalog (15 §3.5); `WorldInit::default()` = no data, no world.
    pub world: WorldInit,
    /// The store behind `schedule.last_fired.<job>` (10 §5.5, 13 §15.2): the server installs one over `server_kv`
    /// so a calendar job that fired before a restart does not fire again, and a `RunOnceAtStart` job catches up
    /// the firing it missed while the server was down. `None` = in memory (tests; nothing survives a restart).
    pub wall_store: Option<Box<dyn crate::sched::wall::WallStore>>,
}

/// The loop's state (10 §4.1).
pub(crate) enum RunState {
    Running,
    ShuttingDown(crate::shutdown::Countdown),
    Flushing { ticket: Ticket, since_tick: u64 },
    Exiting(GameExit),
}

/// The game thread's state (Plan/10 §4.1). Constructed on the game thread (the Lua VM is `!Send`).
///
/// Subsystem fields that belong to later work packages (`worlds`, `services`, `sessions`, `tickets`) are
/// empty placeholders; their owners replace the types, the field names stay.
pub struct Game {
    // ---- time, randomness, configuration ----
    pub(crate) clock: TickClock,
    pub(crate) rng: RngSet,
    pub(crate) cfg: GameConfig,
    pub(crate) live: Arc<LiveConfig>,
    /// The `runtime_override` rows in force (canonical key to value).
    pub(crate) overrides: crate::overrides::RowsInForce,
    // ---- world and services ----
    pub(crate) data: Arc<GameData>,
    pub(crate) worlds: Worlds,
    pub(crate) spawn_stats: SpawnCacheStats,
    pub(crate) services: Services,
    pub(crate) lua: LuaHost,
    /// The time source of the Lua families, sampled once per tick (Plan/20 §3.6).
    pub(crate) lua_clock: std::rc::Rc<crate::event::GameLuaClock>,
    // ---- runtime plumbing ----
    pub(crate) sessions: SessionTable,
    pub(crate) outbox: Outbox,
    pub(crate) sched: Scheduler,
    pub(crate) persist: PersistHandle,
    pub(crate) tickets: TicketRegistry,
    pub(crate) audit: AuditLog,
    pub(crate) telemetry: Arc<Telemetry>,
    // ---- input plumbing (10 §4.1, §4.6) ----
    /// Runtime-synthesised inputs (slow-consumer `SessionLost`), drained first in phase 1.
    pub(crate) deferred: VecDeque<GameInput>,
    /// The plug points of the dispatch.
    pub(crate) handlers: Handlers,
    /// Leave requests queued through `GameCtx::request_leave`, executed after the requesting handler.
    pub(crate) leaves: Vec<(ObjId, LeaveTrigger)>,
    /// Dispatch counters.
    pub(crate) counters: DispatchStats,
    /// The last 900 tick durations behind the rolling p50/p99 of `/metrics` and `status`.
    pub(crate) tick_window: crate::stats_pub::TickWindow,
    /// The admin console's subscriptions (25 §3.15): observers only, not part of the simulation or of any digest.
    pub(crate) admin_views: crate::admin::views::AdminViews,
    // ---- loop state and channels ----
    next_tick: u64,
    inbox: flume::Receiver<GameInput>,
    db_replies: flume::Receiver<DbReply>,
    pub(crate) net_events: flume::Sender<NetEvent>,
    pub(crate) run_state: RunState,
    shutdown: Box<dyn ShutdownHandle>,
    wall: WallClockCell,
    driver: TickDriver,
    manual_pending: u32,
    manual_done: Option<Reply<()>>,
    hook_log: Option<Vec<(Tick, Hook)>>,
}

/// Starts the `flyff-game` thread (16 MiB stack: Lua and deep recursion headroom, Plan/10 §2.2).
pub fn spawn_game_thread(
    mut init: GameInit,
    inbox: flume::Receiver<GameInput>,
) -> std::io::Result<std::thread::JoinHandle<GameExit>> {
    std::thread::Builder::new()
        .name("flyff-game".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let ready = init.ready.take();
            match Game::new(init, inbox) {
                Ok(game) => {
                    if let Some(r) = ready {
                        let _ = r.send(Ok(()));
                    }
                    game.run()
                }
                Err(e) => {
                    tracing::error!(error = %e, "game thread could not start");
                    if let Some(r) = ready {
                        let _ = r.send(Err(e));
                    }
                    GameExit::StartFailed
                }
            }
        })
}

impl Game {
    /// Builds the game on the current thread (must be the game thread: the Lua VM is `!Send`).
    ///
    /// Resolves the server time zone, derives every random stream from `init.seed` and registers the job
    /// table of Plan/10 §5.3/§5.4 (`sched::registry`).
    pub fn new(
        mut init: GameInit,
        inbox: flume::Receiver<GameInput>,
    ) -> Result<Game, GameStartError> {
        let lua_init = std::mem::take(&mut init.lua);
        let zone =
            WallZone::resolve(&init.cfg.timezone).map_err(|e| GameStartError(e.to_string()))?;
        let wall = WallClockCell::new(init.wall_clock);
        let mut sched = match init.wall_store.take() {
            Some(store) => Scheduler::new(zone.clone(), store),
            None => Scheduler::with_zone(zone.clone()),
        };
        registry::register_all(&mut sched, &init.cfg);
        // 10 §8.6 S8: the `runtime_override` rows on top of the file values (ignored with
        // `fidelity.persist_gm_overrides = false`; bad rows are warned about, never deleted)
        let (live, overrides, override_warnings) = if init.cfg.rules.fidelity.persist_gm_overrides {
            crate::overrides::apply_rows(&init.cfg.rules, &init.startup.runtime_overrides)
        } else {
            (
                LiveConfig::from_rules(&init.cfg.rules),
                crate::overrides::RowsInForce::new(),
                Vec::new(),
            )
        };
        for w in &override_warnings {
            tracing::warn!("{w}");
        }
        let live = Arc::new(live);
        // 22 §3.1: the event-flag store from the initial LiveConfig, before the worlds spawn their NPCs (the filter
        // of 15 §13.2 reads flags 11, 15, 22, 23, 24, 25)
        let event_flags = crate::event::EventFlags::init(&live);
        let outbox = Outbox::new(u64::from(init.cfg.flush_every_ticks));
        let data: Arc<GameData> = init.world.data.unwrap_or_default();
        // the Lua families (Plan/20 §3.2, 22 §4.1) are built here, on the game thread, from the live scripts of the
        // game data: a broken script is a diagnostic and a `Faulted` family, never a failed start
        let lua_clock = crate::event::GameLuaClock::new(zone.clone());
        lua_clock.sample(wall.now_ms(), GameMs::ZERO);
        let (lua, lua_diags) = LuaHost::boot(lua_init, &data, lua_clock.clone());
        for d in &lua_diags {
            tracing::error!("{d}");
        }
        let world_ids: Vec<flyff_common::WorldId> = init
            .world
            .catalog
            .as_ref()
            .map(|c| c.world_ids().collect())
            .unwrap_or_default();
        let mut rng = RngSet::new(init.seed, world_ids);
        // 15 §3.5: the worlds with their `.dyo` NPCs and ctrls, the ticket layers, the revival index; the NPC
        // filters read doc 22's event-flag store
        let mut worlds = match &init.world.catalog {
            Some(c) => Worlds::start(c, &data, &init.world.ticket_layers, &event_flags, &mut rng),
            None => Worlds::default(),
        };
        worlds.seasons = init.world.seasons;
        // 15 §15.1: the respawn lines of every world folder (the RANK_GUARD rule reads the startup `events.pk`)
        worlds.init_respawn(
            &data,
            &crate::world::RespawnConfig::from_rules(
                &init.cfg.rules,
                live.event_state(flyff_common::config::EventId::Pk) != 0,
            ),
            rng.seed(),
        );
        worlds
            .npc_checker
            .set_range(init.cfg.rules.gameplay.npc_menu_range);
        // 17 §10: bosses and super monsters (and `DPSProp.txt`) get a damage meter when they spawn
        worlds.set_respawn_hooks(Box::new(crate::combat::dps::DpsMarker::new(
            &data,
            init.cfg.rules.features.dps_meter,
        )));
        // the movement pipeline's tables and switches (18 §3.1): `Handlers.movement` and the rules config
        worlds.movement =
            crate::movement::MovementState::new(&init.handlers.movement, &init.cfg, &data);
        // ---- bosses (doc 18 §10.4, task boss-ai-core): the rigs the ClockWorks hits read
        worlds
            .movement
            .set_boss_poses(init.handlers.boss_poses.clone());
        // 15 §14.7: the `FLYK` models the ctrl colliders of every layer pose their ctrls with (the colliders
        // themselves register at the `_add` of each ctrl)
        if let Some(models) = &init.world.ctrl_models {
            worlds.set_ctrl_models(Arc::clone(models));
        }
        let mut services = Services::new(&init.startup, init.serial_blocks.clone());
        services.event.flags = event_flags;
        // 19 §11.1: a system mail written before the first tick still gets the wall clock of the start
        services.econ.mail.set_clock(wall.now_ms().to_unix_time());
        services.saves = crate::persist::SaveScheduler::new(init.cfg.save.clone());
        services.chat = crate::chat::ingress::ChatService::new(init.chat_data);
        // 16 §18: the rule tables and switches of the stat functions (handlers, timers)
        services.stats = crate::stats::StatsService::new(Arc::new(crate::stats::StatsCore::new(
            Arc::clone(&data),
            &init.cfg.rules,
        )));
        // 19 §23: the environment of the item functions shares the stats environment
        services.items = crate::item::ItemService::new(Arc::new(crate::item::ItemCore::new(
            Arc::clone(&data),
            services.stats.handle(),
            &init.cfg.rules,
        )));
        // 23 §5.1: the command texts of the `IK2_GMTEXT` items, validated against the whitelist (the init report)
        crate::gm::item_text::install(&mut services.gm, &data);
        // 19 §20.4 (WP-ECON-09): the delivery of the pending grants (poll P12 and the enter-world pass) and the
        // Helena switch of PL-149 (`guild.create_requires_quest`)
        services.grants.deliver = Some(crate::econ::grants::deliver);
        services.npc.guild_create_requires_quest = init.cfg.guild_create_requires_quest;
        // ---- quests (doc 20 §9, task quest-core): the real predicates replace the M4 `NoQuestData` (also on empty
        // data, where they behave the same), and the guild-quest regions of `propGuildQuest.inc` reach Blinkpool
        services.quest = crate::quest::QuestService::new(
            Arc::clone(&data),
            init.cfg.rules.fidelity.quest_condition_fixes,
        );
        services.npc.predicates = services.quest.predicates();
        services.skill.guild_quest_regions = services.quest.guild_quest_regions();
        tracing::info!(
            quests = data.quests.quests.len(),
            guild_quests = data.quests.guild_quests.len(),
            party_quests = data.quests.party_quests.len(),
            "quest tables"
        );
        // ---- quest effects (doc 20 §9.9, task quest-effects): the quest item drops of Plan/19 §6.2 step 3
        services.drops.quest =
            Box::new(crate::quest::drops::QuestDrops::new(services.quest.rules()));
        // 25 §1.8: `/ban` and `/setauth` tell the admin console, which ends the staff sessions of the account
        services.gm.account_events = init.handlers.account_events.clone();
        // 23 §4.6 (WP-GM-25): `/loadscript` and `/reload config` ask the server through this channel
        services.gm.server_requests = init.handlers.server_requests.clone();
        // 20 §8.5: the `NpcBuff` handler checks VR-012 and hands the request to the buff rules of doc 17 (17 §7.11); the
        // hook of `NpcService` defaults to "log and do nothing" and nobody installed the rules (found by S-BUFF-01, tests-m4a)
        services.npc.on_npc_buff = crate::buff::npc::on_npc_buff;
        // 22 §4.1 step 2: the first Event.lua pass before tick 0, so the rates hold from the start
        services
            .event
            .lua_events
            .set_board(init.handlers.event_desc.clone());
        let started = services.event.lua_events.start(&lua, &data, wall.now_ms());
        // 22 §4.10 (task event-lua-extras): the spawn regions of Madrigal and the spawn events running at the start
        crate::event::spawns::boot(&mut services.event, &lua, &data, &worlds, &started);
        // 22 §2.5 rule 2: the typed configs of the other event families and the schedule of the four wall jobs
        services.event.read_configs(&lua, &data, &mut sched);
        // ---- event monsters (WP-EVENT-09): the static EventMonster.lua table and the Rangda controllers (the first 1 s
        // run spawns them; memory only)
        services.event.monsters = crate::event::monsters::EventMonsterTable::from_data(&data);
        services.event.rangda = crate::event::rangda::RangdaService::new(&data.rangda);
        // ---- instances (doc 22 §8, task instances) ----
        // the dungeon worlds of `PartyDungeon.lua`, the other end of the world hooks' notes, the GM `/teleport` rule
        // and the drive-out of an invalidated dungeon layer (no batch at boot: the state is memory only, KQ-21)
        services.event.instances.configure(&data);
        services
            .event
            .instances
            .set_notes(init.handlers.event_world.clone());
        services.gm.instances = Box::new(crate::event::InstanceWorldSet::from_data(&data));
        worlds.set_drive_out(Box::new(crate::event::hooks::EventDriveOut));
        // 22 §9 (task lord): the election, the lord events and the cooldowns from the start-up state; the election row of
        // a freshly prepared election rides in the first lord write (no batch at boot), and the factors of the loaded
        // lord events reach the event modifiers once
        services.event.lord = crate::event::lord::LordService::load(
            &init.startup,
            &data.lord,
            wall.now_ms(),
            sched.zone(),
        );
        let (lord_exp, lord_item) = services.event.lord.factors();
        services
            .event
            .lua_events
            .set_lord_factors(lord_exp, lord_item);
        // 22 §10 (task tax-tower): the owners, rates and counters of the round in force from the start-up state, with the
        // continents the shops stand on and the numbers of `Tax.lua`; a fresh database writes its rows with the first P5
        // run (no batch at boot)
        services.event.tax = crate::event::tax::TaxService::load(
            &init.startup.tax,
            &init.startup.server_kv,
            worlds.continents.clone(),
            wall.now_ms(),
        );
        services.event.tax.apply_config(&services.event.configs.tax);
        // 22 §11 (task secret-room): the `T` tenders of the round with their line-ups from the start-up state (every side
        // and the manager closed; a war never survives a restart; no batch at boot), and the invite lock of guild-core
        services.event.secret_room =
            crate::event::secret_room::SecretRoomService::load(&init.startup.secret_room);
        services.social.guild.seams.secret_room_invite_locked =
            crate::event::secret_room::rules::invite_locked;
        // 22 §15 (task quiz): the quiz event ids continue at `meta.next_quiz_event_id` (memory only, no batch at boot)
        services.event.quiz =
            crate::event::quiz::QuizService::new(init.startup.counters.next_quiz_event_id);
        // ---- rainbow race (doc 22 §14, task rainbow-race) ----
        // the round, the applicants (`N` and `R` rows) and the previous ranking; the race itself is memory only (E9)
        crate::event::rainbow::boot(
            &mut services.event.rainbow,
            &init.startup.rainbow_race,
            &init.persist,
        );
        // 22 §12 (task guild-house): the houses and their furniture from the start-up state (memory only, no batch at
        // boot; the rooms are made at the first entry)
        services.event.guild_houses =
            crate::event::guild_house::GuildHouseService::load(&init.startup);
        // 21 §7 (WP-SOCIAL-16): the siege's keys and script, the wall job `siege_open` (one spec per `AUTO_OPEN_IDC`
        // line) and the guild-war predicate of the death pipeline; the row of the applying siege is created by the
        // first batch that needs it (`SiegeService::row_ops`)
        services.social.siege.configure(&data.guild.combat);
        crate::social::siege::schedule::register(&mut sched, &data.guild.combat);
        services.combat.death.gw_open = crate::social::siege::gw_open;
        // 21 §7.5 (WP-SOCIAL-17): the combat seams of the siege (`IsGuildCombatTarget`, the Holy Cross point); the first
        // real `CombatHooks` of the game: a later owner of this field wraps, it does not replace
        // 22 §4.10 (KQ-32, task event-lua-extras): the one event wrapper around it answers `is_event_spawn_monster`
        // for the props of the running Event.lua spawn events and forwards every other seam
        worlds.movement.combat.hooks = Box::new(crate::event::combat::EventCombatHooks::new(
            Box::new(
                crate::social::siege::fight::SiegeCombatHooks::new(
                    services.social.siege.facts(),
                    services.social.siege.fight.holycross_queue(),
                )
                .with_gc1to1(services.social.gc1to1.facts()),
            ),
            services.event.spawns.facts(),
        ));
        // 21 §8 (task gc11): the globals of GuildCombat1TO1.lua and the wall job `gc1to1_open` (one spec per OPENTIME
        // entry); the flag only gates the slot and the requests, so the job is always registered
        services.social.gc1to1.configure(&lua, &init.cfg);
        crate::social::gc1to1::register(&mut sched, &services.social.gc1to1.config);
        Ok(Game {
            clock: TickClock::new(Tick(0), wall.now_ms(), &zone),
            rng,
            cfg: init.cfg,
            live,
            overrides,
            data,
            worlds,
            spawn_stats: SpawnCacheStats::default(),
            services,
            lua,
            lua_clock,
            sessions: SessionTable::default(),
            outbox,
            sched,
            persist: init.persist,
            tickets: TicketRegistry::default(),
            audit: init.audit,
            telemetry: init.telemetry,
            deferred: VecDeque::new(),
            handlers: init.handlers,
            leaves: Vec::new(),
            counters: DispatchStats::default(),
            tick_window: crate::stats_pub::TickWindow::default(),
            admin_views: crate::admin::views::AdminViews::default(),
            next_tick: 0,
            inbox,
            db_replies: init.db_replies,
            net_events: init.net_events,
            run_state: RunState::Running,
            shutdown: init.shutdown,
            wall,
            driver: init.driver,
            manual_pending: 0,
            manual_done: None,
            hook_log: None,
        })
    }

    /// The shared wall-clock cell (`ServerHandle::set_wall_ms`, the gateway's `ServerTime`).
    pub fn wall_clock(&self) -> WallClockCell {
        self.wall.clone()
    }

    /// The loaded game data (empty unless `GameInit.world.data` was set).
    pub fn data(&self) -> &Arc<GameData> {
        &self.data
    }

    /// The worlds (read-only).
    pub fn worlds(&self) -> &Worlds {
        &self.worlds
    }

    /// The counters of the encode-once spawn cache (Plan/15 §7.3).
    pub fn spawn_stats(&self) -> SpawnCacheStats {
        self.spawn_stats
    }

    /// The scheduler (inspection, registration before the first tick).
    pub fn scheduler(&self) -> &Scheduler {
        &self.sched
    }

    /// The scheduler, mutably (registration of wall jobs before the first tick; tests).
    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.sched
    }

    /// The random streams.
    pub fn rng(&self) -> &RngSet {
        &self.rng
    }

    /// The startup configuration.
    pub fn config(&self) -> &GameConfig {
        &self.cfg
    }

    /// The live configuration generation in force.
    pub fn live(&self) -> &Arc<LiveConfig> {
        &self.live
    }

    /// Starts recording the phase hooks of every tick (`TestWorld`, the manual driver tests).
    pub fn record_hooks(&mut self) {
        self.hook_log.get_or_insert_with(Vec::new);
    }

    /// The recorded `(tick, hook)` calls in call order (empty unless [`Game::record_hooks`] was called).
    pub fn hook_log(&self) -> &[(Tick, Hook)] {
        self.hook_log.as_deref().unwrap_or(&[])
    }

    fn hook(&mut self, hook: Hook) {
        let tick = self.clock.tick;
        if let Some(log) = self.hook_log.as_mut() {
            log.push((tick, hook));
        }
    }

    /// The session table.
    pub fn sessions(&self) -> &SessionTable {
        &self.sessions
    }

    /// The outbox (slots, counters).
    pub fn outbox(&self) -> &Outbox {
        &self.outbox
    }

    /// The dispatch counters.
    pub fn dispatch_stats(&self) -> &DispatchStats {
        &self.counters
    }

    /// The ticket registry (tests register continuations; feature code goes through `GameCtx::tickets`).
    pub fn tickets_mut(&mut self) -> &mut TicketRegistry {
        &mut self.tickets
    }

    /// The Lua host (built on this thread).
    pub fn lua(&self) -> &LuaHost {
        &self.lua
    }

    /// The in-game `/shutdown` entry point.
    pub fn shutdown_handle(&self) -> &dyn ShutdownHandle {
        self.shutdown.as_ref()
    }

    /// Clock of the current (or last) tick.
    pub fn clock(&self) -> TickClock {
        self.clock
    }

    /// Runs the loop until the game exits (Plan/10 §4.3).
    pub fn run(mut self) -> GameExit {
        let epoch = Instant::now(); // det-ok: pacing driver (Plan/10 §4.3)
        let elapsed_ns =
            |epoch: Instant| u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let mut grid = DeadlineGrid::new(elapsed_ns(epoch), self.next_tick);
        loop {
            let n = self.next_tick;
            let lag_ns = match &self.driver {
                TickDriver::RealTime => match grid.wait(n, elapsed_ns(epoch)) {
                    Wait::Sleep(ns) => {
                        std::thread::sleep(Duration::from_nanos(ns)); // det-ok: the only sleep of the game thread (tick wait / test hook)
                        0
                    }
                    Wait::Run { lag_ns, resync } => {
                        if resync {
                            // More than one period late: run once, re-anchored, no catch-up burst.
                            self.telemetry.resyncs.fetch_add(1, Ordering::Relaxed);
                        }
                        lag_ns
                    }
                },
                TickDriver::Manual(_) => {
                    if !self.wait_for_step() {
                        // The step channel closed: the owner is gone.
                        return GameExit::Shutdown;
                    }
                    0
                }
            };
            self.next_tick = n + 1;
            if let Some(exit) = self.run_one(n, lag_ns) {
                return exit;
            }
            self.manual_tick_done();
        }
    }

    /// Runs exactly one tick without pacing (used by `TestWorld` and the manual driver).
    /// Returns `Some(exit)` once the game has ended.
    pub fn step(&mut self) -> Option<GameExit> {
        let n = self.next_tick;
        self.next_tick = n + 1;
        self.run_one(n, 0)
    }

    fn wait_for_step(&mut self) -> bool {
        while self.manual_pending == 0 {
            let TickDriver::Manual(rx) = &self.driver else {
                return true;
            };
            match rx.recv() {
                Ok(req) => {
                    if req.ticks == 0 {
                        let _ = req.done.send(());
                    } else {
                        self.manual_pending = req.ticks;
                        self.manual_done = Some(req.done);
                    }
                }
                Err(_) => return false,
            }
        }
        true
    }

    fn manual_tick_done(&mut self) {
        if self.manual_pending > 0 {
            self.manual_pending -= 1;
            if self.manual_pending == 0
                && let Some(done) = self.manual_done.take()
            {
                let _ = done.send(());
            }
        }
    }

    fn run_one(&mut self, n: u64, lag_ns: u64) -> Option<GameExit> {
        // Phase 0 tail: the ONE wall-clock sample of this tick (recorded for replay).
        self.clock = TickClock::new(Tick(n), self.wall.sample(), self.sched.zone());
        self.lua_clock
            .sample(self.clock.unix_ms, self.clock.game_ms);
        // 19 §11.1: `MailService::send_system` has no `GameCtx`, it stamps new mails with this sample
        self.services
            .econ
            .mail
            .set_clock(self.clock.unix_ms.to_unix_time());
        self.telemetry.begin_tick(n, lag_ns);

        let result = catch(|| self.run_tick());
        if let Err(info) = result {
            return Some(self.unscoped_panic_exit(&info));
        }
        let dur = self.telemetry.end_tick();
        if let Some((p50, p99)) = self.tick_window.push(dur) {
            self.telemetry.tick_p50_ns.store(p50, Ordering::Relaxed);
            self.telemetry.tick_p99_ns.store(p99, Ordering::Relaxed);
        }
        self.log_overrun(n, dur);
        match self.run_state {
            RunState::Exiting(exit) => {
                self.finish();
                Some(exit)
            }
            _ => None,
        }
    }

    /// Phase 5.1 (Plan/10 §4.4): a tick over 66.7 ms logs `warn!`, over 100 ms `error!`, with the slowest tick phase,
    /// world phase, job and input of that tick; the limit is one warn line per 75 ticks and one error line per 15 (the
    /// next line reports the suppressed count). The counters of `/metrics` are `Telemetry::end_tick`'s.
    fn log_overrun(&mut self, tick: u64, dur_ns: u64) {
        let Some((level, suppressed)) = self.tick_window.overrun.check(tick, dur_ns) else {
            return;
        };
        let t = &self.telemetry;
        let (phase, phase_ns) = crate::stats_pub::slowest_tick_phase(t);
        let (world_phase, world_ns) = crate::stats_pub::slowest_world_phase(t);
        let Slowest { job, input } = self.tick_window.slowest;
        let ms = |ns: u64| (ns as f64 / 100_000.0).round() / 10.0;
        let (dur_ms, lag_ms) = (ms(dur_ns), ms(t.lag_ns.load(Ordering::Relaxed)));
        let players = self.sessions.len();
        macro_rules! overrun_line {
            ($log:ident) => {
                tracing::$log!(
                    tick,
                    dur_ms,
                    lag_ms,
                    players,
                    slowest_phase = phase,
                    slowest_phase_ms = ms(phase_ns),
                    slowest_world_phase = world_phase,
                    slowest_world_phase_ms = ms(world_ns),
                    slowest_job = job.name,
                    slowest_job_ms = ms(job.ns),
                    slowest_input = input.name,
                    slowest_input_ms = ms(input.ns),
                    suppressed,
                    "tick overrun"
                )
            };
        }
        match level {
            OverrunLevel::Warn => overrun_line!(warn),
            OverrunLevel::Error => overrun_line!(error),
        }
    }

    // ---- phases (Plan/10 §4.4) ----

    fn run_tick(&mut self) {
        let tick = self.clock.tick.0;
        self.tick_window.slowest = Slowest::default();
        self.worlds.phase_times.arm(&self.telemetry);
        let mut lap = self.telemetry.now_ns();
        crate::panic::set_phase(tick, "input");
        self.hook(Hook::Input);
        self.phase_input();
        lap = self.lap(TickPhase::Input, lap);
        crate::panic::set_phase(tick, "worlds");
        self.hook(Hook::Worlds);
        self.phase_worlds();
        lap = self.lap(TickPhase::Worlds, lap);
        crate::panic::set_phase(tick, "timers");
        self.phase_timers();
        lap = self.lap(TickPhase::Timers, lap);
        crate::panic::set_phase(tick, "flush");
        self.hook(Hook::Flush);
        self.phase_flush();
        lap = self.lap(TickPhase::Flush, lap);
        crate::panic::set_phase(tick, "post");
        self.hook(Hook::Post);
        self.phase_post();
        crate::stats_pub::publish_game(
            &self.telemetry,
            tick,
            &self.worlds,
            self.outbox.stats.pushes,
            &self.services.saves.taken,
        );
        self.lap(TickPhase::Post, lap);
        crate::panic::set_phase(tick, "");
    }

    /// Ends the lap of `phase` that began at `start` (ns on the telemetry clock) and returns the start of the next.
    fn lap(&self, phase: TickPhase, start: u64) -> u64 {
        let now = self.telemetry.now_ns();
        self.telemetry
            .record_tick_phase(phase, now.saturating_sub(start));
        now
    }

    /// Phase 1 (10 §4.4): liveness check of the persistence writer, then 1.1 the deferred inputs of the previous
    /// flush, 1.2 C1 (the count present at drain start; the urgent sessions are flushed after each input) and
    /// 1.3 C6 (`DbReply`, urgent flush after each).
    fn phase_input(&mut self) {
        if self.persist.is_closed()
            && !matches!(
                self.run_state,
                RunState::Flushing { .. } | RunState::Exiting(_)
            )
        {
            let corrupt = self.persist.fatal().is_some();
            tracing::error!(corrupt, "persistence writer is gone: ending the game loop");
            self.run_state = RunState::Exiting(GameExit::PersistenceFailed { corrupt });
            return;
        }
        // 1.1 runtime-synthesised inputs (SessionLost{SlowConsumer} of the previous flush).
        while let Some(input) = self.deferred.pop_front() {
            self.dispatch(input);
            self.flush_urgent_after_handler();
        }
        // 1.2 C1.
        let count = self.inbox.len();
        self.telemetry
            .inbox_depth
            .store(count as u64, Ordering::Relaxed);
        for _ in 0..count {
            match self.inbox.try_recv() {
                Ok(input) => {
                    self.dispatch(input);
                    self.flush_urgent_after_handler();
                }
                Err(_) => break,
            }
        }
        // 1.3 C6.
        while let Ok(reply) = self.db_replies.try_recv() {
            self.on_db_reply(reply);
            self.flush_urgent_after_handler();
        }
    }

    /// `flush_urgent(SimFrame(4n))` after one handler (10 §4.4 1.2): the sessions marked urgent leave in the same
    /// tick, before the next input is processed.
    fn flush_urgent_after_handler(&mut self) {
        let report = self
            .outbox
            .flush_urgent(self.clock.frame0, self.cfg.max_batch_bytes);
        if !report.is_empty() {
            self.on_flush_report(report);
        }
    }

    /// Phase 2: per world ascending, per layer ascending `_add/_process/_modifylink/_OnDie/_replace/_delete`,
    /// then the layer GC (Plan/15 §5.3). The spawn and `DelObj` messages of the visibility sets go through the
    /// `Outbox`; the urgent flush after each layer's `_replace` is reported like the periodic one.
    fn phase_worlds(&mut self) {
        if self.worlds.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let now = self.clock.game_ms;
        // doc 17: the services' state the hit frames of the phase read (they have no services themselves)
        {
            let (services, mut ctx) = self.split();
            crate::combat::tick::prepare(services, &mut ctx);
        }
        // the tail of every mover reads the wall clock (absolute SM timers, 16 §14.2)
        self.worlds.movement.unix_now = self.clock.unix_ms.to_unix_time().0;
        let report = {
            let mut writer = SpawnWriter::new(
                &mut self.outbox,
                &self.sessions,
                &mut self.spawn_stats,
                now,
                tick.frames_completed_after(),
                self.cfg.max_batch_bytes,
            );
            let mut env = PhaseEnv {
                tick,
                now,
                sink: &mut writer,
                hooks: &mut *self.handlers.world,
                rng: &mut self.rng,
            };
            self.worlds.run_tick(&mut env);
            writer.take_report()
        };
        self.on_flush_report(report);
        // doc 17: the deaths the hit frames caused (kill EXP, loot, quests ... need the services)
        let (services, mut ctx) = self.split();
        crate::combat::tick::finish(services, &mut ctx);
        // doc 21 §7.5 (WP-SOCIAL-17): the deaths of the siege fighters, their points and the server's own revival
        crate::social::siege::fight::drain_deaths(services, &mut ctx);
        // doc 22 §8 (task instances): the portal triggers and the layer changes the world phase noted (the entry
        // teleport and the push-back are requested here and run at the next tick's `_replace`)
        crate::event::hooks::drain_world_notes(services, &mut ctx);
        // doc 18 §6: the riders that flew into a no-fly cell lose their ride
        crate::movement::flying::finish(services, &mut ctx);
    }

    /// Phase 3: timers (Plan/10 §4.4 steps 3.1–3.8): one-shots, then the periodic table in phase order
    /// (P1 respawn, P2 the 1 s slot with the wall evaluation as item 17, P3 roll call, P4–P14, the per-tick
    /// jobs), the shutdown step and the SaveScheduler pass (3.8, `persist::run_pass`).
    fn phase_timers(&mut self) {
        let tick = self.clock.tick;
        self.hook(Hook::OneShots);
        for due in self.sched.take_due_timers(tick) {
            self.run_timer(due);
        }
        let jobs = self.sched.due_jobs(tick, &self.cfg, &self.live);
        for (group, hook) in [
            (JobGroup::Respawn, Hook::Respawn),
            (JobGroup::Slot, Hook::Slot),
            (JobGroup::RollCall, Hook::RollCall),
            (JobGroup::Periodic, Hook::Periodic),
            (JobGroup::PerTick, Hook::PerTick),
        ] {
            self.hook(hook);
            for job in jobs.iter().filter(|j| j.group() == group) {
                self.run_job(job);
            }
        }
        self.hook(Hook::ShutdownStep);
        self.shutdown_step();
        self.hook(Hook::SaveScheduler);
        let (services, mut ctx) = self.split();
        // 19 §5.2: the item system's per-tick work (due casts, serials of items created in the world phase)
        // runs right before the snapshot pass, so no snapshot sees an item without a serial
        crate::item::tick::process(services, &mut ctx);
        // 21 §2.2: the per-player cadence of the couple, campus and honor systems (every tick; a step every 16th)
        crate::social::relation::process(services, &mut ctx);
        // 19 §8.4, §9.2: trades and stalls end when an avatar died, teleported, left or lost items (same tick)
        crate::econ::lifecycle::process(services, &mut ctx);
        // 19 §11.2: the envelope (MODE_MAILBOX) of the online receivers of this tick's mails
        crate::econ::mail::tick(services, &mut ctx);
        // 17 §7.3: the buffs of the awake movers, one pass per 16 ticks each
        crate::buff::tick::process_all(services, &mut ctx);
        // 17 §7.7: the stun / poison / bleeding / darkness counters and the heal over time, every tick
        crate::buff::tick::process_abnormal_all(services, &mut ctx);
        // 21 §5.11: the guild buffs of the online members, every tick (a member without a guild buff gets its rows)
        crate::social::guild::buffs::process_all(services, &mut ctx);
        crate::persist::run_pass(services, &mut ctx);
    }

    /// Runs one due one-shot under its scope (a player scope for a session timer).
    fn run_timer(&mut self, due: DueTimer) {
        self.sched.note_run(self.clock.tick, "one_shot");
        let started = self.telemetry.now_ns();
        let session = due.scope.0;
        crate::panic::set_unit(
            session,
            session.and_then(|s| self.character_of(s)),
            "one_shot",
        );
        match session {
            Some(session) => {
                self.scoped(HandlerScope::Player(session), move |g| {
                    g.dispatch_timer(due.event)
                });
            }
            None => self.dispatch_timer(due.event),
        }
        self.drain_leaves();
        self.flush_urgent_after_handler();
        crate::panic::clear_unit();
        self.note_slow_job("one_shot", started);
    }

    /// The character behind a session (the panic context and the `server.panic` actor).
    pub(crate) fn character_of(&self, session: SessionId) -> Option<CharacterId> {
        self.sessions
            .player_of(session)
            .and_then(|p| self.sessions.get(p))
            .map(|e| e.character)
    }

    /// Remembers `name` as the slowest job of this tick when it ran longer than the previous ones (overrun log).
    fn note_slow_job(&mut self, name: &'static str, started_ns: u64) {
        let ns = self.telemetry.now_ns().saturating_sub(started_ns);
        self.tick_window.slowest.job.note(name, ns);
    }

    /// One-shot dispatch: one arm per module `Timer` enum, added by the owning WP.
    fn dispatch_timer(&mut self, event: TimerEvent) {
        match event {
            TimerEvent::Session(t) => match t {},
            TimerEvent::World(t) => match t {},
            TimerEvent::Combat(t) => match t {},
            TimerEvent::Skill(t) => match t {},
            TimerEvent::Buff(t) => match t {},
            TimerEvent::Item(t) => match t {},
            TimerEvent::Econ(t) => match t {},
            TimerEvent::Npc(t) => match t {},
            TimerEvent::Quest(t) => match t {},
            TimerEvent::Social(t) => match t {},
            TimerEvent::Chat(t) => match t {},
            TimerEvent::Event(t) => match t {},
            TimerEvent::Gm(t) => {
                let (svc, mut ctx) = self.split();
                crate::admin::actions::world::on_timer(svc, &mut ctx, t);
            }
            TimerEvent::Persist(t) => match t {},
        }
    }

    /// Calls one periodic or slot job (or the wall evaluation) with the split borrow.
    fn run_job(&mut self, job: &DueJob) {
        self.run_job_inner(job);
        // P18 and the services request leaves; they run right after the job (12 §12.5).
        self.drain_leaves();
    }

    fn run_job_inner(&mut self, job: &DueJob) {
        let tick = self.clock.tick;
        match job {
            DueJob::Run { name, run, .. } => {
                self.sched.note_run(tick, name);
                let started = self.telemetry.now_ns();
                crate::panic::set_unit(None, None, name);
                let (services, mut ctx) = self.split();
                run(services, &mut ctx);
                crate::panic::clear_unit();
                self.note_slow_job(name, started);
                self.sched.record_run(tick, self.clock.unix_ms, name, false);
            }
            DueJob::Wall => {
                self.sched.note_run(tick, "wall_evaluate");
                let started = self.telemetry.now_ns();
                crate::panic::set_unit(None, None, "wall_evaluate");
                let fires = self.sched.evaluate_wall(&self.clock);
                self.sched
                    .record_run(tick, self.clock.unix_ms, crate::sched::WALL_EVALUATE, false);
                self.note_slow_job("wall_evaluate", started);
                for fire in fires {
                    self.sched.note_run(tick, fire.name);
                    let started = self.telemetry.now_ns();
                    crate::panic::set_unit(None, None, fire.name);
                    let (services, mut ctx) = self.split();
                    (fire.run)(services, &mut ctx, &fire.stamp);
                    crate::panic::clear_unit();
                    self.note_slow_job(fire.name, started);
                    self.sched
                        .record_run(tick, self.clock.unix_ms, fire.name, false);
                }
                crate::panic::clear_unit();
            }
        }
    }

    /// 3.7: the shutdown countdown (notices at the marks, then everyone leaves at the deadline) and the flush
    /// timeout.
    fn shutdown_step(&mut self) {
        let now = self.clock.tick.0;
        match &self.run_state {
            RunState::ShuttingDown(c) if now >= c.deadline_tick => self.begin_final_shutdown(),
            RunState::ShuttingDown(_) => self.countdown_notices(),
            RunState::Flushing { since_tick, .. }
                if now.saturating_sub(*since_tick)
                    > u64::from(self.cfg.shutdown_flush_timeout_s) * Tick::PER_SECOND =>
            {
                tracing::error!("shutdown flush did not complete in time: exiting anyway");
                self.run_state = RunState::Exiting(GameExit::Shutdown);
            }
            _ => {}
        }
    }

    /// Phase 4: `flush_tick(Tick(n), SimFrame(4(n+1)), net.max_batch_bytes)` (10 §4.4, §6.4): every
    /// `flush_every_ticks` all slots, in between only the urgent ones.
    fn phase_flush(&mut self) {
        let report = self.outbox.flush_tick(
            self.clock.tick,
            self.clock.tick.frames_completed_after(),
            self.cfg.max_batch_bytes,
        );
        self.on_flush_report(report);
    }

    /// Phase 5: post. Gauges that the tick itself owns; the heartbeat (`last_tick_end_ns`) is published
    /// by `Telemetry::end_tick` right after this phase.
    fn phase_post(&mut self) {
        self.telemetry
            .sched_timers
            .store(self.sched.len() as u64, Ordering::Relaxed);
        self.publish_session_gauges();
        crate::stats_pub::publish_movement(&self.telemetry, &self.worlds.movement.stats);
        // 25 §3.15: the admin views collect after the flush (read-only; pushes into the sinks of the connections)
        self.run_admin_views();
    }

    // ---- panic scopes (10 §14) ----

    /// Runs `f` under a handler scope. A `Player` scope stages the outbox (committed on success, dropped on a
    /// panic) and poisons the session on a panic. Returns `false` when `f` panicked.
    pub(crate) fn scoped(&mut self, scope: HandlerScope, f: impl FnOnce(&mut Game)) -> bool {
        let staged = matches!(scope, HandlerScope::Player(_));
        if staged {
            self.outbox.stage_begin();
        }
        match catch(|| f(self)) {
            Ok(()) => {
                if staged {
                    self.outbox.stage_commit();
                }
                true
            }
            Err(info) => {
                if staged {
                    self.outbox.stage_abort();
                }
                self.on_scoped_panic(scope, &info);
                false
            }
        }
    }

    /// Player-scoped panic: log, poison the session (one `Kick{SERVER_ERROR}` frame, close with
    /// `InternalError`), remove the avatar and persist nothing (10 §14.3).
    fn on_scoped_panic(&mut self, scope: HandlerScope, info: &PanicInfo) {
        // the context first: poisoning the session runs further units that overwrite it
        let ctx = crate::panic::context();
        let (counter, label) = match scope {
            HandlerScope::Player(_) => (0, "player"),
            HandlerScope::MultiParty => (1, "multiparty"),
            HandlerScope::Unscoped => (2, "unscoped"),
        };
        if let Some(c) = self.telemetry.panics.get(counter) {
            c.fetch_add(1, Ordering::Relaxed);
        }
        let session = match scope {
            HandlerScope::Player(session) => Some(session),
            HandlerScope::MultiParty | HandlerScope::Unscoped => ctx.session,
        };
        let account = session
            .and_then(|s| self.sessions.player_of(s))
            .and_then(|p| self.sessions.get(p))
            .map(|e| e.account);
        let actor = (account.is_some() || ctx.character.is_some()).then(|| flyff_db::AuditActor {
            account_id: account,
            character_id: ctx.character,
            ..flyff_db::AuditActor::default()
        });
        let location = info.location.as_deref().unwrap_or("");
        tracing::error!(
            session_id = session.map_or(0, |s| s.0),
            character = ctx.character.map_or(0, |c| c.0),
            tick = ctx.tick,
            phase = ctx.phase,
            what = ctx.what,
            location,
            panic = %info.message,
            "{label}-scoped panic: session poisoned"
        );
        crate::audit::server_panic(&self.audit, actor, label, &ctx, info);
        if let HandlerScope::Player(session) = scope {
            self.poison_session(session);
        }
    }

    pub(crate) fn status_snapshot(&self) -> StatusSnapshot {
        let stats = self.persist.stats();
        let shutdown_in_s = match &self.run_state {
            RunState::ShuttingDown(c) => Some(
                u32::try_from(c.deadline_tick.saturating_sub(self.clock.tick.0) / Tick::PER_SECOND)
                    .unwrap_or(u32::MAX),
            ),
            _ => None,
        };
        let in_world = u32::try_from(self.sessions.len()).unwrap_or(u32::MAX);
        StatusSnapshot {
            tick: self.clock.tick.0,
            uptime_s: self.telemetry.now_ns() / 1_000_000_000,
            in_world,
            lobby: u32::try_from(self.telemetry.sessions.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX)
                .saturating_sub(in_world),
            tick_p50_us: u32::try_from(self.telemetry.tick_p50_ns.load(Ordering::Relaxed) / 1_000)
                .unwrap_or(u32::MAX),
            tick_p99_us: u32::try_from(self.telemetry.tick_p99_ns.load(Ordering::Relaxed) / 1_000)
                .unwrap_or(u32::MAX),
            db_degraded: stats.degraded,
            db_queue: u32::try_from(stats.queue_depth).unwrap_or(u32::MAX),
            shutdown_in_s,
        }
    }

    // ---- shutdown (Plan/10 §15.2) ----

    /// Logs everyone out immediately (`Kick{SHUTDOWN}` + close 4009, then every avatar leaves with the
    /// `Shutdown` trigger) and asks persistence for a flush ticket.
    pub(crate) fn begin_final_shutdown(&mut self) {
        self.outbox.close_all(
            CloseReason::ServerShutdown,
            self.clock.tick.frames_completed_after(),
            self.cfg.max_batch_bytes,
        );
        let avatars: Vec<ObjId> = self.sessions.iter().map(|e| e.player).collect();
        for p in avatars {
            self.leaves.push((p, LeaveTrigger::Shutdown));
        }
        self.drain_leaves();
        if self.persist.is_detached() {
            self.run_state = RunState::Exiting(GameExit::Shutdown);
            return;
        }
        // 22 §10.4 (task tax-tower): the counters that are still dirty reach the database before the flush
        {
            let (svc, mut ctx) = self.split();
            crate::event::tax::flush_all(&mut svc.event.tax, &mut ctx);
        }
        let ticket = self.tickets.issue(Pending::Flush(FlushPurpose::Shutdown));
        self.persist.flush(ticket);
        self.run_state = RunState::Flushing {
            ticket,
            since_tick: self.clock.tick.0,
        };
    }

    /// The shutdown flush ticket completed (committed or failed): the game may exit.
    pub(crate) fn on_shutdown_flush_done(&mut self, ticket: Ticket) {
        if matches!(self.run_state, RunState::Flushing { ticket: t, .. } if t == ticket) {
            self.run_state = RunState::Exiting(GameExit::Shutdown);
        }
    }

    // ---- exits ----

    /// Unscoped panic (Plan/10 §14.4): never persist state after a panic. The sockets are closed
    /// **without** flushing queued frames; the runtime then stops persistence in `AfterPanic` mode.
    fn unscoped_panic_exit(&mut self, info: &PanicInfo) -> GameExit {
        // the unit that panicked is still in the context: nothing cleared it while the panic unwound
        let ctx = crate::panic::context();
        tracing::error!(
            tick = ctx.tick,
            phase = ctx.phase,
            what = ctx.what,
            location = info.location.as_deref().unwrap_or(""),
            panic = %info.message,
            "unscoped panic on the game thread: controlled exit"
        );
        self.telemetry.panics[2].fetch_add(1, Ordering::Relaxed);
        self.telemetry.health.set_state(HealthState::Panicked);
        let _ = catch(|| self.outbox.discard_all(CloseReason::InternalError));
        // best effort, in its own catch: the sink of the audit log works after a panic (13 §25)
        let _ = catch(|| crate::audit::server_panic(&self.audit, None, "unscoped", &ctx, info));
        GameExit::Panicked
    }

    fn finish(&mut self) {
        self.outbox.close_all(
            CloseReason::ServerShutdown,
            self.clock.tick.frames_completed_after(),
            self.cfg.max_batch_bytes,
        );
    }

    // ---- test hooks ----
}

/// Convenience for building a manual-driver request/response pair.
pub fn step_request(ticks: u32) -> (StepRequest, flyff_common::rt::ReplyRx<()>) {
    let (done, rx) = reply_pair();
    (StepRequest { ticks, done }, rx)
}
