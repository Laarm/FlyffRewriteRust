//! `event::secret_room`: the secret room (Plan/22 §11, F-286, WP-EVENT-13; `_Common/SecretRoom.h/.cpp`).
//!
//! A guild PvE race per continent side. In the week the guild masters tender penya at the side NPCs ([`tender`]); at
//! the opening time (`Sun 18:00`, the wall job `secret_room_open`) the best [`SecretRoomConfig::max_guild_num`] tenders
//! of each side (East = Flaris, Saint Morning, Ricis; West = Darkon) are ranked and the rest is refunded 98 % by mail;
//! the masters line up their members ([`lineup`]), the members enter the guild's own room (worlds 152..=167) and kill
//! 100 normal monsters, the mid boss and the boss ([`war`]); the first guild to kill the boss wins the side for a week
//! (the tax service: [`TaxService::set_next_guild`](crate::event::tax::TaxService::set_next_guild)); the winner's
//! master chooses the tax rates and the guild may enter the hunting ground (world 151) at the Darkon NPC. The
//! messages a client shows are in [`wire`]; the rules of the rooms (revival, logout, teleports, the eviction at the
//! weekly tax change, the invite lock) in [`rules`]; the GM and admin calls in [`gm`]; the join messages in [`join`].
//!
//! | module | content |
//! |---|---|
//! | [`mod@self`] | [`SecretRoomService`], the state enums, [`SrSide`] / [`SrTender`], [`load`](SecretRoomService::load), [`process`], [`on_open_time`], [`is_room_world`], [`room_side`] |
//! | [`tender`] | the four NPC handlers of the tender phase (menus 84, 85, 103) |
//! | [`lineup`] | the line-up window and its submission (menu 86) |
//! | [`war`] | open, rooms, entrance, presence, spawns, kills, fail, win, close-wait, close, the result batch |
//! | [`rules`] | revival, logout, the teleports, the hunting ground, the eviction, the invite lock |
//! | [`wire`] | the five S2C layouts and the send helpers |
//! | [`gm`] | the GM / admin seam |
//! | [`join`] | `SecretRoomInfoStep` (legacy `GetAllInfo`) |
//!
//! **Time.** Every duration is a `ctx.clock.game_ms` deadline; a deadline has passed when `now > deadline` (strict,
//! the legacy `m_dwRemainTime < dwTick`); the `remain_ms` on the wire is `deadline - now` saturated at 0 (FIX of the
//! `DWORD` underflow of the legacy at War and Close, invisible: the client ignores the time there).
//!
//! **Persistence.** A tender is ONE `AtomicBatch` with the master's snapshot (the penya leaves the wallet in the same
//! commit block); the line-up is a single op; the drop-outs of the open and the whole result of the war are ONE
//! `secret_room_result` batch each. Nothing of a running war is written (KEEP 13 §11): after a crash every ranked
//! tender reloads as `T` with its line-up and the war is void.

pub mod gm;
pub mod join;
pub mod lineup;
pub mod rules;
pub mod tender;
pub mod war;
pub mod wire;

pub use join::SecretRoomInfoStep;
pub use war::on_world_die;

use crate::ctx::{GameCtx, Services};
use crate::event::scripts::{SecretRoomConfig, SrMonster, SrMonsterKind};
use crate::tick::LocalStamp;
use crate::world::continent::{CONT_EAST, CONT_NODATA, CONT_WEST};
use flyff_common::config::EventId;
use flyff_common::defs::world::{WI_DUNGEON_SECRET_0, WI_DUNGEON_SECRET_1, WI_DUNGEON_SECRET_L};
use flyff_common::{CharacterId, GameMs, GuildId, UnixTime, WorldId};
use flyff_data::types::MoverId;
use flyff_db::{SecretRoomLoad, SecretRoomTenderRecord, SecretRoomTenderState, TaxContinent};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};

/// The first room world (`WI_DUNGEON_SECRET_1`).
pub const ROOM_FIRST: u32 = WI_DUNGEON_SECRET_1;
/// The last room world (`WI_DUNGEON_SECRET_L`).
pub const ROOM_LAST: u32 = WI_DUNGEON_SECRET_L;
/// Rooms per side: the East rooms are 152..=159, the West rooms 160..=167 (the legacy stride was `MaxGuildNum`, 8 in
/// the shipped script; a fixed 8 here, open question 7 of the task).
pub const ROOMS_PER_SIDE: u32 = 8;
/// The hunting ground of the occupying guild (`WI_DUNGEON_SECRET_0`).
pub const HUNTING_GROUND: WorldId = WorldId(WI_DUNGEON_SECRET_0);

