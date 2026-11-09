//! Hook points of the start-up sequence (Plan/10 §3.1 S8-S11): everything the later tasks install in the
//! running server goes through the functions of this file, each of which defaults to the M1 stubs.
//!
//! | hook | called at | installs | defaults to | owner |
//! |---|---|---|---|---|
//! | [`first_start`] | S8, after the database is open, before the game thread | the first-start administrator (12 §5.4) | `lobby::bootstrap_admin::run_blocking`: account `admin`, one-time password, `logs/first-start.txt` | lobby-auth (done) |
//! | [`build_game_handlers`] | S9, before `spawn_game_thread` | `GameInit.handlers` (session rules, client router, admin handler) | `CharacterRules` (character create/delete + the new-character template of 13 §24) around the M1 stub rules; the movement hooks | lobby-chars, world-flow (wraps its join/leave rules), feature WPs |
//! | [`build_lobby`] | S11, after the HTTP state exists | the `LobbyPort` behind `/ws` and the `/api/*` router | `lobby::Lobby` (Hello, CharList, create, delete; lobby-chars) + the real `/api/bootstrap\|login\|register` over a `LobbyCore` (lobby-auth); `ServerOptions.use_stub_lobby` keeps the gateway tests' `StubLobby`; `EnterWorld` is world-flow's | lobby-auth, lobby-chars, world-flow |
//! | [`run_account_cli`] | `flyff account ...` | the account CLI (12 §5.5) | `cli::account::run` (offline under the lock, or through the admin API) | server-ops (WP-AUTH-19, done) |
//! | [`run_grant_cli`] | `flyff grant ...` | the grant CLI (23 §8.7) | "not available in this build yet" | doc 23 (WP-GM-27) |
//!
//! Each owner changes one line in the function of its hook (or the body of the function); `runtime.rs` is
//! not edited again. [`RuntimeCtx`] carries what the hooks may need and grows by fields, never by renames.

use crate::assets::AssetsState;
use crate::cli::{AccountCmd, GrantArgs};
use crate::config::{Config, ResolvedPaths};
use crate::exit::{self, ExitStatusCode};
use crate::gateway::{LobbyDeps, LobbyPort, StubLobby, StubOptions};
use crate::http::HttpState;
use crate::lobby::online::LobbyClock;
use crate::lobby::{Lobby, LobbyCore, LobbyCoreInit, LobbyInfo, LobbySettings};
use crate::runtime::StartupError;
use axum::Router;
use flyff_assets::ServerDerived;
use flyff_common::rt::Telemetry;
use flyff_data::{ClientData, GameData};
use flyff_db::PersistHandle;
use flyff_game::persist::new_character::{BuildOptions, NewCharacterTemplate};
use flyff_game::session::{CharacterRules, WorldRules};
use flyff_game::{GameInput, Handlers, WallClockCell};
use std::path::PathBuf;
use std::sync::Arc;

/// What the hooks may use (built once after the derived assets exist, S7).
#[derive(Clone)]
pub struct RuntimeCtx {
    /// The runtime home.
    pub home: PathBuf,
    /// The loaded configuration.
    pub cfg: Arc<Config>,
    /// The resolved directories.
    pub paths: ResolvedPaths,
    /// The game data (empty with `DataMode::Empty`).
    pub data: Arc<GameData>,
    /// The client-only data (`None` with `DataMode::Empty`).
    pub client: Option<Arc<ClientData>>,
    /// The server-side derived assets: motion table, model bounds, collision worlds (`ServerDerived`).
    pub derived: ServerDerived,
    /// The persistence handle.
    pub persist: PersistHandle,
    /// Shared counters.
    pub telemetry: Arc<Telemetry>,
    /// C1: the game inbox.
    pub game_tx: flume::Sender<GameInput>,
    /// The wall-clock cell of the game thread and `ServerTime`.
    pub wall: WallClockCell,
    /// The effective server time zone (IANA name).
    pub zone_name: String,
    /// `--dev`.
    pub dev: bool,
    /// `ServerOptions::wander_ai_ms` (tests and bots only).
    pub wander_ai_ms: Option<u64>,
    /// The embedded web client's build id.
    pub web_build: String,
    /// The audit appender of the server layer (`logs/audit/audit-YYYY-MM-DD.jsonl`).
    pub audit: Arc<crate::audit::AuditFile>,
}

/// The notice of the first-start administrator (12 §5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstStartNotice {
    /// The banner text (also in `logs/first-start.txt`).
    pub text: String,
}

// ---- S8: first start --------------------------------------------------------------------------------------

