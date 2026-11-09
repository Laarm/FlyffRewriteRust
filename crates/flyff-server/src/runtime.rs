//! Process bootstrap and the library entry point (Plan/10 §2.4, §3; 01 §6.1).
//!
//! [`start`] performs the start-up sequence **without** calling `process::exit`, so the in-process
//! test harness (`flyff-e2e::TestServer`) and `main` share one code path. Threads created here:
//! `flyff-game` (15 Hz loop), `flyff-persist` + `flyff-persist-ro-*` (inside `flyff_db::open`),
//! `flyff-startup-*` (temporary, inside `AssetPrep::load_server_derived`), `flyff-asset-*` (niced pool),
//! `flyff-io-*` (tokio workers), `flyff-console` (stdin).
//!
//! The sequence (Plan/10 §3.1; every step's duration goes into `Telemetry.startup_ns`):
//!
//! | step | what |
//! |---|---|
//! | S1-S2 | home, `flyff.toml` (written commented on the first start), configuration |
//! | S3 | directories, logging, panic hook, banner |
//! | S4 | `Resource/` must exist (exit 66) **before** any database file is created |
//! | S5 | `scripts/npc/` from the embedded copy when missing |
//! | S6 | in parallel: (A) database open + instance lock + migrations, (B) VFS index + `GameData` + `ClientData` |
//! | S7 | `AssetPrep::start` (data-bundle export), `load_server_derived` (motion table, model bounds, collision of every loadable world) on the temporary start-up pool, manifest finalisation |
//! | S8 | first start: administrator account, `data/admin.token` |
//! | S9 | the game thread |
//! | S10-S11 | tokio runtime, gateway, HTTP router (+ `/metrics`), signals, console |
//! | S12 | ready: health `Running`, background prewarm, exposure warning, the `ready:` line |
//!
//! The lobby, the game handlers and the first-start step are installed through [`crate::wiring`].

use crate::admin::auth::AdminAuthInit;
use crate::admin::session::{AssetGate, EndReason};
use crate::admin::shared::{AdminShared, SharedInit};
use crate::admin::{self, AdminAuth, AdminOps, ReloadSource};
use crate::assets::{AssetsState, DataBundleBuilder, ServerClock, prep_config, start_prep};
use crate::audit::{Actor, AuditFile};
use crate::build_info;
use crate::config::{self, CliOverrides, Config, ConfigError, ResolvedPaths};
use crate::console::{self, ConsoleCtx};
use crate::data_load::{self, DataLoadError, DataMode, LoadedData};
use crate::exit::{self, ExitStatusCode};
use crate::exposure;
use crate::gateway::{self, GatewayShared, LobbyDeps, LobbyPort, SessionDirectory};
use crate::http::web::WebSite;
use crate::http::{self, CoreHandles, HttpConfig, HttpState, Mounts, ServeOptions};
use crate::lobby::LobbyCore;
use crate::logging::{self, LogHandles, LogMode};
use crate::metrics::{self, BuildLabels, MetricsSources};
use crate::shutdown::{ShutdownCoordinator, ShutdownTrigger};
use crate::web_dist::WebDistSource;
use crate::wiring::{self, FirstStartNotice, LobbyBuild, RuntimeCtx};
use flyff_assets::{AssetBuilder, AssetTelemetry, Clock, ServerDerived};
use flyff_common::rt::telemetry::HealthState;
use flyff_common::rt::{Telemetry, reply_pair};
use flyff_db::{DbErrorKind, Persistence, PersistenceConfig, ShutdownMode};
use flyff_game::tick::{
    GameInit, StepRequest, TickDriver, WallClockCell, WallClockSource, spawn_game_thread,
};
use flyff_game::{GameExit, GameInput, NetEvent, WallZone};
use flyff_lua::{LuaInit, ScriptSources};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How the game loop is paced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TickDriverKind {
    /// Real time at 15 Hz.
    #[default]
    RealTime,
    /// Tests step ticks with [`ServerHandle::step_ticks`].
    Manual,
}

/// Where `start` logs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoggingMode {
    /// Console + rolling file (the `serve` command).
    #[default]
    Full,
    /// No subscriber is installed (tests; the embedding process owns tracing).
    Disabled,
}

/// Inputs of [`start`] (Plan/10 §2.4).
pub struct ServerOptions {
    /// Runtime home directory (temp dir in tests).
    pub home: PathBuf,
    /// `--resource` / `FLYFF_RESOURCE`; overrides `paths.resource_dir`.
    pub resource: Option<PathBuf>,
    /// `--bind`; overrides `net.bind` (tests: `127.0.0.1:0`).
    pub bind: Option<SocketAddr>,
    /// Dotted-key overrides applied after env and flags (tests).
    pub config_overrides: Vec<(String, toml::Value)>,
    /// `--dev`.
    pub dev: bool,
    /// `--web-dir`.
    pub web_dir: Option<PathBuf>,
    /// Allow the stdin console (still subject to `admin.console`).
    pub console: bool,
    /// Tick pacing.
    pub tick_driver: TickDriverKind,
    /// Wall-clock sample source.
    pub wall_clock: WallClockSource,
    /// Logging setup.
    pub logging: LoggingMode,
    /// Install the SIGINT/SIGTERM/SIGHUP handlers (off in tests): SIGINT and SIGTERM shut down gracefully, SIGHUP reloads
    /// `flyff.toml` (`AdminOps::reload`).
    pub handle_signals: bool,
    /// Write `flyff.toml` from the template when missing (`serve` does).
    pub write_default_config: bool,
    /// Exit 66 when `Resource/` lacks `Masquerade.prj` (the plan's rule; tests may relax it).
    pub require_resource: bool,
    /// `None` = read `FLYFF__*` from the process environment; `Some` = use exactly these pairs.
    pub env: Option<Vec<(String, String)>>,
    /// Print the `Open http://...` lines to stdout.
    pub print_banner: bool,
    /// A lobby behind the gateway's handshake and character list that replaces the default (tests). `None`
    /// installs the real lobby (`lobby::Lobby`), or the stub with `use_stub_lobby`.
    pub lobby: Option<LobbyFactory>,
    /// Install the stub lobby (`StubLobby`: any token, a fixed character list) instead of the real WebSocket
    /// lobby of doc 12 (`lobby::Lobby`). Only the gateway tests want that; `false` in production.
    pub use_stub_lobby: bool,
    /// Options of the stub lobby (only used with `use_stub_lobby`).
    pub stub_lobby: gateway::StubOptions,
    /// Load `GameData`/`ClientData` from `Resource/` (production) or run with an empty `GameData` (tests that
    /// use a fake `Resource/` containing only `Masquerade.prj`).
    pub data: DataMode,
    /// The host's interface addresses for the exposure warning (`None` = read them from `/proc/net`; tests
    /// inject a list).
    pub interfaces: Option<Vec<IpAddr>>,
    /// Test and bot runs: install the trivial wandering AI (`flyff_game::ai::WanderAi`, a walk to a random point
    /// around the spawn every this many game milliseconds) for every NPC, so that observers have moving NPCs
    /// before the monster brain of doc 18 §9 exists (Plan/60 S-WALK-04). `None` in production.
    pub wander_ai_ms: Option<u64>,
}