/// `CSecretRoomMng::IsInTheSecretRoom`: the worlds 152..=167 and no others (the hunting ground 151 is not a room).
pub fn is_room_world(world: WorldId) -> bool {
    (ROOM_FIRST..=ROOM_LAST).contains(&world.0)
}

/// The side a room belongs to: East for 152..=159, West for 160..=167, `CONT_NODATA` for every other world.
pub fn room_side(world: WorldId) -> u8 {
    if !is_room_world(world) {
        CONT_NODATA
    } else if world.0 < ROOM_FIRST + ROOMS_PER_SIDE {
        CONT_EAST
    } else {
        CONT_WEST
    }
}

/// `WI_DUNGEON_SECRET_1 + rank + 8 * k` (`k` = 0 East, 1 West): the room of the tender at `rank` (0-based) of the
/// side at map position `k`; `None` for a rank of 8 or more (the legacy configuration allowed more guilds only with a
/// larger `MaxGuildNum`; such a guild has no room and fails at the first presence check).
pub fn room_for(rank: usize, k: usize) -> Option<WorldId> {
    let rank = u32::try_from(rank).ok().filter(|r| *r < ROOMS_PER_SIDE)?;
    let k = u32::try_from(k).ok()?;
    let world = ROOM_FIRST + rank + ROOMS_PER_SIDE * k;
    (world <= ROOM_LAST).then_some(WorldId(world))
}

// ---- the states ----------------------------------------------------------------------------------------------------------

/// The manager states (`SRMNG_*`, `SecretRoom.h:16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum SrMngState {
    /// 1: nothing runs; the tender phase.
    #[default]
    Close = 1,
    /// 2: the sides are open (one second, then the line-up).
    Open = 2,
    /// 3: the masters line up their members.
    LineUp = 3,
    /// 4: the entrance (`EntranceTime`, 0 in the shipped script).
    Entrance = 4,
    /// 5: the wait before the war.
    WarWait = 5,
    /// 6: the war; every side runs its own state.
    War = 6,
}

impl SrMngState {
    /// The wire value of `SecretRoomMngState.state`.
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// The side states (`SRCONT_*`, `SecretRoom.h:17`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum SrSideState {
    /// 101: not running.
    #[default]
    Close = 101,
    /// 102: opened at the opening time, waiting for the war.
    Open = 102,
    /// 103: the war.
    War = 103,
    /// 104: a winner or the end of the time: everybody waits to be sent out.
    CloseWait = 104,
}

impl SrSideState {
    /// The wire value of `SecretRoomMngState.state`.
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// The war state of a tender (`MONSTER_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum SrWarState {
    /// 0: the guild lost (master dead or absent).
    Failed = 0,
    /// 1: the normal monsters.
    #[default]
    Normal = 1,
    /// 2: the mid boss.
    MidBoss = 2,
    /// 3: the boss.
    Boss = 3,
    /// 4: the guild won.
    Win = 4,
}

impl SrWarState {
    /// The wire / stored value.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The monster class the state fights, `None` for `Failed` and `Win`.
    pub const fn kind(self) -> Option<SrMonsterKind> {
        match self {
            SrWarState::Normal => Some(SrMonsterKind::Normal),
            SrWarState::MidBoss => Some(SrMonsterKind::MidBoss),
            SrWarState::Boss => Some(SrMonsterKind::Boss),
            SrWarState::Failed | SrWarState::Win => None,
        }
    }
}

// ---- the records ---------------------------------------------------------------------------------------------------------

/// One group of the queue of a tender: `AddMonster(type, id, num, red, x1, z1, x2, z2, y)` with the count still to
/// spawn (`__SECRETROOM_MONSTER`). A guild owns its queue (PL-102).
#[derive(Clone, Debug, PartialEq)]
pub struct SrSpawn {
    /// The mover prop (`NULL_ID` for an unknown name: nothing is created, the count still runs down).
    pub mover: MoverId,
    /// How many are still to spawn.
    pub num: i32,
    /// `bRed`: aggressive.
    pub red: bool,
    /// The corners `[x1, z1, x2, z2]` of the spawn rectangle (in any order).
    pub rect: [i32; 4],
    /// The height.
    pub y: i32,
}

