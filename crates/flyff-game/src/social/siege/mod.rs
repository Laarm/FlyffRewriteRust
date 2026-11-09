//! The guild siege ("guild combat", F-273, Plan/21 §7; WP-SOCIAL-16): the weekly schedule, the state machine that
//! drives every phase, the bidding and the login state. The line-up, the entry, the fight, the points, the results
//! and the claims are siege-b (WP-SOCIAL-17, -18); they reach this half through [`PhaseHooks`], [`SiegeFacts`] and the
//! accessors below.
//!
//! | module | content |
//! |---|---|
//! | [`schedule`] | the wall job `siege_open` (one `Weekly` spec per `AUTO_OPEN_IDC` line), [`schedule::on_open_time`], [`schedule::open`] |
//! | [`script`] | [`script::ScriptTable`] (the `OPEN` block of `GuildCombat.txt` and its jump targets) and the row interpreter [`script::step`] |
//! | [`bids`] | the ranking, [`bids::request`], [`bids::cancel`], the three C2S handlers and the guild-core seams |
//! | [`login`] | [`login::GuildCombatStep`]: the §7.8 messages of every join batch |
//! | [`lineup`] | the line-up records, the boards, `SelectPlayerGuildCombat`, the entry, the observer (siege-b) |
//! | [`fight`] | [`fight::FightState`], the maintenance, the fighter spawns, the deaths, the logout and the combat seams (siege-b) |
//! | [`result`] | the winner, the chips, the refunds, the result batch, the claims and the close (siege-b) |
//!
//! **One owner.** [`SiegeService`] (`Services.social.siege`) is the only place where the siege state, the war state,
//! the script cursor and the bids live. A state change goes through [`set_state`] / [`set_war_state`], which keep
//! [`SiegeFacts`] (the two numbers the combat code reads, atomics so that it needs no borrow of the service) in step
//! and send `GC_GCSTATE` to everybody. Time is the `TickClock` only (`game_ms` for the script, `unix_ms` for the
//! persisted times and the weekly boundary).
//!
//! **Persistence.** A bid is `AtomicBatch::siege_bid` (the master's penya and the bid row), a cancel is
//! `AtomicBatch::siege_cancel` (the bid row in status 11 and the 80 % refund mail), the opening is
//! `AtomicBatch::siege_open` (the siege row in status 20, the fighting bids, the expiry of old rewards); all three
//! commit in the same block as the memory change and the messages (21 §2.4). The row of the applying siege (the bid
//! rows have a foreign key to it) is created by the first of those batches ([`SiegeService::row_ops`]): a fresh
//! database gets it with the first bid, not in a batch of its own at boot (a deviation, see the crate NOTES).

pub mod bids;
pub mod fight;
pub mod lineup;
pub mod login;
pub mod result;
pub mod schedule;
pub mod script;

use crate::ctx::{GameCtx, Services};
use crate::outbox::Audience;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{CharacterId, GameMs, GuildId, ObjId, UnixTime};
use flyff_data::social::guild::GuildCombatSettings;
use flyff_db::{
    AtomicBatch, BidStatus, DbOp, GuildSiegeBidRecord, GuildSiegePlayerRecord, MetaCounters,
    SiegeLoad, SiegeRankRow,
};
use flyff_proto::pb;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

pub use script::ScriptTable;

// ---- states ----------------------------------------------------------------------------------------------------------

/// `CGuildCombat::m_nState` (`OPEN_STATE` .. `GM_COLSE_STATE`, `eveschool.h:74`), the number of `GC_GCSTATE.state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SiegeState {
    /// 0: opened, the line-up and the entry run.
    Open = 0,
    /// 1: closed, bids are accepted. The state at start.
    #[default]
    Close = 1,
    /// 2: the fight phase of the script.
    War = 2,
    /// 3: the fight is over (the early-end jump target).
    CompleteClose = 3,
    /// 4: closed by a GM (the cancel jump target).
    GmClose = 4,
}

impl SiegeState {
    /// The wire number.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The state of a wire number (`None` outside `0..=4`).
    pub const fn from_code(code: i32) -> Option<SiegeState> {
        match code {
            0 => Some(SiegeState::Open),
            1 => Some(SiegeState::Close),
            2 => Some(SiegeState::War),
            3 => Some(SiegeState::CompleteClose),
            4 => Some(SiegeState::GmClose),
            _ => None,
        }
    }
}

