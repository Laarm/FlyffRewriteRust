//! `[events]` (Plan/03 §5): the initial values of the runtime `EventFlags` store (owned by doc 22).
//! The struct `EventDefaults` is generated in [`tables`](super::tables).

pub use super::tables::EventDefaults;

/// First event id of the generic (script-defined, `propEvent.inc`) range; ids from here on have no keys.
pub const EVENT_GENERIC_BASE: u16 = 500;

/// Number of event slots: the runtime store is `EventFlags([u8; MAX_EVENT])`.
pub const MAX_EVENT: usize = 1024;

/// Canonical event-flag ids (Plan/03 §5.2), the legacy `g_eLocal` ids.
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum EventId {
    /// Client April-fools text (dead).
    Eve0401a = 0,
    /// "18+ server": PvP/PK case computation, duel/PK menu entries.
    Eve18 = 1,
    /// Guild bank, guild-house furniture, tender results.
    GuildInventory = 2,
    /// Guild wars.
    GuildWar = 3,
    /// School event (dead).
    School = 4,
    /// School battle (dead).
    SchoolBattle = 5,
    /// Never referenced.
    NoFlyMonster = 6,
    /// Never referenced.
    NoDarkon = 7,
    /// Never referenced.
    NoGuild = 8,
    /// "Wormon server".
    Wormon = 9,
    /// 0 = no monster respawn at all.
    Respawn = 10,
    /// PK server.
    Pk = 11,
    /// PK penalties.
    PkCost = 12,
    /// `true` = kill-steal protection OFF.
    Steal = 13,
    /// x2 drop event (dead).
    Event0913 = 14,
    /// Guild siege.
    GuildCombat = 15,
    /// Player "drop" destroys the item.
    DropItemRemove = 16,
    /// Hard-coded 2005/2006 Core event (dead).
    Event1206 = 17,
    /// Hard-coded 2005/2006 Core event (dead).
    Event1219 = 18,
    /// Hard-coded 2005/2006 Core event (dead).
    Event0127 = 19,
    /// Hard-coded 2005/2006 Core event (dead).
    Event0214 = 20,
    /// Level-up recommendation report.
    Recommend = 21,
    /// 1:1 guild combat.
    GuildCombat1to1 = 22,
    /// Arena NPC.
    Arena = 23,
    /// Secret room.
    SecretRoom = 24,
    /// Rainbow race.
    RainbowRace = 25,
}

impl EventId {
    /// The numeric id (`SNAPSHOTTYPE_SET_LOCAL_EVENT` `id`).
    pub const fn id(self) -> u16 {
        self as u16
    }
}

impl EventDefaults {
    /// The initial value of every named event slot as `(EventId, value)`; booleans become 0/1 and
    /// `respawn` keeps its own value. The ids without a key stay 0 (Plan/03 §5.1).
    pub fn initial_flags(&self) -> [(EventId, u8); 15] {
        let b = u8::from;
        [
            (EventId::Eve18, b(self.eve_18)),
            (EventId::GuildInventory, b(self.guild_bank)),
            (EventId::GuildWar, b(self.guild_war)),
            (EventId::Wormon, b(self.wormon)),
            (EventId::Respawn, self.respawn),
            (EventId::Pk, b(self.pk)),
            (EventId::PkCost, b(self.pk_cost)),
            (EventId::Steal, b(self.steal)),
            (EventId::GuildCombat, b(self.guild_combat)),
            (EventId::DropItemRemove, b(self.drop_item_remove)),
            (EventId::Recommend, b(self.recommend)),
            (EventId::GuildCombat1to1, b(self.guild_combat_1to1)),
            (EventId::Arena, b(self.arena)),
            (EventId::SecretRoom, b(self.secret_room)),
            (EventId::RainbowRace, b(self.rainbow_race)),
        ]
    }
}
