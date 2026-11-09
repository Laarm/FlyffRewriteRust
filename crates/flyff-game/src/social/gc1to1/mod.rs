//! The 1:1 guild combat ("GC 1:1", F-274, Plan/21 §8; WP-SOCIAL-25, -26; `src:_Common/GuildCombat1to1.cpp`): the
//! tenders, the opening, the line-ups, the arenas (worlds 241..=244), the rounds and the results. The feature is
//! OFF by default (`events.guild_combat_1to1 = false`, PD-14): every handler, hook and slot body returns at once
//! without the flag, and a running event stays frozen while it is off (the flag only gates the slot, the requests, the
//! logout step and the login message).
//!
//! | module | content |
//! |---|---|
//! | [`config`] | [`Gc1to1Config`] (the globals of `GuildCombat1TO1.lua`), the wall job `gc1to1_open`, [`config::on_open_time`] |
//! | [`tender`] | the tender window and list, the tender, the cancel, the refund of a failed tender, [`tender::set_state`], [`tender::open`] |
//! | [`lineup`] | the line-up window and its submission |
//! | [`arena`] | the pairing, the entrance, the rounds, the chips, the close, the deaths and the sweep |
//! | [`login`] | [`login::Gc1to1LoginStep`] and [`login::Gc1to1Logout`] |
//! | [`gm`] | the API of the GM rows `/gc1to1open`, `/gc1to1close`, `/gc1to1next` and `/content gc1to1` |
//!
//! **One owner.** [`Gc1to1Service`] (`Services.social.gc1to1`) holds the manager state, the tender list (only `T`
//! rows, in rank order: penya descending, then the time the CURRENT amount was set, then the guild id), the list of the
//! failed tenders (only `F` rows) and the arenas. The manager state changes only through
//! `Gc1to1Service::set_state`, which keeps [`Gc1to1Facts`] (the one number the combat code reads) in step. Time is the
//! `TickClock` only: every timer is a `due` deadline in game milliseconds (`GameMs(0)` = none).
//!
//! **Persistence.** A tender, a cancel and a refund are ONE `AtomicBatch` each (the master's snapshot and the tender
//! row, VR-018); the opening, the line-up, the round chips and the result are batches of their own, all in the same
//! block as the memory change and the messages (21 §2.4). The arena state is never persisted (21 §16): a restart
//! during an event loses it, the tenders stay `T` with their line-ups for the next opening.

pub mod arena;
pub mod config;
pub mod gm;
pub mod lineup;
pub mod login;
pub mod tender;

pub use config::{GC1TO1_OPEN, Gc1to1Config, on_open_time, register, specs};
pub use gm::{AlreadyOpen, IsClose};

use crate::chat::send_to;
use crate::combat::hooks::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::text::format_positional;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::config::EventId;
use flyff_common::defs::world::WI_WORLD_GUILDWAR1TO1_0;
use flyff_common::{CharacterId, GameMs, GuildId, ObjId, UnixTime, WorldId};
use flyff_db::{AtomicBatch, Gc1to1Load, Gc1to1TenderRecord, MetaCounters, TenderState};
use flyff_proto::pb;
use std::cmp::Reverse;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// The first arena world (`WI_WORLD_GUILDWAR1TO1_0`); the stage `s` plays in `FIRST_ARENA + s`.
pub const FIRST_ARENA: u32 = WI_WORLD_GUILDWAR1TO1_0;
/// The number of arena worlds (241..=244).
pub const ARENA_WORLDS: u32 = 4;
/// The most guilds that can fight at once: two per arena world (the original sent a fifth pair to world 245).
pub const MAX_TENDER_GUILDS: i32 = 2 * ARENA_WORLDS as i32;
/// The most fighters of a guild: the `max_len` of `Gc1to1MemberLineUp.ids`.
pub const MAX_LINEUP_PLAYERS: i32 = 32;

// ---- states ------------------------------------------------------------------------------------------------------------