impl SrSpawn {
    /// The queue entry of a script row.
    pub fn of(m: &SrMonster) -> SrSpawn {
        SrSpawn {
            mover: m.mover,
            num: m.num,
            red: m.red,
            rect: [m.x1, m.z1, m.x2, m.z2],
            y: m.y,
        }
    }
}

/// A tendering guild (`__SECRETROOM_TENDER`).
#[derive(Clone, Debug, PartialEq)]
pub struct SrTender {
    /// The guild.
    pub guild: GuildId,
    /// The round of the guild's tender row (the open of a side with fewer than `MinGuildNum` tenders carries the row
    /// over to a later round).
    pub round: u32,
    /// The tender in penya.
    pub penya: i32,
    /// The kills in the current war state.
    pub kills: i32,
    /// The room world (`152 + rank + 8 k`), assigned at the entrance.
    pub room: Option<WorldId>,
    /// The war state.
    pub war: SrWarState,
    /// The line-up (character ids, in the order of the submission).
    pub lineup: Vec<CharacterId>,
    /// The groups still to spawn in this guild's room.
    pub pending: VecDeque<SrSpawn>,
    /// When the guild tendered or raised (the order of equal tenders after a restart).
    pub tendered_at: UnixTime,
}

impl SrTender {
    /// A new tender of `guild`.
    pub fn new(guild: GuildId, round: u32, penya: i32, tendered_at: UnixTime) -> SrTender {
        SrTender {
            guild,
            round,
            penya,
            kills: 0,
            room: None,
            war: SrWarState::Normal,
            lineup: Vec::new(),
            pending: VecDeque::new(),
            tendered_at,
        }
    }

    /// `true` when `c` is in the line-up.
    pub fn has_member(&self, c: CharacterId) -> bool {
        self.lineup.contains(&c)
    }

    /// The stored row of the tender in `state`.
    pub fn record(
        &self,
        side: u8,
        state: SecretRoomTenderState,
        round: u32,
    ) -> SecretRoomTenderRecord {
        SecretRoomTenderRecord {
            round,
            continent: tax_continent(side),
            guild_id: self.guild,
            penya: i64::from(self.penya),
            state,
            world_id: self.room.unwrap_or(WorldId(0)),
            war_state: self.war.code(),
            kills: self.kills,
            tendered_at: self.tendered_at,
        }
    }
}

/// One side of the secret room (`CSecretRoomContinent`).
#[derive(Clone, Debug, PartialEq)]
pub struct SrSide {
    /// The side state.
    pub state: SrSideState,
    /// When the side leaves War or CloseWait.
    pub deadline: Option<GameMs>,
    /// The guilds that failed in this run (`m_nFailedGuild`).
    pub failed: u32,
    /// The tenders in rank order (penya descending, stable).
    pub tenders: Vec<SrTender>,
}

impl Default for SrSide {
    fn default() -> SrSide {
        SrSide {
            state: SrSideState::Close,
            deadline: None,
            failed: 0,
            tenders: Vec::new(),
        }
    }
}

impl SrSide {
    /// `GetTenderGuild`: the rank index (0-based) of `guild`.
    pub fn index_of(&self, guild: GuildId) -> Option<usize> {
        self.tenders.iter().position(|t| t.guild == guild)
    }

    /// The tender of `guild`.
    pub fn tender(&self, guild: GuildId) -> Option<&SrTender> {
        self.tenders.iter().find(|t| t.guild == guild)
    }

    /// `SortTenderGuild`: penya descending; the legacy bubble sort with `<` is stable, so a new tender stands behind its
    /// equals and a raised one moves only past strictly lower bids.
    pub fn sort(&mut self) {
        self.tenders.sort_by_key(|t| std::cmp::Reverse(t.penya));
    }

    /// `true` when `c` is in the line-up of the tender of `guild`.
    pub fn is_lineup_member(&self, guild: GuildId, c: CharacterId) -> bool {
        self.tender(guild).is_some_and(|t| t.has_member(c))
    }
}

/// The stored side of a map key.
pub fn tax_continent(side: u8) -> TaxContinent {
    if side == CONT_WEST {
        TaxContinent::West
    } else {
        TaxContinent::East
    }
}

/// The map key of a stored side.
pub fn side_of(c: TaxContinent) -> Option<u8> {
    match c {
        TaxContinent::East => Some(CONT_EAST),
        TaxContinent::West => Some(CONT_WEST),
        TaxContinent::All => None,
    }
}