/// Builds the lobby once the gateway's shared pieces exist (the lobby needs the session directory and C1).
pub type LobbyFactory = Box<dyn FnOnce(LobbyDeps) -> Arc<dyn LobbyPort> + Send>;

impl ServerOptions {
    /// Options for `home` with the production defaults.
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            resource: None,
            bind: None,
            config_overrides: Vec::new(),
            dev: false,
            web_dir: None,
            console: false,
            tick_driver: TickDriverKind::RealTime,
            wall_clock: WallClockSource::System,
            logging: LoggingMode::Full,
            handle_signals: true,
            write_default_config: true,
            require_resource: true,
            env: None,
            print_banner: false,
            lobby: None,
            use_stub_lobby: false,
            stub_lobby: gateway::StubOptions::default(),
            data: DataMode::Load,
            interfaces: None,
            wander_ai_ms: None,
        }
    }
}

/// Why the server could not start; [`StartupError::exit_code`] maps it to Plan/10 §3.4.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartupError {
    /// Invalid configuration (all findings).
    #[error("invalid configuration:\n{}", format_config_errors(.0))]
    Config(Vec<ConfigError>),
    /// `Resource/` is missing or lacks `Masquerade.prj`.
    #[error(
        "the Flyff V15 Resource folder was not found at {0}\n\
         Copy your original `Resource` folder next to the executable (as `Resource/`), or pass --resource <DIR> / set FLYFF_RESOURCE."
    )]
    ResourceMissing(PathBuf),
    /// The game data could not be loaded (a Fatal diagnostic of Plan/14 §8.2; exit 2).
    #[error("game data: {0}")]
    Data(String),
    /// The database could not be opened.
    #[error("database: {0}")]
    Db(#[from] flyff_db::DbError),
    /// The listen address could not be bound.
    #[error("cannot bind {addr}: {source}")]
    Bind {
        /// Address.
        addr: String,
        /// Cause.
        source: std::io::Error,
    },
    /// The game thread could not be started.
    #[error("game: {0}")]
    Game(String),
    /// Other I/O or setup failure.
    #[error("{0}")]
    Setup(String),
    /// `SIGINT`/`SIGTERM` arrived before the server was ready (Plan/10 §15.2): the start-up was abandoned and
    /// everything that was opened was shut down gracefully. Not a failure: exit 0.
    #[error("start-up interrupted by signal {0}")]
    Interrupted(i32),
}

fn format_config_errors(errs: &[ConfigError]) -> String {
    errs.iter()
        .map(|e| format!("  - {e}"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl StartupError {
    /// The process exit code for this failure (Plan/10 §3.4).
    pub fn exit_code(&self) -> ExitStatusCode {
        match self {
            StartupError::Config(_) => exit::CONFIG,
            StartupError::ResourceMissing(_) => exit::NOINPUT,
            StartupError::Data(_) => exit::DATA,
            StartupError::Db(e) => match e.kind {
                DbErrorKind::Locked => exit::TEMPFAIL,
                DbErrorKind::Corrupt => exit::IOERR,
                DbErrorKind::InitOnlyMismatch | DbErrorKind::InvalidConfig => exit::CONFIG,
                _ => exit::FAILURE,
            },
            StartupError::Bind { .. } | StartupError::Game(_) | StartupError::Setup(_) => {
                exit::FAILURE
            }
            StartupError::Interrupted(_) => exit::OK,
        }
    }
}

/// Error of [`ServerHandle::step_ticks`].
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// The handle was not started with [`TickDriverKind::Manual`].
    #[error("the server runs with the real-time tick driver")]
    NotManual,
    /// The game thread is gone.
    #[error("the game thread has ended")]
    GameGone,
}

/// Names the process environment variables that carry config overrides.
fn process_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| k.starts_with("FLYFF_"))
        .collect()
}

/// `true` when `dir` contains `Masquerade.prj` (any letter case).
pub fn resource_present(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.filter_map(Result::ok).any(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("masquerade.prj")
        })
    })
}

/// Resolves the home directory: `--home` > `FLYFF_HOME` > the directory of the executable.
pub fn resolve_home(flag: Option<&Path>) -> Result<PathBuf, StartupError> {
    if let Some(h) = flag {
        return Ok(h.to_path_buf());
    }
    if let Some(h) = std::env::var_os("FLYFF_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(h));
    }
    let exe = std::env::current_exe()
        .map_err(|e| StartupError::Setup(format!("cannot locate the executable: {e}")))?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| StartupError::Setup("the executable has no parent directory".to_owned()))
}

/// Loads `<home>/flyff.toml` with the process environment and the given flag overrides.
pub fn load_config(
    home: &Path,
    cli: &CliOverrides,
    env: &[(String, String)],
    overrides: &[(String, toml::Value)],
) -> Result<config::LoadedConfig, StartupError> {
    config::load(home, cli, env, overrides).map_err(StartupError::Config)
}

/// First `scripts/npc/` population: extracts the embedded copy when the folder is missing entirely
/// (Plan/10 §3.1 S5). A partially present folder is left alone.
fn ensure_npc_scripts(dir: &Path) -> Result<(), StartupError> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| StartupError::Setup(format!("create {}: {e}", dir.display())))?;
    #[cfg(feature = "embed-web")]
    {
        let mut count = 0usize;
        for name in crate::web_dist::EmbeddedNpcScripts::iter() {
            let Some(file) = crate::web_dist::EmbeddedNpcScripts::get(&name) else {
                continue;
            };
            let target = dir.join(name.as_ref());
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    StartupError::Setup(format!("create {}: {e}", parent.display()))
                })?;
            }
            std::fs::write(&target, file.data.as_ref())
                .map_err(|e| StartupError::Setup(format!("write {}: {e}", target.display())))?;
            count += 1;
        }
        tracing::info!(dir = %dir.display(), files = count, "extracted the embedded NPC scripts");
    }
    Ok(())
}

