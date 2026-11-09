//! `LuaConfigs`: the typed result of the Lua configuration bridge (Plan/14 §6.17, WP-DATA-14).
//!
//! Seven configuration scripts (`PartyDungeon`, `EventMonster`, `MonsterSkill`, `CreateMonster`,
//! `QuizEvent`, `Campus`, `ItemUpgrade`) are executed **once** at load time by a short-lived sandbox VM
//! (Addendum R2, rule X6; the VM is [`vm`], native only) and their globals are projected into the plain
//! data below. The five live scripts (`Event`, `Tax`, `SecretRoom`, `RainbowRace`, `GuildCombat1TO1`)
//! are *not* run here: their source text and `dofile` dependencies are kept in
//! [`LuaConfigs::live_scripts`] after a syntax check, and the game thread's `flyff-lua` host runs them
//! (Plan/20 §3.2).
//!
//! The types are always compiled (they are serialisable and wasm-safe); only [`vm`] needs `mlua` and sits
//! behind the `load` feature.
//!
//! Define names given as strings in the scripts (`"MI_LUCIFER01"`, `"SI_..."`, `"WI_..."`) are resolved through
//! the define table; an unknown name is `NULL_ID` plus an Error `UnknownIdentifier` diagnostic (the original
//! `GetDefineNum` returned -1). The string stays in the `*_name` field so a consumer or `flyff check` can
//! still report it.

use crate::types::{ItemId, MoverId, QuestId, WorldId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "load")]
pub mod extract;
#[cfg(feature = "load")]
pub mod vm;

/// `ID_NORMAL`: a normal instance-dungeon monster (`InstanceDungeonBase.lua`).
pub const ID_NORMAL: i32 = 0;
/// `ID_MIDBOSS`.
pub const ID_MIDBOSS: i32 = 1;
/// `ID_BOSS`.
pub const ID_BOSS: i32 = 2;
/// `ATK_MELEE` of `MonsterSkillFunc.lua`.
pub const ATK_MELEE: i32 = 1;
/// `ATK_RANGE` of `MonsterSkillFunc.lua`.
pub const ATK_RANGE: i32 = 2;
/// `TYPE_OX` of `QuizEventFunc.lua`.
pub const QUIZ_TYPE_OX: i32 = 1;
/// `TYPE_4C` of `QuizEventFunc.lua`.
pub const QUIZ_TYPE_4C: i32 = 2;

/// The logical names of the live scripts, in `ScriptFamily` order (Plan/20 §3.2) with their file.
pub const LIVE_SCRIPTS: [(&str, &str); 5] = [
    ("event", "Event.lua"),
    ("tax", "Tax.lua"),
    ("secret_room", "SecretRoom.lua"),
    ("rainbow_race", "RainbowRace.lua"),
    // the original loads "GuildCombat1to1.lua" while the file is "GuildCombat1TO1.lua": the VFS is
    // case-insensitive, so either spelling resolves
    ("guild_combat_1to1", "GuildCombat1TO1.lua"),
];

/// A live script kept for the game thread's Lua host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuaSource {
    /// Normalised VFS path of the main file (`event.lua`).
    pub path: String,
    /// The decoded UTF-8 text (CP949 strings of `Event.lua` are already converted).
    pub text: String,
    /// Normalised paths of the files the script `dofile`s (`luafunc/eventfunc.lua`), in order of appearance.
    pub deps: Vec<String>,
}

/// Everything the Lua configuration scripts define (Plan/14 §6.17).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LuaConfigs {
    /// The live scripts by logical name (`event`, `tax`, `secret_room`, `rainbow_race`, `guild_combat_1to1`).
    /// A missing or syntactically broken script is absent (the host reports `FamilyStatus::Missing`).
    pub live_scripts: BTreeMap<String, LuaSource>,
    /// `PartyDungeon.lua` (+ `LuaFunc/InstanceDungeonBase.lua`).
    pub party_dungeon: Option<PartyDungeonConfig>,
    /// `EventMonster.lua`: `tEventMonster`.
    pub event_monsters: Vec<EventMonsterDef>,
    /// `MonsterSkill.lua`: `tMonsterSkill`.
    pub monster_skills: Vec<MonsterSkillDef>,
    /// `CreateMonster.lua`.
    pub create_monster: Option<CreateMonsterConfig>,
    /// `QuizEvent.lua`.
    pub quiz: Option<QuizConfig>,
    /// `Campus.lua` (+ `LuaFunc/CampusFunc.lua`).
    pub campus: Option<CampusConfig>,
    /// `ItemUpgrade.lua`.
    pub item_upgrade: Option<ItemUpgradeConfig>,
}

impl LuaConfigs {
    /// The live script `name` (`"event"`, ...).
    pub fn live(&self, name: &str) -> Option<&LuaSource> {
        self.live_scripts.get(name)
    }
}

// ---- PartyDungeon.lua -------------------------------------------------------------------------------------

