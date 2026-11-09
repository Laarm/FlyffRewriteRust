//! Cooldown groups of item use (Plan/19 §5.2; src:_Common/CooltimeMgr.cpp:16-60).
//!
//! A group exists only for items with `skill_ready > 0`: `IK2_FOOD` is group 1 (`IK3_PILL`: group 2),
//! `IK2_SKILL` group 3; potions and refreshers have none. Items of one group share the cooldown. The timers
//! are on the game clock and are not persisted.

use crate::item::inventory::CooldownTimers;
use flyff_common::GameMs;
use flyff_common::defs::itemkind::{IK2_FOOD, IK2_SKILL, IK3_PILL};
use flyff_data::props::item::ItemProp;

/// The three cooldown groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CooldownGroup {
    /// Instant food and cooking.
    Food = 1,
    /// Pills.
    Pill = 2,
    /// Skill items.
    Skill = 3,
}

/// `GetGroup(prop)`.
pub fn group_of(prop: &ItemProp) -> Option<CooldownGroup> {
    if prop.skill_ready == 0 || prop.skill_ready == u32::MAX {
        return None;
    }
    match prop.item_kind2 {
        IK2_FOOD if prop.item_kind3 == IK3_PILL => Some(CooldownGroup::Pill),
        IK2_FOOD => Some(CooldownGroup::Food),
        IK2_SKILL => Some(CooldownGroup::Skill),
        _ => None,
    }
}

impl CooldownTimers {
    /// `CanUse(group)`: the group may be used once the clock is **past** its end.
    pub fn can_use(&self, g: CooldownGroup, now: GameMs) -> bool {
        self.until
            .get(g as usize - 1)
            .is_none_or(|until| now.0 > until.0)
    }

    /// `SetTime(group, ms)`: the group is blocked for `ms` from `now`.
    pub fn set(&mut self, g: CooldownGroup, now: GameMs, ms: u32) {
        if let Some(slot) = self.until.get_mut(g as usize - 1) {
            *slot = GameMs(now.0.saturating_add(u64::from(ms)));
        }
    }
}
