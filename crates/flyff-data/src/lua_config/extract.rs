//! Typed extraction of the configuration globals after a script ran (Plan/14 §6.17).
//!
//! Each reader pulls the documented globals/tables out of the finished VM with `mlua`'s serde bridge into
//! private "raw" mirrors of the Lua tables (field names as in the scripts), then projects them into the
//! public types of [`crate::lua_config`], resolving define names through [`Resolver`]. A table that does
//! not have the expected shape is an `Err(message)`; the caller reports it as `LuaError` and leaves that
//! config `None`/empty.
//!
//! Numbers are read as `f64` (`Num`) because Lua 5.3 distinguishes integers and floats and a script
//! may compute either (`dwSkillTime * 1000`, `nPayPenya * rate`); the conversion is the C cast
//! (truncate, saturate), as `lua_tonumber` + `(int)` did in the original.

use super::{
    AttributeUpgrade, CampusBuff, CampusConfig, CampusQuest, CampusReward, CreateMonsterConfig,
    CreateMonsterEntry, CreateMonsterItem, DungeonDef, DungeonMonster, DungeonTeleport,
    EventMonsterDef, ItemUpgradeConfig, MonsterSkillDef, MonsterSkillEntry, PartyDungeonConfig,
    QuizConfig, QuizNpcDef,
};
use crate::types::NULL_ID;
use flyff_script::{Defines, DiagCode, Diagnostics, Severity};
use mlua::{LuaSerdeExt, Table, Value};
use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

// ---- helpers -------------------------------------------------------------------------------------------------

/// A Lua number (integer or float, or a numeric string as `lua_tonumber` accepts).
#[derive(Clone, Copy, Debug, Default)]
struct Num(f64);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Num;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Num, E> {
                Ok(Num(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Num, E> {
                v.trim()
                    .parse::<f64>()
                    .map(Num)
                    .map_err(|_| E::custom(format!("expected a number, found the string {v:?}")))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Num, E> {
                Ok(Num(0.0)) // nil: `lua_tonumber` of a missing field is 0
            }
        }
        d.deserialize_any(V)
    }
}

impl Num {
    fn i32(self) -> i32 {
        self.0 as i32
    }
    fn i64(self) -> i64 {
        self.0 as i64
    }
    fn u32(self) -> u32 {
        self.0 as u32
    }
    fn f32(self) -> f32 {
        self.0 as f32
    }
}

/// A Lua truth value (`nil`/`false` are false, `true` and any number other than 0 are true).
#[derive(Clone, Copy, Debug, Default)]
struct Truthy(bool);

impl<'de> Deserialize<'de> for Truthy {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Truthy;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Truthy, E> {
                Ok(Truthy(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Truthy, E> {
                Ok(Truthy(v != 0))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Truthy, E> {
                Ok(Truthy(v != 0.0))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Truthy, E> {
                Ok(Truthy(false))
            }
        }
        d.deserialize_any(V)
    }
}

/// Resolves define names and reports the unknown ones once per name.
pub struct Resolver<'a> {
    defines: &'a Arc<Defines>,
    diags: &'a mut Diagnostics,
    file: &'a str,
    seen: BTreeSet<String>,
}

impl<'a> Resolver<'a> {
    /// A resolver for the diagnostics of `file`.
    pub fn new(defines: &'a Arc<Defines>, diags: &'a mut Diagnostics, file: &'a str) -> Self {
        Self {
            defines,
            diags,
            file,
            seen: BTreeSet::new(),
        }
    }