fn prepare_dirs(paths: &ResolvedPaths) -> Result<(), StartupError> {
    for dir in [&paths.data, &paths.cache, &paths.logs, &paths.backups] {
        std::fs::create_dir_all(dir)
            .map_err(|e| StartupError::Setup(format!("create {}: {e}", dir.display())))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in [&paths.data, &paths.backups] {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    Ok(())
}

fn persistence_config(cfg: &Config) -> PersistenceConfig {
    cfg.persistence()
}

/// The kind of thread a panic happened on, from its name (Plan/10 §14.1): the game thread counts its own scopes
/// (`panics[0..=2]`), every other thread counts as a task (`panics[3]`).
fn thread_kind(name: &str) -> &'static str {
    match name {
        "flyff-game" => "game",
        "flyff-audit" => "audit",
        n if n.starts_with("flyff-persist") => "persist",
        _ => "task",
    }
}

/// The telemetry of the latest started server: the process-wide hook counts task panics in it. One server per
/// process in production; tests that start several assert deltas.
static HOOK_TELEMETRY: std::sync::Mutex<std::sync::Weak<Telemetry>> =
    std::sync::Mutex::new(std::sync::Weak::new());

/// Installs the process-wide panic hook (Plan/10 §14.6, S3): once per process, whichever server starts first. It logs
/// at ERROR with the thread name and kind, the `PanicContext` of the game thread (tick, phase, session, character,
/// the unit of work), the location, the payload and a backtrace; hands the location to `flyff_game::panic::catch`;
/// counts a panic of any thread but the game thread in `panics[3]`; and chains the previous hook. It takes no lock
/// the game thread could hold (`try_lock` on the telemetry cell, which only `start` holds for an assignment).
fn install_panic_hook(telemetry: &Arc<Telemetry>) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    *HOOK_TELEMETRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(telemetry);
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let backtrace = std::backtrace::Backtrace::force_capture();
            let thread = std::thread::current();
            let name = thread.name().unwrap_or("<unnamed>");
            let kind = thread_kind(name);
            let on_game_thread = kind == "game";
            let ctx = if on_game_thread {
                flyff_game::panic::context()
            } else {
                flyff_game::panic::PanicContext::EMPTY
            };
            let location = info.location();
            if on_game_thread && let Some(l) = location {
                flyff_game::panic::note_location(l);
            }
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string payload>".to_owned());
            tracing::error!(
                thread = name,
                kind,
                tick = ctx.tick,
                phase = ctx.phase,
                what = ctx.what,
                session_id = ctx.session.map_or(0, |s| s.0),
                character = ctx.character.map_or(0, |c| c.0),
                location = %location.map_or_else(String::new, ToString::to_string),
                panic = %message,
                "panic on thread {name}\n{backtrace}"
            );
            if !on_game_thread
                && let Ok(guard) = HOOK_TELEMETRY.try_lock()
                && let Some(t) = guard.upgrade()
                && let Some(c) = t.panics.get(3)
            {
                c.fetch_add(1, Ordering::Relaxed);
            }
            previous(info);
        }));
    });
}

/// Best-effort LAN address of this host (a connected UDP socket sends no packets).
fn lan_ip() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// The URLs to show for a listener bound to `addr`.
pub fn open_urls(addr: SocketAddr) -> Vec<String> {
    if addr.ip().is_unspecified() {
        let mut urls = vec![format!("http://localhost:{}/", addr.port())];
        if let Some(ip) = lan_ip() {
            urls.push(format!("http://{ip}:{}/", addr.port()));
        }
        urls
    } else {
        vec![format!("http://{addr}/")]
    }
}

/// A running server (Plan/10 §2.4). Dropping it shuts the server down immediately.
pub struct ServerHandle {
    local_addr: SocketAddr,
    telemetry: Arc<Telemetry>,
    persist: flyff_db::PersistHandle,
    coordinator: ShutdownCoordinator,
    game_tx: flume::Sender<GameInput>,
    wall: WallClockCell,
    step_tx: Option<flume::Sender<StepRequest>>,
    game: Option<JoinHandle<GameExit>>,
    persistence: Option<Persistence>,
    assets: Option<Arc<AssetsState>>,
    http_state: HttpState,
    runtime: Option<tokio::runtime::Runtime>,
    http_stop: Option<tokio::sync::watch::Sender<bool>>,
    http_done: Option<flume::Receiver<()>>,
    _console: Option<JoinHandle<()>>,
    _log: Arc<LogHandles>,
    home: PathBuf,
    ctx: RuntimeCtx,
    notice: Option<FirstStartNotice>,
    startup: StartupReport,
    ops: Arc<AdminOps>,
    lobby_core: Arc<LobbyCore>,
    gateway: Arc<GatewayShared>,
}

/// How long the start-up took and where (Plan/10 §3.2; also in the `ready:` log line and `/metrics`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupReport {
    /// `main()` entry to the end of S12 (ms).
    pub total_ms: u64,
    /// Duration of S1..S12 (ms; S6 is the longer of its two parallel parts).
    pub steps_ms: [u64; 12],
    /// S6 (A): the database (ms).
    pub db_ms: u64,
    /// S6 (B): VFS index + game data (ms).
    pub data_ms: u64,
    /// S7: `AssetPrep::start` (bundle export included) (ms).
    pub assets_start_ms: u64,
    /// S7: `load_server_derived` (ms).
    pub derived_ms: u64,
}

impl ServerHandle {
    /// The bound HTTP/WebSocket address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Shared counters.
    pub fn telemetry(&self) -> Arc<Telemetry> {
        Arc::clone(&self.telemetry)
    }

    /// The state of the HTTP layer (counters, limiter, asset tokens, assets).
    pub fn http_state(&self) -> HttpState {
        self.http_state.clone()
    }

    /// The asset side: VFS, game data, `AssetPrep` and the manifests.
    pub fn assets(&self) -> Option<Arc<AssetsState>> {
        self.assets.clone()
    }

    /// The persistence handle.
    pub fn persist(&self) -> flyff_db::PersistHandle {
        self.persist.clone()
    }

    /// The loaded game data (empty with [`DataMode::Empty`]).
    pub fn data(&self) -> Arc<flyff_data::GameData> {
        Arc::clone(&self.ctx.data)
    }

    /// The server-side derived assets (motion table, model bounds, collision worlds).
    pub fn derived(&self) -> &ServerDerived {
        &self.ctx.derived
    }

    /// The first-start notice (account `admin` and its one-time password) when this start created it.
    pub fn first_start_notice(&self) -> Option<&FirstStartNotice> {
        self.notice.as_ref()
    }

    /// Where the start-up spent its time.
    pub fn startup_report(&self) -> &StartupReport {
        &self.startup
    }

    /// The admin surfaces (console grammar, `/admin/api/*`, reload): tests drive them in-process.
    pub fn admin_ops(&self) -> Arc<AdminOps> {
        Arc::clone(&self.ops)
    }

    /// The lobby core: online registry, tokens, throttle, statistics, `AccountAdmin`.
    pub fn lobby_core(&self) -> Arc<LobbyCore> {
        Arc::clone(&self.lobby_core)
    }

    /// The gateway state: session directory, per-IP limiter, counters.
    pub fn gateway(&self) -> Arc<GatewayShared> {
        Arc::clone(&self.gateway)
    }

    /// The audit appender of the server layer (`logs/audit/`).
    pub fn audit(&self) -> Arc<AuditFile> {
        Arc::clone(&self.ctx.audit)
    }

    /// The wall-clock cell behind the game's per-tick sample and `ServerTime.server_unix_ms` (tests switch it
    /// to a manual source; changes apply from the next tick's sample).
    pub fn wall_clock(&self) -> WallClockCell {
        self.wall.clone()
    }

    /// The game inbox (tests inject inputs and hooks).
    pub fn game_sender(&self) -> flume::Sender<GameInput> {
        self.game_tx.clone()
    }

    /// The shutdown coordinator.
    pub fn coordinator(&self) -> &ShutdownCoordinator {
        &self.coordinator
    }

    /// The home directory in use.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Runs `n` ticks (only with [`TickDriverKind::Manual`]) and returns when they completed.
    pub fn step_ticks(&self, n: u32) -> Result<(), RuntimeError> {
        let Some(tx) = &self.step_tx else {
            return Err(RuntimeError::NotManual);
        };
        let (req, rx) = flyff_game::tick::step_request(n);
        tx.send(req).map_err(|_| RuntimeError::GameGone)?;
        rx.recv().map_err(|_| RuntimeError::GameGone)
    }