/// The first-start administrator (Plan/12 §5.4, WP-AUTH-05): when `server_kv["admin.bootstrap_done"]` is not set
/// and no account exists, creates account `admin` (authority `P`, `created_via = 'bootstrap'`) with a random 12
/// character password, records the flag in the same batch, writes the notice to `logs/first-start.txt` (mode
/// 0600) and returns it. The password is never logged. The work is `lobby::bootstrap_admin::run_blocking`.
pub fn first_start(
    ctx: &RuntimeCtx,
    _startup: &flyff_db::StartupState,
) -> Result<Option<FirstStartNotice>, StartupError> {
    let port = ctx.cfg.net.bind.rsplit(':').next().unwrap_or("8080");
    let login_url = format!("http://<this-host>:{port}/");
    let notice = crate::lobby::bootstrap_admin::run_blocking(
        &ctx.persist,
        &ctx.cfg.auth.legacy_md5_salt,
        flyff_common::UnixTime(unix_now()),
        &ctx.paths.logs,
        &login_url,
    )
    .map_err(|e| StartupError::Setup(format!("first start: {e}")))?;
    Ok(notice.map(|n| FirstStartNotice { text: n.text }))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

// ---- S9: game handlers ------------------------------------------------------------------------------------

/// doc 22 (task instances): the outermost logout link and the placement predicates around the world rules.
fn with_event_rules(mut rules: WorldRules, iw: &flyff_game::event::InstanceWorldSet) -> WorldRules {
    let inner = std::mem::replace(
        &mut rules.logout,
        Box::new(flyff_game::world::logout::NoLogoutHooks),
    );
    rules.logout = Box::new(flyff_game::event::hooks::EventLogout::with_inner(
        iw.clone(),
        inner,
    ));
    rules.with_policy(Box::new(flyff_game::event::hooks::EventJoinPolicy::new(
        iw.clone(),
    )))
}

/// The plug points of the game thread's input dispatch (`GameInit.handlers`, flyff-game NOTES "Session
/// plumbing"). The M1 stubs: `StubRules`, `DefaultRouter`, `NoAdmin`. The lobby and world tasks replace the
/// parts they implement, for example `Handlers { rules: Box::new(RealRules::new(ctx)), ..Handlers::default() }`.
///
/// lobby-chars (WP-AUTH-07/08, WP-DB-06): `rules` is a [`CharacterRules`] around the M1 stub: the character create and
/// delete of the lobby run on the game thread with the new-character template of 13 §24, which is read and checked
/// here (an override `new_character.toml` of the VFS, else the embedded default). A template that does not resolve
/// against the loaded data refuses the start. World-flow wraps its join and leave rules the same way
/// (`CharacterRules::new(Box::new(its_rules), template)`).
pub fn build_game_handlers(
    ctx: &RuntimeCtx,
    vfs: &flyff_vfs::Vfs,
) -> Result<Handlers, StartupError> {
    let template = NewCharacterTemplate::load(Some(vfs), &ctx.data)
        .map_err(|e| StartupError::Setup(e.to_string()))?;
    // world-flow: the join, the leave rule and `finish_leave` over the real worlds (12 §11-§12, 15 §9, §18); the
    // character rules wrap them. With no world loaded (no game data) the M1 stub join of `WorldRules` applies.
    // doc 16 (task mover-progress): the logout of a dead player pays the death EXP penalty and stores the table HP rate
    // (KQ-09); the other documents' cleanups chain behind it with `StatsLogout::with_inner`
    // doc 17 (task combat-server): a leaving player drops its enemies and its claims
    // doc 21 (task siege-b): a fighter who leaves during the maintenance minute or the fight is taken out of the siege
    // doc 22 (task instances): the dungeon worlds of `PartyDungeon.lua` for the world hooks, the logout object and the
    // join policy, and the channel the world hooks leave their notes on (`Handlers.event_world`)
    let notes = flyff_game::event::hooks::WorldNotes::default();
    let iw = flyff_game::event::InstanceWorldSet::from_data(&ctx.data);
    let world_rules = WorldRules::new().with_logout(Box::new(
        flyff_game::social::siege::fight::SiegeLogout::with_inner(Box::new(
            // doc 21 (task gc11): a fighter who leaves during his 1:1 guild combat round loses it
            flyff_game::social::gc1to1::login::Gc1to1Logout::with_inner(Box::new(
                flyff_game::stats::death_penalty::StatsLogout::with_inner(Box::new(
                    // doc 19 (task econ-trade): the trade, the stall and the vendor chat room of a leaving player end
                    // doc 19 (task pets-eat): the eat pet of a leaving player is dismissed
                    flyff_game::combat::CombatLogout::new().with_inner(Box::new(
                        flyff_game::item::pets::eat::EatPetLogout::with_inner(Box::new(
                            // doc 22 (task event-propevent): a leaving player's five-system stakes are refunded
                            flyff_game::event::minigame::MiniGameLogout::with_inner(Box::new(
                                flyff_game::econ::lifecycle::EconLogout::new(),
                            )),
                        )),
                    )),
                )),
            )),
        )),
    ));
    // doc 22 (task instances): `EventLogout` is THE doc-22 logout object, the OUTERMOST link of the chain above (the
    // siblings' arms live in it); `EventJoinPolicy` answers the placement predicates
    let world_rules = with_event_rules(world_rules, &iw);
    // doc 21 (task party-server): the social part of a character deletion (the party side so far)
    let rules = CharacterRules::new(Box::new(world_rules), template)
        .with_options(BuildOptions::from_rules(&ctx.cfg.rules()))
        .with_hooks(Box::new(flyff_game::social::hooks::SocialDeleteHooks));
    // doc 16 / doc 19: the stat functions' environment, shared by the tick tail of the movers and the pickup of
    // ground items (the world phase has no `Services`)
    let stats_core = Arc::new(flyff_game::stats::StatsCore::new(
        Arc::clone(&ctx.data),
        &ctx.cfg.rules(),
    ));
    let item_core = Arc::new(flyff_game::item::ItemCore::new(
        Arc::clone(&ctx.data),
        Arc::clone(&stats_core),
        &ctx.cfg.rules(),
    ));
    let event_desc = flyff_game::event::DescBoard::default();
    Ok(Handlers {
        rules: Box::new(rules),
        // doc 23: `AdminCmd::RunLine` (the console, `POST /admin/api/console` and the `/admin` page run GM lines through
        // the command registry); the other admin commands are the runtime's own (`flyff_game::admin_ops`)
        admin: Box::new(flyff_game::gm::GmAdmin::default()),
        // the movement pipeline of doc 18 (task movement-server): the shared sim per mover in phase 2, the
        // `PlayerMoveState` / click-to-move handlers read the same tables through `Handlers.movement`
        world: Box::new(
            match ctx.wander_ai_ms {
                Some(ms) => flyff_game::movement::MovementHooks::new()
                    .with_ai(Box::new(flyff_game::ai::WanderAi::new(ms))),
                // doc 18 (tasks ai-core, ai-attacks): the real monster brains (idle/stand/rage, aggro scan, leashes,
                // attacks, runaway, summons, skills); the wandering test AI stays for `wander_ai_ms` (bots, tests)
                None => flyff_game::movement::MovementHooks::new().with_monster_ai(),
            }
            // doc 19 (task pets-eat): the item side of the eat-pet AI (loot rights and the pet filter, the pickup for
            // the owner, the dismissal)
            .with_pets(Box::new(flyff_game::item::pets::eat::EatPetWorld::new(
                Arc::clone(&item_core),
                ctx.audit.log().clone(),
            )))
            // doc 17 (task combat-server): the hit frames, the projectile phase, the claims and the invulnerability
            // counter of the world phase; doc 15 (task ctrls): the arrival of a player at a ctrl runs the open flow
            // (15 §14.3), chained behind it; doc 19 (task items-core): the arrival at a ground item picks it up
            // (19 §6.7), chained last
            .with_consumers(Box::new(
                flyff_game::combat::CombatConsumers::new().with_inner(Box::new(
                    flyff_game::world::ctrl::CtrlArrival::new(
                        Arc::clone(&item_core),
                        ctx.audit.log().clone(),
                    )
                    // doc 20 (task quest-effects): `UA_QUEST` / `UA_QUEST_END` read and advance the quest book
                    .with_quests(Box::new(flyff_game::quest::track::QuestCtrl::new(
                        Arc::clone(&ctx.data),
                    )))
                    .with_inner(Box::new(flyff_game::item::ItemArrival::new(
                        item_core,
                        ctx.audit.log().clone(),
                    ))),
                )),
            ))
            // doc 16 (task mover-stats): the attack counter, regeneration and the per-player 1 s timer run in the
            // tick tail of every awake mover (16 §6.1); doc 22 (task events-core): the 90 s `EventLuaDesc` resend of the
            // same 1 s timer reads the board the game publishes the running events to (`Handlers.event_desc`)
            .with_inner(Box::new(
                // doc 22 (task instances): the instance portal, the dungeon worlds and the layer-change notes; every
                // other world hook goes on to the stats chain behind it
                flyff_game::event::hooks::EventWorldHooks::new(iw.clone(), notes.clone())
                    .with_inner(Box::new(
                // doc 21 (task couple-campus-honor): the 90 s pass also raises the honor check flag
                flyff_game::stats::tick::StatsHooks::new(stats_core).with_second(Box::new(
                    flyff_game::social::honor::HonorSecondHooks::new(Box::new(
                        flyff_game::event::EventSecondHooks::new(event_desc.clone()),
                    )),
                )),
                    )),
            )),
        ),
        event_desc,
        event_world: notes,
        movement: flyff_game::movement::MoveInit {
            motions: ctx.derived.motions.clone(),
            bounds: ctx.derived.bounds.clone(),
            npc_keyframe_hz: ctx.cfg.net.npc_keyframe_hz,
            dev_surfaces: ctx.dev || ctx.cfg.debug.dev_surfaces,
        },
        // ---- bosses (doc 18 §10.4, task boss-ai-core): the rigs the ClockWorks hit handlers read
        boss_poses: ctx.derived.rigs.clone().map(|rigs| {
            Arc::new(BossRigPoses(rigs)) as Arc<dyn flyff_game::ai::boss::pose::BossPoseSource>
        }),
        ..Handlers::default()
    })
}

/// `ServerDerived.rigs` as the pose source of the boss hit handlers (`flyff-game` may not depend on
/// `flyff-assets`): `BossRigs::skeleton` / `BossRigs::clip` already apply the motion table's resolution and the
/// `motion_max -> MTI_STAND` rule.
struct BossRigPoses(Arc<flyff_assets::build::rigs::BossRigs>);

impl flyff_game::ai::boss::pose::BossPoseSource for BossRigPoses {
    fn skeleton(&self, mover: u32) -> Option<&flyff_sim::pose::SimSkeleton> {
        self.0.skeleton(mover)
    }

    fn clip(&self, mover: u32, mti: u32) -> Option<&flyff_sim::pose::SimClip> {
        self.0.clip(mover, mti)
    }
}

// ---- S9: the world catalog -------------------------------------------------------------------------------

/// `GameInit.world` (Plan/15 §3.5): the loaded `GameData` and the world catalog built from the server-derived
/// collision worlds and the `.dyo` files of the VFS (one `WorldStatic` per folder, shared by every id of the
/// folder). Without derived collision (`DataMode::Empty`) the game starts without worlds, as in M1.
pub fn build_world_init(ctx: &RuntimeCtx, vfs: &flyff_vfs::Vfs) -> flyff_game::world::WorldInit {
    let catalog = ctx.derived.collision.as_ref().map(|set| {
        let source = |folder: &str| set.folder(folder).cloned();
        let (catalog, report) = flyff_game::world::WorldCatalog::build(&ctx.data, vfs, &source);
        tracing::info!(
            worlds = report.loaded.len(),
            folders = report.folders.len(),
            skipped = report.skipped.len(),
            static_kib = report.static_bytes / 1024,
            "world catalog built"
        );
        for w in &report.warnings {
            tracing::warn!("world catalog: {w}");
        }
        for (id, why) in &report.skipped {
            tracing::debug!(world = id.0, ?why, "world not loaded");
        }
        if let Err(e) = flyff_game::world::check_c_w01(catalog.world_ids()) {
            tracing::warn!("{e}");
        }
        catalog
    });
    // `layer.inc` (the ticket overflow layers) and `EnvironmentEffect.txt` (the seasons) are read here: the data
    // crate does not parse them (14 §6.12, §6.14 are not implemented there yet)
    let defines = Arc::clone(&ctx.data.defines);
    let ticket_layers = flyff_game::world::TicketLayers::load(vfs, |name| {
        defines
            .value_u32(name)
            .or_else(|| flyff_common::defs::lookup(name).and_then(|v| u32::try_from(v).ok()))
    });
    flyff_game::world::WorldInit {
        data: Some(Arc::clone(&ctx.data)),
        catalog,
        ticket_layers,
        seasons: flyff_game::world::SeasonTable::load(vfs),
        // doc 15 §14.7 (task ctrls): the `FLYK` models the dynamic ctrl colliders pose their ctrls with
        ctrl_models: ctx.derived.ctrl_models.clone(),
    }
}

// ---- S11: lobby and /api ----------------------------------------------------------------------------------

/// What [`build_lobby`] needs besides the context.
pub struct LobbyBuild<'a> {
    /// The runtime context.
    pub ctx: &'a RuntimeCtx,
    /// The gateway pieces the lobby needs (C1 sender, the session directory, telemetry, wall clock, limits).
    pub deps: LobbyDeps,
    /// The HTTP state (for `http::prepare::bootstrap_assets`, the asset token issuer, the rate limiter).
    pub http: &'a HttpState,
    /// Options of the stub lobby (tests).
    pub stub: StubOptions,
    /// Install the stub lobby instead of the real one (tests of the gateway; `ServerOptions.use_stub_lobby`).
    pub use_stub: bool,
    /// A lobby installed through `ServerOptions.lobby` (tests); it wins over the default.
    pub factory: Option<crate::runtime::LobbyFactory>,
    /// The tokio runtime (the lobby's sweeper and drain tasks run on it).
    pub runtime: tokio::runtime::Handle,
}