    /// `GetDefineNum(name)`: the define as `u32`, or `NULL_ID` plus an Error `UnknownIdentifier`.
    pub fn id(&mut self, name: &str) -> u32 {
        match self.defines.get(name) {
            Some(v) => v as u32,
            None => {
                if self.seen.insert(name.to_owned()) {
                    let file: Arc<str> = Arc::from(self.file);
                    self.diags.report(
                        Severity::Error,
                        DiagCode::UnknownIdentifier,
                        Some(&file),
                        0,
                        format!(
                            "{}: define {name:?} is not defined (NULL_ID used)",
                            self.file
                        ),
                    );
                }
                NULL_ID
            }
        }
    }
}

fn global(g: &Table, name: &str) -> Result<Value, String> {
    g.get::<Value>(name).map_err(|e| format!("{name}: {e}"))
}

fn list<T: for<'de> Deserialize<'de>>(
    lua: &mlua::Lua,
    g: &Table,
    name: &str,
) -> Result<Vec<T>, String> {
    match global(g, name)? {
        Value::Nil => Ok(Vec::new()),
        v => lua
            .from_value::<Vec<T>>(v)
            .map_err(|e| format!("{name}: {e}")),
    }
}

fn scalar<T: for<'de> Deserialize<'de> + Default>(
    lua: &mlua::Lua,
    g: &Table,
    name: &str,
) -> Result<T, String> {
    match global(g, name)? {
        Value::Nil => Ok(T::default()),
        v => lua.from_value::<T>(v).map_err(|e| format!("{name}: {e}")),
    }
}

fn pos(x: Num, y: Num, z: Num) -> [f32; 3] {
    [x.f32(), y.f32(), z.f32()]
}

// ---- PartyDungeon.lua ----------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawDungeon {
    #[serde(rename = "strWorldId", default)]
    world: String,
    #[serde(rename = "dwClass", default)]
    class: Num,
    #[serde(rename = "nMinLevel", default)]
    min_level: Num,
    #[serde(rename = "nMaxLevel", default)]
    max_level: Num,
    #[serde(rename = "dwCoolTime", default)]
    cool_time: Num,
    #[serde(rename = "tMonster", default)]
    monsters: Vec<RawDungeonMonster>,
    #[serde(rename = "tTeleport", default)]
    teleports: Vec<RawDungeonTeleport>,
}

#[derive(Deserialize)]
struct RawDungeonMonster {
    #[serde(rename = "nType", default)]
    kind: Num,
    #[serde(rename = "strMonsterId", default)]
    mover: String,
    #[serde(rename = "bRed", default)]
    red: Truthy,
    #[serde(default)]
    x: Num,
    #[serde(default)]
    y: Num,
    #[serde(default)]
    z: Num,
}

#[derive(Deserialize)]
struct RawDungeonTeleport {
    #[serde(rename = "nType", default)]
    kind: Num,
    #[serde(default)]
    x: Num,
    #[serde(default)]
    y: Num,
    #[serde(default)]
    z: Num,
}

/// `PartyDungeon.lua`: `nMaxInstanceDungeon` and `tDungeon`.
pub fn party_dungeon(
    lua: &mlua::Lua,
    g: &Table,
    r: &mut Resolver<'_>,
) -> Result<PartyDungeonConfig, String> {
    let max: Num = scalar(lua, g, "nMaxInstanceDungeon")?;
    let raw: Vec<RawDungeon> = list(lua, g, "tDungeon")?;
    let dungeons = raw
        .into_iter()
        .map(|d| DungeonDef {
            world: r.id(&d.world),
            world_name: d.world,
            class_mask: d.class.u32(),
            min_level: d.min_level.i32(),
            max_level: d.max_level.i32(),
            cool_time_ms: d.cool_time.u32(),
            monsters: d
                .monsters
                .into_iter()
                .map(|m| DungeonMonster {
                    kind: m.kind.i32(),
                    mover: r.id(&m.mover),
                    mover_name: m.mover,
                    aggressive: m.red.0,
                    pos: pos(m.x, m.y, m.z),
                })
                .collect(),
            teleports: d
                .teleports
                .into_iter()
                .map(|t| DungeonTeleport {
                    kind: t.kind.i32(),
                    pos: pos(t.x, t.y, t.z),
                })
                .collect(),
        })
        .collect();
    Ok(PartyDungeonConfig {
        max_instance_dungeon: max.i32(),
        dungeons,
    })
}

// ---- EventMonster.lua ----------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawEventMonster {
    #[serde(rename = "strMonsterId", default)]
    mover: String,
    #[serde(rename = "nLevel", default)]
    level: Num,
    #[serde(rename = "nLootTime", default)]
    loot_time: Num,
    #[serde(rename = "fItemDropRange", default)]
    range: Num,
    #[serde(rename = "bPet", default)]
    pet: Truthy,
    #[serde(rename = "bGiftBox", default)]
    gift_box: Truthy,
}

/// `EventMonster.lua`: `tEventMonster`.
pub fn event_monsters(
    lua: &mlua::Lua,
    g: &Table,
    r: &mut Resolver<'_>,
) -> Result<Vec<EventMonsterDef>, String> {
    let raw: Vec<RawEventMonster> = list(lua, g, "tEventMonster")?;
    Ok(raw
        .into_iter()
        .map(|m| EventMonsterDef {
            mover: r.id(&m.mover),
            mover_name: m.mover,
            level_gap: m.level.i32(),
            loot_time_s: m.loot_time.i32(),
            item_drop_range: m.range.f32(),
            pet: m.pet.0,
            gift_box: m.gift_box.0,
        })
        .collect())
}

// ---- MonsterSkill.lua ----------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawMonsterSkill {
    #[serde(rename = "strMonsterId", default)]
    mover: String,
    #[serde(rename = "tSkill", default)]
    skills: Vec<RawSkill>,
}