    /// Blocks until the game loop ended (shutdown, panic, persistence failure), tears everything down
    /// and returns the process exit code.
    pub fn wait(mut self) -> ExitStatusCode {
        self.teardown()
    }

    /// Graceful shutdown with the given countdown, then [`ServerHandle::wait`].
    pub fn shutdown(self, countdown_s: u32) -> ExitStatusCode {
        self.coordinator
            .request(Some(countdown_s), None, ShutdownTrigger::Test);
        self.wait()
    }

    /// `Kick{SHUTDOWN}` + close 4009 for every gateway session that is still open, then waits (at most 2 s) for the
    /// session tasks to write them.
    fn close_idle_sockets(&self) {
        let directory = &self.gateway.directory;
        for info in directory.list() {
            let _ = info
                .handle
                .close(flyff_common::rt::CloseReason::ServerShutdown);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !directory.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn teardown(&mut self) -> ExitStatusCode {
        let Some(game) = self.game.take() else {
            return exit::OK;
        };
        // Under the manual driver the game only advances when told to: keep stepping until it ended.
        if let Some(step_tx) = self.step_tx.take() {
            while !game.is_finished() {
                let (req, rx) = flyff_game::tick::step_request(1);
                if step_tx.send(req).is_err() {
                    break;
                }
                let _ = rx.recv_timeout(Duration::from_millis(500));
            }
        }
        let game_exit = game.join().unwrap_or(GameExit::Panicked);
        let telemetry = &self.telemetry;
        // `server.stop` before the persistence layer stops (Plan/10 §15.2 step 5)
        {
            let summary = self.coordinator.summary();
            let reason = match game_exit {
                GameExit::Shutdown => summary.reason.unwrap_or("shutdown"),
                GameExit::Panicked => "panic",
                GameExit::PersistenceFailed { .. } => "persistence_failed",
                GameExit::StartFailed => "start_failed",
            };
            self.ctx.audit.emit(
                "server.stop",
                &Actor::none(),
                serde_json::json!({
                    "reason": reason,
                    "uptime_s": telemetry.now_ns() / 1_000_000_000,
                    "players_at_start_of_countdown": summary.players_at_start.unwrap_or(0),
                }),
            );
        }
        if game_exit != GameExit::Panicked {
            telemetry.health.set_state(HealthState::Stopped);
        }
        telemetry
            .health
            .accepting_logins
            .store(false, Ordering::Relaxed);

        // every admin session ends (the admin WebSockets get `SessionEnded{shutdown}`, the asset tokens are revoked)
        self.ops.admin.sessions.end_all(EndReason::Shutdown);
        // the sockets of the owner on loopback have no session to end
        self.ops.admin.ws.stop();

        // Sockets that are not in the world (login, character select, entering) get the same `Kick{SHUTDOWN}` and
        // the close 4009 as the avatars did (the game closed those sinks itself): the browser goes back to the
        // login screen instead of waiting for a server that is gone.
        if game_exit == GameExit::Shutdown {
            self.close_idle_sockets();
        }

        // Assets first (01 §6.10 step 5): queued prewarm jobs are dropped and waiting `/gen` requests get a
        // `503`, so the HTTP drain below does not wait for builds.
        if let Some(assets) = self.assets.take() {
            assets.prep.shutdown(Duration::from_secs(5));
        }

        // HTTP: stop accepting, give in-flight requests a few seconds.
        if let Some(stop) = self.http_stop.take() {
            let _ = stop.send(true);
        }
        if let Some(done) = self.http_done.take() {
            // the listener drains in-flight requests for at most 10 s (Plan/24 §3.1)
            let _ = done.recv_timeout(Duration::from_secs(11));
        }

        // Persistence: commit what is queued, checkpoint, release the instance lock.
        let mut code = match game_exit {
            // a restart from the admin console: the supervisor starts the server again (Plan/25 §9.7)
            GameExit::Shutdown if self.ops.admin.restart_requested.load(Ordering::Relaxed) => {
                exit::RESTART
            }
            GameExit::Shutdown => exit::OK,
            GameExit::Panicked => exit::SOFTWARE,
            GameExit::PersistenceFailed { corrupt: true } => exit::IOERR,
            GameExit::PersistenceFailed { corrupt: false } => exit::SOFTWARE,
            GameExit::StartFailed => exit::FAILURE,
        };
        if let Some(p) = self.persistence.take() {
            let mode = if game_exit == GameExit::Shutdown {
                ShutdownMode::Graceful
            } else {
                ShutdownMode::AfterPanic
            };
            if let Err(e) = p.shutdown(mode, Duration::from_secs(60)) {
                tracing::error!(error = %e, "persistence did not shut down cleanly");
                if code == exit::OK {
                    code = exit::FAILURE;
                }
            }
        }

        if let Some(rt) = self.runtime.take() {
            // Does not block and is safe inside an async context (tests).
            rt.shutdown_background();
        }
        tracing::info!(code, "server stopped");
        code
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if self.game.is_some() {
            self.coordinator
                .request(Some(0), None, ShutdownTrigger::Test);
            let _ = self.teardown();
        }
    }
}

/// Aborting a start must still release the instance lock and stamp the clean shutdown: dropping the guard
/// shuts the persistence layer down gracefully.
struct DbGuard(Option<Persistence>);

impl DbGuard {
    fn take(&mut self) -> Option<Persistence> {
        self.0.take()
    }
}

impl Drop for DbGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = p.shutdown(ShutdownMode::Graceful, Duration::from_secs(10));
        }
    }
}

/// A game thread that was started but whose server did not come up (a later start-up step failed): dropping the
/// guard asks it to stop at once and joins it, so no thread outlives the failed start.
struct GameGuard {
    coordinator: ShutdownCoordinator,
    game: Option<JoinHandle<GameExit>>,
    step_tx: Option<flume::Sender<StepRequest>>,
}

impl GameGuard {
    /// The start succeeded: the handle owns the thread now.
    fn disarm(&mut self) -> Option<JoinHandle<GameExit>> {
        self.game.take()
    }
}

