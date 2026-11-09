//! The DTOs of the World & Content area (Plan/25 §7; WP-ADMIN-30): the spawn table, the census, the Event.lua
//! modifiers with the weather, and the content-panel registry. Generated into `web/src/admin/gen/dto.ts`
//! (`declare`).

use serde::{Deserialize, Serialize};

/// One respawn line (a row of the Spawns table).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SpawnRowDto {
    /// The line (the `spawn_region` of an object card is this plus one).
    pub region: u32,
    /// `monster`, `ctrl` or `item`.
    pub kind: String,
    /// The mover, ctrl or item prop id.
    pub prop: u32,
    /// Its name.
    pub name: String,
    /// The monster level (0 for the other kinds).
    pub level: u32,
    /// The rank code of a monster: 0 normal, 1 captain, 2 mid-boss, 3 boss, 4 giant, 5 guard.
    pub rank: u8,
    /// The spawn rectangle `[left, top, right, bottom]`, world units.
    pub rect: [i32; 4],
    /// The population the line refills to.
    pub target: u32,
    /// Alive, as the respawner counts them.
    pub alive: u32,
    /// Alive and awake (somebody observes them).
    pub awake: u32,
    /// The line can make aggressive monsters.
    pub aggressive: bool,
    /// The aggressive monsters alive.
    pub aggressive_alive: u32,
    /// Seconds until the next refill check (below zero: due now).
    pub countdown_s: i32,
    /// The next check refills only up to half of the target.
    pub half: bool,
    /// The seconds between two checks (before the 50-149 % roll).
    pub interval_s: u32,
    /// One monster at a fixed spot.
    pub fixed: bool,
    /// Flyers.
    pub flying: bool,
    /// The day and hour window of the line admits the in-game time now.
    pub in_window: bool,
}

/// `GET /admin/api/spawns?world=`: the respawn lines of a world (layer 0).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SpawnTableDto {
    /// The world.
    pub world: u32,
    /// The layer the counters are of.
    pub layer: i32,
    /// Respawning is on (`events.respawn`).
    pub respawn_on: bool,
    /// The in-game day, 1..=30.
    pub game_day: u8,
    /// The in-game hour, 1..=24.
    pub game_hour: u8,
    /// The lines in order.
    pub lines: Vec<SpawnRowDto>,
}

/// `POST /admin/api/spawns/{region}/respawn` (`Mutation<RespawnNowBody>`): make a line due now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RespawnNowBody {
    /// The world.
    pub world: u32,
    /// The layer (left out = 0).
    #[serde(default)]
    pub layer: i32,
}

/// One rank of the census.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MonsterRankDto {
    /// The rank code (see `SpawnRowDto.rank`).
    pub rank: u8,
    /// Alive.
    pub alive: u32,
    /// Asleep.
    pub asleep: u32,
    /// Awake.
    pub awake: u32,
}

/// One monster type of the census.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MonsterTypeDto {
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

/// `GET /admin/api/census?world=`: the monsters of a world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MonsterCensusDto {
    /// The world.
    pub world: u32,
    /// The live layers counted.
    pub layers: u32,
    /// Monsters alive (pets excluded).
    pub alive: u32,
    /// Asleep.
    pub asleep: u32,
    /// Awake.
    pub awake: u32,
    /// The six ranks.
    pub by_rank: Vec<MonsterRankDto>,
    /// The most numerous types (at most 20).
    pub top: Vec<MonsterTypeDto>,
    /// Players in the world.
    pub players: u32,
    /// NPCs in the world.
    pub npcs: u32,
}

/// One Event.lua modifier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ModifierRow {
    /// The field name of `EventModifiers` (22 §4.3).
    pub key: String,
    /// For the operator.
    pub label: String,
    /// The value in force.
    pub value: f64,
    /// The value that changes nothing (1 for a factor, 0 for a bonus).
    pub neutral: f64,
    /// The value differs from the neutral one.
    pub active: bool,
    /// `x` (a factor), `+` (a flat bonus) or `ms`.
    pub unit: String,
    /// Where it comes from (`Event.lua GetExpFactor()`).
    pub source: String,
}

/// One running Lua event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LuaEventDto {
    /// The event id.
    pub id: u32,
    /// What the clients are told about it.
    pub desc: String,
}

/// The weather effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WeatherDto {
    /// A weather effect is on.
    pub effect: bool,
    /// The title of the weather event.
    pub title: String,
    /// The console can change it (needs WP-GM-30).
    pub control: bool,
}

/// `GET /admin/api/events/modifiers`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EventInfoDto {
    /// The Event.lua aggregates with their sources.
    pub modifiers: Vec<ModifierRow>,
    /// The Lua events that are running.
    pub running: Vec<LuaEventDto>,
    /// The weather.
    pub weather: WeatherDto,
}

/// `PUT /admin/api/weather/{world}` (`Mutation<WeatherBody>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WeatherBody {
    /// `none`, `rain` or `snow`.
    pub kind: WeatherKindDto,
}

/// A kind of weather.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WeatherKindDto {
    /// Clear.
    None,
    /// Rain.
    Rain,
    /// Snow.
    Snow,
}

/// The answer of a weather change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WeatherResult {
    /// The world.
    pub world: u32,
    /// What was set.
    pub kind: WeatherKindDto,
}

/// One content panel of the registry (25 §7.3): the panel the console shows and the work package it waits for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PanelDto {
    /// The panel id (`/admin/api/content/{panel}`).
    pub id: String,
    /// The title.
    pub title: String,
    /// The work package that has to land first.
    pub wp: String,
}

/// Appends the declarations of this module to `out` (the generator of `dto.rs` calls it).
#[cfg(feature = "ts")]
pub fn declare(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;

    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }

    push::<SpawnRowDto>(out, cfg);
    push::<SpawnTableDto>(out, cfg);
    push::<RespawnNowBody>(out, cfg);
    push::<MonsterRankDto>(out, cfg);
    push::<MonsterTypeDto>(out, cfg);
    push::<MonsterCensusDto>(out, cfg);
    push::<ModifierRow>(out, cfg);
    push::<LuaEventDto>(out, cfg);
    push::<WeatherDto>(out, cfg);
    push::<EventInfoDto>(out, cfg);
    push::<WeatherKindDto>(out, cfg);
    push::<WeatherBody>(out, cfg);
    push::<WeatherResult>(out, cfg);
    push::<PanelDto>(out, cfg);
}