/// `CGuildCombat1to1Mng::m_nState` (`GC1TO1_CLOSE` .. `GC1TO1_WAR`), the number of `Gc1to1NowState.state` 0..=3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Gc1to1State {
    /// 0: tenders are taken; also the state after the event.
    #[default]
    Close = 0,
    /// 1: the line-up time.
    Open = 1,
    /// 2: the entrance time (the members walk into their arena).
    Entrance = 2,
    /// 3: the arenas fight.
    War = 3,
}

impl Gc1to1State {
    /// The wire number.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The state of a wire number (`None` outside `0..=3`).
    pub const fn from_code(code: i32) -> Option<Gc1to1State> {
        match code {
            0 => Some(Gc1to1State::Close),
            1 => Some(Gc1to1State::Open),
            2 => Some(Gc1to1State::Entrance),
            3 => Some(Gc1to1State::War),
            _ => None,
        }
    }

    /// The plan's enum name (the `/content gc1to1` line).
    pub const fn name(self) -> &'static str {
        match self {
            Gc1to1State::Close => "Close",
            Gc1to1State::Open => "Open",
            Gc1to1State::Entrance => "Entrance",
            Gc1to1State::War => "War",
        }
    }
}

/// `CGuildCombat1to1::m_nState` (`GC1TO1WAR_CLOSE` .. `GC1TO1WAR_CLOSEWAIT`), the number of `Gc1to1NowState.state`
/// 100..=104.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArenaState {
    /// 100: not started, or closed.
    #[default]
    Close = 100,
    /// 101: the wait before a round.
    Wait = 101,
    /// 102: a round is fought.
    War = 102,
    /// 103: a round is decided (handled at the next step).
    Finish = 103,
    /// 104: the wait after the last round.
    CloseWait = 104,
}

impl ArenaState {
    /// The wire number.
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// The result of a round for one side (`GC1TO1_PLAYER_READY` .. `GC1TO1_PLAYER_DRAW`, `Gc1to1WarResult.state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayerResult {
    /// 200.
    Ready = 200,
    /// 201: the round runs.
    War = 201,
    /// 202.
    Win = 202,
    /// 203.
    Lose = 203,
    /// 204: the state of a side before its first round.
    #[default]
    Draw = 204,
}

impl PlayerResult {
    /// The wire number.
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// The one number the combat code reads (`g_GuildCombat1to1Mng.m_nState != GC1TO1_WAR`): an atomic so that a reader
/// needs no borrow of the service.
#[derive(Debug, Default)]
pub struct Gc1to1Facts {
    state: AtomicU8,
}

impl Gc1to1Facts {
    /// The manager state.
    pub fn state(&self) -> Gc1to1State {
        Gc1to1State::from_code(i32::from(self.state.load(Ordering::Relaxed))).unwrap_or_default()
    }

    /// `state == War` (`IsGuildCombatTarget` needs it).
    pub fn war(&self) -> bool {
        self.state() == Gc1to1State::War
    }

    fn set(&self, state: Gc1to1State) {
        self.state.store(state.code() as u8, Ordering::Relaxed);
    }
}

// ---- the records -------------------------------------------------------------------------------------------------------

/// One tendering guild (`__GC1TO1TENDER`): only `T` rows live here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tender {
    /// The guild.
    pub guild: GuildId,
    /// The tender in penya (`nPenya`).
    pub penya: i32,
    /// The round of the stored row.
    pub round: u32,
    /// When the CURRENT amount was set: a new tender, a re-tender after a cancel and every raise write the time. This
    /// equals the original's stable bubble sort (a raised entry ends behind the entries that already had its amount,
    /// a new one behind all equals) and survives a restart.
    pub tendered_at: UnixTime,
    /// The line-up in fight order (`vecMemberId`), empty until submitted.
    pub lineup: Vec<CharacterId>,
    /// The arena stage 0..=3 once the guilds are paired (`nStageId`).
    pub stage: Option<u8>,
}

impl Tender {
    /// The stored row in `state`.
    pub fn record(&self, state: TenderState) -> Gc1to1TenderRecord {
        Gc1to1TenderRecord {
            round: self.round,
            guild_id: self.guild,
            penya: i64::from(self.penya),
            state,
            tendered_at: self.tendered_at,
        }
    }