/// `CGuildCombat::m_nGCState` (`NOTENTER_STATE` .. `WAR_TELEPORT_STATE`, `eveschool.h:75`), the number of
/// `GC_GCSTATE.war_state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SiegeWarState {
    /// 100: freshly opened.
    NotEnter = 100,
    /// 101: the line-up time.
    NotEnterCount = 101,
    /// 102: the entry window.
    Enter = 102,
    /// 103: the maintenance minute (the fighters are picked).
    Maintenance = 103,
    /// 104: the fight.
    WarWar = 104,
    /// 105: the fight is closed. The state at start (the C++ constructor).
    #[default]
    WarClose = 105,
    /// 106: the results are computed.
    WarCloseWait = 106,
    /// 107: everybody goes home.
    WarTeleport = 107,
}

impl SiegeWarState {
    /// The wire number.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The war state of a wire number (`None` outside `100..=107`).
    pub const fn from_code(code: i32) -> Option<SiegeWarState> {
        match code {
            100 => Some(SiegeWarState::NotEnter),
            101 => Some(SiegeWarState::NotEnterCount),
            102 => Some(SiegeWarState::Enter),
            103 => Some(SiegeWarState::Maintenance),
            104 => Some(SiegeWarState::WarWar),
            105 => Some(SiegeWarState::WarClose),
            106 => Some(SiegeWarState::WarCloseWait),
            107 => Some(SiegeWarState::WarTeleport),
            _ => None,
        }
    }
}

/// `CGuildCombat::m_nStopWar`: a request to leave the fight early. Ordered: a [`StopWar::GmCancel`] is never
/// downgraded to [`StopWar::Early`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum StopWar {
    /// Nothing requested (0).
    #[default]
    None,
    /// 1: one guild is left with lives; the script jumps to the `STATE 3` row.
    Early,
    /// 2: a GM cancels the siege; the script jumps to the `STATE 4` row (and results are skipped, 200).
    GmCancel,
}

/// The two numbers the combat code reads (`guild_combat_running`, `guild_combat_war_state`, 21 §7.5), kept in step by
/// every state change. Atomics so that a reader needs no borrow of the service.
#[derive(Debug)]
pub struct SiegeFacts {
    state: AtomicI32,
    war_state: AtomicI32,
}

impl Default for SiegeFacts {
    fn default() -> Self {
        SiegeFacts {
            state: AtomicI32::new(SiegeState::Close.code()),
            war_state: AtomicI32::new(SiegeWarState::WarClose.code()),
        }
    }
}

impl SiegeFacts {
    /// The siege state.
    pub fn state(&self) -> SiegeState {
        SiegeState::from_code(self.state.load(Ordering::Relaxed)).unwrap_or_default()
    }

    /// The war state.
    pub fn war_state(&self) -> SiegeWarState {
        SiegeWarState::from_code(self.war_state.load(Ordering::Relaxed)).unwrap_or_default()
    }

    /// `state != Close` (`g_GuildCombatMng.m_nState != CLOSE_STATE`).
    pub fn running(&self) -> bool {
        self.state() != SiegeState::Close
    }

    fn set(&self, state: SiegeState, war: SiegeWarState) {
        self.state.store(state.code(), Ordering::Relaxed);
        self.war_state.store(war.code(), Ordering::Relaxed);
    }
}

// ---- configuration -----------------------------------------------------------------------------------------------------

/// The scalar keys of `GuildCombat.txt` (21 §7.1), `int` like the `CGuildCombat` members. A missing key keeps the
/// value of the C++ constructor (0); the three keys the constructor leaves uninitialised (`MINJOINGUILDSIZE`,
/// `MAXJOINGUILDSIZE`, `MAXMAPTIME`) default to 2 / 15 / 10 with a warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiegeConfig {
    /// `JOINPENYA`: the minimum bid.
    pub join_penya: i32,
    /// `GUILDLEVEL`: the minimum guild level to bid.
    pub guild_level: i32,
    /// `MINJOINGUILDSIZE`: the minimum number of bidders to open / of guilds with fighters to go on.
    pub min_join_guild_size: i32,
    /// `MAXJOINGUILDSIZE`: the admitted guilds (the top bids).
    pub max_join_guild_size: i32,
    /// `MAXPLAYERLIFE`: lives per selected player.
    pub max_player_life: i32,
    /// `MAXJOINMEMBERSIZE`: selected players per guild.
    pub max_join_member_size: i32,
    /// `MAXWARPLAYER`: simultaneous fighters per guild.
    pub max_war_player: i32,
    /// `MAXMAPTIME`: seconds between the spawn-gate window and the teleport.
    pub max_map_time: i32,
    /// `MAXGUILDPERCENT`: the percentage of the prize pool for the winner.
    pub max_guild_percent: i32,
    /// `MAXPLAYERPERCENT`: the percentage for the best player.
    pub max_player_percent: i32,
    /// `REQUESTCANCLEPERCENT`: the refund percentage of a cancelled bid.
    pub request_cancel_percent: i32,
    /// `NOTREQUESTPERCENT`: the refund percentage of a bidder that was not admitted.
    pub not_request_percent: i32,
}

