//! `event::scripts`: the typed adapters over doc 20's Lua families (Plan/22 §2.5, WP-EVENT-03).
//!
//! Everything the event code asks of `Event.lua`, `Tax.lua`, `SecretRoom.lua` and `RainbowRace.lua` goes through
//! here, so the rules of 22 §2.5 hold in one place:
//!
//! 1. **Neutral values.** [`EventScripts`] wraps one `call_opt` per `Event` getter of 20 §4.2. A missing function
//!    (`GetWeatherEventExpFactor`, `GetShopBuyFactor`, ... are absent from the shipped `EventFunc.lua`, KQ-11), an
//!    `Err(LuaFault)` and a family that is not `Ready` all give the neutral value: 1.0 for factors, 0 for ATK/DEF
//!    and times, empty lists. A non-`Ready` family is not called at all. Numbers are read as `f64` and converted
//!    like the C++ did (`(float)` for factors, `(int)` for counts, `(DWORD)` for tick values).
//! 2. **Typed configs.** [`SecretRoomConfig`], [`TaxConfig`] and [`RainbowRaceConfig`] read the globals and tables
//!    of 22 §10.1, §11.1 and §14.1 through `LuaHost::global`; [`EventConfigs`] holds the three and the derived
//!    [`EventScheduleConfig`]. A family that is `Missing` or `Faulted` gives the documented defaults and the
//!    feature stays inert (one warning).
//! 3. **Defines.** Item and monster ids arrive as strings (`"II_..."`, `"MI_..."`); [`WarnOnce::item`] /
//!    [`WarnOnce::mover`] resolve them with `GetDefineNum` semantics: an unknown name is `NULL_ID` and one warning
//!    per distinct string.
//!
//! The exact-minute time functions of the families (`CheckOpenTime`, `CheckPayTime`, `CheckChangeTime`,
//! `IsApplicationTime`, `IsOpenTime`) are never called: the `WallJob`s of [`super::calendar`] reproduce them with
//! boundary crossing (22 §0 E3, PL-100).

use super::calendar::EventScheduleConfig;
use flyff_common::GameMs;
use flyff_data::GameData;
use flyff_data::types::{ItemId, MoverId, NULL_ID};
use flyff_lua::{FamilyStatus, HVal, LuaHost, ScriptFamily};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

const EVENT: ScriptFamily = ScriptFamily::Event;

// ---- warn-once define resolution ---------------------------------------------------------------------------------

/// Remembers which warnings were issued, so a script that names an unknown item on every kill logs it once.
#[derive(Debug, Default)]
pub struct WarnOnce {
    seen: RefCell<BTreeSet<String>>,
}

impl WarnOnce {
    /// `true` the first time `key` is seen.
    pub fn first(&self, key: &str) -> bool {
        self.seen.borrow_mut().insert(key.to_owned())
    }

    /// How many distinct warnings were issued (tests).
    pub fn count(&self) -> usize {
        self.seen.borrow().len()
    }

    /// Forgets every warning (a successful reload of the scripts starts afresh).
    pub fn clear(&self) {
        self.seen.borrow_mut().clear();
    }

    /// `GetDefineNum(name)` for an item (`II_...`): `NULL_ID` and one warning for an unknown string. The marker
    /// `"NoItem"` of the rainbow prize table is `NULL_ID` without a warning.
    pub fn item(&self, data: &GameData, name: &str) -> ItemId {
        self.define(data, name, "item")
    }

    /// `GetDefineNum(name)` for a monster (`MI_...`).
    pub fn mover(&self, data: &GameData, name: &str) -> MoverId {
        self.define(data, name, "monster")
    }

    fn define(&self, data: &GameData, name: &str, what: &str) -> u32 {
        if name == "NoItem" || name.is_empty() {
            return NULL_ID;
        }
        match data.defines.value_u32(name) {
            Some(id) if id != NULL_ID => id,
            _ => {
                if self.first(&format!("{what}:{name}")) {
                    tracing::warn!(
                        name,
                        "an event script names a {what} that is not defined; it is ignored"
                    );
                }
                NULL_ID
            }
        }
    }
}

// ---- Event.lua getters ---------------------------------------------------------------------------------------------

/// One state change returned by `GetEventState` (`nId` is the 1-based insertion order of `AddEvent`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct StateChange {
    /// The event id.
    #[serde(rename = "nId")]
    pub id: u32,
    /// The new `State` (1 = running, 0 = stopped).
    #[serde(rename = "State")]
    pub state: i32,
}

/// One row of `GetAllEventList` (`/eventlist`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EventListEntry {
    /// The event id.
    #[serde(rename = "nId")]
    pub id: u32,
    /// The title (`Desc`).
    #[serde(rename = "strTitle", default)]
    pub title: String,
    /// `State`.
    #[serde(rename = "nState", default)]
    pub state: i32,
}

/// One level-up gift of `GetLevelUpGift(level, account)` (22 §4.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelUpGift {
    /// The `II_*` name.
    pub item_name: String,
    /// Its item id.
    pub item: ItemId,
    /// `nItemNum`.
    pub count: i32,
    /// `byFlag` (2 = character-bound).
    pub flag: u8,
}