impl Drop for GameGuard {
    fn drop(&mut self) {
        let Some(game) = self.game.take() else {
            return;
        };
        self.coordinator
            .request(Some(0), None, ShutdownTrigger::Test);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !game.is_finished() && Instant::now() < deadline {
            if let Some(tx) = &self.step_tx {
                let (req, rx) = flyff_game::tick::step_request(1);
                if tx.send(req).is_ok() {
                    let _ = rx.recv_timeout(Duration::from_millis(200));
                }
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = game.join();
    }
}

/// Records the duration of the start-up steps into `Telemetry.startup_ns` (Plan/10 §3.2).
struct StepClock {
    telemetry: Arc<Telemetry>,
    last: Instant,
    steps_ms: [u64; 12],
}

impl StepClock {
    fn new(telemetry: &Arc<Telemetry>, t0: Instant) -> Self {
        Self {
            telemetry: Arc::clone(telemetry),
            last: t0,
            steps_ms: [0; 12],
        }
    }

    /// Ends step `n` (1-based) now.
    fn mark(&mut self, n: usize) {
        let now = Instant::now();
        let d = now.duration_since(self.last);
        self.last = now;
        self.set(n, d);
    }

    fn set(&mut self, n: usize, d: Duration) {
        let idx = n.saturating_sub(1);
        if let Some(slot) = self.telemetry.startup_ns.get(idx) {
            slot.store(
                u64::try_from(d.as_nanos()).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
        }
        if let Some(slot) = self.steps_ms.get_mut(idx) {
            *slot = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        }
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// A `SIGINT`/`SIGTERM` that arrived during the start-up abandons it (Plan/10 §15.2): the guards of what was
/// opened shut it down gracefully on the way out.
fn check_interrupt(handle_signals: bool) -> Result<(), StartupError> {
    if handle_signals && let Some(sig) = crate::sys::take_startup_signal() {
        tracing::warn!(signal = sig, "start-up interrupted: shutting down");
        return Err(StartupError::Interrupted(sig));
    }
    Ok(())
}

/// Runs the start-up sequence of Plan/10 §3.1 (S1-S12; see the module documentation) and returns the running
/// server.
pub fn start(mut opts: ServerOptions) -> Result<ServerHandle, StartupError> {
    let t0 = Instant::now();
    let started_unix_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let handle_signals = opts.handle_signals;
    if handle_signals {
        // until tokio owns the signals (S10) a SIGINT/SIGTERM only records itself
        crate::sys::install_startup_handlers();
    }
    let telemetry = Arc::new(Telemetry::new());
    let mut steps = StepClock::new(&telemetry, t0);
    let env = opts.env.clone().unwrap_or_else(process_env);

    // S1-S2: home, config.
    let home = opts.home.clone();
    std::fs::create_dir_all(&home)
        .map_err(|e| StartupError::Setup(format!("create {}: {e}", home.display())))?;
    steps.mark(1);
    let config_path = home.join("flyff.toml");
    let wrote_default_config = opts.write_default_config && !config_path.exists();
    if wrote_default_config {
        write_default_config(&config_path)?;
    }
    let resource_flag = opts.resource.clone().or_else(|| {
        env.iter()
            .find(|(k, _)| k == "FLYFF_RESOURCE")
            .map(|(_, v)| PathBuf::from(v))
            .filter(|p| !p.as_os_str().is_empty())
    });
    let cli = CliOverrides {
        bind: opts.bind.map(|b| b.to_string()),
        resource: resource_flag,
        dev: opts.dev,
    };
    let loaded_cfg = load_config(&home, &cli, &env, &opts.config_overrides)?;
    let cfg = Arc::new(loaded_cfg.config.clone());
    let paths = cfg.resolve_paths(&home);
    steps.mark(2);

    // S3: directories, logging, panic hook, banner.
    prepare_dirs(&paths)?;
    let log = Arc::new(match opts.logging {
        LoggingMode::Full => logging::init(&cfg.log, &paths.logs, LogMode::Serve)
            .map_err(|e| StartupError::Setup(format!("logging: {e:#}")))?,
        LoggingMode::Disabled => LogHandles::inactive(),
    });
    install_panic_hook(&telemetry);
    let web = WebDistSource::select(opts.web_dir.clone());
    let web_build = web.build_id();
    let version_line = build_info::version_line(&web_build);
    tracing::info!("{version_line}");
    if wrote_default_config {
        tracing::info!(file = %config_path.display(), "wrote the default configuration");
    }
    for w in &loaded_cfg.warnings {
        tracing::warn!("{w}");
    }
    steps.mark(3);
    check_interrupt(handle_signals)?;

    // S4: Resource must exist before any database file is created.
    if opts.require_resource && !resource_present(&paths.resource) {
        return Err(StartupError::ResourceMissing(paths.resource.clone()));
    }
    steps.mark(4);
    // S5: NPC scripts.
    ensure_npc_scripts(&paths.npc_scripts)?;
    steps.mark(5);
    check_interrupt(handle_signals)?;

    // `server.timezone = "local"` is resolved once here (TZ, then the host zone); the game thread gets the name and
    // the backup layer the offsets (the day and week boundaries of the backup rotation are local, 13 §21.2).
    let zone =
        WallZone::resolve(&cfg.server.timezone).map_err(|e| StartupError::Setup(e.to_string()))?;
    let db_options = flyff_db::OpenOptions::default().with_clock(Arc::new(
        flyff_db::backup::SystemClock::with_zone(Arc::new({
            let zone = zone.clone();
            move |t: flyff_common::UnixTime| {
                zone.local(flyff_common::UnixMs(t.0.saturating_mul(1000)))
                    .utc_offset_s
            }
        })),
    ));

    // S6: (A) the database with its instance lock, (B) the VFS index and the game data, in parallel.
    let persist_cfg = persistence_config(&cfg);
    let diag_log = paths.logs.join("data-load.jsonl");

    let (db_result, data_result) = std::thread::scope(|scope| {
        let db_job = scope.spawn(|| {
            let t = Instant::now();

            let opened = flyff_db::open_with(&persist_cfg, &home, &db_options);
            (opened, t.elapsed())
        });
        let t = Instant::now();
        let data = data_load::load(&cfg, &paths, opts.data, Some(&diag_log));
        let data_elapsed = t.elapsed();
        (db_job.join(), (data, data_elapsed))
    });
    let (db_open, db_elapsed) =
        db_result.map_err(|_| StartupError::Setup("the database thread panicked".to_owned()))?;
    // The instance lock (exit 75) and database errors come first: they say another server is running.
    let db = db_open?;
    let (data_open, data_elapsed) = data_result;
    steps.set(6, db_elapsed.max(data_elapsed));
    steps.last = Instant::now();
    let flyff_db::OpenOutput {
        persistence,
        handle: persist,
        completions,
        startup,
        serial_blocks,
        audit,
        unclean_previous_run,
        user_version: db_user_version,
        ..
    } = db;
    let mut db_guard = DbGuard(Some(persistence));
    if unclean_previous_run {
        tracing::warn!("the previous run did not shut down cleanly");
    }
    let LoadedData {
        vfs,
        game: game_data,
        client: client_data,
        report: data_report,
    } = data_open.map_err(|e| match e {
        DataLoadError::ResourceMissing(p) => StartupError::ResourceMissing(p),
        DataLoadError::Data(m) => StartupError::Data(m),
    })?;
    data_load::log_report(&data_report, opts.data);
    // test hook: a start-up that takes long enough for a signal to arrive between the database and the assets

    check_interrupt(handle_signals)?;

    // S7: the asset framework: AssetPrep (data-bundle export), the server-derived assets on the temporary
    // start-up pool, the manifests (Plan/24 §11.1).
    let asset_clock: Arc<dyn Clock> = Arc::new(ServerClock::default());
    let t7 = Instant::now();
    let prep = start_prep(
        Arc::clone(&vfs),
        Arc::clone(&game_data),
        prep_config(&cfg, &paths),
        Arc::clone(&asset_clock),
        AssetTelemetry::new(),
        |_parts| {
            client_data
                .iter()
                .map(|c| Arc::new(DataBundleBuilder::new(Arc::clone(c))) as Arc<dyn AssetBuilder>)
                .collect()
        },
    )
    .map_err(|e| StartupError::Setup(format!("assets: {e}")))?;
    let assets_start = t7.elapsed();
    let t7b = Instant::now();
    let derived = if opts.data == DataMode::Load {
        prep.load_server_derived()
            .map_err(|e| StartupError::Setup(format!("derived assets: {e}")))?
    } else {
        ServerDerived::default()
    };
    let derived_elapsed = t7b.elapsed();
    let assets = Arc::new(
        AssetsState::new(
            Arc::clone(&vfs),
            Arc::clone(&game_data),
            client_data.clone(),
            prep,
        )
        .map_err(|e| StartupError::Setup(format!("assets: {e}")))?,
    );
    log_derived(&derived, assets_start, derived_elapsed);
    steps.mark(7);
    check_interrupt(handle_signals)?;

    // The game inbox and the shared cells exist before the hooks run.
    let (game_tx, game_rx) = flume::bounded::<GameInput>(65_536);
    let (net_events_tx, net_events_rx) = flume::unbounded::<NetEvent>();
    // 25 §1.8: the game thread's `/ban` and `/setauth` tell the admin console, which ends the staff sessions
    let (account_change_tx, account_change_rx) =
        flume::bounded::<flyff_game::input::AccountChange>(1024);
    // 23 §4.6 (WP-GM-25): `/loadscript` and `/reload config` ask the server for what the game thread cannot do
    let (server_request_tx, server_request_rx) =
        flume::bounded::<flyff_game::gm::ServerRequest>(16);
    let wall_cell = WallClockCell::new(opts.wall_clock);
    let ctx = RuntimeCtx {
        home: home.clone(),
        cfg: Arc::clone(&cfg),
        paths: paths.clone(),
        data: Arc::clone(&game_data),
        client: client_data.clone(),
        derived: derived.clone(),
        persist: persist.clone(),
        telemetry: Arc::clone(&telemetry),
        game_tx: game_tx.clone(),
        wall: wall_cell.clone(),
        zone_name: zone.name().to_owned(),
        dev: opts.dev,
        wander_ai_ms: opts.wander_ai_ms,
        web_build: web_build.clone(),
        // one writer thread for the whole process: the log that `flyff_db::open` started (13 §25)
        audit: AuditFile::from_log(audit.clone()),
    };

    // S8: runtime overrides (M2), the first-start administrator, the admin token.
    if !startup.runtime_overrides.is_empty() {
        tracing::debug!(
            rows = startup.runtime_overrides.len(),
            "runtime_override rows are applied from M2 (WP-OPS-20); ignored"
        );
    }
    let notice = wiring::first_start(&ctx, &startup)?;
    let admin_token = admin::ensure_admin_token(&paths.data)
        .map_err(|e| StartupError::Setup(format!("data/admin.token: {e}")))?;
    steps.mark(8);
    check_interrupt(handle_signals)?;

    // Bind early (std) so a busy port fails before the game thread exists.
    let bind_text = cfg.net.bind.clone();
    let bind_addr: SocketAddr = bind_text
        .parse()
        .map_err(|_| StartupError::Setup(format!("net.bind '{bind_text}' is not an address")))?;
    let listener = match std::net::TcpListener::bind(bind_addr)
        .and_then(|l| l.set_nonblocking(true).map(|()| l))
    {
        Ok(l) => l,
        Err(source) => {
            return Err(StartupError::Bind {
                addr: bind_text,
                source,
            });
        }
    };
    let local_addr = listener
        .local_addr()
        .map_err(|e| StartupError::Setup(format!("local_addr: {e}")))?;
    // where the admin API of this process listens, for `flyff account` (online mode) when the address was given by
    // `--bind` or `:0` and is therefore not in `flyff.toml`
    if let Err(e) = std::fs::write(
        paths.data.join(admin::ADMIN_ADDR_FILE),
        format!("{local_addr}\n"),
    ) {
        tracing::warn!(error = %e, "cannot write data/{}", admin::ADMIN_ADDR_FILE);
    }

    // S9: the game thread.
    let (step_tx, driver) = match opts.tick_driver {
        TickDriverKind::RealTime => (None, TickDriver::RealTime),
        TickDriverKind::Manual => {
            let (tx, rx) = flume::unbounded();
            telemetry.manual_ticks.store(true, Ordering::Relaxed);
            (Some(tx), TickDriver::Manual(rx))
        }
    };
    let coordinator = ShutdownCoordinator::new(
        game_tx.clone(),
        Arc::clone(&telemetry),
        cfg.server.shutdown_countdown_s,
    );
    coordinator.set_audit(Arc::clone(&ctx.audit));
    let seed = if cfg.debug.seed != 0 {
        cfg.debug.seed
    } else {
        getrandom::u64().map_err(|e| StartupError::Setup(format!("OS random: {e}")))?
    };
    let npc_sources = ScriptSources::read(&paths.npc_scripts).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "cannot read the NPC scripts");
        ScriptSources::default()
    });
    let (ready_tx, ready_rx) = reply_pair();
    // `schedule.last_fired.<job>` lives in `server_kv` (10 §5.5): a restart does not fire a job twice
    let wall_store: Box<dyn flyff_game::WallStore> = Box::new(
        crate::wall_store::KvWallStore::load(&startup.server_kv, persist.clone()),
    );
    let init = GameInit {
        cfg: cfg.game_config(zone.name()),
        persist: persist.clone(),
        net_events: net_events_tx,
        db_replies: completions,
        serial_blocks,
        audit,
        telemetry: Arc::clone(&telemetry),
        lua: LuaInit {
            npc_sources,
            seed,
            // the resource tree behind the families' `dofile` / `require` and their reload (Plan/20 §3.4)
            vfs: Some(Arc::clone(&vfs)),
            ..LuaInit::default()
        },
        startup,
        seed,
        // the profanity filter and the emotes of the client data (23 §6.9, §6.10; empty without client data)
        chat_data: client_data
            .as_deref()
            .map(flyff_game::chat::ChatData::from_client)
            .unwrap_or_default(),
        shutdown: Box::new(coordinator.clone()),
        driver,
        wall_clock: WallClockSource::Cell(wall_cell.clone()),
        ready: Some(ready_tx),
        handlers: {
            let mut handlers = wiring::build_game_handlers(&ctx, &vfs)?;
            handlers.account_events = flyff_game::input::AccountEventSink::new(account_change_tx);
            handlers.server_requests = flyff_game::gm::ServerRequestSink::new(server_request_tx);
            handlers
        },
        world: wiring::build_world_init(&ctx, &vfs),
        wall_store: Some(wall_store),
    };
    let game = match spawn_game_thread(init, game_rx) {
        Ok(g) => g,
        Err(e) => {
            return Err(StartupError::Game(format!(
                "cannot spawn the game thread: {e}"
            )));
        }
    };
    match ready_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = game.join();
            return Err(StartupError::Game(e.0));
        }
        Err(_) => {
            if let Some(p) = db_guard.take() {
                let _ = p.shutdown(ShutdownMode::AfterPanic, Duration::from_secs(10));
            }
            return Err(StartupError::Game(
                "the game thread did not become ready".to_owned(),
            ));
        }
    }
    let mut game_guard = GameGuard {
        coordinator: coordinator.clone(),
        game: Some(game),
        step_tx: step_tx.clone(),
    };
    steps.mark(9);
    check_interrupt(handle_signals)?;

