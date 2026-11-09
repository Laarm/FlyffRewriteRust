//! World queries of the console's "World & Content" category (Plan/25 §7.1, §7.2, §11.5; **WP-ADMIN-30**):
//!
//! * `spawn_status` (`SpawnStatus{world}`): one row per respawn line of the world, from the respawn table (15 §15)
//!   and the respawner state of the main layer (layer 0): what the line makes, where, how many it should have, how
//!   many are alive and awake (a monster that nobody observes sleeps, 15 §5.6), the countdown to the next refill
//!   check and whether the line is paused by its day/hour window or by `events.respawn`;
//! * `census` (`Census{world}`): the monsters of every live layer of the world by rank, asleep or awake, and the
//!   20 most numerous types;
//! * `event_info` (`EventInfo`): the Event.lua modifiers (22 §4.13 "list modifiers"), the running Lua events and
//!   the weather effect as the clients know it.
//!
//! Everything here takes `&Game` and reads only: no visibility function runs and nothing wakes (ADM-D4).

use super::AdminQueryResult;
use crate::admin::map::rank_code;
use crate::tick::Game;
use crate::world::{LayerState, ObjKind};
use flyff_common::config::EventId;
use flyff_common::{LayerId, WorldId};
use flyff_rules::consts::defs::neuz::AII_PET;
use serde_json::Value;
use std::collections::BTreeMap;

/// The layer of the spawn table: the public one (instances and mini-rooms have their own respawners).
pub const MAIN_LAYER: LayerId = LayerId(0);

/// The census names this many monster types.
pub const CENSUS_TOP: usize = 20;

/// What a respawn line makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    /// Monsters.
    Monster,
    /// Controls (chests, doors, ...).
    Ctrl,
    /// Items on the ground.
    Item,
}

impl SpawnKind {
    /// `monster`, `ctrl` or `item`.
    pub fn name(self) -> &'static str {
        match self {
            SpawnKind::Monster => "monster",
            SpawnKind::Ctrl => "ctrl",
            SpawnKind::Item => "item",
        }
    }
}

/// One respawn line (a row of the spawn table).
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnRow {
    /// The line (`gm_index`; `EntityDetail.spawn_region` is this plus one).
    pub region: u32,
    /// What it makes.
    pub kind: SpawnKind,
    /// The mover, ctrl or item prop id.
    pub prop: u32,
    /// Its name (empty when the data has none).
    pub name: String,
    /// The monster level; 0 for the other kinds.
    pub level: u32,
    /// The rank code of a monster (0 normal, 1 captain, 2 mid-boss, 3 boss, 4 giant, 5 guard).
    pub rank: u8,
    /// The spawn rectangle `[left, top, right, bottom]` in world units.
    pub rect: [i32; 4],
    /// The population the line refills to (`m_nMaxcb`).
    pub target: u32,
    /// The objects the respawner counts as alive (`m_cb`).
    pub alive: u32,
    /// The living objects of the line in the layer that are awake (observed or a pet; 15 §5.6).
    pub awake: u32,
    /// The line can make aggressive monsters (the cap is not zero, and the level is at least 7 or the line is a
    /// giant line).
    pub aggressive: bool,
    /// The aggressive monsters alive (`m_nActiveAttackNum`).
    pub aggressive_alive: u32,
    /// Seconds until the next refill check; below zero the check is due.
    pub countdown_s: i32,
    /// The next check refills only up to half of the target.
    pub half: bool,
    /// The seconds between two checks, before the 50-149 % roll.
    pub interval_s: u32,
    /// The line makes one monster at a fixed spot.
    pub fixed: bool,
    /// The line makes flyers.
    pub flying: bool,
    /// The day and hour window of the line admits the in-game time now.
    pub in_window: bool,
}

/// The spawn table of a world (`SpawnStatus`).
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnStatusResult {
    /// The world.
    pub world: WorldId,
    /// The layer the counters are of.
    pub layer: LayerId,
    /// `events.respawn != 0`: respawning is on.
    pub respawn_on: bool,
    /// The in-game day `1..=30`.
    pub game_day: u8,
    /// The in-game hour `1..=24`.
    pub game_hour: u8,
    /// The lines in `gm_index` order.
    pub lines: Vec<SpawnRow>,
}

/// One rank of the census.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CensusRank {
    /// The rank code (see [`SpawnRow::rank`]).
    pub rank: u8,
    /// Monsters alive.
    pub alive: u32,
    /// Of them asleep (nobody observes them).
    pub asleep: u32,
    /// Of them awake.
    pub awake: u32,
}

