//! `event::monsters`: the `EventMonster.lua` monsters (`MI_EVENT01..04`, Plan/22 §7.1, WP-EVENT-09; legacy
//! `CEventMonster`, `_Common/EventMonster.cpp:30-116`; ref/15 §7; KQ-32).
//!
//! `EventMonster.lua` lists mover props whose monsters follow three special rules:
//!
//! * **the level-gap miss** (this module's [`level_gap`], asked by `combat::arbiter::check_valid_damage_msg` through
//!   `CombatHooks::event_monster_level_gap`): a player at least `nLevel` levels above the monster misses it
//!   (`AF_MISS`, a zero `Damage` broadcast, no HP change),
//! * **the claim rule** (`combat::target::set_jjim`): nobody claims an event monster,
//! * **the loot rules** (`item::drop` / `item::loot`: owner-only loot time, spread, pets, gift boxes), which are the
//!   item module's and read the same table with the same first-wins lookup ([`find`]).
//!
//! `EventMonster.lua` is a static script: the original loads it once and no `/lua` reload re-reads it, so the table
//! is built once by `Game::new` ([`EventMonsterTable::from_data`]).
//!
//! The monster raid (`MonsterRaid.lua`, F-188) stays DEFER and nothing of it is built here. Event.lua spawn events
//! are `event::spawns` (WP-EVENT-06), Rangda is `event::rangda`.

use flyff_data::GameData;
use flyff_data::lua_config::EventMonsterDef;
use flyff_data::types::{MoverId, NULL_ID};
use std::collections::BTreeMap;

/// The rule of `mover` in `defs`, the FIRST entry of the script winning (`m_mapEventMonster.insert` keeps the
/// existing entry of a repeated id, `EventMonster.cpp:50`). The free function for the code that has no services:
/// the arbiter hook, `set_jjim`, the drop and loot code.
pub fn find(defs: &[EventMonsterDef], mover: MoverId) -> Option<&EventMonsterDef> {
    defs.iter().find(|d| d.mover == mover)
}

/// `GetLevelGap()` of the event monster `mover` (`nLevel`), `None` when `mover` is not one (the original answers
/// `MAX_LEVEL` for an unknown id, but only after `SetEventMonster` was false and nobody asks then).
pub fn level_gap(defs: &[EventMonsterDef], mover: MoverId) -> Option<i32> {
    find(defs, mover).map(|d| d.level_gap)
}

/// The event monsters of `EventMonster.lua` by mover id (`CEventMonster::m_mapEventMonster`), the first entry of a
/// mover winning.
#[derive(Clone, Debug, Default)]
pub struct EventMonsterTable {
    by_mover: BTreeMap<MoverId, EventMonsterDef>,
}

impl EventMonsterTable {
    /// Builds the table from `data.lua.event_monsters` in script order (`map::insert`: the first entry of a mover is
    /// kept). An entry whose define is unknown (`NULL_ID`) is skipped with one warning.
    pub fn from_data(data: &GameData) -> EventMonsterTable {
        let mut by_mover = BTreeMap::new();
        for def in &data.lua.event_monsters {
            if def.mover == NULL_ID {
                tracing::warn!(
                    mover = %def.mover_name,
                    "EventMonster.lua: an unknown monster id, the entry is skipped"
                );
                continue;
            }
            by_mover.entry(def.mover).or_insert_with(|| def.clone());
        }
        EventMonsterTable { by_mover }
    }

    /// `CEventMonster::SetEventMonster(mover)` and the getters behind it: the rule of an event monster, `None` for
    /// any other mover (the query the other documents use, 22 §2.1).
    pub fn event_rule(&self, mover: MoverId) -> Option<&EventMonsterDef> {
        self.by_mover.get(&mover)
    }

    /// `CEventMonster::IsEventMonster(mover)`.
    pub fn is_event_monster(&self, mover: MoverId) -> bool {
        self.by_mover.contains_key(&mover)
    }

    /// `GetLevelGap()` of `mover`; `None` when it is not an event monster.
    pub fn level_gap(&self, mover: MoverId) -> Option<i32> {
        self.event_rule(mover).map(|d| d.level_gap)
    }

    /// The rules by ascending mover id.
    pub fn iter(&self) -> impl Iterator<Item = &EventMonsterDef> {
        self.by_mover.values()
    }

    /// How many event monsters there are.
    pub fn len(&self) -> usize {
        self.by_mover.len()
    }

    /// `true` when the script lists none.
    pub fn is_empty(&self) -> bool {
        self.by_mover.is_empty()
    }
}