/// One entry of `GetSpawn(id)` (22 §4.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnDef {
    /// `ITEM` (4) or `MONSTER` (5).
    pub kind: u32,
    /// The define name.
    pub name: String,
    /// The resolved id (`NULL_ID` for an unknown name).
    pub id: u32,
    /// `nDayNum`: how many per day.
    pub per_day: u32,
}

#[derive(Deserialize)]
struct RawGift {
    #[serde(rename = "strItemId")]
    item: String,
    #[serde(rename = "nItemNum")]
    count: f64,
    #[serde(rename = "byFlag", default)]
    flag: f64,
}

#[derive(Deserialize)]
struct RawDrop {
    #[serde(rename = "ItemId")]
    item: String,
    #[serde(rename = "ItemNum")]
    count: f64,
}

#[derive(Deserialize)]
struct RawKeepItem {
    #[serde(rename = "strItemId")]
    item: String,
    #[serde(rename = "nItemNum")]
    count: f64,
}

#[derive(Deserialize)]
struct RawSpawn {
    #[serde(rename = "nType")]
    kind: f64,
    #[serde(rename = "strId")]
    name: String,
    #[serde(rename = "nDayNum")]
    per_day: f64,
}

/// The spawn lines of what `GetSpawn` returned: `(list, size)` from the real script, the list alone from a simpler
/// one, nothing for an empty table (which the host presents as an empty object) or `nil`. A line that does not
/// read as a spawn line is skipped with a warning.
fn spawn_lines(v: serde_json::Value) -> Vec<RawSpawn> {
    use serde_json::Value;
    let Value::Array(items) = v else {
        return Vec::new();
    };
    // the host presents several results as a sequence: `(tSpawn, #tSpawn)` is `[list, size]`; the lines of a list
    // alone are objects, never a number
    let list = if items.len() == 2 && items[1].is_number() {
        match items.into_iter().next() {
            Some(Value::Array(list)) => list,
            _ => Vec::new(),
        }
    } else {
        items
    };
    list.into_iter()
        .filter_map(|e| match serde_json::from_value::<RawSpawn>(e) {
            Ok(line) => Some(line),
            Err(err) => {
                tracing::warn!(error = %err, "an Event.lua spawn line is malformed: it is skipped");
                None
            }
        })
        .collect()
}

/// The typed wrappers over the `Event` family (22 §2.5 rule 1). A cheap `Copy` view: the host, the data and the
/// warn-once memory.
#[derive(Clone, Copy)]
pub struct EventScripts<'a> {
    lua: &'a LuaHost,
    data: &'a GameData,
    warned: &'a WarnOnce,
}

impl<'a> EventScripts<'a> {
    /// A view over `lua` and `data`.
    pub fn new(lua: &'a LuaHost, data: &'a GameData, warned: &'a WarnOnce) -> Self {
        Self { lua, data, warned }
    }

    /// `true` when the `Event` family is `Ready` (the legacy `IsPossible()`); every getter is neutral otherwise.
    pub fn ready(&self) -> bool {
        self.lua.family_status(EVENT) == FamilyStatus::Ready
    }

    /// A getter that returns a number: `None` for a missing function, a fault or a family that is not ready.
    fn num(&self, name: &str, args: &[HVal]) -> Option<f64> {
        self.call(name, args)
    }