/// One monster type of the census.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CensusType {
    /// The mover prop id.
    pub prop: u32,
    /// Its name.
    pub name: String,
    /// Its level.
    pub level: u32,
    /// Its rank code.
    pub rank: u8,
    /// Alive.
    pub alive: u32,
    /// Awake.
    pub awake: u32,
}

/// The monster population of a world (`Census`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CensusResult {
    /// The world.
    pub world: WorldId,
    /// The live layers counted (layer 0 and every instance or room of the world).
    pub layers: u32,
    /// Monsters alive (pets excluded).
    pub alive: u32,
    /// Of them asleep.
    pub asleep: u32,
    /// Of them awake.
    pub awake: u32,
    /// The six ranks, in code order, also those with nobody alive.
    pub by_rank: Vec<CensusRank>,
    /// The most numerous types, at most [`CENSUS_TOP`], by number alive (ties: lower prop id first).
    pub top: Vec<CensusType>,
    /// Players in the world (all layers).
    pub players: u32,
    /// NPCs in the world.
    pub npcs: u32,
}

/// One running Lua event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningLuaEvent {
    /// The event id (the 1-based order of `AddEvent`).
    pub id: u32,
    /// `GetDesc(id)`.
    pub desc: String,
}

/// The weather effect as the clients know it (`Worlds.environment`, 15 §17.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeatherState {
    /// A weather effect is on.
    pub effect: bool,
    /// The title of the weather event.
    pub title: String,
    /// The console can change the weather (`SetWeather` has a body: WP-GM-30).
    pub control: bool,
}

/// What `EventInfo` answers.
#[derive(Clone, Debug, PartialEq)]
pub struct EventInfo {
    /// The Event.lua aggregates (`EventModifiers::to_json`, the field names of 22 §4.3).
    pub modifiers: Value,
    /// The Lua events that are running now, ascending by id.
    pub running: Vec<RunningLuaEvent>,
    /// The weather.
    pub weather: WeatherState,
}

fn clamp_u32(v: i32) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(0)
}

/// `SpawnStatus`.
pub(super) fn spawn_status(g: &Game, world: WorldId) -> AdminQueryResult {
    let Some(w) = g.worlds.world(world) else {
        return AdminQueryResult::not_found(format!("world {} is not loaded", world.0));
    };
    let Some(layer) = g.worlds.layer(world, MAIN_LAYER) else {
        return AdminQueryResult::not_found(format!("world {} has no layer 0", world.0));
    };
    let table = &w.respawn;
    // one pass over the layer: the awake objects per line
    let mut awake = vec![0_u32; table.protos.len()];
    let store = g.worlds.store();
    for id in layer.members.iter() {
        let Some(core) = store.core(*id) else {
            continue;
        };
        if core.deleted || core.kind != ObjKind::Mover {
            continue;
        }
        let Some(origin) = core.respawn else {
            continue;
        };
        let Some(m) = store.mover(*id) else { continue };
        if crate::combat::is_die(m) {
            continue;
        }
        let observed =
            !core.viewers.is_empty() || m.monster().is_some_and(|e| e.ai.kind == AII_PET);
        if observed && let Some(slot) = awake.get_mut(usize::from(origin.gm_index)) {
            *slot += 1;
        }
    }
    let day = i32::from(g.worlds.game_time.day);
    let hour = i32::from(g.worlds.game_time.hour);
    let aggressive_rate = g.live.rates.monster_aggressive;
    let mut lines = Vec::with_capacity(table.protos.len());
    for (i, proto) in table.protos.iter().enumerate() {
        let st = layer.respawner.states.get(i).copied().unwrap_or_default();
        let (kind, name, level, rank) = match proto.kind {
            ObjKind::Mover => {
                let prop = g.data.mover(proto.index);
                (
                    SpawnKind::Monster,
                    prop.map(|p| p.name.clone()).unwrap_or_default(),
                    prop.map_or(0, |p| clamp_u32(p.level as i32)),
                    prop.map_or(0, |p| rank_code(p.class)),
                )
            }
            ObjKind::Ctrl => (
                SpawnKind::Ctrl,
                g.data
                    .ctrls
                    .get(proto.index)
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                0,
                0,
            ),
            ObjKind::Item => (
                SpawnKind::Item,
                g.data
                    .item(proto.index)
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
                0,
                0,
            ),
        };
        lines.push(SpawnRow {
            region: u32::from(proto.gm_index),
            kind,
            prop: proto.index,
            name,
            level,
            rank,
            rect: proto.rect,
            target: clamp_u32(proto.max_count),
            alive: clamp_u32(st.alive),
            awake: awake.get(i).copied().unwrap_or(0),
            aggressive: kind == SpawnKind::Monster
                && crate::world::respawn::line_is_aggressive(
                    proto,
                    &st,
                    world,
                    aggressive_rate,
                    level,
                ),
            aggressive_alive: clamp_u32(st.active_alive),
            countdown_s: st.countdown,
            half: st.half,
            interval_s: u32::from(proto.base_time),
            fixed: proto.is_fixed(),
            flying: proto.flying,
            in_window: crate::world::respawn::in_window(day, proto.day)
                && crate::world::respawn::in_window(hour, proto.hour),
        });
    }
    AdminQueryResult::Spawns(Box::new(SpawnStatusResult {
        world,
        layer: MAIN_LAYER,
        respawn_on: g.live.event_state(EventId::Respawn) != 0,
        game_day: g.worlds.game_time.day,
        game_hour: g.worlds.game_time.hour,
        lines,
    }))
}