impl Default for SiegeConfig {
    fn default() -> Self {
        SiegeConfig {
            join_penya: 0,
            guild_level: 0,
            min_join_guild_size: 2,
            max_join_guild_size: 15,
            max_player_life: 0,
            max_join_member_size: 0,
            max_war_player: 0,
            max_map_time: 10,
            max_guild_percent: 0,
            max_player_percent: 0,
            request_cancel_percent: 0,
            not_request_percent: 0,
        }
    }
}

impl SiegeConfig {
    /// The configuration of `GuildCombat.txt`.
    pub fn from_settings(s: &GuildCombatSettings) -> SiegeConfig {
        let mut c = SiegeConfig::default();
        // a file that was loaded but lacks one of the three uninitialised keys is worth a warning; game data without any
        // `GuildCombat.txt` (the tests, a stripped resource folder) is not
        let loaded = !s.values.is_empty();
        let int = |key: &str, slot: &mut i32, uninitialised: bool| match s.get(key) {
            Some(v) => *slot = v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            None if uninitialised && loaded => {
                tracing::warn!(
                    key,
                    default = *slot,
                    "GuildCombat.txt has no {key}: the C++ constructor leaves it uninitialised, the default is used"
                );
            }
            None => {}
        };
        int("JOINPENYA", &mut c.join_penya, false);
        int("GUILDLEVEL", &mut c.guild_level, false);
        int("MINJOINGUILDSIZE", &mut c.min_join_guild_size, true);
        int("MAXJOINGUILDSIZE", &mut c.max_join_guild_size, true);
        int("MAXPLAYERLIFE", &mut c.max_player_life, false);
        int("MAXJOINMEMBERSIZE", &mut c.max_join_member_size, false);
        int("MAXWARPLAYER", &mut c.max_war_player, false);
        int("MAXMAPTIME", &mut c.max_map_time, true);
        int("MAXGUILDPERCENT", &mut c.max_guild_percent, false);
        int("MAXPLAYERPERCENT", &mut c.max_player_percent, false);
        int("REQUESTCANCLEPERCENT", &mut c.request_cancel_percent, false);
        int("NOTREQUESTPERCENT", &mut c.not_request_percent, false);
        c
    }
}

// ---- the phase hooks ---------------------------------------------------------------------------------------------------

/// One phase action of siege-b.
pub type PhaseHook = fn(&mut Services, &mut GameCtx<'_>);

/// What the script's `WARSTATE` rows call (`ProcessCommand`, `eveschool.cpp:2866-2900`) and the per-call fighter
/// teleports: [`PhaseHooks::standard`] is siege-b's table (WP-SOCIAL-17, -18), [`PhaseHooks::none`] the script alone.
/// The interpreter never changes.
#[derive(Clone, Copy)]
pub struct PhaseHooks {
    /// `WARSTATE 102` (`ENTER_STATE`): `SetEnter`.
    pub set_enter: PhaseHook,
    /// `WARSTATE 103` (`MAINTENANCE_STATE`): `SetMaintenance`.
    pub set_maintenance: PhaseHook,
    /// `WARSTATE 104` (`WAR_WAR_STATE`): `SetGuildCombatStart`.
    pub start_fight: PhaseHook,
    /// `WARSTATE 105` (`WAR_CLOSE_STATE`): `SetGuildCombatClose`.
    pub close_fight: PhaseHook,
    /// `WARSTATE 106` (`WAR_CLOSE_WAIT_STATE`): `SetGuildCombatCloseWait`, the results.
    pub results: PhaseHook,
    /// `WARSTATE 107` (`WAR_TELEPORT_STATE`): `GuildCombatCloseTeleport`.
    pub close_teleport: PhaseHook,
    /// `ProcessJoinWar`, after every script step: the spawn teleports of the fighters.
    pub join_war: PhaseHook,
}

fn no_hook(_: &mut Services, _: &mut GameCtx<'_>) {}

impl PhaseHooks {
    /// The no-op table (the script engine alone: siege-a's tests run on it).
    pub fn none() -> PhaseHooks {
        PhaseHooks {
            set_enter: no_hook,
            set_maintenance: no_hook,
            start_fight: no_hook,
            close_fight: no_hook,
            results: no_hook,
            close_teleport: no_hook,
            join_war: no_hook,
        }
    }

    /// The table of the game (siege-b, WP-SOCIAL-17, -18): the line-up, the fight and the result.
    pub fn standard() -> PhaseHooks {
        PhaseHooks {
            set_enter: lineup::set_enter,
            set_maintenance: fight::set_maintenance,
            start_fight: fight::start_fight,
            close_fight: fight::close_fight,
            results: result::results,
            close_teleport: result::close_teleport,
            join_war: fight::process_join_war,
        }
    }
}

impl Default for PhaseHooks {
    fn default() -> Self {
        PhaseHooks::standard()
    }
}

impl std::fmt::Debug for PhaseHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PhaseHooks(..)")
    }
}