    fn call<R: serde::de::DeserializeOwned>(&self, name: &str, args: &[HVal]) -> Option<R> {
        if !self.ready() {
            return None;
        }
        match self.lua.call_opt::<R>(EVENT, name, args) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(function = name, error = %e, "Event.lua getter failed; its neutral value is used");
                None
            }
        }
    }

    /// `GetExpFactor`: the product of the EXP factors of the running events (`(float)`), 1.0 neutral.
    pub fn exp_factor(&self) -> f32 {
        self.num("GetExpFactor", &[]).map_or(1.0, |v| v as f32)
    }

    /// `GetItemDropRate`.
    pub fn item_drop_rate(&self) -> f32 {
        self.num("GetItemDropRate", &[]).map_or(1.0, |v| v as f32)
    }

    /// `GetPieceItemDropRate`.
    pub fn piece_item_drop_rate(&self) -> f32 {
        self.num("GetPieceItemDropRate", &[])
            .map_or(1.0, |v| v as f32)
    }

    /// `GetGoldDropFactor`.
    pub fn gold_drop_factor(&self) -> f32 {
        self.num("GetGoldDropFactor", &[]).map_or(1.0, |v| v as f32)
    }

    /// `GetAttackPower` (`(int)`): `1 + sum` while Event.lua is loaded (KQ-11), 0 neutral.
    pub fn attack_power(&self) -> i32 {
        self.num("GetAttackPower", &[]).map_or(0, |v| v as i32)
    }

    /// `GetDefensePower` (`(int)`).
    pub fn defense_power(&self) -> i32 {
        self.num("GetDefensePower", &[]).map_or(0, |v| v as i32)
    }

    /// `GetCouponEvent` in ms (`(DWORD)`): the first non-zero value of the running events, 0 = off.
    pub fn coupon_event_ms(&self) -> u32 {
        self.num("GetCouponEvent", &[]).map_or(0, |v| v as u32)
    }

    /// `GetCheerExpFactor`.
    pub fn cheer_exp_factor(&self) -> f32 {
        self.num("GetCheerExpFactor", &[]).map_or(1.0, |v| v as f32)
    }

    /// `GetKeepConnectTime` in ms (`(DWORD)`), 0 = off.
    pub fn keep_connect_ms(&self) -> u32 {
        self.num("GetKeepConnectTime", &[]).map_or(0, |v| v as u32)
    }

    /// `GetKeepConnectItem`: every keep-connect item of every running event (an unknown item is dropped).
    pub fn keep_connect_items(&self) -> Vec<(ItemId, i32)> {
        self.call::<Vec<RawKeepItem>>("GetKeepConnectItem", &[])
            .unwrap_or_default()
            .into_iter()
            .filter_map(|i| {
                let id = self.warned.item(self.data, &i.item);
                (id != NULL_ID).then_some((id, i.count as i32))
            })
            .collect()
    }

    /// `GetWeatherEventExpFactor`: missing in the shipped script, so 1.0 (KQ-11).
    pub fn weather_exp_factor(&self) -> f32 {
        self.num("GetWeatherEventExpFactor", &[])
            .map_or(1.0, |v| v as f32)
    }

    /// `GetWeatherEventTitle`: missing in the shipped script.
    pub fn weather_title(&self) -> Option<String> {
        self.call("GetWeatherEventTitle", &[])
    }

    /// `GetRainEventExpFactor`.
    pub fn rain_exp_factor(&self) -> f32 {
        self.num("GetRainEventExpFactor", &[])
            .map_or(1.0, |v| v as f32)
    }

    /// `GetRainEventTitle`: the title of the first running event whose factor is above 1, else `None`.
    pub fn rain_title(&self) -> Option<String> {
        self.call("GetRainEventTitle", &[])
    }

    /// `GetSnowEventExpFactor`.
    pub fn snow_exp_factor(&self) -> f32 {
        self.num("GetSnowEventExpFactor", &[])
            .map_or(1.0, |v| v as f32)
    }

    /// `GetSnowEventTitle`.
    pub fn snow_title(&self) -> Option<String> {
        self.call("GetSnowEventTitle", &[])
    }

    /// `GetShopBuyFactor`: missing in the shipped script, so 1.0 (KQ-11).
    pub fn shop_buy_factor(&self) -> f32 {
        self.num("GetShopBuyFactor", &[]).map_or(1.0, |v| v as f32)
    }

    /// `GetShopSellFactor`: missing in the shipped script, so 1.0 (KQ-11).
    pub fn shop_sell_factor(&self) -> f32 {
        self.num("GetShopSellFactor", &[]).map_or(1.0, |v| v as f32)
    }

    /// `GetEventState()`: flips the `State` of the events whose window opened or closed and returns the changes in
    /// script order (22 §4.2). Empty when the family is not ready.
    pub fn event_state(&self) -> Vec<StateChange> {
        self.call("GetEventState", &[]).unwrap_or_default()
    }

    /// `GetEventList()`: the ids of the running events, ascending.
    pub fn event_list(&self) -> Vec<u32> {
        self.call("GetEventList", &[]).unwrap_or_default()
    }

    /// `GetAllEventList()`: every event with its title and state (`/eventlist`).
    pub fn all_event_list(&self) -> Vec<EventListEntry> {
        self.call("GetAllEventList", &[]).unwrap_or_default()
    }

    /// `GetEventInfo(id)`: the description lines of one event (`/eventinfo`).
    pub fn event_info(&self, id: u32) -> Vec<String> {
        self.call("GetEventInfo", &[HVal::from(id)])
            .unwrap_or_default()
    }

    /// `GetDesc(id)` of an event the script itself reported; empty for a missing function or a fault.
    pub fn desc(&self, id: u32) -> String {
        self.call("GetDesc", &[HVal::from(id)]).unwrap_or_default()
    }

    /// `GetLevelUpGift(level, account)` (22 §4.6): the gifts for reaching `level`, in script order. Lua matches
    /// `strAccount == "all"` or `string.find(account, strAccount)`. An unknown item is skipped with a warning.
    pub fn level_up_gifts(&self, level: u32, account: &str) -> Vec<LevelUpGift> {
        let raw: Vec<RawGift> = self
            .call("GetLevelUpGift", &[HVal::from(level), HVal::from(account)])
            .unwrap_or_default();
        raw.into_iter()
            .filter_map(|g| {
                let item = self.warned.item(self.data, &g.item);
                (item != NULL_ID).then_some(LevelUpGift {
                    item_name: g.item,
                    item,
                    count: g.count as i32,
                    flag: g.flag as u8,
                })
            })
            .collect()
    }

    /// `GetItem(tick, monsterLevel)` (22 §4.7): the event items a monster kill drops now, as `(item, count)`.
    /// The C++ collected them in a `map<item, count>`: **KEEP-QUIRK** a duplicate item id keeps the first count
    /// and the result is ordered by ascending item id. The pacing state (`TimeOut`, `Skip`) lives in Lua.
    pub fn event_items(&self, game_ms: GameMs, monster_level: u32) -> Vec<(ItemId, i32)> {
        let raw: Vec<RawDrop> = self
            .call(
                "GetItem",
                &[
                    HVal::Int(i64::from(game_ms.0 as u32)),
                    HVal::from(monster_level),
                ],
            )
            .unwrap_or_default();
        let mut by_item: BTreeMap<ItemId, i32> = BTreeMap::new();
        for d in raw {
            let id = self.warned.item(self.data, &d.item);
            if id != NULL_ID {
                by_item.entry(id).or_insert(d.count as i32);
            }
        }
        by_item.into_iter().collect()
    }

    /// `GetSpawn(id)` (22 §4.10): the spawn entries of an event. The real `EventFunc.lua` returns TWO values,
    /// `(tSpawn, #tSpawn)` (the C++ read the size from the top of the stack), which the host presents as a
    /// sequence; a script may also return the list alone (task event-lua-extras: the first reading of 22 §4.10
    /// expected only that and found no spawn line in the real script).
    pub fn spawn(&self, id: u32) -> Vec<SpawnDef> {
        let raw = spawn_lines(
            self.call::<serde_json::Value>("GetSpawn", &[HVal::from(id)])
                .unwrap_or(serde_json::Value::Null),
        );
        raw.into_iter()
            .map(|s| {
                let kind = s.kind as u32;
                let resolved = match kind {
                    4 => self.warned.item(self.data, &s.name),
                    _ => self.warned.mover(self.data, &s.name),
                };
                SpawnDef {
                    kind,
                    id: resolved,
                    name: s.name,
                    per_day: s.per_day as u32,
                }
            })
            .collect()
    }

    /// `IsNoticeTime()` (22 §4.11): requires `bNotice`, compares `"%a %H:%M"` with the (pinned) minute, counts down.
    pub fn is_notice_time(&self) -> bool {
        self.call("IsNoticeTime", &[]).unwrap_or(false)
    }

    /// `GetNoticeMessage()`: the lines of the auto notice.
    pub fn notice_messages(&self) -> Vec<String> {
        self.call("GetNoticeMessage", &[]).unwrap_or_default()
    }
}