/// `nMaxInstanceDungeon` and `tDungeon` (ref/15 §16.1).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyDungeonConfig {
    /// `nMaxInstanceDungeon`: the most dungeon layers at a time (40).
    pub max_instance_dungeon: i32,
    /// One entry per `AddDungeon`, in script order.
    pub dungeons: Vec<DungeonDef>,
}

/// One `AddDungeon` block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DungeonDef {
    /// The `WI_*` name given to `AddDungeon`.
    pub world_name: String,
    /// Its world id (`NULL_ID` when unknown).
    pub world: WorldId,
    /// `dwClass`: the sum of `CLASS_NORMAL` 1, `CLASS_MASTER` 2, `CLASS_HERO` 4 (default 65535).
    pub class_mask: u32,
    /// `nMinLevel` (default 1).
    pub min_level: i32,
    /// `nMaxLevel` (default 120).
    pub max_level: i32,
    /// `dwCoolTime` in ms (default 30 min).
    pub cool_time_ms: u32,
    /// `SetMonster` rows.
    pub monsters: Vec<DungeonMonster>,
    /// `SetTeleportPos` rows (none in this data set).
    pub teleports: Vec<DungeonTeleport>,
}

/// One `SetMonster(nType, strMonsterId, bRed, x, y, z)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DungeonMonster {
    /// [`ID_NORMAL`], [`ID_MIDBOSS`] or [`ID_BOSS`] (other values are kept as written).
    pub kind: i32,
    /// The `MI_*` name.
    pub mover_name: String,
    /// Its mover id (`NULL_ID` when unknown).
    pub mover: MoverId,
    /// `bRed`: the monster attacks first.
    pub aggressive: bool,
    /// Spawn position `(x, y, z)`.
    pub pos: [f32; 3],
}

/// One `SetTeleportPos(nType, x, y, z)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DungeonTeleport {
    /// The state the position belongs to.
    pub kind: i32,
    /// Position `(x, y, z)`.
    pub pos: [f32; 3],
}

// ---- EventMonster.lua -------------------------------------------------------------------------------------

/// One `AddMonster(strMonsterId, nLevel, nLootTime, fItemDropRange, bPet, bGiftBox)` of `EventMonster.lua`
/// (ref/15 §7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventMonsterDef {
    /// The `MI_*` name.
    pub mover_name: String,
    /// Its mover id (`NULL_ID` when unknown).
    pub mover: MoverId,
    /// `nLevel`: a player at least this many levels above cannot hit it.
    pub level_gap: i32,
    /// `nLootTime`: loot reservation in seconds.
    pub loot_time_s: i32,
    /// `fItemDropRange`: the spread of the drops.
    pub item_drop_range: f32,
    /// `bPet`: pets may pick the drops up.
    pub pet: bool,
    /// `bGiftBox`: extra gift-box rolls.
    pub gift_box: bool,
}

// ---- MonsterSkill.lua -------------------------------------------------------------------------------------

/// One `AddMonster` of `MonsterSkill.lua` with its `AddSkill` rows (ref/11 §6.11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonsterSkillDef {
    /// The `MI_*` name.
    pub mover_name: String,
    /// Its mover id (`NULL_ID` when unknown).
    pub mover: MoverId,
    /// The skills in script order.
    pub skills: Vec<MonsterSkillEntry>,
}

/// One `AddSkill(nAtkMethod, strSkillId, nSkillLv, nHitCount, nRange, nProb, dwSkillTime, bIgnore)`, with the
/// transformations `MonsterSkillFunc.lua` applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonsterSkillEntry {
    /// [`ATK_MELEE`] or [`ATK_RANGE`].
    pub atk_method: i32,
    /// The `SI_*` name.
    pub skill_name: String,
    /// Its skill id (`NULL_ID` when unknown).
    pub skill: u32,
    /// `dwSkillLv`.
    pub skill_level: i32,
    /// `nHitCount - 1` (the script stores the argument minus one).
    pub hit_count: i32,
    /// `nRange`: 0 = the target only.
    pub range: i32,
    /// `nProb` in percent.
    pub prob: i32,
    /// `dwSkillTime * 1000`: the skill duration in ms.
    pub skill_time_ms: u32,
    /// `bIgnore`: ignore the skill's own probability.
    pub ignore: bool,
}

// ---- CreateMonster.lua ------------------------------------------------------------------------------------

/// `m_nMaxCreateNum` and `tCreateMonster` (ref/12 §12.5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CreateMonsterConfig {
    /// `m_nMaxCreateNum`: the global cap of item-summoned monsters (1000).
    pub max_create_num: i32,
    /// One entry per `AddItem`.
    pub items: Vec<CreateMonsterItem>,
}

/// One `AddItem(strItemId, dwKeepTime)` block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateMonsterItem {
    /// The `II_*` name.
    pub item_name: String,
    /// Its item id (`NULL_ID` when unknown).
    pub item: ItemId,
    /// `dwKeepTime`: how long the summoned monster lives, in ms.
    pub keep_time_ms: u32,
    /// The `AddMonster` rows.
    pub monsters: Vec<CreateMonsterEntry>,
}