/// The lobby: the port behind the WebSocket handshake and character list, and the `/api/*` router.
pub struct LobbyParts {
    /// The gateway's lobby.
    pub port: Arc<dyn LobbyPort>,
    /// `/api/bootstrap`, `/api/login`, `/api/register` (mounted as `Mounts.lobby`). `/api/prepare` is the
    /// HTTP layer's own and must not be registered here.
    pub routes: Option<Router>,
    /// The account and session core (online registry, tokens, throttle, `AccountAdmin`): what the WebSocket lobby
    /// of lobby-chars is built on.
    pub core: Arc<LobbyCore>,
}

/// Builds the lobby. The `/api/*` routes are lobby-auth's (`lobby::http::router` over a [`LobbyCore`]: bootstrap,
/// login, registration; the registry sweeper starts here). The gateway's port is lobby-chars' `Lobby` over the same
/// `LobbyCore` (`LobbyParts.core`): `Hello`, the character list, create and delete; `EnterWorld` is world-flow's.
/// `ServerOptions.lobby` (a factory) and `ServerOptions.use_stub_lobby` replace it in tests.
pub fn build_lobby(b: LobbyBuild<'_>) -> LobbyParts {
    let directory = Arc::clone(&b.deps.directory);
    let core = LobbyCore::new(LobbyCoreInit {
        settings: LobbySettings::from_config(&b.ctx.cfg, &b.ctx.zone_name),
        persist: b.ctx.persist.clone(),
        game_tx: b.ctx.game_tx.clone(),
        directory,
        telemetry: Arc::clone(&b.ctx.telemetry),
        clock: LobbyClock::system(),
        asset_tokens: Some(Arc::clone(&b.http.tokens)),
        hasher: None,
        audit: Some(Arc::clone(&b.ctx.audit)),
    });
    core.start_background(&b.runtime);
    let port: Arc<dyn LobbyPort> = match b.factory {
        Some(factory) => factory(b.deps),
        None if b.use_stub => StubLobby::with_options(b.deps, b.stub),
        None => {
            let info = LobbyInfo {
                server_build: crate::build_info::BUILD_ID.to_owned(),
                web_build: b.ctx.web_build.clone(),
                data_version: b.http.assets.bundles.data_version.clone(),
                flush_interval_ms: b.ctx.cfg.net.flush_interval_ms,
                dev: b.ctx.dev,
            };
            Lobby::new(
                Arc::clone(&core),
                b.deps,
                Arc::clone(&b.ctx.data),
                info,
                b.runtime.clone(),
            )
        }
    };
    let routes = crate::lobby::http::router(Arc::clone(&core), b.http.clone());
    LobbyParts {
        port,
        routes: Some(routes),
        core,
    }
}

// ---- CLI hooks ----------------------------------------------------------------------------------------------

/// `flyff account ...` (Plan/12 §5.5): offline under the instance lock, or through the admin API of the running
/// server (`cli::account`, WP-AUTH-19).
pub fn run_account_cli(cli: &crate::cli::Cli, cmd: &AccountCmd) -> ExitStatusCode {
    crate::cli::account::run(cli, cmd)
}

/// `flyff grant ...` (Plan/23 §8.7): the handler is doc 23's (WP-GM-27, M8: `pending_grants` and the delivery
/// of doc 19 do not exist yet), so the command parses its arguments (usage errors are exit 64) and then says so.
#[allow(clippy::print_stderr)]
pub fn run_grant_cli(_cli: &crate::cli::Cli, _args: &GrantArgs) -> ExitStatusCode {
    eprintln!(
        "flyff grant: not available in this build yet (it arrives with the grant delivery of M8, WP-GM-27)"
    );
    exit::FAILURE
}

/// Keeps the asset side reachable from hooks without importing the module (re-export for the owners).
pub type Assets = Arc<AssetsState>;