// ---- the seams of other documents ----------------------------------------------------------------------------------------

/// The questions of other documents the siege asks (plain function pointers with a harmless default).
#[derive(Clone, Copy)]
pub struct SiegeSeams {
    /// The guild is a tenderer of the 1:1 guild combat and so cannot bid (`TID_GAME_GUILDCOMBAT1TO1_ISGC1TO1TENDER`;
    /// 21 §8.2). `false` until WP-SOCIAL-25 installs the real question.
    pub gc1to1_tenderer: fn(&Services, GuildId) -> bool,
}

impl Default for SiegeSeams {
    fn default() -> Self {
        SiegeSeams {
            gc1to1_tenderer: bids::gc1to1_tenderer,
        }
    }
}

impl std::fmt::Debug for SiegeSeams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SiegeSeams(..)")
    }
}

// ---- bids --------------------------------------------------------------------------------------------------------------

/// One guild's entry in the bid list (`__GuildCombatMember`: `dwPenya`, `bRequest`). The list keeps the FIRST-bid
/// order of the siege; an inactive entry (cancelled, or loaded from a status other than 10 / 12 / 20) stays in it so
/// that a re-bid keeps the place of the first application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiegeBidState {
    /// The bidding guild.
    pub guild: GuildId,
    /// The bid in penya; 0 while the entry is inactive (FIX: the original reloaded a cancelled row with its old fee).
    pub fee: u32,
    /// `bRequest`: the entry counts (status 10, 12 or 20).
    pub active: bool,
    /// The status of the stored row.
    pub status: BidStatus,
    /// When the guild FIRST applied (never rewritten by a raise or a re-apply: the tie order of the ranking survives a
    /// restart).
    pub applied_at: UnixTime,
}

impl SiegeBidState {
    /// The row of `UpsertGuildSiegeBid` for this entry in siege `siege_id`.
    pub fn record(&self, siege_id: u32, status: BidStatus) -> GuildSiegeBidRecord {
        GuildSiegeBidRecord {
            siege_id,
            guild_id: self.guild,
            status,
            bid: i64::from(self.fee),
            refund: 0,
            reward: 0,
            points: 0,
            straight_wins: 0,
            applied_at: self.applied_at,
            cancelled_at: None,
            rewarded_at: None,
        }
    }
}

/// One line of the ranking (`__REQUESTGUILD`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankedBid {
    /// The guild.
    pub guild: GuildId,
    /// Its bid.
    pub fee: u32,
}

// ---- the service -------------------------------------------------------------------------------------------------------

/// The siege (21 §7.2): `Services.social.siege`.
#[derive(Debug, Default)]
pub struct SiegeService {
    /// `GuildCombat.txt` keys.
    pub config: SiegeConfig,
    /// The phase actions of siege-b (default: no-ops).
    pub hooks: PhaseHooks,
    /// The questions of other documents (WP-SOCIAL-25 installs the GC 1:1 one).
    pub seams: SiegeSeams,
    script: Arc<ScriptTable>,
    state: SiegeState,
    war_state: SiegeWarState,
    step: usize,
    step_due: GameMs,
    stop_war: StopWar,
    gm_cancelled: bool,
    siege_id: u32,
    siege_opened_at: UnixTime,
    /// The applying siege has no row in the database yet ([`SiegeService::row_ops`]).
    needs_row: bool,
    bids: Vec<SiegeBidState>,
    win_guild: Option<GuildId>,
    win_count: i32,
    best_player: Option<CharacterId>,
    unpaid_bids: Vec<GuildSiegeBidRecord>,
    unpaid_players: Vec<GuildSiegePlayerRecord>,
    player_ranking: Vec<SiegeRankRow>,
    facts: Arc<SiegeFacts>,
    /// The line-ups, the kill log and the death queues of the running siege (siege-b, never persisted).
    pub fight: fight::FightState,
}

