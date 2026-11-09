//! `GameCtx` — the canonical service-call context (Plan/10 §4.1.1, R15).
//!
//! Calling a service stored in `Game.services` with `&mut Game` does not borrow-check. Every feature call
//! therefore takes the split-borrow pair `(&mut Services, &mut GameCtx<'_>)` (or `(&mut self, ctx)` as a
//! method of one service); [`Game::split`] is the only constructor. Players are addressed by
//! [`PlayerRef`], never by a borrowed mover.
//!
//! Several subsystem types are placeholders until their work packages land (marked in their docs): the
//! owner replaces the placeholder type, the field of [`GameCtx`] keeps its name.

use crate::config::GameConfig;
use crate::dispatch::DispatchStats;
use crate::handlers::Handlers;
use crate::input::NetEvent;
use crate::live::LiveConfig;
use crate::mover::Mover;
use crate::outbox::{Audience, GameView, Outbox};
use crate::persist::grants::GrantGuard;
use crate::persist::save::SaveScheduler;
use crate::persist::serial::SerialAllocator;
use crate::presence::PlayerDirectory;
use crate::rng::RngSet;
use crate::sched::Scheduler;
use crate::session::{LeaveTrigger, SessionEntry, SessionTable};
use crate::social::Social;
use crate::tick::{Game, TickClock};
use crate::tickets::TicketRegistry;
use flyff_common::rt::Telemetry;
use flyff_common::{ObjId, SessionId};
use flyff_data::GameData;
use flyff_db::{AuditLog, PersistHandle, StartupState};
use flyff_lua::LuaHost;
use flyff_proto::pb;
use std::sync::Arc;

/// The global services (presence, party, guild, chat, mail, event, gm, …; Plan/01 §5.5). Fields are added
/// by the WPs that own them.
#[derive(Debug, Default)]
pub struct Services {
    /// Every character by id and name, the pending creations and the online references (21 §3.1, WP-SOCIAL-01).
    pub presence: PlayerDirectory,
    /// The social services (21; empty containers until their WPs land).
    pub social: Social,
    /// The item serial source (13 §15.1, WP-DB-06). WP-DB-07's `PersistGlue` may take it over without renaming.
    pub serials: SerialAllocator,
    /// The save generations and the periodic / urgent / world-change snapshot schedule (13 §17, WP-DB-07).
    pub saves: SaveScheduler,
    /// The in-memory guard of the grant poll (13 §18 `GrantDelivery`, WP-DB-07).
    pub grants: GrantGuard,
    /// The chat tables: the profanity filter and the emotes (Plan/23 §6.9, §6.10, WP-GM-05/06).
    pub chat: crate::chat::ingress::ChatService,
    /// The GM command registry and the audit recorder of tests (Plan/23 §3, WP-GM-04).
    pub gm: crate::gm::GmService,
    /// The event services: Event.lua and its cached factors (Plan/22 §2.2; the other services arrive with
    /// their work packages).
    pub event: crate::event::EventServices,
    /// The environment of the stat functions: rule tables, switches, game data (Plan/16 §18, WP-MOVER-05).
    pub stats: crate::stats::StatsService,
    /// The environment of the item functions: game data, the stats environment, the switches (Plan/19 §23).
    pub items: crate::item::ItemService,
    /// The combat seams and the element table (Plan/17 §5, WP-COMBAT-04).
    pub combat: crate::combat::CombatService,
    /// The monster drop seams and the stat-option table (Plan/19 §6, WP-ITEM-06).
    pub drops: crate::item::drop::DropService,
    /// The NPC shops: the tax seam and the built stocks (Plan/19 §7, WP-ECON-01).
    pub econ: crate::econ::EconService,
    /// The NPC dialog state: the quest and social seams, the request counters, the `#auto` timers (Plan/20,
    /// WP-NPC-06..08).
    pub npc: crate::npc::NpcService,
    /// The quest rules over the loaded quest tables: the predicates behind the dialog flows, the index (Plan/20 §9,
    /// WP-NPC-12/13).
    pub quest: crate::quest::QuestService,
    /// The skill engine's data that is not in `GameData`: the guild-quest regions of Blinkpool (Plan/17 §6.14,
    /// WP-SKILL-10).
    pub skill: crate::skill::SkillService,
    /// The item-summoned monsters: the global limit, the keep times and the expiry (Plan/18 §14, WP-AI-16).
    pub summons: crate::ai::create_monster::CreateMonsterRegistry,
    /// The scheduled notices of the admin console, fired by the wall job `admin.notices` (Plan/25 §8.1, WP-ADMIN-27).
    pub notices: crate::admin::actions::comm::NoticeSchedule,
}