/// `Census`.
pub(super) fn census(g: &Game, world: WorldId) -> AdminQueryResult {
    let Some(w) = g.worlds.world(world) else {
        return AdminQueryResult::not_found(format!("world {} is not loaded", world.0));
    };
    let store = g.worlds.store();
    let mut by_rank: Vec<CensusRank> = (0..6)
        .map(|rank| CensusRank {
            rank,
            ..CensusRank::default()
        })
        .collect();
    // prop -> (alive, awake)
    let mut types: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    let (mut layers, mut players, mut npcs) = (0_u32, 0_u32, 0_u32);
    for layer in w.layers.values() {
        if layer.state != LayerState::Live {
            continue;
        }
        layers += 1;
        for id in layer.members.iter() {
            let Some(m) = store.mover(*id) else { continue };
            if m.core.deleted {
                continue;
            }
            if m.is_player() {
                players += 1;
            } else if m.npc().is_some() {
                npcs += 1;
            } else if let Some(e) = m.monster()
                && e.ai.kind != AII_PET
                && !crate::combat::is_die(m)
            {
                let awake = !m.core.viewers.is_empty();
                if let Some(r) = by_rank.get_mut(usize::from(rank_code(e.class))) {
                    r.alive += 1;
                    if awake {
                        r.awake += 1;
                    } else {
                        r.asleep += 1;
                    }
                }
                let t = types.entry(m.core.index).or_default();
                t.0 += 1;
                t.1 += u32::from(awake);
            }
        }
    }
    let mut top: Vec<CensusType> = types
        .into_iter()
        .map(|(prop, (alive, awake))| {
            let p = g.data.mover(prop);
            CensusType {
                prop,
                name: p.map(|p| p.name.clone()).unwrap_or_default(),
                level: p.map_or(0, |p| clamp_u32(p.level as i32)),
                rank: p.map_or(0, |p| rank_code(p.class)),
                alive,
                awake,
            }
        })
        .collect();
    // most alive first; the map order (ascending prop id) breaks ties because the sort is stable
    top.sort_by_key(|t| std::cmp::Reverse(t.alive));
    top.truncate(CENSUS_TOP);
    let (alive, awake) = by_rank
        .iter()
        .fold((0, 0), |(a, w), r| (a + r.alive, w + r.awake));
    AdminQueryResult::Census(Box::new(CensusResult {
        world,
        layers,
        alive,
        asleep: alive - awake,
        awake,
        by_rank,
        top,
        players,
        npcs,
    }))
}

/// `EventInfo`.
pub(super) fn event_info(g: &Game) -> AdminQueryResult {
    let ev = &g.services.event;
    AdminQueryResult::EventInfo(Box::new(EventInfo {
        modifiers: ev.modifiers_json(),
        running: ev
            .lua_events
            .running()
            .iter()
            .map(|r| RunningLuaEvent {
                id: r.id,
                desc: r.desc.clone(),
            })
            .collect(),
        weather: WeatherState {
            effect: g.worlds.environment.effect,
            title: g.worlds.environment.title.clone(),
            control: crate::admin::actions::world::WEATHER_CONTROL,
        },
    }))
}