impl SiegeService {
    /// The service of `StartupState.sieges` (KQ-39 i, Plan/61 m13). `current` in status 10 or 20 is REUSED as the
    /// applying siege (a 20 stays until [`schedule::open`]); status 30 or no row allocates `counters.next_siege_id`
    /// and asks for the row ([`SiegeService::row_ops`]). Bids with status 10, 12 and 20 load as active with
    /// their stored fee, every other status as inactive with fee 0 (FIX). The winner, his streak and the best player
    /// come from `last_ended` (FIX: not only from rows with a reward). Lives and points of an interrupted event are
    /// lost (m13).
    pub fn load(load: &SiegeLoad, counters: &MetaCounters) -> SiegeService {
        let mut svc = SiegeService::default();
        let reuse = load.current.as_ref().filter(|c| {
            matches!(
                c.status,
                flyff_db::SiegeStatus::Open | flyff_db::SiegeStatus::Started
            )
        });
        match reuse {
            Some(cur) => {
                svc.siege_id = cur.id;
                svc.siege_opened_at = cur.opened_at;
                svc.bids = load
                    .bids
                    .iter()
                    .filter(|b| b.siege_id == cur.id)
                    .map(|b| {
                        let active = b.status.is_active();
                        SiegeBidState {
                            guild: b.guild_id,
                            fee: if active { clamp_fee(b.bid) } else { 0 },
                            active,
                            status: b.status,
                            applied_at: b.applied_at,
                        }
                    })
                    .collect();
            }
            None => {
                let after_current = load.current.as_ref().map_or(1, |c| c.id.saturating_add(1));
                svc.siege_id = counters.next_siege_id.max(after_current).max(1);
                svc.needs_row = true;
            }
        }
        if let Some(last) = &load.last_ended {
            svc.win_guild = last.winner_guild_id;
            svc.win_count = last.winner_streak;
            svc.best_player = last.best_player_id;
        }
        svc.unpaid_bids = load.unpaid_bids.clone();
        svc.unpaid_players = load.unpaid_players.clone();
        svc.player_ranking = load.ranking.clone();
        svc.fight = fight::FightState::seeded(load);
        svc
    }

    /// Installs the configuration and the script of `GuildCombat.txt` (at `Game` construction, before the first tick).
    pub fn configure(&mut self, combat: &GuildCombatSettings) {
        self.config = SiegeConfig::from_settings(combat);
        self.script = Arc::new(ScriptTable::build(combat));
    }

    /// The ops that create the row of the applying siege (`UpsertGuildSiege{id, status 10, opened_at: now}` and
    /// `RaiseCounter{next_siege_id, id + 1}`); empty when the stored row is reused or a batch that carried them
    /// committed. The bid rows have a foreign key to the siege row, so the first batch that writes a bid
    /// ([`bids::request`]) takes the ops at the front of its own list; the batch of [`schedule::open`] creates the row
    /// itself and adds the counter. They are idempotent, so a second batch before the first one is answered may carry
    /// them as well; [`SiegeService::row_created`] ends the offer. A fresh database gets the row with the first
    /// bid instead of a batch of its own at boot (a deviation from the spec: a separate batch makes the probe of every
    /// database test see a batch that is not theirs, depending on a race with the writer thread).
    pub fn row_ops(&mut self, now: UnixTime) -> Vec<DbOp> {
        if !self.needs_row {
            return Vec::new();
        }
        if self.siege_opened_at.0 == 0 {
            self.siege_opened_at = now;
        }
        vec![
            DbOp::UpsertGuildSiege(flyff_db::GuildSiegeRecord::open(
                self.siege_id,
                self.siege_opened_at,
            )),
            self.counter_op(),
        ]
    }

    /// `RaiseCounter{next_siege_id, id + 1}` for the applying siege.
    fn counter_op(&self) -> DbOp {
        DbOp::RaiseCounter {
            key: flyff_db::repo::meta::counter::NEXT_SIEGE_ID,
            at_least: i64::from(self.siege_id) + 1,
        }
    }

