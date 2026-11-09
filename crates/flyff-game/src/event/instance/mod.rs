//! `event::instance`: the instance (party) dungeons (Plan/22 §8, WP-EVENT-08; ref/15 §16;
//! `_Common/InstanceDungeonBase.cpp`, `InstanceDungeonParty.cpp`).
//!
//! Eight worlds of `PartyDungeon.lua` (205, 124, 220, 206, 125, 121, 122, 123) are instance dungeons: a party that
//! walks into an entrance portal lands in a private layer of the dungeon world (`layer = party id`), the layer is
//! created on the first entry and invalidated when the last member leaves. All the state is memory-only (KQ-21): a
//! restart forgets every instance, cooldown and kick timer.
//!
//! | file | content |
//! |---|---|
//! | `mod.rs` | [`InstanceWorldSet`], [`DungeonState`], [`InstanceDungeonService`] and its read-only queries |
//! | `entry.rs` | [`on_portal`] (`TeleportToDungeon`: the refusals, the cooldown, the marking, the jitter), the creation of a layer and its monsters, the push-back of a refusal |
//! | `progress.rs` | [`on_npc_killed`] (`SetInstanceDungeonKill`, `SetNextState`: MIDBOSS, BOSS, cleared) |
//! | `leave.rs` | [`on_replace`] / [`on_logout`] (`LeaveToOutside`), the destroy, [`on_party_left`] and the 3-minute kick, [`go_out`], [`process`] (1 s slot item 8) |
//!
//! The world phase has no `Services`: the portal trigger and the replace hook are noted by
//! [`EventWorldHooks`](super::hooks::EventWorldHooks) and handled by
//! [`drain_world_notes`](super::hooks::drain_world_notes) right after the world phase of the same tick, so the entry
//! teleport and the push-back are requested at the drain and executed by the NEXT tick's `_replace` (one tick,
//! 66.7 ms, later than the original's same-tick `REPLACE`; DEVIATION, open question 1 of the spec).
//!
//! # How to use a layer (the recipe for guild-house, housing, secret-room and rainbow-race)
//!
//! 1. create it: `ctx.worlds.create_layer(world, LayerId(id), LayerKind::{GuildHouse, MiniRoom}, ctx.data,
//!    &svc.event.flags, ctx.rng.event())` (a fresh grid, a copy of the `.dyo` objects and the respawner);
//! 2. put a player in: `ctx.worlds.replace(player, world, pos, LayerId(id), ReplaceKind::Normal)` (executed at the
//!    next `_replace`; the `LayerChanged` note of [`WorldNote`](super::hooks::WorldNote) reports the move);
//! 3. end it with `ctx.worlds.invalidate(world, LayerId(id))` and never release it yourself: the layer garbage
//!    collection releases it once nobody is in it and drives the occupants out every tick meanwhile
//!    ([`EventDriveOut`](super::hooks::EventDriveOut) has an arm per [`LayerKind`](crate::world::LayerKind));
//! 4. add your arm to `EventDriveOut`, your `LogoutStep` arm to `EventLogout`, your `JoinPolicy` method to
//!    `EventJoinPolicy`, your `WorldNote` variant and its drain arm to `event/hooks.rs`, and your kill line to
//!    [`on_world_die`](super::on_world_die), each at your own `// ---- <task>` banner.

mod entry;
mod leave;
mod progress;

pub use entry::{PortalOutcome, on_portal};
pub use leave::{go_out, go_out_worlds, on_logout, on_party_left, on_replace, process};
pub use progress::on_npc_killed;

use crate::event::hooks::WorldNotes;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GameMs, LayerId, PartyId, WorldId};
use flyff_data::GameData;
use flyff_data::lua_config::{DungeonDef, DungeonMonster, ID_BOSS};
use flyff_rules::consts::NULL_ID;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// `MIN( 3 )`: how long a character that left the party inside a dungeon may stay (`SetPartyLeaveTime`).
pub const LEAVE_KICK_MS: u64 = 180_000;

/// The dungeon worlds of `PartyDungeon.lua` (`CInstanceDungeonHelper::IsInstanceDungeon`): shared by the world
/// hooks, the logout hooks, the join policy and the GM `/teleport` rule, none of which can reach the service.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstanceWorldSet(Arc<BTreeSet<WorldId>>);