    // S10-S11: the tokio runtime, the gateway, the HTTP router with /metrics, signals, console.
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let io_threads = if cfg.runtime.io_threads == 0 {
        cores.saturating_sub(1).max(1)
    } else {
        cfg.runtime.io_threads as usize
    };
    let counter = Arc::new(AtomicU64::new(0));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(io_threads)
        .max_blocking_threads(8)
        .thread_name_fn(move || format!("flyff-io-{}", counter.fetch_add(1, Ordering::Relaxed)))
        .enable_all()
        .build()
        .map_err(|e| StartupError::Setup(format!("tokio runtime: {e}")))?;
    if !cfg.net.tls.cert.is_empty() || !cfg.net.tls.key.is_empty() {
        tracing::warn!(
            "net.tls is configured but built-in TLS is not implemented yet (WP-WEB-16): serving plain HTTP; \
             terminate TLS in a reverse proxy and set web.trusted_proxies"
        );
    }
    let http_cfg = HttpConfig::from_config(&cfg, opts.dev);
    let serve_opts = ServeOptions {
        max_tcp_per_ip: http_cfg.max_tcp_per_ip,
        max_tcp_total: http_cfg.max_tcp_total,
        trusted_proxies: http_cfg.trusted_proxies.clone(),
        ..ServeOptions::default()
    };
    let site = WebSite::new(web.clone(), &cfg.web.page_title, http_cfg.dev_surfaces);
    let http_state = HttpState::new(
        http_cfg,
        site,
        Arc::clone(&assets),
        CoreHandles {
            telemetry: Arc::clone(&telemetry),
            persist: persist.clone(),
        },
        asset_clock,
    );
    // the admin IP blocks (25 §4.8) join `net.ip_block` from the first request on
    match admin::ip_blocks::load(&persist, &http_state.live) {
        Ok(0) => {}
        Ok(n) => tracing::info!(blocks = n, "admin IP blocks loaded"),
        Err(e) => tracing::warn!(error = %e, "cannot load the admin IP blocks"),
    }
    // The gateway: the lobby (wiring), the session directory and the NetEvent router.
    let directory = SessionDirectory::new();
    let deps = LobbyDeps {
        game_tx: game_tx.clone(),
        directory: Arc::clone(&directory),
        telemetry: Arc::clone(&telemetry),
        wall: wall_cell.clone(),
        cfg: cfg.gateway_config(),
    };
    let lobby = wiring::build_lobby(LobbyBuild {
        ctx: &ctx,
        deps,
        http: &http_state,
        stub: opts.stub_lobby,
        use_stub: opts.use_stub_lobby,
        factory: opts.lobby.take(),
        runtime: runtime.handle().clone(),
    });
    let gateway_state = GatewayShared::new(
        cfg.gateway_config(),
        game_tx.clone(),
        Arc::clone(&telemetry),
        Arc::clone(&lobby.port),
        wall_cell.clone(),
        directory,
        opts.dev,
    );
    runtime.spawn(gateway::run_net_event_router(
        net_events_rx,
        Arc::clone(&lobby.port),
    ));
    // the admin console (Plan/25): sessions, capabilities, guards, limits; account events end sessions (§1.8)
    let admin_shared = AdminShared::from_config(
        &cfg,
        SharedInit {
            persist: persist.clone(),
            clock: lobby.core.clock.clone(),
            assets: Some(AssetGate {
                tokens: Arc::clone(&http_state.tokens),
                gated: !http_state.cfg.public_assets,
            }),
            events: lobby.core.account_events.clone(),
        },
    )
    .map_err(|problems| {
        StartupError::Setup(format!("[admin.capabilities]: {}", problems.join("; ")))
    })?;
    admin::account_events::spawn_session_ender(
        runtime.handle(),
        &admin_shared.events,
        Arc::clone(&admin_shared.sessions),
    );
    admin::account_events::spawn_game_forwarder(
        runtime.handle(),
        account_change_rx,
        admin_shared.events.clone(),
    );
    admin::account_events::spawn_poller(
        runtime.handle(),
        persist.clone(),
        Arc::clone(&admin_shared.sessions),
        admin_shared.events.clone(),
        admin_shared.clock.clone(),
        admin::account_events::POLL_EVERY,
    );
    {
        // sessions past their idle timeout or lifetime end even when nobody asks for them
        let sessions = Arc::clone(&admin_shared.sessions);
        runtime.spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(15));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                sessions.sweep();
            }
        });
    }
    if cfg.admin.allow_remote && !http_state.cfg.tls {
        tracing::warn!(
            "admin.allow_remote is on without TLS: admin session cookies are visible on the network; use TLS \
             (a reverse proxy or net.tls) or an SSH tunnel to the loopback (Plan/25 §1.7)"
        );
    }
    let auth = AdminAuth::build(AdminAuthInit {
        allow_remote: cfg.admin.allow_remote,
        owner_loopback: cfg.admin.owner_loopback,
        token: admin_token.clone(),
        limiter: Some(Arc::clone(&gateway_state.ip_limiter)),
        sessions: Arc::clone(&admin_shared.sessions),
        metrics: Arc::clone(&admin_shared.metrics),
    });
    let build_labels = BuildLabels {
        version: build_info::BUILD_ID.to_owned(),
        proto_hash: flyff_proto::PROTO_HASH.to_owned(),
        web_build: web_build.clone(),
    };
    let ops = Arc::new(AdminOps {
        game_tx: game_tx.clone(),
        telemetry: Arc::clone(&telemetry),
        gateway: Arc::clone(&gateway_state),
        core: Arc::clone(&lobby.core),
        accounts: lobby.core.account_admin(),
        coordinator: coordinator.clone(),
        persist: persist.clone(),
        log: Arc::clone(&log),
        http: http_state.clone(),
        audit: Arc::clone(&ctx.audit),
        build: build_labels.clone(),
        version_line: version_line.clone(),
        reload_source: ReloadSource {
            home: home.clone(),
            cli: cli.clone(),
            env: env.clone(),
            overrides: opts.config_overrides.clone(),
            zone_name: zone.name().to_owned(),
        },
        current: parking_lot::Mutex::new(Arc::clone(&cfg)),
        reload_lock: tokio::sync::Mutex::new(()),
        started: t0,
        admin: Arc::clone(&admin_shared),
        auth: Arc::clone(&auth),
    });
    admin::history::spawn(runtime.handle(), Arc::clone(&ops));
    // the GM commands' requests: the dialog scripts of `scripts/npc/`, the file reload (23 §4.6)
    admin::gm_reload::spawn(runtime.handle(), Arc::clone(&ops), server_request_rx);
    // the stored scheduled notices go to the game thread (25 §8.1)
    admin::api::comm::schedule::spawn_loader(runtime.handle(), Arc::clone(&ops));
    let metrics_router = metrics::router(
        Arc::new(MetricsSources {
            telemetry: Arc::clone(&telemetry),
            persist: persist.clone(),
            http: Arc::clone(&http_state.telemetry),
            assets: Arc::clone(&assets),
            gateway: Some(Arc::clone(&gateway_state)),
            lobby: Some(Arc::clone(&lobby.core)),
            admin: Some(Arc::clone(&admin_shared)),
            log: Arc::clone(&log),
            build: build_labels,
            started_unix_s,
            started: t0,
        }),
        &auth,
    );
    let (admin_routes, route_report) =
        admin::api::build_router(&admin::api::ApiState::new(Arc::clone(&ops)));
    if !route_report.rejected.is_empty() {
        tracing::error!(
            routes = ?route_report.rejected,
            "admin routes outside admin::api::CATALOG were not mounted"
        );
    }
    let admin_router = auth.layer(admin_routes);
    let lobby_core = Arc::clone(&lobby.core);
    let gateway_handle = Arc::clone(&gateway_state);
    let router = http::router(
        http_state.clone(),
        Mounts {
            lobby: lobby.routes,
            gateway: Some(gateway::router(gateway_state)),
            metrics: Some(metrics_router),
            admin: Some(admin_router),
        },
    );
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let (done_tx, done_rx) = flume::bounded::<()>(1);
    {
        let _guard = runtime.enter();
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|e| StartupError::Setup(format!("listener: {e}")))?;
        let serve_telemetry = Arc::clone(&http_state.telemetry);
        runtime.spawn(async move {
            http::serve(listener, router, serve_opts, serve_telemetry, async move {
                let _ = stop_rx.changed().await;
            })
            .await;
            let _ = done_tx.send(());
        });
        if opts.handle_signals {
            let coord = coordinator.clone();
            let reload_ops = Arc::clone(&ops);
            runtime.spawn(async move {
                use tokio::signal::unix::{SignalKind, signal};
                let (Ok(mut sigint), Ok(mut sigterm)) = (signal(SignalKind::interrupt()), signal(SignalKind::terminate())) else {
                    tracing::error!("cannot install the signal handlers");
                    return;
                };
                // SIGHUP re-reads flyff.toml like the console `reload` (10 §2 rule 6, §8.6); without the handler
                // SIGINT and SIGTERM still work
                let mut sighup = signal(SignalKind::hangup())
                    .inspect_err(|e| tracing::error!(error = %e, "cannot install the SIGHUP handler: flyff.toml is not reloaded on SIGHUP"))
                    .ok();
                loop {
                    tokio::select! {
                        _ = sigint.recv() => { coord.request(None, None, ShutdownTrigger::Signal(2)); }
                        _ = sigterm.recv() => { coord.request(None, None, ShutdownTrigger::Signal(15)); }
                        _ = async {
                            match sighup.as_mut() {
                                Some(s) => { s.recv().await; }
                                None => std::future::pending::<()>().await,
                            }
                        } => {
                            tracing::info!("SIGHUP: reloading flyff.toml");
                            // the loop never waits for a reload: a storm of signals queues on `reload_lock`
                            let ops = Arc::clone(&reload_ops);
                            tokio::spawn(async move {
                                ops.reload(admin::reload::ReloadBy::Signal).await;
                            });
                        }
                    }
                }
            });
        }
    }
    let console_handle = if opts.console && console::enabled(&cfg.admin) {
        console::spawn(ConsoleCtx {
            ops: Arc::clone(&ops),
            rt: runtime.handle().clone(),
        })
        .ok()
    } else {
        None
    };
    steps.mark(11);

    // The listener is bound: the background prewarm of the derived assets (Plan/24 §7.5, 01 §6.1 step 12). It
    // runs on the niced pool and never delays the first player.
    if cfg.assets.prewarm {
        assets.prep.prewarm();
    } else {
        assets.prep.sweep_in_background();
    }

    // S12: ready.
    telemetry.health.set_state(HealthState::Running);
    telemetry
        .health
        .accepting_logins
        .store(true, Ordering::Relaxed);
    let interfaces = opts
        .interfaces
        .take()
        .unwrap_or_else(exposure::interface_addrs);
    if let Some(w) = exposure::exposure_warning(&cfg, &interfaces) {
        tracing::warn!("{w}");
    }
    steps.mark(12);
    let urls = open_urls(local_addr);
    let total_ms = ms(t0.elapsed());
    let report = StartupReport {
        total_ms,
        steps_ms: steps.steps_ms,
        db_ms: ms(db_elapsed),
        data_ms: ms(data_elapsed),
        assets_start_ms: ms(assets_start),
        derived_ms: ms(derived_elapsed),
    };
    tracing::info!(
        addr = %local_addr,
        elapsed_ms = total_ms,
        data_ms = report.data_ms,
        db_ms = report.db_ms,
        assets_ms = report.assets_start_ms,
        derived_ms = report.derived_ms,
        game_ms = report.steps_ms[8],
        "ready: {}",
        urls.join(" ")
    );
    // `server.start` (Plan/10 §11): the server is ready
    let mut start_data = serde_json::json!({
        "version": build_info::BUILD_ID,
        "user_version": db_user_version,
        "clean": !unclean_previous_run,
        "startup_ms": total_ms,
    });
    if cfg.debug.record_inputs
        && let Some(map) = start_data.as_object_mut()
    {
        // only when the inputs are recorded: a replay needs the seed
        map.insert("seed".to_owned(), serde_json::json!(seed));
    }
    ctx.audit.emit("server.start", &Actor::none(), start_data);
    if opts.print_banner {
        print_banner(&urls, notice.as_ref());
    }

    // a signal that arrived between the last check and the tokio handlers is an ordinary shutdown request
    if handle_signals && let Some(sig) = crate::sys::take_startup_signal() {
        coordinator.request(Some(0), None, ShutdownTrigger::Signal(sig));
    }
    let persistence = db_guard.take();
    let game = game_guard.disarm();
    Ok(ServerHandle {
        local_addr,
        telemetry,
        persist,
        coordinator,
        game_tx,
        wall: wall_cell,
        step_tx,
        game,
        persistence,
        assets: Some(assets),
        http_state,
        runtime: Some(runtime),
        http_stop: Some(stop_tx),
        http_done: Some(done_rx),
        _console: console_handle,
        _log: log,
        home,
        ctx,
        notice,
        startup: report,
        ops,
        lobby_core,
        gateway: gateway_handle,
    })
}