    /// `true` while the row of the applying siege is not known to exist in the database.
    pub fn row_pending(&self) -> bool {
        self.needs_row
    }

    /// A batch that carried [`SiegeService::row_ops`] (or created the row itself) committed.
    pub fn row_created(&mut self) {
        self.needs_row = false;
    }

    // ---- read access ---------------------------------------------------------------------------------------------------

    /// The siege state.
    pub fn state(&self) -> SiegeState {
        self.state
    }

    /// The war state.
    pub fn war_state(&self) -> SiegeWarState {
        self.war_state
    }

    /// The facts the combat code reads (shared, always current).
    pub fn facts(&self) -> Arc<SiegeFacts> {
        Arc::clone(&self.facts)
    }

    /// The id of the applying (or running) siege.
    pub fn siege_id(&self) -> u32 {
        self.siege_id
    }

    /// When the row of the applying siege was created (0 when this service did not create it and the stored value is
    /// not known, see [`SiegeService::begin_next_siege`]).
    pub fn opened_at(&self) -> UnixTime {
        self.siege_opened_at
    }

    /// The script row that runs next (`m_nProcessGo`).
    pub fn step(&self) -> usize {
        self.step
    }

    /// When that row is due (`m_dwTime`).
    pub fn step_due(&self) -> GameMs {
        self.step_due
    }

    /// The pending stop request.
    pub fn stop_war(&self) -> StopWar {
        self.stop_war
    }

    /// `m_nStopWar == 200`: a GM cancelled this siege (siege-b's `results` persists nothing then).
    pub fn gm_cancelled(&self) -> bool {
        self.gm_cancelled
    }

    /// The script table.
    pub fn script(&self) -> &Arc<ScriptTable> {
        &self.script
    }

    /// Every entry of the bid list in first-bid order, inactive ones included.
    pub fn bid_entries(&self) -> &[SiegeBidState] {
        &self.bids
    }

    /// The entry of `guild`.
    pub fn entry_of(&self, guild: GuildId) -> Option<&SiegeBidState> {
        self.bids.iter().find(|b| b.guild == guild)
    }

    /// `true` when `guild` has an active bid (`IsRequestWarGuild(guild, TRUE)`).
    pub fn is_bidder(&self, guild: GuildId) -> bool {
        self.entry_of(guild).is_some_and(|b| b.active)
    }

    /// The bid of `guild` (0 without an active one: `GetRequstPenya`).
    pub fn fee_of(&self, guild: GuildId) -> u32 {
        self.entry_of(guild)
            .filter(|b| b.active)
            .map_or(0, |b| b.fee)
    }