impl InstanceWorldSet {
    /// The worlds of the configured dungeons (an unknown world name, `NULL_ID`, is skipped).
    pub fn from_data(data: &GameData) -> InstanceWorldSet {
        InstanceWorldSet::from_worlds(
            data.lua
                .party_dungeon
                .iter()
                .flat_map(|c| c.dungeons.iter().map(|d| WorldId(d.world))),
        )
    }

    /// A set of the given worlds (`0` and `NULL_ID` are skipped).
    pub fn from_worlds(worlds: impl IntoIterator<Item = WorldId>) -> InstanceWorldSet {
        InstanceWorldSet(Arc::new(
            worlds
                .into_iter()
                .filter(|w| w.0 != 0 && w.0 != NULL_ID)
                .collect(),
        ))
    }

    /// `true` when `world` is an instance-dungeon world.
    pub fn contains(&self, world: WorldId) -> bool {
        self.0.contains(&world)
    }

    /// The worlds, ascending.
    pub fn iter(&self) -> impl Iterator<Item = WorldId> + '_ {
        self.0.iter().copied()
    }

    /// The number of dungeon worlds.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` without a dungeon world.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl crate::gm::InstanceWorlds for InstanceWorldSet {
    fn is_instance_world(&self, world: WorldId) -> bool {
        self.contains(world)
    }
}

/// The progress of a dungeon (`ID_INFO::nState`, `ID_NORMAL = 0 .. ID_BOSS = 2`) plus the plan's explicit
/// `Cleared` after the boss fell (the original keeps `nState = ID_BOSS` and only logs `'C'`).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DungeonState {
    /// `ID_NORMAL`.
    Normal = 0,
    /// `ID_MIDBOSS`.
    MidBoss = 1,
    /// `ID_BOSS`.
    Boss = 2,
    /// The boss was killed.
    Cleared = 3,
}

impl DungeonState {
    /// The `nType` of the script rows that belong to this state.
    pub const fn kind(self) -> i32 {
        self as i32
    }

    /// The next state; `Cleared` stays.
    pub const fn next(self) -> DungeonState {
        match self {
            DungeonState::Normal => DungeonState::MidBoss,
            DungeonState::MidBoss => DungeonState::Boss,
            DungeonState::Boss | DungeonState::Cleared => DungeonState::Cleared,
        }
    }
}

/// One running dungeon, keyed by `(world, party)` (`ID_INFO`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceState {
    /// `nState`.
    pub state: DungeonState,
    /// `nKillCount`: the monsters of the current state that fell.
    pub kill_count: u32,
    /// `nPlayerCount`: the members that entered through the portal and have not left (a GM or a character that
    /// was never counted can make it negative or zero while somebody is inside, KEEP).
    pub player_count: i32,
    /// When the layer was created.
    pub created_at: GameMs,
}

/// One cooldown record (`COOLTIME_INFO`): at most one per `(character, world)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cooldown {
    /// The dungeon world.
    pub world: WorldId,
    /// The party whose dungeon this belongs to; `None` after that dungeon was destroyed
    /// (`ResetDungeonCoolTimeInfo` sets the dungeon id to `NULL_ID`): from then on the cooldown binds the party too.
    pub party: Option<PartyId>,
    /// The game time the cooldown ends.
    pub until: GameMs,
}

/// The read-only view of a running dungeon ([`InstanceDungeonService::list`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceInfo {
    /// The dungeon world.
    pub world: WorldId,
    /// The party that owns it (its layer id).
    pub party: PartyId,
    /// Its state.
    pub state: DungeonState,
    /// The monsters of the state that fell.
    pub kill_count: u32,
    /// The counted members.
    pub player_count: i32,
    /// When it was created.
    pub created_at: GameMs,
}

/// The instance-dungeon service (`Services.event.instances`, 22 §8): memory only (KQ-21).
#[derive(Debug, Default)]
pub struct InstanceDungeonService {
    pub(crate) instances: BTreeMap<(WorldId, PartyId), InstanceState>,
    pub(crate) cooldowns: BTreeMap<CharacterId, Vec<Cooldown>>,
    pub(crate) leave_timers: BTreeMap<CharacterId, GameMs>,
    pub(crate) worlds: InstanceWorldSet,
    pub(crate) notes: WorldNotes,
}

