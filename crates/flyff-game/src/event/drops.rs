//! `event::drops`: Event.lua's part of the monster drop (Plan/22 §4.7, 19 §6.2 step 2, WP-EVENT-05).
//!
//! Doc 19's drop generator asks an [`EventDropHooks`] for the event items of a kill: the generic (`propEvent.inc`)
//! items first, then the Lua items `GetItem(game_ms, monster_level)`. [`EventDrops`] is that hook over the real
//! `Event` family: the Lua items come from the script (the pacing state, `TimeOut` and `Skip`, lives in Lua; the
//! `map<item, count>` quirk is in [`EventScripts::event_items`](super::EventScripts::event_items)), the generic
//! items are those of the installed inner hook until WP-EVENT-07 provides them. The EXP, item, piece and gold
//! **factors** are not here: the drop and progression code read them from [`EventModifiers`](super::EventModifiers)
//! (22 §4.4).
//!
//! With the shipped `Event.lua` (no `SetItem`) the list is empty; the pacing is tested on a fixture.

use super::flags::EventFlags;
use super::generic::{GenericEventService, due_items};
use super::lua_events::EventLuaService;
use super::spawns::EventSpawnService;
use crate::item::drop::EventDropHooks;
use flyff_common::GameMs;
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_data::types::{ItemId, MoverId};
use flyff_lua::LuaHost;

/// The event hook of one kill: the installed hook for what only it knows (generic items, event-spawn monsters) and
/// the `Event` family for the Lua items.
pub struct EventDrops<'a> {
    inner: &'a mut dyn EventDropHooks,
    lua: &'a LuaHost,
    lua_events: &'a EventLuaService,
    data: &'a GameData,
    now: GameMs,
    spawns: Option<&'a EventSpawnService>,
    generic: Option<GenericDrops<'a>>,
}

/// What the `propEvent.inc` items of a kill need (22 §6.3, WP-EVENT-07): the paced item lists, the flags that switch
/// the events on, the local hour and the event random stream (the caller writes the stream back afterwards).
pub struct GenericDrops<'a> {
    /// The generic-event service (its item pacers are advanced by the kill).
    pub service: &'a mut GenericEventService,
    /// The flag store.
    pub flags: &'a EventFlags,
    /// The local hour of the kill (`CTime::GetCurrentTime().GetHour()`).
    pub hour: u8,
    /// The `event` random stream.
    pub rng: &'a mut dyn GameRng,
}

impl<'a> EventDrops<'a> {
    /// The hook of a kill at game time `now`.
    pub fn new(
        inner: &'a mut dyn EventDropHooks,
        lua: &'a LuaHost,
        lua_events: &'a EventLuaService,
        data: &'a GameData,
        now: GameMs,
    ) -> Self {
        Self {
            inner,
            lua,
            lua_events,
            data,
            now,
            spawns: None,
            generic: None,
        }
    }

    /// The generic (`propEvent.inc`) item lists of the events that are on (22 §6.3, WP-EVENT-07); without them
    /// the generic items are those of the installed inner hook.
    pub fn with_generic(mut self, g: GenericDrops<'a>) -> Self {
        self.generic = Some(g);
        self
    }

    /// The spawn events of Event.lua whose mover props are exempt from the level gate (KQ-32, 22 §4.10).
    pub fn with_spawns(mut self, spawns: &'a EventSpawnService) -> Self {
        self.spawns = Some(spawns);
        self
    }
}

impl EventDropHooks for EventDrops<'_> {
    fn generic_event_items(&mut self) -> Vec<(ItemId, i32)> {
        let mut v = match self.generic.as_mut() {
            Some(g) => due_items(g.service, g.flags, self.data, self.now, g.hour, &mut *g.rng),
            None => Vec::new(),
        };
        v.extend(self.inner.generic_event_items());
        v
    }

    fn lua_event_items(&mut self, monster_level: u32) -> Vec<(ItemId, i32)> {
        let mut v = self.inner.lua_event_items(monster_level);
        v.extend(
            self.lua_events
                .lua_event_items(self.lua, self.data, self.now, monster_level),
        );
        v
    }

    fn is_event_spawn_monster(&self, mover: MoverId) -> bool {
        self.spawns.is_some_and(|s| s.is_event_monster(mover))
            || self.inner.is_event_spawn_monster(mover)
    }
}