    /// `SetRequestRanking`: the active bids, the highest first; equal bids keep the first-bid order (`__S_BUG_GC`).
    pub fn ranking(&self) -> Vec<RankedBid> {
        let mut v: Vec<RankedBid> = self
            .bids
            .iter()
            .filter(|b| b.active)
            .map(|b| RankedBid {
                guild: b.guild,
                fee: b.fee,
            })
            .collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.fee)); // stable
        v
    }

    /// The admitted guilds: the first `MAXJOINGUILDSIZE` of the ranking.
    pub fn admitted(&self) -> Vec<GuildId> {
        let max = usize::try_from(self.config.max_join_guild_size).unwrap_or(0);
        self.ranking()
            .into_iter()
            .take(max)
            .map(|r| r.guild)
            .collect()
    }

    /// All active bidders, admitted or not (the chip formula of siege-b uses it, KQ-39 i).
    pub fn n_bidders(&self) -> usize {
        self.bids.iter().filter(|b| b.active).count()
    }

    /// `GetPrizePenya(2)`: the sum of the first `MAXJOINGUILDSIZE` ranked bids.
    pub fn prize_pool(&self) -> i64 {
        let max = usize::try_from(self.config.max_join_guild_size).unwrap_or(0);
        self.ranking()
            .into_iter()
            .take(max)
            .map(|r| i64::from(r.fee))
            .sum()
    }

    /// The guild that won the last siege (`m_uWinGuildId`).
    pub fn win_guild(&self) -> Option<GuildId> {
        self.win_guild
    }

    /// Its consecutive wins (`m_nWinGuildCount`).
    pub fn win_count(&self) -> i32 {
        self.win_count
    }

    /// The best player of the last siege (`m_uBestPlayer`).
    pub fn best_player(&self) -> Option<CharacterId> {
        self.best_player
    }

    /// The bids of older sieges that can still be claimed (status 30), carried for siege-b.
    pub fn unpaid_bids(&self) -> &[GuildSiegeBidRecord] {
        &self.unpaid_bids
    }

    /// The player rewards of older sieges that can still be claimed, carried for siege-b.
    pub fn unpaid_players(&self) -> &[GuildSiegePlayerRecord] {
        &self.unpaid_players
    }

    /// The cross-siege player ranking (`GC_PLAYERPOINT`), carried for siege-b.
    pub fn player_ranking(&self) -> &[SiegeRankRow] {
        &self.player_ranking
    }

    // ---- mutators (siege-b, gm-social) ---------------------------------------------------------------------------------

    /// Asks the script to leave the fight early (`m_nStopWar`). A [`StopWar::GmCancel`] is never downgraded to
    /// [`StopWar::Early`]; the request is carried out by the next [`script::step`].
    pub fn request_stop(&mut self, stop: StopWar) {
        if stop > self.stop_war {
            self.stop_war = stop;
        }
    }

    /// GM `/gcnext`: the next script row is due at once (`m_dwTime = 0`).
    pub fn skip_timer(&mut self) {
        self.step_due = GameMs(0);
    }

    /// A finished siege forgets its stop request and the cancel mark (`GuildCombatCloseTeleport`: `m_nStopWar = 0`;
    /// siege-b's `close_teleport` calls it).
    pub fn reset_stop(&mut self) {
        self.stop_war = StopWar::None;
        self.gm_cancelled = false;
    }

    /// Stores the outcome of a finished siege (`m_uWinGuildId`, `m_nWinGuildCount`, `m_uBestPlayer`); siege-b calls
    /// it from `results` (the persistence is its `siege_result` batch).
    pub fn set_result(&mut self, winner: Option<GuildId>, streak: i32, best: Option<CharacterId>) {
        self.win_guild = winner;
        self.win_count = streak;
        self.best_player = best;
    }

    /// The next applying siege begins (the `siege_result` batch created its row `id`): the bids of the finished
    /// siege are gone, the carried reward lists are siege-b's to refresh. The creation time of the new row is not
    /// known here ([`SiegeService::opened_at`] is 0 until the siege opens); the stored row keeps its own.
    pub fn begin_next_siege(&mut self, id: u32) {
        self.needs_row = false; // the `siege_result` batch created the row
        self.siege_id = id;
        self.siege_opened_at = UnixTime(0);
        self.bids.clear();
    }

    /// Replaces the carried reward lists (siege-b after a result or a claim).
    pub fn set_unpaid(
        &mut self,
        bids: Vec<GuildSiegeBidRecord>,
        players: Vec<GuildSiegePlayerRecord>,
    ) {
        self.unpaid_bids = bids;
        self.unpaid_players = players;
    }

    /// Replaces the cross-siege ranking (siege-b after a result).
    pub fn set_player_ranking(&mut self, ranking: Vec<SiegeRankRow>) {
        self.player_ranking = ranking;
    }

    // ---- private state moves -------------------------------------------------------------------------------------------

    fn publish(&self) {
        self.facts.set(self.state, self.war_state);
    }
}

/// A stored bid as a fee: penya fit `i32` (the bid is refused above `INT_MAX`); a damaged row is clamped.
fn clamp_fee(bid: i64) -> u32 {
    u32::try_from(bid.clamp(0, i64::from(i32::MAX))).unwrap_or(0)
}

// ---- state changes ---------------------------------------------------------------------------------------------------

/// `GC_GCSTATE{state, war_state}` to everybody online (`AddGuildCombatState`).
pub fn broadcast_state(svc: &Services, ctx: &mut GameCtx<'_>) {
    send_all(
        svc,
        ctx,
        pb::guild_combat::Sub::GcGcState(pb::guild_combat::GcGcState {
            state: svc.social.siege.state.code(),
            war_state: svc.social.siege.war_state.code(),
        }),
    );
}

/// Sets the siege state, keeps [`SiegeFacts`] in step and sends `GC_GCSTATE` to everybody.
pub fn set_state(svc: &mut Services, ctx: &mut GameCtx<'_>, state: SiegeState) {
    svc.social.siege.state = state;
    svc.social.siege.publish();
    broadcast_state(svc, ctx);
}

/// Sets the war state, keeps [`SiegeFacts`] in step and sends `GC_GCSTATE` to everybody.
pub fn set_war_state(svc: &mut Services, ctx: &mut GameCtx<'_>, war: SiegeWarState) {
    svc.social.siege.war_state = war;
    svc.social.siege.publish();
    broadcast_state(svc, ctx);
}