#[derive(Deserialize)]
struct RawSkill {
    #[serde(rename = "nAtkMethod", default)]
    method: Num,
    #[serde(rename = "strSkillId", default)]
    skill: String,
    #[serde(rename = "dwSkillLv", default)]
    level: Num,
    #[serde(rename = "nHitCount", default)]
    hit_count: Num,
    #[serde(rename = "nRange", default)]
    range: Num,
    #[serde(rename = "nProb", default)]
    prob: Num,
    #[serde(rename = "dwSkillTime", default)]
    skill_time: Num,
    #[serde(rename = "bIgnore", default)]
    ignore: Truthy,
}

/// `MonsterSkill.lua`: `tMonsterSkill`.
pub fn monster_skills(
    lua: &mlua::Lua,
    g: &Table,
    r: &mut Resolver<'_>,
) -> Result<Vec<MonsterSkillDef>, String> {
    let raw: Vec<RawMonsterSkill> = list(lua, g, "tMonsterSkill")?;
    Ok(raw
        .into_iter()
        .map(|m| MonsterSkillDef {
            mover: r.id(&m.mover),
            mover_name: m.mover,
            skills: m
                .skills
                .into_iter()
                .map(|s| MonsterSkillEntry {
                    atk_method: s.method.i32(),
                    skill: r.id(&s.skill),
                    skill_name: s.skill,
                    skill_level: s.level.i32(),
                    hit_count: s.hit_count.i32(),
                    range: s.range.i32(),
                    prob: s.prob.i32(),
                    skill_time_ms: s.skill_time.u32(),
                    ignore: s.ignore.0,
                })
                .collect(),
        })
        .collect())
}

// ---- CreateMonster.lua ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawCreateItem {
    #[serde(rename = "strItemId", default)]
    item: String,
    #[serde(rename = "dwKeepTime", default)]
    keep_time: Num,
    #[serde(rename = "tMonster", default)]
    monsters: Vec<RawCreateMonster>,
}

#[derive(Deserialize)]
struct RawCreateMonster {
    #[serde(rename = "strMonsterId", default)]
    mover: String,
    #[serde(rename = "nProbability", default)]
    probability: Num,
}