/// Writes the commented default configuration (mode 0640, Plan/10 §3.1 S2).
fn write_default_config(path: &Path) -> Result<(), StartupError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o640)
        .open(path)
        .and_then(|mut f| f.write_all(config::TEMPLATE.as_bytes()))
        .map_err(|e| StartupError::Setup(format!("write {}: {e}", path.display())))
}

fn log_derived(d: &ServerDerived, assets_start: Duration, derived: Duration) {
    let (worlds, lands) = d.collision.as_ref().map_or((0, 0), |c| {
        (c.stats().len(), u64::from(c.total_stats().loaded))
    });
    tracing::info!(
        assets_start_ms = ms(assets_start),
        derived_ms = ms(derived),
        motions = d.motions.is_some(),
        bounds = d.bounds.is_some(),
        collision_worlds = worlds,
        collision_lands = lands,
        "derived assets ready"
    );
}

// CLI output (the only direct stdout writes of the runtime).
#[allow(clippy::print_stdout)]
fn print_banner(urls: &[String], notice: Option<&FirstStartNotice>) {
    if let Some(n) = notice {
        print!("{}", n.text);
    }
    for url in urls {
        println!("Open {url}");
    }
    // 25 §1.3: the banner names the admin console (the host itself is the owner; staff sign in there)
    if let Some(first) = urls.first() {
        println!("Admin console {first}admin");
    }
}