impl Services {
    /// The services of a starting game: the directory from `StartupState.directory` (13 §16.9) and the two
    /// initial serial blocks of `GameInit.serial_blocks`.
    pub fn new(startup: &StartupState, serial_blocks: [std::ops::Range<u64>; 2]) -> Self {
        Self {
            presence: PlayerDirectory::load(&startup.directory),
            social: Social::from_startup(startup),
            serials: SerialAllocator::new(serial_blocks),
            saves: SaveScheduler::default(),
            grants: GrantGuard::default(),
            chat: crate::chat::ingress::ChatService::default(),
            gm: crate::gm::GmService::default(),
            event: crate::event::EventServices::default(),
            stats: crate::stats::StatsService::default(),
            items: crate::item::ItemService::default(),
            combat: {
                // 17 §8.7 / 21 §4: the party duels read the membership of the party service
                let mut combat = crate::combat::CombatService::default();
                combat.duel.view = Box::new(crate::social::party::share::PartyDuelAdapter);
                combat
            },
            drops: crate::item::drop::DropService::default(),
            econ: {
                // 19 §16 / 21 §12: the exchange scripts read and pay the campus points of the campus service
                let mut econ = crate::econ::EconService::from_startup(startup);
                econ.exchange.campus = Box::new(crate::social::campus::points::CampusExchange);
                econ
            },
            npc: crate::npc::NpcService::default(),
            quest: crate::quest::QuestService::default(),
            skill: crate::skill::SkillService::default(),
            summons: crate::ai::create_monster::CreateMonsterRegistry::default(),
            notices: crate::admin::actions::comm::NoticeSchedule::default(),
        }
    }
}

pub use crate::world::Worlds;

/// A player avatar, addressed by its `ObjId` (stable for the whole stay in the world, kept across
/// re-attach). Service APIs take this instead of a `CharacterId`, which is for offline players only.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlayerRef(pub ObjId);

/// Everything a feature function may touch besides the services (Plan/10 §4.1.1). All fields are disjoint
/// borrows of [`Game`]; see [`Game::split`].
pub struct GameCtx<'a> {
    /// Tick, game ms, first frame, wall clock and its local breakdown (§4.2).
    pub clock: &'a TickClock,
    /// Every random stream (§4.8).
    pub rng: &'a mut RngSet,
    /// Startup-class configuration.
    pub cfg: &'a GameConfig,
    /// Runtime-class configuration (rates, events).
    pub live: &'a LiveConfig,
    /// The loaded game data (empty in tests and the M1 skeleton).
    pub data: &'a Arc<GameData>,
    /// The worlds (doc 15).
    pub worlds: &'a mut Worlds,
    /// The session table (10 §6.1).
    pub sessions: &'a mut SessionTable,
    /// The fan-out service (10 §6.2).
    pub outbox: &'a mut Outbox,
    /// Persistence commands (the `PersistGlue` of WP-DB-07 replaces the bare handle).
    pub persist: &'a mut PersistHandle,
    /// Timers and wall jobs (§5).
    pub sched: &'a mut Scheduler,
    /// Ticket continuations (§4.7).
    pub tickets: &'a mut TicketRegistry,
    /// The Lua host (doc 20).
    pub lua: &'a mut LuaHost,
    /// Audit events (13 §25).
    pub audit: &'a AuditLog,
    /// Shared counters.
    pub telemetry: &'a Telemetry,
    /// Events to the lobby/gateway (C3); use [`GameCtx::emit`].
    pub net: &'a flume::Sender<NetEvent>,
    /// Dispatch counters (dropped inputs, unhandled messages).
    pub counters: &'a mut DispatchStats,
    /// Leave requests, executed by the runtime in order right after the requesting handler returns
    /// ([`GameCtx::request_leave`]).
    pub leaves: &'a mut Vec<(ObjId, LeaveTrigger)>,
}