    /// The rank key: penya descending, then the time the amount was set, then the guild id.
    fn rank_key(&self) -> (Reverse<i32>, UnixTime, GuildId) {
        (Reverse(self.penya), self.tendered_at, self.guild)
    }
}

/// One failed tender (an `F` row): the 98 % refund can be claimed until the next opening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedTender {
    /// The guild.
    pub guild: GuildId,
    /// The tender in penya.
    pub penya: i32,
    /// The round of the stored row.
    pub round: u32,
    /// The stored time of the row (passed on unchanged by the `N` and `G` writes).
    pub tendered_at: UnixTime,
}

impl FailedTender {
    /// The stored row in `state`.
    pub fn record(&self, state: TenderState) -> Gc1to1TenderRecord {
        Gc1to1TenderRecord {
            round: self.round,
            guild_id: self.guild,
            penya: i64::from(self.penya),
            state,
            tendered_at: self.tendered_at,
        }
    }
}

/// One side of an arena.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    /// The guild.
    pub guild: GuildId,
    /// Its line-up in fight order, copied at the pairing.
    pub lineup: Vec<CharacterId>,
    /// The lives left in the running round (`nNowPlayerLife`).
    pub lives: i32,
    /// The rounds won (`nWinCount`).
    pub wins: i32,
    /// The result of the last round (`nLastWinState`).
    pub last: PlayerResult,
}

/// One arena (`CGuildCombat1to1`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arena {
    /// The stage 0..=3.
    pub stage: u8,
    /// The arena world, `241 + stage`.
    pub world: WorldId,
    /// Side 0 is the first tender of the stage in rank order, side 1 the second.
    pub sides: [Side; 2],
    /// The arena state.
    pub state: ArenaState,
    /// The deadline of the running wait or round (`GameMs(0)` = none).
    pub due: GameMs,
    /// The round being fought (`m_nProgWarCount`); the k-th fighters are `lineup[prog]`.
    pub prog: usize,
    /// The number of rounds: the longer line-up (`m_nTotalWarCount`).
    pub total: usize,
    /// `close_wait` ran. An arena is ALSO in `Close` between the pairing and the start, so `Close` alone does not mean
    /// finished.
    pub ended: bool,
}

// ---- the service -------------------------------------------------------------------------------------------------------

/// The 1:1 guild combat (21 §8): `Services.social.gc1to1`.
#[derive(Debug)]
pub struct Gc1to1Service {
    /// The globals of `GuildCombat1TO1.lua` (installed by [`Gc1to1Service::configure`]).
    pub config: Gc1to1Config,
    state: Gc1to1State,
    due: GameMs,
    round: u32,
    tenders: Vec<Tender>,
    failed: Vec<FailedTender>,
    arenas: Vec<Arena>,
    /// The guilds whose row of the current round left `T` / `C` in this process (`E`, `G`, `N`): a new tender of such a
    /// guild takes the next round, a row can only be written forward.
    spent: std::collections::BTreeSet<GuildId>,
    facts: Arc<Gc1to1Facts>,
}

impl Default for Gc1to1Service {
    fn default() -> Self {
        Gc1to1Service {
            config: Gc1to1Config::default(),
            state: Gc1to1State::Close,
            due: GameMs(0),
            round: 1,
            tenders: Vec::new(),
            failed: Vec::new(),
            arenas: Vec::new(),
            spent: std::collections::BTreeSet::new(),
            facts: Arc::new(Gc1to1Facts::default()),
        }
    }
}

/// A stored penya as an `i32` (the tender is refused above `INT_MAX`; a damaged row is clamped).
fn clamp_penya(p: i64) -> i32 {
    i32::try_from(p.clamp(0, i64::from(i32::MAX))).unwrap_or(i32::MAX)
}