// ---- the service ---------------------------------------------------------------------------------------------------------

/// The secret-room manager (`CSecretRoomMng`, `Services.event.secret_room`).
#[derive(Debug)]
pub struct SecretRoomService {
    /// The script numbers of the run in force, a COPY taken at the open: a `/lua` reload during a run applies from
    /// the next open. While the manager is `Close` every check reads `Services.event.configs.secret_room`
    /// ([`cfg_of`]).
    cfg: SecretRoomConfig,
    state: SrMngState,
    deadline: Option<GameMs>,
    /// `meta.secret_room_round`: the round the tenders of the week belong to.
    round: u32,
    /// The two sides, keyed by `CONT_EAST` 15 and `CONT_WEST` 240: map order is East first (`k` 0), West second.
    sides: BTreeMap<u8, SrSide>,
    /// The rows of the tenders of a finished war, written by the ONE batch of the manager close.
    pending_result: Vec<SecretRoomTenderRecord>,
    /// The sides whose winner went to `set_next_guild` in this run: their tax rows go into the close batch.
    pending_tax_sides: Vec<u8>,
}

impl Default for SecretRoomService {
    fn default() -> SecretRoomService {
        SecretRoomService {
            cfg: SecretRoomConfig::default(),
            state: SrMngState::Close,
            deadline: None,
            round: 0,
            sides: BTreeMap::from([
                (CONT_EAST, SrSide::default()),
                (CONT_WEST, SrSide::default()),
            ]),
            pending_result: Vec::new(),
            pending_tax_sides: Vec::new(),
        }
    }
}

impl SecretRoomService {
    /// The service from the start-up state (`StartupState.secret_room`): the `T` tenders of the round with their
    /// line-ups, every side and the manager `Close`. Tenders are loaded whatever flag 24 says (FIX: the legacy dropped
    /// them when the flag was off at the start). A running war never survives a restart (KEEP 13 §11).
    pub fn load(load: &SecretRoomLoad) -> SecretRoomService {
        let mut s = SecretRoomService {
            round: load.round,
            ..SecretRoomService::default()
        };
        for row in &load.tenders {
            if row.state != SecretRoomTenderState::Tendered {
                continue;
            }
            let Some(side) = side_of(row.continent).and_then(|k| s.sides.get_mut(&k)) else {
                continue;
            };
            if side.index_of(row.guild_id).is_some() {
                continue;
            }
            let mut t = SrTender::new(
                row.guild_id,
                row.round,
                i32::try_from(row.penya).unwrap_or(i32::MAX),
                row.tendered_at,
            );
            if let Some(l) = load
                .lineups
                .iter()
                .find(|l| l.continent == row.continent && l.guild_id == row.guild_id)
            {
                t.lineup = l.members.clone();
            }
            side.tenders.push(t);
        }
        for side in s.sides.values_mut() {
            side.sort();
        }
        s
    }

    /// The manager state.
    pub fn state(&self) -> SrMngState {
        self.state
    }

    /// The round of the tenders of the week.
    pub fn round(&self) -> u32 {
        self.round
    }

    /// The sides, East first.
    pub fn sides(&self) -> &BTreeMap<u8, SrSide> {
        &self.sides
    }

    /// The side `cont` (`CONT_EAST` or `CONT_WEST`).
    pub fn side(&self, cont: u8) -> Option<&SrSide> {
        self.sides.get(&cont)
    }

    /// The script numbers of the run in force (the copy taken at the open; the defaults before the first open).
    pub fn cfg(&self) -> &SecretRoomConfig {
        &self.cfg
    }

    /// The manager deadline.
    pub fn deadline(&self) -> Option<GameMs> {
        self.deadline
    }

    /// `(side, tender)` of `guild` on either side.
    pub fn tender_of(&self, guild: GuildId) -> Option<(u8, &SrTender)> {
        self.sides
            .iter()
            .find_map(|(k, s)| s.tender(guild).map(|t| (*k, t)))
    }

    /// `true` when `guild` has a tender on either side.
    pub fn has_tender(&self, guild: GuildId) -> bool {
        self.tender_of(guild).is_some()
    }

    /// The side whose line-up of `guild` holds `c` (`GetRevivalPos`, `IsLineUpMember`).
    pub fn lineup_side(&self, guild: GuildId, c: CharacterId) -> Option<u8> {
        self.sides
            .iter()
            .find_map(|(k, s)| s.is_lineup_member(guild, c).then_some(*k))
    }