// ---- weekday and time strings ----------------------------------------------------------------------------------------

/// `0` = Sunday ... `6` = Saturday for the English `%a` names the scripts use.
pub fn weekday_of(name: &str) -> Option<u8> {
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        .iter()
        .position(|d| d.eq_ignore_ascii_case(name))
        .and_then(|i| u8::try_from(i).ok())
}

/// `"Sun 18:00"` -> `(0, 18, 0)`.
pub fn parse_weekday_time(s: &str) -> Option<(u8, u8, u8)> {
    let (day, hm) = s.trim().split_once(char::is_whitespace)?;
    let (h, m) = parse_hm(hm)?;
    Some((weekday_of(day)?, h, m))
}

/// `"00:00"` -> `(0, 0)`.
pub fn parse_hm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.trim().parse::<u8>().ok()?, m.trim().parse::<u8>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

// ---- reading globals ---------------------------------------------------------------------------------------------------

/// The global reader of one family: a missing global or a conversion error is `None`.
struct Globals<'a> {
    lua: &'a LuaHost,
    family: ScriptFamily,
}

impl Globals<'_> {
    fn get<R: serde::de::DeserializeOwned>(&self, name: &str) -> Option<R> {
        match self.lua.global::<R>(self.family, name) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    family = self.family.logical_name(),
                    global = name,
                    error = %e,
                    "script global could not be read; the default is used"
                );
                None
            }
        }
    }

    fn i32_or(&self, name: &str, default: i32) -> i32 {
        self.get::<f64>(name).map_or(default, |v| v as i32)
    }

    fn u32_or(&self, name: &str, default: u32) -> u32 {
        self.get::<f64>(name).map_or(default, |v| v as u32)
    }

    fn string_or(&self, name: &str, default: &str) -> String {
        self.get::<String>(name)
            .unwrap_or_else(|| default.to_owned())
    }
}

/// `true` when `family` can be read; otherwise one warning and the caller keeps its defaults.
fn readable(lua: &LuaHost, warned: &WarnOnce, family: ScriptFamily) -> bool {
    let ready = lua.family_status(family) == FamilyStatus::Ready;
    if !ready && warned.first(&format!("family:{}", family.logical_name())) {
        tracing::warn!(
            family = family.logical_name(),
            "the script family is not ready: its feature uses the built-in defaults and stays inert"
        );
    }
    ready
}

// ---- SecretRoom.lua ----------------------------------------------------------------------------------------------------