impl Gc1to1Service {
    /// The service of `StartupState.gc1to1` (Plan/21 §8, `DbManager.cpp:7873-7932`). The `T` rows become the tender
    /// list in rank order with their line-ups; the `F` rows of ANY round become the failed list (FIX: the original
    /// loaded only the newest round's failures, older unclaimed refunds vanished). The round is the counter, or the
    /// next one when no loaded `T` row has it (a fresh database gives 1). An event that a restart interrupted is lost
    /// (21 §16): the state starts at `Close`, the tenders stay for the next opening.
    pub fn load(load: &Gc1to1Load, counters: &MetaCounters) -> Gc1to1Service {
        let mut svc = Gc1to1Service::default();
        for r in &load.tenders {
            match r.state {
                TenderState::Tendered => {
                    let lineup = load
                        .lineups
                        .iter()
                        .find(|(round, guild, _)| *round == r.round && *guild == r.guild_id)
                        .map(|(_, _, members)| members.clone())
                        .unwrap_or_default();
                    svc.tenders.push(Tender {
                        guild: r.guild_id,
                        penya: clamp_penya(r.penya),
                        round: r.round,
                        tendered_at: r.tendered_at,
                        lineup,
                        stage: None,
                    });
                }
                TenderState::Failed => svc.failed.push(FailedTender {
                    guild: r.guild_id,
                    penya: clamp_penya(r.penya),
                    round: r.round,
                    tendered_at: r.tendered_at,
                }),
                // the load query returns T and F only
                _ => {}
            }
        }
        // stable: equal keys cannot occur (the guild id ends the key)
        svc.tenders.sort_by_key(Tender::rank_key);
        let counter = counters.gc1to1_round;
        svc.round = if svc.tenders.iter().any(|t| t.round == counter) {
            counter
        } else {
            counter.saturating_add(1)
        }
        .max(1);
        svc
    }

    /// Installs the configuration of the script and the override of `gameplay.gc1to1_min_join_player` (at `Game`
    /// construction, before the first tick).
    pub fn configure(&mut self, lua: &flyff_lua::LuaHost, cfg: &crate::config::GameConfig) {
        self.config = Gc1to1Config::read(lua, &crate::event::WarnOnce::default());
        self.config.apply_overrides(cfg);
    }

    /// Re-reads the configuration after a successful reload of the family and re-specs the wall job `gc1to1_open`
    /// (`Game::admin_reload_scripts`). Plain references so that both the `Game` fields and a [`GameCtx`] fit.
    pub fn reconfigure(
        &mut self,
        lua: &flyff_lua::LuaHost,
        cfg: &crate::config::GameConfig,
        sched: &mut crate::sched::Scheduler,
    ) {
        self.configure(lua, cfg);
        sched.respec_wall(GC1TO1_OPEN, specs(&self.config));
    }

    // ---- read access ---------------------------------------------------------------------------------------------------

    /// The manager state.
    pub fn state(&self) -> Gc1to1State {
        self.state
    }

    /// The facts the combat code reads (shared, always current).
    pub fn facts(&self) -> Arc<Gc1to1Facts> {
        Arc::clone(&self.facts)
    }

    /// The milliseconds left of the manager timer (Open and Entrance), 0 when none runs.
    pub fn remaining_ms(&self, now: GameMs) -> i32 {
        remaining(self.due, now)
    }

    /// The round (`m_nCombatId`).
    pub fn round(&self) -> u32 {
        self.round
    }

    /// The tender list in rank order.
    pub fn tenders(&self) -> &[Tender] {
        &self.tenders
    }

    /// The failed tenders.
    pub fn failed(&self) -> &[FailedTender] {
        &self.failed
    }

    /// The arenas (empty outside Entrance and War).
    pub fn arenas(&self) -> &[Arena] {
        &self.arenas
    }

    /// The tender of `guild`.
    pub fn tender_of(&self, guild: GuildId) -> Option<&Tender> {
        self.tenders.iter().find(|t| t.guild == guild)
    }

    /// The failed tender of `guild`.
    pub fn failed_of(&self, guild: GuildId) -> Option<&FailedTender> {
        self.failed.iter().find(|f| f.guild == guild)
    }