impl GameCtx<'_> {
    /// The mover of a live player, resolved through `worlds`; `None` when `p` is not a live player.
    pub fn player(&self, p: PlayerRef) -> Option<&Mover> {
        self.worlds.player(p.0)
    }

    /// Mutable access to the mover of a live player.
    pub fn player_mut(&mut self, p: PlayerRef) -> Option<&mut Mover> {
        self.worlds.player_mut(p.0)
    }

    /// The session currently bound to a player; `None` while the avatar lingers without a socket.
    pub fn session_of(&self, p: PlayerRef) -> Option<SessionId> {
        self.sessions.get(p.0).and_then(|e| e.session)
    }

    /// The session record of a player.
    pub fn entry(&self, p: PlayerRef) -> Option<&SessionEntry> {
        self.sessions.get(p.0)
    }

    /// Resolve + encode once + push (10 §4.1.1, §6.2): builds the read-only [`GameView`] from disjoint fields.
    /// `s` is `None` when the caller is itself a service method (`Party`/`Guild` then resolve to nobody).
    pub fn broadcast(
        &mut self,
        s: Option<&Services>,
        subject: ObjId,
        aud: Audience,
        body: impl Into<pb::server_msg::Msg>,
    ) {
        let view = GameView {
            worlds: &*self.worlds,
            sessions: &*self.sessions,
            services: s,
            rect_xz: self.cfg.rules.fidelity.world_msg_rect_xz,
        };
        self.outbox.broadcast(&view, subject, aud, body);
    }

    /// Sends a `NetEvent` to the lobby/gateway (C3; never blocks, the channel is unbounded). A closed channel
    /// (shutdown) is ignored.
    pub fn emit(&self, ev: NetEvent) {
        let _ = self.net.send(ev);
    }

    /// Asks the runtime to start a leave for `player` once the current handler returned (12 §12.2); requests
    /// are executed in order by [`crate::session::begin_leave`].
    pub fn request_leave(&mut self, player: ObjId, trigger: LeaveTrigger) {
        self.leaves.push((player, trigger));
    }
}

impl Game {
    /// Disjoint field borrows: the services and everything else (the only constructor of [`GameCtx`]).
    ///
    /// Feature code never takes `&mut Game`; it takes the pair this returns. The two shapes of §4.1.1 both
    /// borrow-check through it:
    ///
    /// ```
    /// use flyff_game::{Game, GameCtx, Services};
    ///
    /// /// A service method that touches no other service.
    /// struct Chat;
    /// impl Chat {
    ///     fn say(&mut self, ctx: &mut GameCtx<'_>) -> u32 {
    ///         ctx.rng.service().random_below(10)
    ///     }
    /// }
    /// /// A free function that needs several services.
    /// fn g(_s: &mut Services, ctx: &mut GameCtx<'_>) -> u64 {
    ///     ctx.clock.tick.0
    /// }
    ///
    /// fn call(game: &mut Game, chat: &mut Chat) -> (u32, u64) {
    ///     let (services, mut ctx) = game.split();
    ///     (chat.say(&mut ctx), g(services, &mut ctx))
    /// }
    /// # use flyff_common::rng::GameRng;
    /// # let _ = call;
    /// ```
    pub fn split(&mut self) -> (&mut Services, GameCtx<'_>) {
        let (_handlers, services, ctx) = self.split_all();
        (services, ctx)
    }

    /// [`Game::split`] plus the dispatch plug points (runtime code only: the handlers are what the dispatcher
    /// calls with the pair).
    pub(crate) fn split_all(&mut self) -> (&mut Handlers, &mut Services, GameCtx<'_>) {
        let Game {
            clock,
            rng,
            cfg,
            live,
            services,
            data,
            worlds,
            sessions,
            outbox,
            persist,
            sched,
            tickets,
            lua,
            audit,
            telemetry,
            net_events,
            counters,
            leaves,
            handlers,
            ..
        } = self;
        (
            handlers,
            services,
            GameCtx {
                clock,
                rng,
                cfg,
                live,
                data,
                worlds,
                sessions,
                outbox,
                persist,
                sched,
                tickets,
                lua,
                audit,
                telemetry,
                net: net_events,
                counters,
                leaves,
            },
        )
    }
}