/// `MONSTER_NORMAL`, `MONSTER_MIDBOSS`, `MONSTER_BOSS` of `SecretRoomFunc.lua`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SrMonsterKind {
    /// 1.
    Normal = 1,
    /// 2.
    MidBoss = 2,
    /// 3.
    Boss = 3,
}

impl SrMonsterKind {
    fn from_lua(n: f64) -> Option<SrMonsterKind> {
        match n as i32 {
            1 => Some(SrMonsterKind::Normal),
            2 => Some(SrMonsterKind::MidBoss),
            3 => Some(SrMonsterKind::Boss),
            _ => None,
        }
    }
}

/// One `AddMonster(type, id, num, red, x1, z1, x2, z2, y)` row.
#[derive(Clone, Debug, PartialEq)]
pub struct SrMonster {
    /// The monster class of the row.
    pub kind: SrMonsterKind,
    /// The `MI_*` name.
    pub name: String,
    /// Its mover id (`NULL_ID` for an unknown name: the row spawns nothing).
    pub mover: MoverId,
    /// How many.
    pub num: i32,
    /// `bRed`: aggressive ("red") monster.
    pub red: bool,
    /// First corner x.
    pub x1: i32,
    /// First corner z.
    pub z1: i32,
    /// Second corner x.
    pub x2: i32,
    /// Second corner z.
    pub z2: i32,
    /// Height.
    pub y: i32,
}

#[derive(Deserialize)]
struct RawSrMonster {
    #[serde(rename = "nType")]
    kind: f64,
    #[serde(rename = "dwId")]
    name: String,
    #[serde(rename = "nNum", default)]
    num: f64,
    #[serde(rename = "bRed", default)]
    red: bool,
    #[serde(default)]
    x1: f64,
    #[serde(default)]
    z1: f64,
    #[serde(default)]
    x2: f64,
    #[serde(default)]
    z2: f64,
    #[serde(default)]
    y: f64,
}

/// `SecretRoom.lua` (22 §11.1), read once at start and after `/lua` reloads.
#[derive(Clone, Debug, PartialEq)]
pub struct SecretRoomConfig {
    /// `MinGuildLevel`.
    pub min_guild_level: i32,
    /// `MinGuildMemberLevel`.
    pub min_guild_member_level: i32,
    /// `MinPenya`.
    pub min_penya: i32,
    /// `MinGuildNum`.
    pub min_guild_num: i32,
    /// `MaxGuildNum` (per side).
    pub max_guild_num: i32,
    /// `MinGuildMemberNum`.
    pub min_guild_member_num: i32,
    /// `MaxGuildMemberNum`.
    pub max_guild_member_num: i32,
    /// `CancelReturnRate` in percent.
    pub cancel_return_rate: i32,
    /// `DropoutReturnRate` in percent.
    pub dropout_return_rate: i32,
    /// `OpenTime`, e.g. `"Sun 18:00"`.
    pub open_time: String,
    /// `LineUpTime` in ms.
    pub line_up_time_ms: u32,
    /// `EntranceTime` in ms.
    pub entrance_time_ms: u32,
    /// `WarWaitTime` in ms.
    pub war_wait_time_ms: u32,
    /// `WarTime` in ms.
    pub war_time_ms: u32,
    /// `CloseWaitTime` in ms.
    pub close_wait_time_ms: u32,
    /// `MonsterGenNum`: how many monsters one spawn step creates.
    pub monster_gen_num: i32,
    /// `tMonster` in script order.
    pub monsters: Vec<SrMonster>,
}

impl Default for SecretRoomConfig {
    /// The shipped numbers with an empty monster table: the feature is inert without its script.
    fn default() -> Self {
        SecretRoomConfig {
            min_guild_level: 4,
            min_guild_member_level: 30,
            min_penya: 100_000_000,
            min_guild_num: 1,
            max_guild_num: 8,
            min_guild_member_num: 1,
            max_guild_member_num: 11,
            cancel_return_rate: 20,
            dropout_return_rate: 2,
            open_time: "Sun 18:00".to_owned(),
            line_up_time_ms: 60_000,
            entrance_time_ms: 0,
            war_wait_time_ms: 60_000,
            war_time_ms: 7_200_000,
            close_wait_time_ms: 30_000,
            monster_gen_num: 10,
            monsters: Vec::new(),
        }
    }
}