/// `CreateMonster.lua`: `m_nMaxCreateNum` and `tCreateMonster`.
pub fn create_monster(
    lua: &mlua::Lua,
    g: &Table,
    r: &mut Resolver<'_>,
) -> Result<CreateMonsterConfig, String> {
    let max: Num = scalar(lua, g, "m_nMaxCreateNum")?;
    let raw: Vec<RawCreateItem> = list(lua, g, "tCreateMonster")?;
    Ok(CreateMonsterConfig {
        max_create_num: max.i32(),
        items: raw
            .into_iter()
            .map(|i| CreateMonsterItem {
                item: r.id(&i.item),
                item_name: i.item,
                keep_time_ms: i.keep_time.u32(),
                monsters: i
                    .monsters
                    .into_iter()
                    .map(|m| CreateMonsterEntry {
                        mover: r.id(&m.mover),
                        mover_name: m.mover,
                        probability: m.probability.i32(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

// ---- QuizEvent.lua -------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawQuizNpc {
    #[serde(rename = "strNPCId", default)]
    mover: String,
    #[serde(rename = "strCharKey", default)]
    char_key: String,
    #[serde(rename = "xPos", default)]
    x: Num,
    #[serde(rename = "yPos", default)]
    y: Num,
    #[serde(rename = "zPos", default)]
    z: Num,
}

/// `QuizEvent.lua`: the globals and `tNPC`.
pub fn quiz(lua: &mlua::Lua, g: &Table, r: &mut Resolver<'_>) -> Result<QuizConfig, String> {
    let npcs: Vec<RawQuizNpc> = list(lua, g, "tNPC")?;
    let prize: String = scalar(lua, g, "PrizeItemId")?;
    Ok(QuizConfig {
        auto_mode: scalar::<Truthy>(lua, g, "AutoMode")?.0,
        quiz_type: scalar::<Num>(lua, g, "QuizType")?.i32(),
        channel: scalar::<Num>(lua, g, "QuizChannel")?.i32(),
        npcs: npcs
            .into_iter()
            .map(|n| QuizNpcDef {
                mover: r.id(&n.mover),
                mover_name: n.mover,
                char_key: n.char_key,
                pos: pos(n.x, n.y, n.z),
            })
            .collect(),
        entrance_ms: scalar::<Num>(lua, g, "EntranceTime")?.u32(),
        wait_ms: scalar::<Num>(lua, g, "WaitTime")?.u32(),
        question_ms: scalar::<Num>(lua, g, "QuestionTime")?.u32(),
        answer_ms: scalar::<Num>(lua, g, "AnswerTime")?.u32(),
        watching_zone_open_ms: scalar::<Num>(lua, g, "WatchingZoneOpenTime")?.u32(),
        close_wait_ms: scalar::<Num>(lua, g, "CloseWaitTime")?.u32(),
        prize_item: if prize.is_empty() {
            NULL_ID
        } else {
            r.id(&prize)
        },
        prize_item_name: prize,
        prize_item_num: scalar::<Num>(lua, g, "PrizeItemNum")?.i32(),
    })
}

// ---- Campus.lua ----------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawCampusQuest {
    #[serde(rename = "strQuestId", default)]
    quest: String,
}

#[derive(Deserialize)]
struct RawCampusBuff {
    #[serde(rename = "nLevel", default)]
    level: Num,
    #[serde(rename = "strBuffId", default)]
    buff: String,
}

#[derive(Deserialize)]
struct RawCampusReward {
    #[serde(rename = "nLevel", default)]
    level: Num,
    #[serde(rename = "nMasterReward", default)]
    master: Num,
    #[serde(rename = "nPupilReward", default)]
    pupil: Num,
}

/// `Campus.lua`: `RecoveryTime`, `RecoveryPoint` and the three tables.
pub fn campus(lua: &mlua::Lua, g: &Table, r: &mut Resolver<'_>) -> Result<CampusConfig, String> {
    let quests: Vec<RawCampusQuest> = list(lua, g, "tCampusQuest")?;
    let buffs: Vec<RawCampusBuff> = list(lua, g, "tCampusBuff")?;
    let rewards: Vec<RawCampusReward> = list(lua, g, "tCampusReward")?;
    Ok(CampusConfig {
        recovery_time_ms: scalar::<Num>(lua, g, "RecoveryTime")?.u32(),
        recovery_point: scalar::<Num>(lua, g, "RecoveryPoint")?.i32(),
        quests: quests
            .into_iter()
            .map(|q| CampusQuest {
                quest: r.id(&q.quest),
                quest_name: q.quest,
            })
            .collect(),
        buffs: buffs
            .into_iter()
            .map(|b| CampusBuff {
                level: b.level.i32(),
                item: r.id(&b.buff),
                item_name: b.buff,
            })
            .collect(),
        rewards: rewards
            .into_iter()
            .map(|w| CampusReward {
                level: w.level.i32(),
                master: w.master.i32(),
                pupil: w.pupil.i32(),
            })
            .collect(),
    })
}

// ---- ItemUpgrade.lua -----------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawAttribute {
    #[serde(rename = "nProb", default)]
    prob: Num,
    #[serde(rename = "nDamageRate", default)]
    damage: Num,
    #[serde(rename = "nDefenseRate", default)]
    defense: Num,
    #[serde(rename = "nAddAtkDmgRate", default)]
    add_atk: Num,
}

fn numbers(lua: &mlua::Lua, g: &Table, name: &str) -> Result<Vec<i32>, String> {
    Ok(list::<Num>(lua, g, name)?
        .into_iter()
        .map(Num::i32)
        .collect())
}

/// `ItemUpgrade.lua`: the probability tables, `tAttribute` and the transy limits.
pub fn item_upgrade(lua: &mlua::Lua, g: &Table) -> Result<ItemUpgradeConfig, String> {
    // `tAttribute[nNum]`: keyed by the refine level, not necessarily a sequence
    let attribute: BTreeMap<i64, RawAttribute> = match global(g, "tAttribute")? {
        Value::Nil => BTreeMap::new(),
        v => lua.from_value(v).map_err(|e| format!("tAttribute: {e}"))?,
    };
    Ok(ItemUpgradeConfig {
        suit_prob: numbers(lua, g, "tSuitProb")?,
        weapon_prob: numbers(lua, g, "tWeaponProb")?,
        general: numbers(lua, g, "tGeneral")?,
        attribute: attribute
            .into_iter()
            .map(|(level, a)| AttributeUpgrade {
                level: i32::try_from(level).unwrap_or(i32::MAX),
                prob: a.prob.i32(),
                damage_rate: a.damage.i32(),
                defense_rate: a.defense.i32(),
                add_atk_dmg_rate: a.add_atk.i32(),
            })
            .collect(),
        transy_low_level: scalar::<Num>(lua, g, "nItemTransyLowLevel")?.i64(),
        transy_high_level: scalar::<Num>(lua, g, "nItemTransyHighLevel")?.i64(),
    })
}