    /// The index of the arena that `guild` fights in (`m_vecGuilCombat1to1[nStageId]`).
    pub fn arena_of(&self, guild: GuildId) -> Option<usize> {
        let stage = self.tender_of(guild)?.stage?;
        self.arenas.iter().position(|a| a.stage == stage)
    }

    // ---- state moves ---------------------------------------------------------------------------------------------------

    /// The one way to change the manager state: keeps [`Gc1to1Facts`] in step.
    pub(crate) fn set_state(&mut self, to: Gc1to1State) {
        self.state = to;
        self.facts.set(to);
    }

    /// The round a NEW tender of `guild` is written under: the current round, unless the guild already has a row of it
    /// that can no longer become `T` (a failed tender not yet claimed, or a row that ended, was given or forfeited in
    /// this process): then the next one, so that the batch can never be refused by the guard of the table.
    pub(crate) fn round_for_new_tender(&self, guild: GuildId) -> u32 {
        let clash = self.spent.contains(&guild)
            || self
                .failed
                .iter()
                .any(|f| f.guild == guild && f.round >= self.round);
        if clash {
            self.round.saturating_add(1)
        } else {
            self.round
        }
    }

    /// Puts `tender` into the list at its rank.
    pub(crate) fn insert_ranked(&mut self, tender: Tender) {
        self.tenders.push(tender);
        self.tenders.sort_by_key(Tender::rank_key);
    }

    /// Re-sorts after a raise.
    pub(crate) fn resort(&mut self) {
        self.tenders.sort_by_key(Tender::rank_key);
    }
}

/// `due - now` saturated at 0, as an `i32` (the wire type of a time).
pub(crate) fn remaining(due: GameMs, now: GameMs) -> i32 {
    i32::try_from(due.0.saturating_sub(now.0)).unwrap_or(i32::MAX)
}

// ---- helpers shared by the modules -------------------------------------------------------------------------------------

/// `events.guild_combat_1to1` (every handler, hook and slot body returns at once without it).
pub(crate) fn enabled(ctx: &GameCtx<'_>) -> bool {
    ctx.live.event_state(EventId::GuildCombat1to1) != 0
}

/// The text of `tid` (`prj.GetText`).
pub(crate) fn text_of(ctx: &GameCtx<'_>, tid: u32) -> String {
    ctx.data.texts.get(tid).to_owned()
}

/// The text of `tid` with the printf conversions replaced by `args` in order.
pub(crate) fn text_with(ctx: &GameCtx<'_>, tid: u32, args: &[String]) -> String {
    format_positional(ctx.data.texts.get(tid), args)
}

/// The avatar of `who` as a character.
pub(crate) fn character_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<CharacterId> {
    crate::social::guild::character_of(ctx, who)
}

/// A message to one avatar (`obj_id` 0).
pub(crate) fn send_one(ctx: &mut GameCtx<'_>, obj: ObjId, msg: impl Into<pb::server_msg::Msg>) {
    send_to(ctx, PlayerRef(obj), msg);
}

/// `AddDefinedText( tid, ... )` to one avatar.
pub(crate) fn defined(ctx: &mut GameCtx<'_>, obj: ObjId, tid: u32, args: Vec<String>) {
    crate::chat::ingress::notice::defined(ctx, PlayerRef(obj), tid, args);
}

/// `AddDefinedCaption( TRUE, tid, ... )`: a small caption to one avatar.
pub(crate) fn small_caption(ctx: &mut GameCtx<'_>, obj: ObjId, tid: u32, args: Vec<String>) {
    send_to(
        ctx,
        PlayerRef(obj),
        pb::DefinedText {
            tid: i32::try_from(tid).unwrap_or(i32::MAX),
            args,
            display: pb::defined_text::Display::CaptionSmall as i32,
        },
    );
}

/// `g_DPCoreClient.SendCaption( line )`: a big caption to everybody online.
pub(crate) fn big_caption_all(svc: &Services, ctx: &mut GameCtx<'_>, line: &str) {
    crate::chat::ingress::notice::caption(
        svc,
        ctx,
        crate::outbox::Audience::AllOnline,
        false,
        WorldId(0),
        line,
    );
}