impl SecretRoomConfig {
    /// Reads the `SecretRoom` family; the defaults when it is not `Ready`.
    pub fn read(lua: &LuaHost, data: &GameData, warned: &WarnOnce) -> SecretRoomConfig {
        let mut c = SecretRoomConfig::default();
        if !readable(lua, warned, ScriptFamily::SecretRoom) {
            return c;
        }
        let g = Globals {
            lua,
            family: ScriptFamily::SecretRoom,
        };
        c.min_guild_level = g.i32_or("MinGuildLevel", c.min_guild_level);
        c.min_guild_member_level = g.i32_or("MinGuildMemberLevel", c.min_guild_member_level);
        c.min_penya = g.i32_or("MinPenya", c.min_penya);
        c.min_guild_num = g.i32_or("MinGuildNum", c.min_guild_num);
        c.max_guild_num = g.i32_or("MaxGuildNum", c.max_guild_num);
        c.min_guild_member_num = g.i32_or("MinGuildMemberNum", c.min_guild_member_num);
        c.max_guild_member_num = g.i32_or("MaxGuildMemberNum", c.max_guild_member_num);
        c.cancel_return_rate = g.i32_or("CancelReturnRate", c.cancel_return_rate);
        c.dropout_return_rate = g.i32_or("DropoutReturnRate", c.dropout_return_rate);
        c.open_time = g.string_or("OpenTime", &c.open_time);
        c.line_up_time_ms = g.u32_or("LineUpTime", c.line_up_time_ms);
        c.entrance_time_ms = g.u32_or("EntranceTime", c.entrance_time_ms);
        c.war_wait_time_ms = g.u32_or("WarWaitTime", c.war_wait_time_ms);
        c.war_time_ms = g.u32_or("WarTime", c.war_time_ms);
        c.close_wait_time_ms = g.u32_or("CloseWaitTime", c.close_wait_time_ms);
        c.monster_gen_num = g.i32_or("MonsterGenNum", c.monster_gen_num);
        let rows: Vec<RawSrMonster> = g.get("tMonster").unwrap_or_default();
        c.monsters = rows
            .into_iter()
            .filter_map(|r| {
                let Some(kind) = SrMonsterKind::from_lua(r.kind) else {
                    tracing::warn!(
                        kind = r.kind,
                        "SecretRoom.lua: AddMonster with an unknown type; the row is ignored"
                    );
                    return None;
                };
                Some(SrMonster {
                    kind,
                    mover: warned.mover(data, &r.name),
                    name: r.name,
                    num: r.num as i32,
                    red: r.red,
                    x1: r.x1 as i32,
                    z1: r.z1 as i32,
                    x2: r.x2 as i32,
                    z2: r.z2 as i32,
                    y: r.y as i32,
                })
            })
            .collect();
        c
    }

    /// `m_mapMonsterNum`: how many monsters of `kind` the table creates in total (`sum nNum`).
    pub fn monster_total(&self, kind: SrMonsterKind) -> i32 {
        self.monsters
            .iter()
            .filter(|m| m.kind == kind)
            .map(|m| m.num)
            .sum()
    }
}

// ---- Tax.lua -------------------------------------------------------------------------------------------------------

/// `Tax.lua` (22 §10.1).
#[derive(Clone, Debug, PartialEq)]
pub struct TaxConfig {
    /// `MinTaxRate` in percent.
    pub min_tax_rate: i32,
    /// `MaxTaxRate`.
    pub max_tax_rate: i32,
    /// `TaxSecretRoomRate`: the occupying guild's share of the taxes, percent.
    pub tax_secret_room_rate: i32,
    /// `TaxLordRate`: the lord's share, percent.
    pub tax_lord_rate: i32,
    /// `AdmissionSecretRoomRate`.
    pub admission_secret_room_rate: i32,
    /// `AdmissionLordRate`.
    pub admission_lord_rate: i32,
    /// `PayTime`, `"00:00"`.
    pub pay_time: String,
    /// `ChangeTaxTime`, `"Mon 00:00"`.
    pub change_tax_time: String,
    /// `DBSaveCount` (superseded by the 10 s rule of doc 13).
    pub db_save_count: i32,
    /// `tOccupationShopItem`: the items only the occupying guild may buy.
    pub occupation_shop_items: Vec<ItemId>,
}

impl Default for TaxConfig {
    /// The shipped numbers; no occupation shop item.
    fn default() -> Self {
        TaxConfig {
            min_tax_rate: 5,
            max_tax_rate: 20,
            tax_secret_room_rate: 45,
            tax_lord_rate: 45,
            admission_secret_room_rate: 45,
            admission_lord_rate: 45,
            pay_time: "00:00".to_owned(),
            change_tax_time: "Mon 00:00".to_owned(),
            db_save_count: 10,
            occupation_shop_items: Vec::new(),
        }
    }
}

impl TaxConfig {
    /// Reads the `Tax` family; the defaults when it is not `Ready`.
    pub fn read(lua: &LuaHost, data: &GameData, warned: &WarnOnce) -> TaxConfig {
        let mut c = TaxConfig::default();
        if !readable(lua, warned, ScriptFamily::Tax) {
            return c;
        }
        let g = Globals {
            lua,
            family: ScriptFamily::Tax,
        };
        c.min_tax_rate = g.i32_or("MinTaxRate", c.min_tax_rate);
        c.max_tax_rate = g.i32_or("MaxTaxRate", c.max_tax_rate);
        c.tax_secret_room_rate = g.i32_or("TaxSecretRoomRate", c.tax_secret_room_rate);
        c.tax_lord_rate = g.i32_or("TaxLordRate", c.tax_lord_rate);
        c.admission_secret_room_rate =
            g.i32_or("AdmissionSecretRoomRate", c.admission_secret_room_rate);
        c.admission_lord_rate = g.i32_or("AdmissionLordRate", c.admission_lord_rate);
        c.pay_time = g.string_or("PayTime", &c.pay_time);
        c.change_tax_time = g.string_or("ChangeTaxTime", &c.change_tax_time);
        c.db_save_count = g.i32_or("DBSaveCount", c.db_save_count);
        let names: Vec<String> = g.get("tOccupationShopItem").unwrap_or_default();
        c.occupation_shop_items = names
            .iter()
            .map(|n| warned.item(data, n))
            .filter(|id| *id != NULL_ID)
            .collect();
        c
    }
}