    /// The map position `k` of a side (0 East, 1 West).
    pub fn position_of(&self, cont: u8) -> usize {
        self.sides.keys().position(|k| *k == cont).unwrap_or(0)
    }

    /// The rows of finished tenders waiting for the close batch.
    pub fn pending_result(&self) -> &[SecretRoomTenderRecord] {
        &self.pending_result
    }
}

/// The script numbers in force: the copy of a running event, else the live config (`/lua` reloads apply from the
/// next open).
pub fn cfg_of(s: &Services) -> &SecretRoomConfig {
    if s.event.secret_room.state == SrMngState::Close {
        &s.event.configs.secret_room
    } else {
        &s.event.secret_room.cfg
    }
}

// ---- time ----------------------------------------------------------------------------------------------------------------

/// `now + ms`.
pub fn deadline_in(now: GameMs, ms: u32) -> GameMs {
    GameMs(now.0.saturating_add(u64::from(ms)))
}

/// `true` when the deadline exists and `now > deadline` (strict, the legacy `m_dwRemainTime < dwTick`).
pub fn passed(deadline: Option<GameMs>, now: GameMs) -> bool {
    deadline.is_some_and(|d| now.0 > d.0)
}

/// The milliseconds left to `deadline` saturated at 0 (0 without one).
pub fn remain_ms(deadline: Option<GameMs>, now: GameMs) -> u32 {
    deadline.map_or(0, |d| {
        u32::try_from(d.0.saturating_sub(now.0)).unwrap_or(u32::MAX)
    })
}

/// `true` when `events.secret_room` (flag 24) is on.
pub fn enabled(ctx: &GameCtx<'_>) -> bool {
    ctx.live.event_state(EventId::SecretRoom) != 0
}

// ---- the entry points of the scheduler ----------------------------------------------------------------------------------

/// 1 s slot item 6 `event::secret_room::process` (22 §2.3, §11.3), gated on `events.secret_room`: the manager state
/// machine of `CSecretRoomMng::Process`. `Close` does nothing here (the wall job opens); `Open` goes to `LineUp` at
/// the next second; `LineUp`, `Entrance` and `WarWait` leave when their deadline has passed; `War` runs the sides.
pub fn process(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let sr = &s.event.secret_room;
    match sr.state {
        SrMngState::Close => {}
        SrMngState::Open => war::begin_line_up(s, ctx, now),
        SrMngState::LineUp => {
            if passed(sr.deadline, now) {
                war::begin_entrance(s, ctx, now);
            }
        }
        SrMngState::Entrance => {
            if passed(sr.deadline, now) {
                war::begin_war_wait(s, ctx, now);
            }
        }
        SrMngState::WarWait => {
            if passed(sr.deadline, now) {
                war::begin_war(s, ctx);
            }
        }
        SrMngState::War => war::process_war(s, ctx, now),
    }
}

/// Wall job `secret_room_open` (22 §2.4, `Weekly{0,18,0}`, `Skip`): opens the manager when flag 24 is on and it is
/// `Close` ([`war::open`] checks both).
pub fn on_open_time(s: &mut Services, ctx: &mut GameCtx<'_>, _at: &LocalStamp) {
    let _ = war::open(s, ctx);
}

// ---- the admin view ------------------------------------------------------------------------------------------------------

impl SecretRoomService {
    /// The admin view of phase C (WP-ADMIN-31): the manager, the sides and their tenders as JSON.
    pub fn to_json(&self, now: GameMs) -> Value {
        let sides: Vec<Value> = self
            .sides
            .iter()
            .map(|(k, side)| {
                json!({
                    "continent": k,
                    "state": format!("{:?}", side.state),
                    "remain_ms": remain_ms(side.deadline, now),
                    "failed": side.failed,
                    "tenders": side.tenders.iter().map(|t| json!({
                        "guild": t.guild.0,
                        "penya": t.penya,
                        "war": format!("{:?}", t.war),
                        "kills": t.kills,
                        "room": t.room.map(|r| r.0),
                        "lineup": t.lineup.iter().map(|c| c.0).collect::<Vec<_>>(),
                        "pending": t.pending.iter().map(|p| p.num).sum::<i32>(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({
            "state": format!("{:?}", self.state),
            "remain_ms": remain_ms(self.deadline, now),
            "round": self.round,
            "pending_result": self.pending_result.len(),
            "sides": sides,
        })
    }
}