/// The online avatars of the members of `guild` in character order (a lingering or removed avatar excluded).
pub(crate) fn live_members(svc: &Services, ctx: &GameCtx<'_>, guild: GuildId) -> Vec<ObjId> {
    svc.social
        .guild
        .get(guild)
        .map(|g| {
            g.members
                .keys()
                .filter_map(|c| {
                    crate::social::siege::fight::live_avatar(&svc.presence, &*ctx.worlds, *c)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `Gc1to1NowState` of the MANAGER for the member `c` (`CGuildCombat1to1Mng::SendNowState`): Open to a member of
/// a tendering guild `{1, remaining}`; Entrance to a line-up member `{2, remaining, members, enemy}`, else to a
/// master `{3, 0}`; War to a member of a tendering guild `{3, 0}`; Close to a member of a tendering guild `{0, 0}`.
pub(crate) fn manager_state_msg(
    svc: &Services,
    c: CharacterId,
    now: GameMs,
) -> Option<pb::Gc1to1NowState> {
    let g = &svc.social.gc1to1;
    let guild = svc.social.guild.guild_of(c)?;
    let tender = g.tender_of(guild)?;
    let plain = |state: Gc1to1State, time_ms: i32| pb::Gc1to1NowState {
        state: state.code(),
        time_ms,
        members: Vec::new(),
        enemy_guild: 0,
        prog_count: 0,
    };
    match g.state {
        Gc1to1State::Open => Some(plain(Gc1to1State::Open, g.remaining_ms(now))),
        Gc1to1State::Entrance => {
            if tender.lineup.contains(&c) {
                let enemy = g
                    .tenders
                    .iter()
                    .find(|t| t.guild != guild && t.stage == tender.stage && t.stage.is_some())
                    .map_or(0, |t| t.guild.0);
                Some(pb::Gc1to1NowState {
                    state: Gc1to1State::Entrance.code(),
                    time_ms: g.remaining_ms(now),
                    members: tender.lineup.iter().map(|m| m.0).collect(),
                    enemy_guild: enemy,
                    prog_count: 0,
                })
            } else if svc.social.guild.is_master(c) {
                Some(plain(Gc1to1State::War, 0))
            } else {
                None
            }
        }
        Gc1to1State::War => Some(plain(Gc1to1State::War, 0)),
        Gc1to1State::Close => Some(plain(Gc1to1State::Close, 0)),
    }
}

/// Sends [`manager_state_msg`] to the avatar `obj` of the character `c`.
pub(crate) fn send_manager_state(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    obj: ObjId,
    c: CharacterId,
) {
    if let Some(msg) = manager_state_msg(svc, c, ctx.clock.game_ms) {
        send_one(ctx, obj, msg);
    }
}

/// Logs the answer of a GC 1:1 batch: a `Failed` social batch is a bug (21 §2.4, the memory is kept), anything but
/// `Committed` is unexpected.
pub(crate) fn log_batch_result(label: &'static str, result: &TicketResult) {
    match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => tracing::error!(
            batch = label,
            error = %e,
            "CRITICAL: a GC 1:1 batch failed: the memory is kept (a bug, 21 §2.4)"
        ),
        other => tracing::error!(
            batch = label,
            ?other,
            "a GC 1:1 batch completed with an unexpected result"
        ),
    }
}

/// Queues `batch` with a ticket whose continuation runs [`log_batch_result`] (the snapshots of a batch are recorded by
/// the caller).
pub(crate) fn submit_logged(ctx: &mut GameCtx<'_>, label: &'static str, batch: AtomicBatch) {
    submit_then(ctx, label, batch, |_| {});
}

/// [`submit_logged`] with `on_failed`, which runs (after the log line) when the batch failed.
pub(crate) fn submit_then(
    ctx: &mut GameCtx<'_>,
    label: &'static str,
    batch: AtomicBatch,
    on_failed: impl FnOnce(&mut Services) + Send + 'static,
) {
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| {
            log_batch_result(label, &result);
            if matches!(result, TicketResult::Failed(_)) {
                on_failed(svc);
            }
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
}

// ---- the questions of other documents ---------------------------------------------------------------------------------------

/// `GetTenderGuildIndexByUser( .. ) != NULL_ID`: the guild is in the tender list, in any state. Installed as
/// `SiegeSeams::gc1to1_tenderer` (`DPSrvr.cpp:7615-7623`).
pub fn is_tenderer(svc: &Services, guild: GuildId) -> bool {
    svc.social.gc1to1.tender_of(guild).is_some()
}

/// A tenderer may not invite while the event is not closed (21 §5.5, `DPSrvr.cpp:10016-10023`,
/// `TID_GAME_GUILDCOMBAT1TO1_NOT_INVITATION_GUILD`). Installed as `GuildSeams::gc1to1_invite_locked`.
pub fn invite_locked(svc: &Services, guild: GuildId) -> bool {
    is_tenderer(svc, guild) && svc.social.gc1to1.state != Gc1to1State::Close
}

/// FIX of a rule the original enforced only in the client (`WndGuildTabMember.cpp:645-652`): a tenderer may not lose a
/// member by leave or kick while the event is not closed (`TID_GAME_GUILDCOMBAT1TO1_NOTLEAVEGUILD`). Installed as
/// `GuildSeams::gc1to1_leave_locked`.
pub fn leave_locked(svc: &Services, guild: GuildId) -> bool {
    is_tenderer(svc, guild) && svc.social.gc1to1.state != Gc1to1State::Close
}

/// FIX (no GC 1:1 text exists, the siege's is used): a tenderer may not dissolve its guild
/// (`TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD`). Installed as `GuildSeams::gc1to1_dissolve_locked`.
pub fn dissolve_locked(svc: &Services, guild: GuildId) -> bool {
    is_tenderer(svc, guild)
}

/// `g_GuildCombat1to1Mng.IsPossibleUser( pUser )` (`GuildCombat1to1.cpp:912-926`): the avatar's guild is a tender with
/// a stage and the avatar stands in that arena's world. No flag test, as the original.
pub fn is_participant(svc: &Services, ctx: &GameCtx<'_>, obj: ObjId) -> bool {
    let Some(c) = ctx
        .worlds
        .player(obj)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return false;
    };
    let Some(stage) = svc
        .social
        .guild
        .guild_of(c)
        .and_then(|g| svc.social.gc1to1.tender_of(g))
        .and_then(|t| t.stage)
    else {
        return false;
    };
    ctx.worlds
        .core(obj)
        .is_some_and(|core| core.world.0 == FIRST_ARENA + u32::from(stage))
}

/// [`is_participant`] as the `PlayerPredicate` of `DeathHooks::gc_1to1_player`.
pub fn death_participant(svc: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> bool {
    is_participant(svc, ctx, who.0)
}

/// `DeathHooks::world_die` (`DoDie` step 9, after the siege's): see [`arena::on_world_die`].
pub fn on_world_die(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    arena::on_world_die(svc, ctx, k);
}

/// Slot item 5 (1 Hz, gate `events.guild_combat_1to1`, `CGuildCombat1to1Mng::Process`, `ThreadMng.cpp:418-424`): the
/// manager timers and, in the war, every arena; the sweep of stale combat states last.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    match svc.social.gc1to1.state {
        Gc1to1State::Close => return,
        Gc1to1State::Open => {
            if now >= svc.social.gc1to1.due {
                arena::pair(svc, ctx);
                arena::entrance(svc, ctx);
            }
        }
        Gc1to1State::Entrance => {
            if now >= svc.social.gc1to1.due {
                arena::start_war(svc, ctx);
            }
        }
        Gc1to1State::War => arena::step_all(svc, ctx),
    }
    if svc.social.gc1to1.state != Gc1to1State::Close {
        arena::sweep(svc, ctx);
    }
}