// ---- RainbowRace.lua ---------------------------------------------------------------------------------------------------

/// One `SetTime(weekday, start, end, open, fee)` row (22 §14.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RrTime {
    /// `nStartTime`: the application window opens, `HHMM`.
    pub start_hm: u16,
    /// `nEndTime`: the application window closes, `HHMM`.
    pub end_hm: u16,
    /// `nOpenTime`: the race opens, `HHMM`.
    pub open_hm: u16,
    /// `nPayPenya`: the entry fee.
    pub fee: i64,
}

#[derive(Deserialize)]
struct RawRrTime {
    #[serde(rename = "nStartTime")]
    start: f64,
    #[serde(rename = "nEndTime")]
    end: f64,
    #[serde(rename = "nOpenTime")]
    open: f64,
    #[serde(rename = "nPayPenya", default)]
    fee: f64,
}

/// One `SetNPC(id, key, x, y, z)` row.
#[derive(Clone, Debug, PartialEq)]
pub struct RrNpc {
    /// The `MI_*` name.
    pub name: String,
    /// Its mover id (`NULL_ID` when unknown).
    pub mover: MoverId,
    /// The `character.inc` key (`MaFl_RainbowRed`).
    pub char_key: String,
    /// Position x.
    pub x: f32,
    /// Position y.
    pub y: f32,
    /// Position z.
    pub z: f32,
}

#[derive(Deserialize)]
struct RawRrNpc {
    #[serde(rename = "strNPCId")]
    name: String,
    #[serde(rename = "strCharKey", default)]
    key: String,
    #[serde(rename = "xPos", default)]
    x: f64,
    #[serde(rename = "yPos", default)]
    y: f64,
    #[serde(rename = "zPos", default)]
    z: f64,
}

/// One `SetPrize` rank row: `penya` is the fee times the rate (`nPenya` as Lua computed it).
#[derive(Clone, Debug, PartialEq)]
pub struct RrPrize {
    /// The item (`NULL_ID` for `"NoItem"`).
    pub item: ItemId,
    /// `nItemNum`.
    pub count: i32,
    /// `byFlag`.
    pub flag: u8,
    /// `nPenya`.
    pub penya: f64,
}

#[derive(Deserialize)]
struct RawRrPrize {
    #[serde(rename = "strItemId", default)]
    item: String,
    #[serde(rename = "nItemNum", default)]
    count: f64,
    #[serde(rename = "byFlag", default)]
    flag: f64,
    #[serde(rename = "nPenya", default)]
    penya: f64,
}

/// One `SetMiniGamePrize` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RrMiniGamePrize {
    /// The item (`NULL_ID` when unknown).
    pub item: ItemId,
    /// `nItemNum`.
    pub count: i32,
    /// `byFlag`.
    pub flag: u8,
}

#[derive(Deserialize)]
struct RawRrMini {
    #[serde(rename = "strItemId", default)]
    item: String,
    #[serde(rename = "nItemNum", default)]
    count: f64,
    #[serde(rename = "byFlag", default)]
    flag: f64,
}

/// `RainbowRace.lua` (22 §14.1), read at start and after `/lua rr`.
#[derive(Clone, Debug, PartialEq)]
pub struct RainbowRaceConfig {
    /// `tTime` by weekday (0 = Sunday).
    pub times: BTreeMap<u8, RrTime>,
    /// `Wait_Time` in ms.
    pub wait_time_ms: u32,
    /// `Ready_Time` in ms.
    pub ready_time_ms: u32,
    /// `Progress_Time` in ms.
    pub progress_time_ms: u32,
    /// `tNPC`.
    pub npcs: Vec<RrNpc>,
    /// `tPrize` by weekday: rank 1 first.
    pub prizes: BTreeMap<u8, Vec<RrPrize>>,
    /// `tMiniGamePrize` by weekday and completed count.
    pub mini_game_prizes: BTreeMap<u8, BTreeMap<u32, RrMiniGamePrize>>,
}

/// The entry fee when today has no `SetTime` row (the legacy `GetPayPenya` default).
pub const RR_DEFAULT_FEE: i64 = 1_000_000;

impl Default for RainbowRaceConfig {
    /// The shipped Saturday row and timings; no NPC and no prize, so the race cannot run without its script.
    fn default() -> Self {
        let mut times = BTreeMap::new();
        times.insert(
            6,
            RrTime {
                start_hm: 1200,
                end_hm: 2000,
                open_hm: 2030,
                fee: RR_DEFAULT_FEE,
            },
        );
        RainbowRaceConfig {
            times,
            wait_time_ms: 1_800_000,
            ready_time_ms: 600_000,
            progress_time_ms: 7_200_000,
            npcs: Vec::new(),
            prizes: BTreeMap::new(),
            mini_game_prizes: BTreeMap::new(),
        }
    }
}