impl InstanceDungeonService {
    /// Reads the dungeon worlds out of `data` (`Game::new`).
    pub fn configure(&mut self, data: &GameData) {
        self.worlds = InstanceWorldSet::from_data(data);
    }

    /// Installs the game's end of `Handlers.event_world` (the receiver the world hooks' notes arrive on).
    pub fn set_notes(&mut self, notes: WorldNotes) {
        self.notes = notes;
    }

    /// `IsInstanceDungeon(world)`.
    pub fn is_instance_world(&self, world: WorldId) -> bool {
        self.worlds.contains(world)
    }

    /// The dungeon worlds.
    pub fn world_set(&self) -> &InstanceWorldSet {
        &self.worlds
    }

    /// The running dungeon of `(world, party)`.
    pub fn instance_of(&self, world: WorldId, party: PartyId) -> Option<&InstanceState> {
        self.instances.get(&(world, party))
    }

    /// Every running dungeon, ascending by `(world, party)`.
    pub fn list(&self) -> Vec<InstanceInfo> {
        self.instances
            .iter()
            .map(|(&(world, party), s)| InstanceInfo {
                world,
                party,
                state: s.state,
                kill_count: s.kill_count,
                player_count: s.player_count,
                created_at: s.created_at,
            })
            .collect()
    }

    /// The number of running dungeons (`m_nIDLayerNum`).
    pub fn count(&self) -> usize {
        self.instances.len()
    }

    /// The cooldown records of a character.
    pub fn cooldowns_of(&self, c: CharacterId) -> &[Cooldown] {
        self.cooldowns.get(&c).map_or(&[], Vec::as_slice)
    }

    /// The game time the kick timer of `c` ends, when one runs.
    pub fn leave_deadline(&self, c: CharacterId) -> Option<GameMs> {
        self.leave_timers.get(&c).copied()
    }

    /// `true` when no instance, cooldown or kick timer exists.
    pub fn is_pristine(&self) -> bool {
        self.instances.is_empty() && self.cooldowns.is_empty() && self.leave_timers.is_empty()
    }
}

// ---- helpers shared by the files ----------------------------------------------------------------------------

/// The layer id of a party's dungeon; `None` for a party id above `i32::MAX`.
pub(crate) fn layer_of(party: PartyId) -> Option<LayerId> {
    i32::try_from(party.0).ok().map(LayerId)
}

/// The party whose dungeon is the layer `layer` of an instance world (`None` for layer 0 or a ticket layer).
pub(crate) fn party_of_layer(layer: LayerId) -> Option<PartyId> {
    u32::try_from(layer.0).ok().filter(|&p| p > 0).map(PartyId)
}

/// `m_mapDungeonData.find(world)`: the first definition of the world.
pub(crate) fn def_of(data: &GameData, world: WorldId) -> Option<&DungeonDef> {
    data.lua
        .party_dungeon
        .as_ref()?
        .dungeons
        .iter()
        .find(|d| d.world == world.0)
}

/// `GetObjCount`: the monsters configured for `state`.
pub(crate) fn count_of(def: &DungeonDef, state: DungeonState) -> usize {
    def.monsters
        .iter()
        .filter(|m| m.kind == state.kind())
        .count()
}

/// The monsters of `state` in script order.
pub(crate) fn monsters_of(
    def: &DungeonDef,
    state: DungeonState,
) -> impl Iterator<Item = &DungeonMonster> {
    def.monsters.iter().filter(move |m| m.kind == state.kind())
}

/// `GetTeleportPos`: the configured position of `state`; `None` for none or `(0, 0, 0)` (the original's "no
/// position").
pub(crate) fn teleport_of(def: &DungeonDef, state: DungeonState) -> Option<Vec3> {
    def.teleports
        .iter()
        .find(|t| t.kind == state.kind())
        .map(|t| Vec3::new(t.pos[0], t.pos[1], t.pos[2]))
        .filter(|p| *p != Vec3::ZERO)
}

/// The state a fresh dungeon starts in: `Normal`, skipping the states without monsters up to and including
/// `Boss` (`while GetObjCount <= 0 && nState < ID_BOSS`; a dungeon without a BOSS stays at `Boss`, KEEP).
pub(crate) fn first_state(def: &DungeonDef) -> DungeonState {
    let mut s = DungeonState::Normal;
    while count_of(def, s) == 0 && s.kind() < ID_BOSS {
        s = s.next();
    }
    s
}