/// One `AddMonster(strMonsterId, nProbability)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateMonsterEntry {
    /// The `MI_*` name.
    pub mover_name: String,
    /// Its mover id (`NULL_ID` when unknown).
    pub mover: MoverId,
    /// `nProbability` (weights, they need not sum to 100).
    pub probability: i32,
}

// ---- QuizEvent.lua ----------------------------------------------------------------------------------------

/// The globals of `QuizEvent.lua` (ref/15 §13).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuizConfig {
    /// `AutoMode`.
    pub auto_mode: bool,
    /// `QuizType`: [`QUIZ_TYPE_OX`] or [`QUIZ_TYPE_4C`].
    pub quiz_type: i32,
    /// `QuizChannel` (0 = every channel).
    pub channel: i32,
    /// `tNPC`: the quiz host NPC(s).
    pub npcs: Vec<QuizNpcDef>,
    /// `EntranceTime` in ms.
    pub entrance_ms: u32,
    /// `WaitTime` in ms.
    pub wait_ms: u32,
    /// `QuestionTime` in ms.
    pub question_ms: u32,
    /// `AnswerTime` in ms.
    pub answer_ms: u32,
    /// `WatchingZoneOpenTime` in ms.
    pub watching_zone_open_ms: u32,
    /// `CloseWaitTime` in ms.
    pub close_wait_ms: u32,
    /// `PrizeItemId` (the `II_*` name).
    pub prize_item_name: String,
    /// Its item id (`NULL_ID` when unknown).
    pub prize_item: ItemId,
    /// `PrizeItemNum`.
    pub prize_item_num: i32,
}

/// One `SetNPC(strNPCId, strCharKey, x, y, z)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuizNpcDef {
    /// The `MI_*` name.
    pub mover_name: String,
    /// Its mover id (`NULL_ID` when unknown).
    pub mover: MoverId,
    /// The character key (`MaFl_Ete`).
    pub char_key: String,
    /// Position `(x, y, z)`.
    pub pos: [f32; 3],
}

// ---- Campus.lua -------------------------------------------------------------------------------------------

/// The globals and tables of `Campus.lua` (ref/14 §12.1).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CampusConfig {
    /// `RecoveryTime` in ms (60 min).
    pub recovery_time_ms: u32,
    /// `RecoveryPoint`.
    pub recovery_point: i32,
    /// `tCampusQuest`: the quests a master must have completed.
    pub quests: Vec<CampusQuest>,
    /// `tCampusBuff`: the buff per "buff level".
    pub buffs: Vec<CampusBuff>,
    /// `tCampusReward`: points granted when a pupil reaches a level.
    pub rewards: Vec<CampusReward>,
}

/// One `SetQuest(strQuestId)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampusQuest {
    /// The `QUEST_*` name.
    pub quest_name: String,
    /// Its quest id (`NULL_ID` when unknown).
    pub quest: QuestId,
}

/// One `SetBuffLevel(nLevel, strBuffId)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampusBuff {
    /// The buff level (1..3).
    pub level: i32,
    /// The `II_*` name of the buff item.
    pub item_name: String,
    /// Its item id (`NULL_ID` when unknown).
    pub item: ItemId,
}

/// One `SetCampusReward(nLevel, nMasterReward, nPupilReward)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampusReward {
    /// The pupil level that triggers the reward.
    pub level: i32,
    /// Points for the master.
    pub master: i32,
    /// Points for the pupil.
    pub pupil: i32,
}

// ---- ItemUpgrade.lua --------------------------------------------------------------------------------------

/// The tables of `ItemUpgrade.lua` (ref/13 §18.1-18.2; probabilities are n/10000).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemUpgradeConfig {
    /// `tSuitProb`: piercing size probabilities of suits.
    pub suit_prob: Vec<i32>,
    /// `tWeaponProb`: piercing size probabilities of weapons.
    pub weapon_prob: Vec<i32>,
    /// `tGeneral`: the refine probabilities +1..+10.
    pub general: Vec<i32>,
    /// `tAttribute`: the element upgrade table, ascending by level.
    pub attribute: Vec<AttributeUpgrade>,
    /// `nItemTransyLowLevel`.
    pub transy_low_level: i64,
    /// `nItemTransyHighLevel`.
    pub transy_high_level: i64,
}

/// One `AddAttribute(nNum, nProb, nDamageRate, nDefenseRate, nAddAtkDmgRate)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeUpgrade {
    /// The refine level (1..20).
    pub level: i32,
    /// Success probability, n/10000.
    pub prob: i32,
    /// Damage increase, n/10000.
    pub damage_rate: i32,
    /// Defense increase, n/10000.
    pub defense_rate: i32,
    /// Element-correction increase, n/10000.
    pub add_atk_dmg_rate: i32,
}

impl ItemUpgradeConfig {
    /// The element upgrade row of `level`.
    pub fn attribute_at(&self, level: i32) -> Option<&AttributeUpgrade> {
        self.attribute.iter().find(|a| a.level == level)
    }
}