impl RainbowRaceConfig {
    /// Reads the `RainbowRace` family; the defaults when it is not `Ready`. The tables are read by weekday instead
    /// of calling `IsApplicationTime` / `IsOpenTime` / `GetPayPenya` / `GetPrize` / `GetMiniGamePrize` (22 §0 E3).
    pub fn read(lua: &LuaHost, data: &GameData, warned: &WarnOnce) -> RainbowRaceConfig {
        let mut c = RainbowRaceConfig::default();
        if !readable(lua, warned, ScriptFamily::RainbowRace) {
            return c;
        }
        let g = Globals {
            lua,
            family: ScriptFamily::RainbowRace,
        };
        c.wait_time_ms = g.u32_or("Wait_Time", c.wait_time_ms);
        c.ready_time_ms = g.u32_or("Ready_Time", c.ready_time_ms);
        c.progress_time_ms = g.u32_or("Progress_Time", c.progress_time_ms);
        // the script's own rows replace the shipped default row
        c.times = by_weekday::<RawRrTime>(&g, "tTime")
            .into_iter()
            .map(|(d, r)| {
                (
                    d,
                    RrTime {
                        start_hm: r.start as u16,
                        end_hm: r.end as u16,
                        open_hm: r.open as u16,
                        fee: r.fee as i64,
                    },
                )
            })
            .collect();
        let npcs: Vec<RawRrNpc> = g.get("tNPC").unwrap_or_default();
        c.npcs = npcs
            .into_iter()
            .map(|n| RrNpc {
                mover: warned.mover(data, &n.name),
                name: n.name,
                char_key: n.key,
                x: n.x as f32,
                y: n.y as f32,
                z: n.z as f32,
            })
            .collect();
        c.prizes = by_weekday::<Vec<RawRrPrize>>(&g, "tPrize")
            .into_iter()
            .map(|(d, rows)| {
                let rows = rows
                    .into_iter()
                    .map(|r| RrPrize {
                        item: warned.item(data, &r.item),
                        count: r.count as i32,
                        flag: r.flag as u8,
                        penya: r.penya,
                    })
                    .collect();
                (d, rows)
            })
            .collect();
        c.mini_game_prizes = by_weekday::<BTreeMap<u32, RawRrMini>>(&g, "tMiniGamePrize")
            .into_iter()
            .map(|(d, rows)| {
                let rows = rows
                    .into_iter()
                    .map(|(n, r)| {
                        (
                            n,
                            RrMiniGamePrize {
                                item: warned.item(data, &r.item),
                                count: r.count as i32,
                                flag: r.flag as u8,
                            },
                        )
                    })
                    .collect();
                (d, rows)
            })
            .collect();
        c
    }

    /// The entry fee of `weekday`: its `SetTime` row, else [`RR_DEFAULT_FEE`] (the legacy `GetPayPenya`).
    pub fn fee_on(&self, weekday: u8) -> i64 {
        self.times.get(&weekday).map_or(RR_DEFAULT_FEE, |t| t.fee)
    }
}

/// A table keyed by weekday names (`tTime["Sat"]`) as `(weekday number, row)`; unknown names are ignored.
fn by_weekday<R: serde::de::DeserializeOwned>(g: &Globals<'_>, name: &str) -> BTreeMap<u8, R> {
    let raw: BTreeMap<String, R> = g.get(name).unwrap_or_default();
    raw.into_iter()
        .filter_map(|(day, row)| match weekday_of(&day) {
            Some(d) => Some((d, row)),
            None => {
                tracing::warn!(table = name, key = %day, "RainbowRace.lua: unknown weekday name; the row is ignored");
                None
            }
        })
        .collect()
}

// ---- the four configs together ----------------------------------------------------------------------------------------

/// The typed script configs of the event services and the schedule derived from them (22 §2.5 rule 2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventConfigs {
    /// `SecretRoom.lua`.
    pub secret_room: SecretRoomConfig,
    /// `Tax.lua`.
    pub tax: TaxConfig,
    /// `RainbowRace.lua`.
    pub rainbow: RainbowRaceConfig,
    /// The wall-job specs and the application windows.
    pub schedule: EventScheduleConfig,
}

impl EventConfigs {
    /// Reads the three families and derives the schedule.
    pub fn read(lua: &LuaHost, data: &GameData, warned: &WarnOnce) -> EventConfigs {
        let secret_room = SecretRoomConfig::read(lua, data, warned);
        let tax = TaxConfig::read(lua, data, warned);
        let rainbow = RainbowRaceConfig::read(lua, data, warned);
        let schedule = EventScheduleConfig::from_configs(&secret_room, &tax, &rainbow);
        EventConfigs {
            secret_room,
            tax,
            rainbow,
            schedule,
        }
    }
}