/// `g_GuildCombatMng.m_nState != CLOSE_STATE` for `DeathHooks::gw_open` (the guild-war world is in use).
pub fn gw_open(svc: &Services, _ctx: &GameCtx<'_>) -> bool {
    svc.social.siege.state != SiegeState::Close
}

/// Slot item 4 (1 Hz, gate `events.guild_combat`, `CGuildCombat::Process`): while the siege is not closed one script
/// row at most, then the fighter teleports of siege-b.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if svc.social.siege.state == SiegeState::Close {
        return;
    }
    script::step(svc, ctx);
    let hooks = svc.social.siege.hooks;
    (hooks.join_war)(svc, ctx);
}

/// Everybody in the guild-war world goes to Madrigal (6968, 0, 3328.8), layer 0, forced (`ReplaceWorld`, script row
/// `WARSTATE 101`; siege-b's `close_teleport` calls it too). A missing world or a refused move is skipped.
pub(crate) fn kick_to_madrigal(ctx: &mut GameCtx<'_>) {
    use crate::world::ReplaceKind;
    use flyff_common::defs::world::{WI_WORLD_GUILDWAR, WI_WORLD_MADRIGAL};
    use flyff_common::math::Vec3;
    use flyff_common::{LayerId, WorldId};
    let inside: Vec<ObjId> = match ctx.worlds.world(WorldId(WI_WORLD_GUILDWAR)) {
        Some(w) => w
            .layers
            .values()
            .flat_map(|l| l.members.iter().copied())
            .filter(|o| ctx.worlds.store.is_player(*o))
            .collect(),
        None => return,
    };
    for obj in inside {
        if let Err(e) = ctx.worlds.replace(
            obj,
            WorldId(WI_WORLD_MADRIGAL),
            Vec3::new(6968.0, 0.0, 3328.8),
            LayerId(0),
            ReplaceKind::Force,
        ) {
            tracing::debug!(error = %e, "the siege world was emptied: a move to Madrigal was refused");
        }
    }
}

// ---- persistence -----------------------------------------------------------------------------------------------------

/// Logs the answer of a siege batch: a `Failed` social batch is a bug (21 §2.4, the memory is kept), anything but
/// `Committed` is unexpected.
pub(crate) fn log_batch_result(label: &'static str, result: &TicketResult) {
    match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => tracing::error!(
            batch = label,
            error = %e,
            "CRITICAL: a siege batch failed: the memory is kept (a bug, 21 §2.4)"
        ),
        other => tracing::error!(
            batch = label,
            ?other,
            "a siege batch completed with an unexpected result"
        ),
    }
}

/// Queues `batch` (no snapshot in it) with a ticket whose continuation runs [`log_batch_result`]; `creates_row` marks
/// a batch that carries the row of the applying siege ([`SiegeService::row_ops`]): the service is told when it
/// committed.
pub(crate) fn submit_logged(
    ctx: &mut GameCtx<'_>,
    label: &'static str,
    batch: AtomicBatch,
    creates_row: bool,
) {
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| {
            if creates_row && matches!(result, TicketResult::Committed(_)) {
                svc.social.siege.row_created();
            }
            log_batch_result(label, &result);
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
}

// ---- senders ---------------------------------------------------------------------------------------------------------

/// `GuildCombat{sub}` as a message.
fn gc(sub: pb::guild_combat::Sub) -> pb::GuildCombat {
    pb::GuildCombat { sub: Some(sub) }
}

/// `GuildCombat{sub}` to everybody online, `obj_id` 0.
pub(crate) fn send_all(svc: &Services, ctx: &mut GameCtx<'_>, sub: pb::guild_combat::Sub) {
    ctx.broadcast(Some(svc), ObjId::NULL, Audience::AllOnline, gc(sub));
}

/// `GuildCombat{sub}` to one avatar, `obj_id` 0.
pub(crate) fn send_one(ctx: &mut GameCtx<'_>, obj: ObjId, sub: pb::guild_combat::Sub) {
    crate::social::guild::wire::to_one(ctx, obj, gc(sub));
}

/// `GuildCombat{sub}` to the online members of `guild` in `CharacterId` order, `obj_id` 0.
pub(crate) fn send_guild(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    sub: pb::guild_combat::Sub,
) {
    ctx.broadcast(Some(svc), ObjId::NULL, Audience::Guild(guild), gc(sub));
}
