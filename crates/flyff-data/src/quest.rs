//! Quest definitions (L21 `propQuest*.inc`, L37 `propGuildQuest.inc`, L38 `propPartyQuest.inc`; Plan/14 §6.18,
//! ref/16 §4.4-4.5; WP-DATA-15).
//!
//! [`QuestProp`] mirrors the original `QuestProp` of `_Common/Project.h` field by field, with the C widths kept
//! where they change behaviour (`char` -> `i8`, `BYTE` -> `u8`, `WORD` -> `u16`): the loader stores what the
//! original stored, so the predicates of the quest engine (Plan/20 §9.4-9.5) read the same values. The NPC links
//! the original wrote into the `CHARACTER` rows (`m_awSrcQuest`, `m_awDstQuest`) are [`QuestTables::quest_src`] and
//! [`QuestTables::quest_dst`] here (keyed by the lower-cased NPC key). The types are wasm-safe; the grammar
//! parsers (`CProject::LoadPropQuest`, `LoadPropGuildQuest`, `LoadPropPartyQuest`) are behind feature `load`.
//!
//! With the shipped data (every quest file is 2 bytes) all tables are empty (C18 = 0 / 0 / 0) and the client
//! hides its quest UI (PL-148).

use crate::types::{IdTable, ItemId, MoverId, QuestId, SkillId, WorldId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `MAX_QUESTCONDITEM`: the longest item list of a quest directive family.
pub const MAX_QUESTCONDITEM: usize = 64;
/// `MAX_QUESTREMOVE`: the quests one `SetEndRemoveQuest` removes.
pub const MAX_QUESTREMOVE: usize = 12;
/// `m_questState[16]`: the described states of a quest.
pub const MAX_QUEST_STATE: usize = 16;
/// `m_apQuestDialog[32]`: the dialog lines of a quest (`QSAY_*`).
pub const MAX_QUEST_DIALOG: usize = 32;
/// `MAX_QUESTITEM`: the quest drops one mover can carry (`CQuestItemGenerator`).
pub const MAX_QUESTITEM: usize = 16;
/// `MAX_JOB`: the jobs of `SetBeginCondJob`.
pub const MAX_QUEST_JOB: usize = 32;
/// `MAX_GUILD_QUEST_STATE` / `MAX_PARTY_QUEST_STATE`: the described states of a guild or party quest.
pub const MAX_GUILD_QUEST_STATE: usize = 16;
/// The party quest wormon list of one block ends the file at this many entries (`nErrCnt > 1000`).
pub const MAX_PARTY_WORMON: usize = 1000;

/// `QuestGoalData` (client quest helper): the optional `, x, z, GOAL_TEXT_ID_nnnn` tail of a directive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestGoal {
    /// `m_dwGoalIndex`: the pre-incremented running counter of all quest files in load order (the first goal is 1).
    pub index: u32,
    /// `m_fGoalPositionX`.
    pub x: f32,
    /// `m_fGoalPositionZ`.
    pub z: f32,
    /// `m_dwGoalTextID` = `atoi` of the last four characters of the `GOAL_TEXT_ID_nnnn` token.
    pub text_id: u32,
}

/// `QuestPropItem`: one entry of the item directives (`SetBeginCondItem`, `SetBeginCondNotItem`,
/// `SetEndCondOneItem`, `SetEndCondItem`, `SetEndRewardItem[WithAbilityOption]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestPropItem {
    /// `m_nSex` (`char`): -1 any, 0 male, 1 female.
    pub sex: i8,
    /// `m_nType` (`char`): 0 `job_or_item` is a job (-1 any), 1 it is an item the player must own (-1 any).
    pub kind: i8,
    /// `m_nJobOrItem`; `kind` 0 with a value of 64 or more is coerced to -1 with an Error.
    pub job_or_item: i32,
    /// `m_nItemIdx`.
    pub item: ItemId,
    /// `m_nItemNum`.
    pub num: i32,
    /// `m_nAbilityOption` (`SetEndRewardItemWithAbilityOption` only).
    pub ability_option: i32,
    /// `m_byFlag` = `static_cast<BYTE>` of the optional bind flag of the reward directives.
    pub flag: u8,
    /// `m_ItemGoalData`: only `SetEndCondItem` can carry a goal.
    pub goal: Option<QuestGoal>,
}

/// One `SetEndCondKillNPC` slot (`m_nEndCondKillNPCIdx/Num`, `m_KillNPCGoalData`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestKillDef {
    /// The mover to kill (0 = unused slot).
    pub mover: MoverId,
    /// The count.
    pub num: i32,
    /// The goal marker (only with the V15 tail).
    pub goal: Option<QuestGoal>,
}

/// `SetEndCondPatrolZone`: the zone, the optional destination name id and goal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestPatrolDef {
    /// `m_dwEndCondPatrolWorld` (0 = no patrol: the engine treats world 0 as absent).
    pub world: WorldId,
    /// `m_rectEndCondPatrol` after `NormalizeRect`: `[left, top, right, bottom]`.
    pub rect: [i32; 4],
    /// `m_dwPatrolDestinationID` = `atoi` of the last four characters of `PATROL_DESTINATION_ID_nnnn`.
    pub destination: Option<u32>,
    /// `m_PatrolWorldGoalData`.
    pub goal: Option<QuestGoal>,
}

/// `SetEndCondDialog(npc, key)`: both strings are byte-exact (compared with `strcmp`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestDialogCond {
    /// `m_szEndCondDlgCharKey`.
    pub npc: String,
    /// `m_szEndCondDlgAddKey`.
    pub key: String,
    /// `m_DialogCharacterGoalData`.
    pub goal: Option<QuestGoal>,
}

/// One `QuestItem(mover, item, prob, max)` entry (`QUESTITEM`): a monster drop while the quest is at `state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestItemDef {
    /// The state it applies to: 0 at the top level, N inside `state N { .. }`.
    pub state: u8,
    /// The monster.
    pub mover: MoverId,
    /// The item.
    pub item: ItemId,
    /// `dwProbability` (a `DWORD`: 3 000 000 000 is valid).
    pub prob: u32,
    /// `dwNumber`: the largest stack.
    pub max_num: u32,
}

/// One slot of `SetBeginSetAddItem`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestAddItem {
    /// `m_nBeginSetAddItemIdx` (0 = unused).
    pub item: ItemId,
    /// `m_nBeginSetAddItemNum` (`char`).
    pub num: i8,
}

/// One slot of `SetEndRemoveItem` or `SetDlgRewardItem`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestItemSlot {
    /// The item (0 = unused).
    pub item: ItemId,
    /// The count.
    pub num: i32,
}

/// `QuestState`: a `state N { .. }` block (`m_szDesc`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestStateDef {
    /// `SetDesc` (UI text), at most 511 bytes.
    pub desc: String,
}

/// `QuestProp`: one quest (see the module documentation for the field naming and the C widths).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuestProp {
    /// `m_wId`.
    pub id: QuestId,
    /// `SetTitle` / `m_szTitle` (an empty `SetTitle` text is one space, `GetLangScript`), at most 63 bytes.
    pub title: String,
    /// `SetNPCName`, at most 31 bytes (unused by the engine).
    pub npc_name: String,
    /// `m_nHeadQuest` (`WORD`) after the 1800..=1999 remap to `QUEST_KIND_*`.
    pub head_quest: u16,
    /// `m_nQuestType` (`CHAR`, `QT_*`).
    pub quest_type: i8,
    /// `m_bNoRemove` = `SetRemove(0)`: the player cannot cancel the quest.
    pub no_remove: bool,
    /// `SetRepeat`: the quest does not go to the completed list.
    pub repeat: bool,
    /// `m_nParam[4]` (`SetParam`, unused by the engine).
    pub params: [i32; 4],
    /// The givers in directive order: `(npc key as written, item)`; a `SetCharacter` giver carries item 0, a
    /// `SetMultiCharacter` giver the item the player must hold for the NPC to show the quest. A giver whose NPC
    /// does not exist is kept here (it has no `quest_src` link).
    pub givers: Vec<(String, ItemId)>,

    /// `m_nBeginCondPreviousQuestType` (`char`).
    pub begin_cond_previous_quest_type: i8,
    /// `m_anBeginCondPreviousQuest[6]` (`WORD`, 0 = unused).
    pub begin_cond_previous_quest: [u16; 6],
    /// `m_anBeginCondExclusiveQuest[6]` (`WORD`, 0 = unused).
    pub begin_cond_exclusive_quest: [u16; 6],
    /// `m_nBeginCondJob[..m_nBeginCondJobNum]` (`char`, at most 32; repeated directives append).
    pub begin_cond_job: Vec<i8>,
    /// `m_nBeginCondLevelMin` (`BYTE`, 0 = no condition).
    pub begin_cond_level_min: u8,
    /// `m_nBeginCondLevelMax` (`BYTE`).
    pub begin_cond_level_max: u8,
    /// `m_nBeginCondParty` (`char`): 0 any, 1 not in a party, 2 in a party.
    pub begin_cond_party: i8,
    /// `m_nBeginCondPartyNumComp` (`char`): 0 `==`, 1 `>=`, -1 `<=`.
    pub begin_cond_party_num_comp: i8,
    /// `m_nBeginCondPartyNum` (`char`).
    pub begin_cond_party_num: i8,
    /// `m_nBeginCondPartyLeader` (`char`).
    pub begin_cond_party_leader: i8,
    /// `m_nBeginCondGuild` (`char`).
    pub begin_cond_guild: i8,
    /// `m_nBeginCondGuildNumComp` (`char`).
    pub begin_cond_guild_num_comp: i8,
    /// `m_nBeginCondGuildNum` (`WORD`).
    pub begin_cond_guild_num: u16,
    /// `m_nBeginCondGuildLeader` (`char`).
    pub begin_cond_guild_leader: i8,
    /// `m_paBeginCondItem` (at most 64).
    pub begin_cond_item: Vec<QuestPropItem>,
    /// `m_paBeginCondNotItem` (at most 64).
    pub begin_cond_not_item: Vec<QuestPropItem>,
    /// `m_nBeginCondSex` (`char`, default -1).
    pub begin_cond_sex: i8,
    /// `m_nBeginCondSkillIdx` (0 = none).
    pub begin_cond_skill_idx: SkillId,
    /// `m_nBeginCondSkillLvl` (`char`).
    pub begin_cond_skill_lvl: i8,
    /// `m_nBeginCondPKValue`.
    pub begin_cond_pk_value: i32,
    /// `m_nBeginCondDisguiseMoverIndex` (0 none, -1 must not be disguised).
    pub begin_cond_disguise_mover_index: i32,
    /// `m_nBeginCondPetExp`.
    pub begin_cond_pet_exp: i32,
    /// `m_nBeginCondPetLevel` (default -1).
    pub begin_cond_pet_level: i32,
    /// `m_nBeginCondTutorialState` (default -1).
    pub begin_cond_tutorial_state: i32,
    /// `m_nBeginCondTSP`.
    pub begin_cond_tsp: i32,
    /// `m_nBeginSetAddItemIdx/Num[4]`.
    pub begin_set_add_items: [QuestAddItem; 4],
    /// `m_nBeginSetAddGold`.
    pub begin_set_add_gold: i32,
    /// `m_nBeginSetDisguiseMoverIndex`.
    pub begin_set_disguise_mover_index: i32,

    /// `m_nEndCondLimitTime` (minutes; the timer starts at begin).
    pub end_cond_limit_time: i32,
    /// `m_paEndCondItem` (all of them, at most 64).
    pub end_cond_item: Vec<QuestPropItem>,
    /// `m_paEndCondOneItem` (any of them, at most 64).
    pub end_cond_one_item: Vec<QuestPropItem>,
    /// `m_nEndCondKillNPCIdx/Num[2]` with their goals.
    pub end_cond_kill_npc: [QuestKillDef; 2],
    /// `SetEndCondPatrolZone` (`None` when the directive never appeared).
    pub end_cond_patrol: Option<QuestPatrolDef>,
    /// `m_szEndCondCharacter` (at most 63 bytes): the NPC that completes the quest; empty for a multi-giver
    /// quest without an end character (KEEP-QUIRK: such a quest is never completable through dialogs).
    pub end_character: String,
    /// `m_MeetCharacterGoalData`.
    pub end_character_goal: Option<QuestGoal>,
    /// `m_lpszEndCondMultiCharacter`: `(npc key as written, item)` (display only).
    pub end_multi_characters: Vec<(String, ItemId)>,
    /// `m_nEndCondSkillIdx` (0 = none).
    pub end_cond_skill_idx: SkillId,
    /// `m_nEndCondSkillLvl` (`char`).
    pub end_cond_skill_lvl: i8,
    /// `m_nEndCondGold`.
    pub end_cond_gold: i32,
    /// `m_nEndCondLevelMin` (`BYTE`, 0 = no condition).
    pub end_cond_level_min: u8,
    /// `m_nEndCondLevelMax` (`BYTE`).
    pub end_cond_level_max: u8,
    /// `m_nEndCondExpPercentMin` (units of 1/100 percent).
    pub end_cond_exp_percent_min: i32,
    /// `m_nEndCondExpPercentMax`.
    pub end_cond_exp_percent_max: i32,
    /// `m_nEndCondPetExp`.
    pub end_cond_pet_exp: i32,
    /// `m_nEndCondPetLevel` (default -1).
    pub end_cond_pet_level: i32,
    /// `m_nEndCondDisguiseMoverIndex`.
    pub end_cond_disguise_mover_index: i32,
    /// `m_nEndCondParty` (`char`).
    pub end_cond_party: i8,
    /// `m_nEndCondPartyNumComp` (`char`).
    pub end_cond_party_num_comp: i8,
    /// `m_nEndCondPartyNum` (`WORD`).
    pub end_cond_party_num: u16,
    /// `m_nEndCondPartyLeader` (`char`).
    pub end_cond_party_leader: i8,
    /// `m_nEndCondGuild` (`char`).
    pub end_cond_guild: i8,
    /// `m_nEndCondGuildNumComp` (`char`).
    pub end_cond_guild_num_comp: i8,
    /// `m_nEndCondGuildNum` (`WORD`).
    pub end_cond_guild_num: u16,
    /// `m_nEndCondGuildLeader` (`char`).
    pub end_cond_guild_leader: i8,
    /// `m_nEndCondState` (`BYTE`; 0 = the last defined state).
    pub end_cond_state: u8,
    /// `m_nEndCondCompleteQuestOper` (`BYTE`): 0 none, 1 any, 2 all.
    pub end_cond_complete_quest_oper: u8,
    /// `m_nEndCondCompleteQuest[6]` (`WORD`).
    pub end_cond_complete_quest: [u16; 6],
    /// `SetEndCondDialog` (`None` when the directive never appeared).
    pub end_cond_dialog: Option<QuestDialogCond>,
    /// `m_nEndCondTSP`.
    pub end_cond_tsp: i32,
    /// `SetPatrolZoneName` (kept empty when 64 bytes or longer).
    pub patrol_zone_name: String,

    /// `m_nDlgRewardItemIdx/Num[4]` (parsed, never used).
    pub dlg_reward_items: [QuestItemSlot; 4],
    /// `m_nEndRemoveItemIdx/Num[8]`.
    pub end_remove_items: [QuestItemSlot; 8],
    /// `m_nEndRemoveGold`.
    pub end_remove_gold: i32,
    /// `m_anEndRemoveQuest[12]` (0 = unused).
    pub end_remove_quests: [i32; 12],
    /// `m_nEndRemoveTSP`.
    pub end_remove_tsp: i32,

    /// `m_paEndRewardItem` (in directive order, at most 64).
    pub end_reward_items: Vec<QuestPropItem>,
    /// `m_nEndRewardGoldMin`.
    pub end_reward_gold_min: i32,
    /// `m_nEndRewardGoldMax`.
    pub end_reward_gold_max: i32,
    /// `m_nEndRewardExpMin`.
    pub end_reward_exp_min: i32,
    /// `m_nEndRewardExpMax`.
    pub end_reward_exp_max: i32,
    /// `m_nEndRewardSkillPoint`.
    pub end_reward_skill_point: i32,
    /// `m_nEndRewardPKValueMin`.
    pub end_reward_pk_value_min: i32,
    /// `m_nEndRewardPKValueMax`.
    pub end_reward_pk_value_max: i32,
    /// `m_nEndRewardTeleport`: the world (0 = none).
    pub end_reward_teleport: WorldId,
    /// `m_nEndRewardTeleportPos`.
    pub end_reward_teleport_pos: [f32; 3],
    /// `m_bEndRewardPetLevelup`.
    pub end_reward_pet_levelup: bool,
    /// `m_bEndRewardHide` (`SetEndRewardHide(n != 0)`).
    pub end_reward_hide: bool,
    /// `m_nEndRewardTSP`.
    pub end_reward_tsp: i32,

    /// `m_apQuestDialog[32]` (`SetDialog`, server only).
    pub dialog: [Option<String>; MAX_QUEST_DIALOG],
    /// `m_questState[16]` (`state N { .. }`).
    pub states: [Option<QuestStateDef>; MAX_QUEST_STATE],
    /// Every `QuestItem` of the definition, in parse order.
    pub quest_items: Vec<QuestItemDef>,
}

impl QuestProp {
    /// A quest with the per-quest defaults of `LoadPropQuest`: everything 0 except `begin_cond_sex`,
    /// `begin_cond_pet_level`, `end_cond_pet_level` and `begin_cond_tutorial_state` (-1).
    pub fn new(id: QuestId) -> QuestProp {
        QuestProp {
            id,
            title: String::new(),
            npc_name: String::new(),
            head_quest: 0,
            quest_type: 0,
            no_remove: false,
            repeat: false,
            params: [0; 4],
            givers: Vec::new(),
            begin_cond_previous_quest_type: 0,
            begin_cond_previous_quest: [0; 6],
            begin_cond_exclusive_quest: [0; 6],
            begin_cond_job: Vec::new(),
            begin_cond_level_min: 0,
            begin_cond_level_max: 0,
            begin_cond_party: 0,
            begin_cond_party_num_comp: 0,
            begin_cond_party_num: 0,
            begin_cond_party_leader: 0,
            begin_cond_guild: 0,
            begin_cond_guild_num_comp: 0,
            begin_cond_guild_num: 0,
            begin_cond_guild_leader: 0,
            begin_cond_item: Vec::new(),
            begin_cond_not_item: Vec::new(),
            begin_cond_sex: -1,
            begin_cond_skill_idx: 0,
            begin_cond_skill_lvl: 0,
            begin_cond_pk_value: 0,
            begin_cond_disguise_mover_index: 0,
            begin_cond_pet_exp: 0,
            begin_cond_pet_level: -1,
            begin_cond_tutorial_state: -1,
            begin_cond_tsp: 0,
            begin_set_add_items: [QuestAddItem::default(); 4],
            begin_set_add_gold: 0,
            begin_set_disguise_mover_index: 0,
            end_cond_limit_time: 0,
            end_cond_item: Vec::new(),
            end_cond_one_item: Vec::new(),
            end_cond_kill_npc: [QuestKillDef::default(); 2],
            end_cond_patrol: None,
            end_character: String::new(),
            end_character_goal: None,
            end_multi_characters: Vec::new(),
            end_cond_skill_idx: 0,
            end_cond_skill_lvl: 0,
            end_cond_gold: 0,
            end_cond_level_min: 0,
            end_cond_level_max: 0,
            end_cond_exp_percent_min: 0,
            end_cond_exp_percent_max: 0,
            end_cond_pet_exp: 0,
            end_cond_pet_level: -1,
            end_cond_disguise_mover_index: 0,
            end_cond_party: 0,
            end_cond_party_num_comp: 0,
            end_cond_party_num: 0,
            end_cond_party_leader: 0,
            end_cond_guild: 0,
            end_cond_guild_num_comp: 0,
            end_cond_guild_num: 0,
            end_cond_guild_leader: 0,
            end_cond_state: 0,
            end_cond_complete_quest_oper: 0,
            end_cond_complete_quest: [0; 6],
            end_cond_dialog: None,
            end_cond_tsp: 0,
            patrol_zone_name: String::new(),
            dlg_reward_items: [QuestItemSlot::default(); 4],
            end_remove_items: [QuestItemSlot::default(); 8],
            end_remove_gold: 0,
            end_remove_quests: [0; 12],
            end_remove_tsp: 0,
            end_reward_items: Vec::new(),
            end_reward_gold_min: 0,
            end_reward_gold_max: 0,
            end_reward_exp_min: 0,
            end_reward_exp_max: 0,
            end_reward_skill_point: 0,
            end_reward_pk_value_min: 0,
            end_reward_pk_value_max: 0,
            end_reward_teleport: 0,
            end_reward_teleport_pos: [0.0; 3],
            end_reward_pet_levelup: false,
            end_reward_hide: false,
            end_reward_tsp: 0,
            dialog: Default::default(),
            states: Default::default(),
            quest_items: Vec::new(),
        }
    }

    /// `true` when the quest has a time limit that expires it (`SetEndCondLimitTime(m)` with `m > 0`).
    pub fn is_timed(&self) -> bool {
        self.end_cond_limit_time > 0
    }

    /// The state the `SetEndCondState(0)` rule demands (E10): the highest state of the consecutive run
    /// `state 1`, `state 2`, ... that is defined (0 when `state 1` is missing).
    pub fn last_defined_state(&self) -> u8 {
        let mut last = 0u8;
        for (i, st) in self.states.iter().enumerate().take(14).skip(1) {
            if st.is_none() {
                break;
            }
            last = u8::try_from(i).unwrap_or(0);
        }
        last
    }

    /// `QuestProp.dialog[slot]` (`QSAY_*`), `None` for an empty or out-of-range slot.
    pub fn dialog_line(&self, slot: usize) -> Option<&str> {
        self.dialog.get(slot)?.as_deref()
    }
}

impl Default for QuestProp {
    fn default() -> Self {
        QuestProp::new(0)
    }
}

/// `GUILDQUESTPROP` (`propGuildQuest.inc`): a guild boss quest. `SetAtGrow`: a later definition of an id wins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GuildQuestProp {
    /// `Title = "text"` (a raw token, at most 31 bytes).
    pub title: String,
    /// `Level = n`.
    pub level: i32,
    /// `Wormon = moverId`.
    pub wormon: MoverId,
    /// `World = worldId`.
    pub world: WorldId,
    /// `Position = x, y, z` (the boss spawn).
    pub pos: [f32; 3],
    /// `Region = x1, y1, x2, y2` as written (`None` without the line; the original registered the arena
    /// rectangle with the processor).
    pub region: Option<[i32; 4]>,
    /// `State n { desc = "text" }` (n < 16; a raw token of at most 259 bytes).
    pub desc: [Option<String>; MAX_GUILD_QUEST_STATE],
}

/// `PARTYQUESTPROP` (`propPartyQuest.inc`). `std::map::insert`: the FIRST definition of an id wins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyQuestProp {
    /// `Title` (at most 31 bytes).
    pub title: String,
    /// `Level`.
    pub level: i32,
    /// `Wormon { id, x, y, z, ... }`: the bosses.
    pub wormons: Vec<(MoverId, [f32; 3])>,
    /// `World`.
    pub world: WorldId,
    /// `WorldKey` (a raw token, at most 31 bytes).
    pub world_key: String,
    /// `Region = x1, y1, x2, y2` as written.
    pub region: Option<[i32; 4]>,
    /// `State n { desc = "text" }`.
    pub desc: [Option<String>; MAX_GUILD_QUEST_STATE],
}

/// The loader's own bookkeeping between the quest files (the original kept it in function-static and
/// mover-owned state). It is not data: it is skipped by serde and never compared.
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(feature = "load"), allow(dead_code))] // only the loaders (feature `load`) read it
pub struct QuestLoaderState {
    /// `m_dwGoalIndex`: the function-static goal counter of `LoadPropQuest`, pre-incremented, shared by the five
    /// files in load order.
    pub(crate) goal_counter: u32,
    /// How many `QuestItem` entries each mover carries so far (`CQuestItemGenerator`, at most 16), counted in
    /// parse order.
    pub(crate) quest_item_counts: BTreeMap<MoverId, u32>,
}

impl PartialEq for QuestLoaderState {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// Everything the quest files define (L21, L37, L38).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestTables {
    /// `propQuest*.inc` x5 in `Masquerade.prj` order (a later definition of an id replaces the earlier one;
    /// `QUEST_KAWIBAWIBO01` is never added).
    pub quests: IdTable<QuestProp>,
    /// `propGuildQuest.inc` (max id 255).
    pub guild_quests: IdTable<GuildQuestProp>,
    /// `propPartyQuest.inc`.
    pub party_quests: IdTable<PartyQuestProp>,
    /// `m_awSrcQuest`: lower-cased NPC key to the quests it gives, in parse order (each pair once).
    pub quest_src: BTreeMap<String, Vec<QuestId>>,
    /// `m_awDstQuest`: lower-cased NPC key to the quests it completes, in parse order (each pair once).
    pub quest_dst: BTreeMap<String, Vec<QuestId>>,
    /// The loader's bookkeeping between the files (not data).
    #[serde(skip)]
    pub loader: QuestLoaderState,
}

impl QuestTables {
    /// `true` when no quest of any kind is defined (the shipped data).
    pub fn is_empty(&self) -> bool {
        self.quests.is_empty() && self.guild_quests.is_empty() && self.party_quests.is_empty()
    }

    /// C18: quests / guild quests / party quests.
    pub fn counts(&self) -> [u32; 3] {
        let n = |len: usize| u32::try_from(len).unwrap_or(u32::MAX);
        [
            n(self.quests.len()),
            n(self.guild_quests.len()),
            n(self.party_quests.len()),
        ]
    }

    /// `GetQuestProp(id)`.
    pub fn quest(&self, id: QuestId) -> Option<&QuestProp> {
        self.quests.get(id)
    }
}

// ---- the client definition (14 §10.5.1) ---------------------------------------------------------------------------

use crate::bundle::types::{
    QuestBegin, QuestClientDef, QuestCompleteQuests, QuestEnd, QuestEndCharacter, QuestEndDialog,
    QuestGoalData, QuestItemCond, QuestKill, QuestPartyCond, QuestPatrol, QuestPrevious,
    QuestRewardItem, QuestRewards, QuestStateDesc, QuestTeleport,
};

impl QuestProp {
    /// The projection `QuestProp -> QuestClientDef` of Plan/14 §10.5.1 (the `quests` bundle, and the golden
    /// condition tables of the predicate tests): values raw as loaded, NPC keys lower-cased, texts resolved; nothing
    /// server-only (no `QSAY` lines, drops, start effects, removals, params). `SetEndRewardHide(1)` gives
    /// `rewards: null`; a patrol zone of world 0 is absent; the goal coordinates truncate toward zero.
    pub fn client_def(&self) -> QuestClientDef {
        client_def(self)
    }
}

/// `QuestGoalData`: the frozen DTO carries integer coordinates (the f32 truncates toward zero).
fn goal(g: Option<&QuestGoal>) -> Option<QuestGoalData> {
    g.map(|g| QuestGoalData {
        index: g.index,
        x: g.x as i32,
        z: g.z as i32,
        text_id: g.text_id,
    })
}

fn item_cond(e: &QuestPropItem) -> QuestItemCond {
    QuestItemCond {
        sex: i32::from(e.sex),
        kind: i32::from(e.kind),
        job_or_item: e.job_or_item,
        item: e.item,
        num: e.num,
        ability_option: e.ability_option,
        flag: i32::from(e.flag),
        goal: goal(e.goal.as_ref()),
    }
}

fn party(mode: i8, comp: i8, num: i32, leader: i8) -> QuestPartyCond {
    QuestPartyCond {
        mode: i32::from(mode),
        comp: i32::from(comp),
        num,
        leader: i32::from(leader),
    }
}

fn widen(v: &[u16]) -> Vec<u32> {
    v.iter().map(|q| u32::from(*q)).collect()
}

fn lower_pairs(v: &[(String, u32)]) -> Vec<(String, u32)> {
    v.iter()
        .map(|(k, item)| (k.to_ascii_lowercase(), *item))
        .collect()
}

fn begin(p: &QuestProp) -> QuestBegin {
    QuestBegin {
        previous: QuestPrevious {
            kind: i32::from(p.begin_cond_previous_quest_type),
            quests: widen(&p.begin_cond_previous_quest),
        },
        exclusive: widen(&p.begin_cond_exclusive_quest),
        jobs: p
            .begin_cond_job
            .iter()
            .map(|j| u32::from(*j as u8))
            .collect(),
        level: [
            i32::from(p.begin_cond_level_min),
            i32::from(p.begin_cond_level_max),
        ],
        party: party(
            p.begin_cond_party,
            p.begin_cond_party_num_comp,
            i32::from(p.begin_cond_party_num),
            p.begin_cond_party_leader,
        ),
        guild: party(
            p.begin_cond_guild,
            p.begin_cond_guild_num_comp,
            i32::from(p.begin_cond_guild_num),
            p.begin_cond_guild_leader,
        ),
        sex: i32::from(p.begin_cond_sex),
        skill: [
            p.begin_cond_skill_idx,
            u32::try_from(p.begin_cond_skill_lvl).unwrap_or(0),
        ],
        pk_value: p.begin_cond_pk_value,
        not_items: p.begin_cond_not_item.iter().map(item_cond).collect(),
        items: p.begin_cond_item.iter().map(item_cond).collect(),
        disguise: p.begin_cond_disguise_mover_index,
        pet_exp: p.begin_cond_pet_exp,
        pet_level: p.begin_cond_pet_level,
        tutorial_state: p.begin_cond_tutorial_state,
        tsp: p.begin_cond_tsp,
        add_items: p
            .begin_set_add_items
            .iter()
            .filter(|a| a.item != 0)
            .map(|a| [a.item, u32::try_from(a.num).unwrap_or(0)])
            .collect(),
    }
}

fn end(p: &QuestProp) -> QuestEnd {
    QuestEnd {
        limit_time_min: p.end_cond_limit_time,
        // the engine treats patrol world 0 as "no patrol" (E2)
        patrol: p
            .end_cond_patrol
            .as_ref()
            .filter(|z| z.world != 0)
            .map(|z| QuestPatrol {
                world: z.world,
                rect: z.rect,
                name: p.patrol_zone_name.clone(),
                destination: z.destination,
                goal: goal(z.goal.as_ref()),
            }),
        kill: p
            .end_cond_kill_npc
            .iter()
            .map(|k| QuestKill {
                mover: k.mover,
                num: k.num,
                goal: goal(k.goal.as_ref()),
            })
            .collect(),
        skill: [
            p.end_cond_skill_idx,
            u32::try_from(p.end_cond_skill_lvl).unwrap_or(0),
        ],
        gold: p.end_cond_gold,
        level: [
            i32::from(p.end_cond_level_min),
            i32::from(p.end_cond_level_max),
        ],
        exp_percent: [p.end_cond_exp_percent_min, p.end_cond_exp_percent_max],
        pet_exp: p.end_cond_pet_exp,
        pet_level: p.end_cond_pet_level,
        disguise: p.end_cond_disguise_mover_index,
        state: i32::from(p.end_cond_state),
        complete_quest: QuestCompleteQuests {
            oper: i32::from(p.end_cond_complete_quest_oper),
            quests: widen(&p.end_cond_complete_quest),
        },
        party: party(
            p.end_cond_party,
            p.end_cond_party_num_comp,
            i32::from(p.end_cond_party_num),
            p.end_cond_party_leader,
        ),
        guild: party(
            p.end_cond_guild,
            p.end_cond_guild_num_comp,
            i32::from(p.end_cond_guild_num),
            p.end_cond_guild_leader,
        ),
        one_items: p.end_cond_one_item.iter().map(item_cond).collect(),
        items: p.end_cond_item.iter().map(item_cond).collect(),
        tsp: p.end_cond_tsp,
        character: QuestEndCharacter {
            npc: p.end_character.to_ascii_lowercase(),
            goal: goal(p.end_character_goal.as_ref()),
        },
        multi_characters: lower_pairs(&p.end_multi_characters),
        dialog: p.end_cond_dialog.as_ref().map(|d| QuestEndDialog {
            npc: d.npc.to_ascii_lowercase(),
            key: d.key.clone(),
            goal: goal(d.goal.as_ref()),
        }),
    }
}

fn rewards(p: &QuestProp) -> Option<QuestRewards> {
    if p.end_reward_hide {
        return None;
    }
    Some(QuestRewards {
        items: p
            .end_reward_items
            .iter()
            .map(|e| QuestRewardItem {
                sex: i32::from(e.sex),
                kind: i32::from(e.kind),
                job_or_item: e.job_or_item,
                item: e.item,
                num: e.num,
                ability_option: e.ability_option,
                bind: i32::from(e.flag),
            })
            .collect(),
        gold: [p.end_reward_gold_min, p.end_reward_gold_max],
        exp: [
            i64::from(p.end_reward_exp_min),
            i64::from(p.end_reward_exp_max),
        ],
        skill_point: p.end_reward_skill_point,
        pk_value: [p.end_reward_pk_value_min, p.end_reward_pk_value_max],
        teleport: (p.end_reward_teleport != 0).then_some(QuestTeleport {
            world: p.end_reward_teleport,
            pos: p.end_reward_teleport_pos,
        }),
        pet_levelup: p.end_reward_pet_levelup,
        tsp: p.end_reward_tsp,
    })
}

/// `QuestProp -> QuestClientDef` (14 §10.5.1).
fn client_def(p: &QuestProp) -> QuestClientDef {
    QuestClientDef {
        id: p.id,
        title: p.title.clone(),
        head_quest: u32::from(p.head_quest),
        quest_type: i32::from(p.quest_type),
        repeat: p.repeat,
        no_remove: p.no_remove,
        givers: lower_pairs(&p.givers),
        begin: begin(p),
        end: end(p),
        rewards: rewards(p),
        states: p
            .states
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.as_ref().map(|s| QuestStateDesc {
                    state: u32::try_from(i).unwrap_or(0),
                    desc: s.desc.clone(),
                })
            })
            .collect(),
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{
    QuestEnv, parse_guild_quest_file, parse_party_quest_file, parse_quest_file,
};

#[cfg(feature = "load")]
mod parse {
    use super::{
        GuildQuestProp, MAX_GUILD_QUEST_STATE, MAX_PARTY_WORMON, MAX_QUEST_DIALOG, MAX_QUEST_JOB,
        MAX_QUEST_STATE, MAX_QUESTCONDITEM, MAX_QUESTITEM, MAX_QUESTREMOVE, PartyQuestProp,
        QuestDialogCond, QuestGoal, QuestItemDef, QuestPatrolDef, QuestProp, QuestPropItem,
        QuestStateDef, QuestTables,
    };
    use crate::items_ext::except::{SERVER_LANG, SERVER_SUBLANG};
    use crate::types::{IdTable, MoverId, QuestId};
    use flyff_common::defs::quest::{
        QUEST_KAWIBAWIBO01, QUEST_KIND_EVENT, QUEST_KIND_NORMAL, QUEST_KIND_REQUEST,
        QUEST_KIND_SCENARIO,
    };
    use flyff_script::{
        DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity, c_atoi,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// What the quest parser asks of the rest of the data: whether an NPC key exists (`GetCharacter`,
    /// case-insensitive) and, optionally, whether a mover exists (`GetMoverProp`, for the `QuestItem` warning).
    pub(crate) struct QuestEnv<'a> {
        /// `GetCharacter(key) != NULL`.
        pub npc_exists: &'a dyn Fn(&str) -> bool,
        /// `GetMoverProp(id) != NULL`; `None` skips the check.
        pub mover_exists: Option<&'a dyn Fn(MoverId) -> bool>,
    }

    /// The longest byte prefix of `s` below `limit` bytes that ends on a character boundary.
    fn clip(s: &str, limit: usize) -> &str {
        if s.len() < limit {
            return s;
        }
        let mut end = limit.saturating_sub(1);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.get(..end).unwrap_or("")
    }

    /// `atoi(Token.Right(4))`.
    fn atoi_right4(token: &str) -> u32 {
        let start = token.char_indices().rev().nth(3).map_or(0, |(i, _)| i);
        c_atoi(token.get(start..).unwrap_or("").as_bytes()) as u32
    }

    /// The per-definition state of `LoadPropQuest`: the quest under construction plus the link bookkeeping.
    struct Def {
        prop: QuestProp,
        /// `szLinkChar[10]` / `szLinkCharNum`.
        link: Vec<String>,
        /// Every NPC key that was linked as a destination, in order.
        dst: Vec<String>,
        /// `bEndCondCharacter`.
        end_given: bool,
        /// `QuestItem` lines in parse order.
        items: Vec<QuestItemDef>,
    }

    /// Reads one directive's item entry (`( sex , type , jobOrItem , item , num`), coercing a job id that does
    /// not fit (`type 0 && jobOrItem >= 64` becomes -1 with an Error).
    fn read_item(sc: &mut Script<'_>, directive: &str) -> QuestPropItem {
        sc.get_token(); // (
        let sex = sc.get_number_i32();
        sc.get_token(); // ,
        let kind = sc.get_number_i32();
        sc.get_token(); // ,
        let mut job_or_item = sc.get_number_i32();
        if kind == 0 && job_or_item >= MAX_QUESTCONDITEM as i32 {
            job_or_item = -1;
            let msg = format!(
                "{directive}: the job id is out of range (0..{MAX_QUESTCONDITEM}); -1 is used"
            );
            sc.report(Severity::Error, DiagCode::ValueOutOfRange, msg);
        }
        sc.get_token(); // ,
        let item = sc.get_number();
        sc.get_token(); // ,
        let num = sc.get_number_i32();
        QuestPropItem {
            sex: sex as i8,
            kind: kind as i8,
            job_or_item,
            item,
            num,
            ability_option: 0,
            flag: 0,
            goal: None,
        }
    }

    /// Appends an item entry; the 65th is a `CapacityExceeded` Error (the original wrote past its buffer).
    fn push_item(
        sc: &mut Script<'_>,
        list: &mut Vec<QuestPropItem>,
        e: QuestPropItem,
        directive: &str,
    ) {
        if list.len() < MAX_QUESTCONDITEM {
            list.push(e);
        } else {
            let msg = format!(
                "{directive}: more than {MAX_QUESTCONDITEM} entries; the extra one is ignored"
            );
            sc.report(Severity::Error, DiagCode::CapacityExceeded, msg);
        }
    }

    /// The optional V15 goal tail after the last mandatory argument: the `,` was already read; reads `x , z ,
    /// GOAL_TEXT_ID_nnnn` and pre-increments the counter.
    fn read_goal(sc: &mut Script<'_>, counter: &mut u32) -> QuestGoal {
        *counter = counter.wrapping_add(1);
        let index = *counter;
        let x = sc.get_float();
        sc.get_token(); // ,
        let z = sc.get_float();
        sc.get_token(); // ,
        sc.get_token_nodef(); // GOAL_TEXT_ID_nnnn
        QuestGoal {
            index,
            x,
            z,
            text_id: atoi_right4(sc.token()),
        }
    }

    /// A fixed-array index of the original: `0 <= idx < len`, else an Error and slot 0 (`nIdx = 0`).
    fn slot_or_zero(sc: &mut Script<'_>, directive: &str, idx: i32, len: usize) -> usize {
        match usize::try_from(idx) {
            Ok(i) if i < len => i,
            _ => {
                let msg = format!(
                    "{directive}: index {idx} is outside 0..={}; slot 0 is used",
                    len - 1
                );
                sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                0
            }
        }
    }

    /// `strcpy` into a `char[limit]`: an over-long text is an Error and cut at a character boundary.
    fn bounded(sc: &mut Script<'_>, directive: &str, text: &str, limit: usize) -> String {
        if text.len() >= limit {
            let msg = format!(
                "{directive}: the text is {} bytes, the buffer holds {}; cut",
                text.len(),
                limit - 1
            );
            sc.report(Severity::Error, DiagCode::CapacityExceeded, msg);
        }
        clip(text, limit).to_owned()
    }

    /// `m_nHeadQuest` remap of 1800..=1999 (`__IMPROVE_QUEST_INTERFACE`).
    fn remap_head_quest(v: u16) -> u16 {
        if !(1800..=1999).contains(&v) {
            return v;
        }
        let kind = match v {
            1992 => QUEST_KIND_EVENT,
            1993..=1995 | 1997 | 1998 => QUEST_KIND_NORMAL,
            1999 => QUEST_KIND_REQUEST,
            _ => QUEST_KIND_SCENARIO,
        };
        u16::try_from(kind).unwrap_or(v)
    }

    /// One `QuestItem(mover, item, prob, max)` entry; `state` is 0 at the top level.
    fn read_quest_item(
        sc: &mut Script<'_>,
        counts: &mut BTreeMap<MoverId, u32>,
        env: &QuestEnv<'_>,
        state: u8,
    ) -> Option<QuestItemDef> {
        sc.get_token(); // (
        let mover = sc.get_number();
        sc.get_token(); // ,
        let item = sc.get_number();
        sc.get_token(); // ,
        let prob = sc.get_number();
        sc.get_token(); // ,
        let max_num = sc.get_number();
        sc.get_token(); // )
        let n = counts.entry(mover).or_insert(0);
        if usize::try_from(*n).unwrap_or(usize::MAX) >= MAX_QUESTITEM {
            let msg = format!(
                "QuestItem: mover {mover} already carries {MAX_QUESTITEM} quest drops; ignored"
            );
            sc.report(Severity::Error, DiagCode::CapacityExceeded, msg);
            return None;
        }
        *n += 1;
        if let Some(exists) = env.mover_exists
            && !exists(mover)
        {
            let msg = format!("QuestItem: mover {mover} does not exist; the drop never fires");
            sc.report(Severity::Warning, DiagCode::RefUnresolved, msg);
        }
        Some(QuestItemDef {
            state,
            mover,
            item,
            prob,
            max_num,
        })
    }

    /// `SetBeginCondParty` / `SetBeginCondGuild` / `SetEndCondParty` / `SetEndCondGuild`:
    /// `( mode , comp , num , leader`.
    fn read_party4(sc: &mut Script<'_>) -> (i32, i32, i32, i32) {
        sc.get_token(); // (
        let mode = sc.get_number_i32();
        sc.get_token(); // ,
        let comp = sc.get_number_i32();
        sc.get_token(); // ,
        let num = sc.get_number_i32();
        sc.get_token(); // ,
        let leader = sc.get_number_i32();
        (mode, comp, num, leader)
    }

    /// Executes the directive whose keyword was just read. Returns `false` when the token is no directive.
    #[allow(clippy::too_many_lines)] // one arm per directive of the original `if` chain
    fn run_directive(
        sc: &mut Script<'_>,
        def: &mut Def,
        loader: &mut super::QuestLoaderState,
        env: &QuestEnv<'_>,
    ) {
        let token = sc.token().to_owned();
        match token.as_str() {
            "SetTitle" => {
                sc.get_token(); // (
                let t = sc.lang_script();
                def.prop.title = bounded(sc, "SetTitle", &t, 64);
            }
            "m_szTitle" => {
                sc.get_token(); // (
                sc.get_token();
                let t = sc.token().to_owned();
                def.prop.title = bounded(sc, "m_szTitle", &t, 64);
            }
            "SetNPCName" => {
                sc.get_token(); // (
                let t = sc.lang_script();
                def.prop.npc_name = bounded(sc, "SetNPCName", &t, 32);
            }
            "m_szNPC" => {
                sc.get_token();
                sc.get_token();
            }
            "SetCharacter" => {
                sc.get_token(); // (
                sc.get_token(); // key
                let key = sc.token().to_owned();
                sc.get_token(); // , or )
                if sc.is(",") {
                    let lang = sc.get_number_i32();
                    sc.get_token(); // ,
                    let mut sub = 0;
                    if sc.is(",") {
                        sub = sc.get_number_i32();
                    }
                    if lang != SERVER_LANG || sub != SERVER_SUBLANG {
                        return; // `continue`: the link list is left alone
                    }
                }
                let key = bounded(sc, "SetCharacter", &key, 64);
                def.link.clear();
                def.link.push(key.clone());
                def.prop.givers.push((key, 0));
            }
            "SetMultiCharacter" => {
                sc.get_token(); // (
                def.link.clear();
                loop {
                    sc.get_token(); // key
                    let key = sc.token().to_owned();
                    let key = bounded(sc, "SetMultiCharacter", &key, 64);
                    sc.get_token(); // ,
                    let item = sc.get_number();
                    if !(env.npc_exists)(&key) {
                        let msg = format!("SetMultiCharacter: the NPC {key} does not exist");
                        sc.report(Severity::Error, DiagCode::RefUnresolved, msg);
                    }
                    def.prop.givers.push((key.clone(), item));
                    def.link.push(key);
                    sc.get_token(); // , or )
                    if sc.token().starts_with(')') || def.link.len() >= 10 || sc.finished() {
                        break;
                    }
                }
            }
            "SetBeginCondSex" => {
                sc.get_token(); // (
                def.prop.begin_cond_sex = sc.get_number_i32() as i8;
            }
            "SetBeginCondSkillLvl" => {
                sc.get_token(); // (
                def.prop.begin_cond_skill_idx = sc.get_number();
                sc.get_token(); // ,
                def.prop.begin_cond_skill_lvl = sc.get_number_i32() as i8;
            }
            "SetBeginCondPKValue" => {
                sc.get_token(); // (
                def.prop.begin_cond_pk_value = sc.get_number_i32();
            }
            "SetBeginCondNotItem" => {
                let e = read_item(sc, "SetBeginCondNotItem");
                push_item(
                    sc,
                    &mut def.prop.begin_cond_not_item,
                    e,
                    "SetBeginCondNotItem",
                );
            }
            "SetBeginCondLevel" => {
                sc.get_token(); // (
                def.prop.begin_cond_level_min = sc.get_number_i32() as u8;
                sc.get_token(); // ,
                def.prop.begin_cond_level_max = sc.get_number_i32() as u8;
            }
            "SetBeginCondParty" => {
                let (m, c, n, l) = read_party4(sc);
                def.prop.begin_cond_party = m as i8;
                def.prop.begin_cond_party_num_comp = c as i8;
                def.prop.begin_cond_party_num = n as i8;
                def.prop.begin_cond_party_leader = l as i8;
            }
            "SetBeginCondGuild" => {
                let (m, c, n, l) = read_party4(sc);
                def.prop.begin_cond_guild = m as i8;
                def.prop.begin_cond_guild_num_comp = c as i8;
                def.prop.begin_cond_guild_num = n as u16;
                def.prop.begin_cond_guild_leader = l as i8;
            }
            "SetBeginCondJob" => {
                sc.get_token(); // (
                loop {
                    let job = sc.get_number_i32() as i8;
                    if def.prop.begin_cond_job.len() < MAX_QUEST_JOB {
                        def.prop.begin_cond_job.push(job);
                    } else {
                        sc.report(
                            Severity::Error,
                            DiagCode::CapacityExceeded,
                            format!("SetBeginCondJob: more than {MAX_QUEST_JOB} jobs; the extra one is ignored"),
                        );
                    }
                    sc.get_token(); // , or )
                    if sc.token().starts_with(')')
                        || def.prop.begin_cond_job.len() >= MAX_QUEST_JOB
                        || sc.finished()
                    {
                        break;
                    }
                }
            }
            "SetBeginCondPreviousQuest" => {
                sc.get_token(); // (
                def.prop.begin_cond_previous_quest_type = sc.get_number_i32() as i8;
                sc.get_token(); // ,
                let mut n = 0usize;
                loop {
                    let q = sc.get_number() as u16;
                    if let Some(slot) = def.prop.begin_cond_previous_quest.get_mut(n) {
                        *slot = q;
                    }
                    n += 1;
                    sc.get_token(); // , or )
                    if sc.token().starts_with(')') || n >= 6 || sc.finished() {
                        break;
                    }
                }
            }
            "SetBeginCondExclusiveQuest" => {
                sc.get_token(); // (
                let mut n = 0usize;
                loop {
                    let q = sc.get_number() as u16;
                    if let Some(slot) = def.prop.begin_cond_exclusive_quest.get_mut(n) {
                        *slot = q;
                    }
                    n += 1;
                    sc.get_token(); // , or )
                    if sc.token().starts_with(')') || n >= 6 || sc.finished() {
                        break;
                    }
                }
            }
            "SetBeginCondItem" => {
                let e = read_item(sc, "SetBeginCondItem");
                push_item(sc, &mut def.prop.begin_cond_item, e, "SetBeginCondItem");
            }
            "SetBeginCondDisguise" => {
                sc.get_token(); // (
                def.prop.begin_cond_disguise_mover_index = sc.get_number_i32();
            }
            "SetBeginSetDisguise" => {
                sc.get_token(); // (
                def.prop.begin_set_disguise_mover_index = sc.get_number_i32();
            }
            "SetBeginSetAddGold" => {
                sc.get_token(); // (
                def.prop.begin_set_add_gold = sc.get_number_i32();
            }
            "SetBeginSetAddItem" => {
                sc.get_token(); // (
                let idx = sc.get_number_i32();
                let i = slot_or_zero(sc, "SetBeginSetAddItem", idx, 4);
                sc.get_token(); // ,
                let item = sc.get_number();
                sc.get_token(); // ,
                let num = sc.get_number_i32() as i8;
                if let Some(slot) = def.prop.begin_set_add_items.get_mut(i) {
                    slot.item = item;
                    slot.num = num;
                }
            }
            "SetBeginCondPetExp" => {
                sc.get_token(); // (
                def.prop.begin_cond_pet_exp = sc.get_number_i32();
            }
            "SetBeginCondPetLevel" => {
                sc.get_token(); // (
                def.prop.begin_cond_pet_level = sc.get_number_i32();
            }
            "SetBeginCondTutorialState" => {
                sc.get_token(); // (
                def.prop.begin_cond_tutorial_state = sc.get_number_i32();
            }
            "SetBeginCondTSP" => {
                sc.get_token(); // (
                def.prop.begin_cond_tsp = sc.get_number_i32();
            }
            "SetEndCondParty" => {
                let (m, c, n, l) = read_party4(sc);
                def.prop.end_cond_party = m as i8;
                def.prop.end_cond_party_num_comp = c as i8;
                def.prop.end_cond_party_num = n as u16;
                def.prop.end_cond_party_leader = l as i8;
            }
            "SetEndCondGuild" => {
                let (m, c, n, l) = read_party4(sc);
                def.prop.end_cond_guild = m as i8;
                def.prop.end_cond_guild_num_comp = c as i8;
                def.prop.end_cond_guild_num = n as u16;
                def.prop.end_cond_guild_leader = l as i8;
            }
            "SetEndCondState" => {
                sc.get_token(); // (
                def.prop.end_cond_state = sc.get_number_i32() as u8;
            }
            "SetEndCondCompleteQuest" => {
                sc.get_token(); // (
                def.prop.end_cond_complete_quest_oper = sc.get_number_i32() as u8;
                sc.get_token(); // ,
                let mut idx = 0usize;
                let mut over = false;
                while !sc.token().starts_with(')') && !sc.finished() {
                    let q = sc.get_number() as u16;
                    match def.prop.end_cond_complete_quest.get_mut(idx) {
                        Some(slot) => *slot = q,
                        None => over = true,
                    }
                    idx += 1;
                    sc.get_token(); // ,
                }
                if over {
                    sc.report(
                        Severity::Error,
                        DiagCode::CapacityExceeded,
                        "SetEndCondCompleteQuest: more than 6 quests; the extra ones are ignored"
                            .to_owned(),
                    );
                }
            }
            "SetEndCondSkillLvl" => {
                sc.get_token(); // (
                def.prop.end_cond_skill_idx = sc.get_number();
                sc.get_token(); // ,
                def.prop.end_cond_skill_lvl = sc.get_number_i32() as i8;
            }
            "SetEndCondLevel" => {
                sc.get_token(); // (
                def.prop.end_cond_level_min = sc.get_number_i32() as u8;
                sc.get_token(); // ,
                def.prop.end_cond_level_max = sc.get_number_i32() as u8;
            }
            "SetEndCondExpPercent" => {
                sc.get_token(); // (
                def.prop.end_cond_exp_percent_min = sc.get_number_i32();
                sc.get_token(); // ,
                def.prop.end_cond_exp_percent_max = sc.get_number_i32();
            }
            "SetEndCondGold" => {
                sc.get_token(); // (
                def.prop.end_cond_gold = sc.get_number_i32();
            }
            "SetEndCondOneItem" => {
                let e = read_item(sc, "SetEndCondOneItem");
                push_item(sc, &mut def.prop.end_cond_one_item, e, "SetEndCondOneItem");
            }
            "SetEndCondLimitTime" => {
                sc.get_token(); // (
                def.prop.end_cond_limit_time = sc.get_number_i32();
            }
            "SetEndCondItem" => {
                let mut e = read_item(sc, "SetEndCondItem");
                sc.get_token(); // , or )
                if sc.is(",") {
                    e.goal = Some(read_goal(sc, &mut loader.goal_counter));
                }
                push_item(sc, &mut def.prop.end_cond_item, e, "SetEndCondItem");
            }
            "SetEndCondKillNPC" => {
                sc.get_token(); // (
                let idx = sc.get_number_i32();
                let i = slot_or_zero(sc, "SetEndCondKillNPC", idx, 2);
                sc.get_token(); // ,
                let mover = sc.get_number();
                sc.get_token(); // ,
                let num = sc.get_number_i32();
                let mut goal = None;
                sc.get_token(); // , or )
                if sc.is(",") {
                    goal = Some(read_goal(sc, &mut loader.goal_counter));
                }
                if let Some(slot) = def.prop.end_cond_kill_npc.get_mut(i) {
                    slot.mover = mover;
                    slot.num = num;
                    if goal.is_some() {
                        slot.goal = goal;
                    }
                }
            }
            "SetEndCondPatrolZone" => {
                sc.get_token(); // (
                let world = sc.get_number();
                sc.get_token(); // ,
                let left = sc.get_number_i32();
                sc.get_token(); // ,
                let top = sc.get_number_i32();
                sc.get_token(); // ,
                let right = sc.get_number_i32();
                sc.get_token(); // ,
                let bottom = sc.get_number_i32();
                // CRect::NormalizeRect
                let rect = [
                    left.min(right),
                    top.min(bottom),
                    left.max(right),
                    top.max(bottom),
                ];
                let p = def
                    .prop
                    .end_cond_patrol
                    .get_or_insert_with(QuestPatrolDef::default);
                p.world = world;
                p.rect = rect;
                sc.get_token(); // , or )
                if sc.is(",") {
                    sc.get_token_nodef();
                    let dest = atoi_right4(sc.token());
                    sc.get_token(); // ,
                    let goal = read_goal(sc, &mut loader.goal_counter);
                    if let Some(p) = def.prop.end_cond_patrol.as_mut() {
                        p.destination = Some(dest);
                        p.goal = Some(goal);
                    }
                }
            }
            "SetEndCondCharacter" => {
                sc.get_token(); // (
                sc.get_token(); // "key"
                let given = sc.token().to_owned();
                // an empty key means szLinkChar[0] as parsed so far
                let key = if given.is_empty() {
                    def.link.first().cloned().unwrap_or_default()
                } else {
                    bounded(sc, "SetEndCondCharacter", &given, 64)
                };
                def.prop.end_character = key.clone();
                def.dst.push(key);
                def.end_given = true;
                sc.get_token(); // , or )
                if sc.is(",") {
                    def.prop.end_character_goal = Some(read_goal(sc, &mut loader.goal_counter));
                }
            }
            "SetEndCondMultiCharacter" => {
                sc.get_token(); // (
                def.prop.end_multi_characters.clear();
                let mut n = 0usize;
                loop {
                    sc.get_token(); // key
                    let key = sc.token().to_owned();
                    let key = bounded(sc, "SetEndCondMultiCharacter", &key, 64);
                    sc.get_token(); // ,
                    let item = sc.get_number();
                    if !(env.npc_exists)(&key) {
                        let msg = format!("SetEndCondMultiCharacter: the NPC {key} does not exist");
                        sc.report(Severity::Error, DiagCode::RefUnresolved, msg);
                    }
                    def.dst.push(key.clone());
                    def.prop.end_multi_characters.push((key, item));
                    n += 1;
                    sc.get_token(); // , or )
                    if sc.token().starts_with(')') || n >= 10 || sc.finished() {
                        break;
                    }
                }
                def.end_given = true;
            }
            "SetEndCondDialog" => {
                sc.get_token(); // (
                sc.get_token();
                let npc = sc.token().to_owned();
                let npc = bounded(sc, "SetEndCondDialog", &npc, 64);
                sc.get_token(); // ,
                sc.get_token();
                let key = sc.token().to_owned();
                let key = bounded(sc, "SetEndCondDialog", &key, 64);
                let d = def
                    .prop
                    .end_cond_dialog
                    .get_or_insert_with(QuestDialogCond::default);
                d.npc = npc;
                d.key = key;
                sc.get_token(); // , or )
                if sc.is(",") {
                    let goal = read_goal(sc, &mut loader.goal_counter);
                    if let Some(d) = def.prop.end_cond_dialog.as_mut() {
                        d.goal = Some(goal);
                    }
                }
            }
            "SetEndCondPetLevel" => {
                sc.get_token(); // (
                def.prop.end_cond_pet_level = sc.get_number_i32();
            }
            "SetEndCondPetExp" => {
                sc.get_token(); // (
                def.prop.end_cond_pet_exp = sc.get_number_i32();
            }
            "SetEndCondDisguise" => {
                sc.get_token(); // (
                def.prop.end_cond_disguise_mover_index = sc.get_number_i32();
            }
            "SetParam" => {
                sc.get_token(); // (
                let idx = sc.get_number_i32();
                let i = slot_or_zero(sc, "SetParam", idx, 4);
                sc.get_token(); // ,
                let v = sc.get_number_i32();
                if let Some(slot) = def.prop.params.get_mut(i) {
                    *slot = v;
                }
            }
            "SetEndCondTSP" => {
                sc.get_token(); // (
                def.prop.end_cond_tsp = sc.get_number_i32();
            }
            "SetDlgRewardItem" => {
                sc.get_token(); // (
                let idx = sc.get_number_i32();
                let i = slot_or_zero(sc, "SetDlgRewardItem", idx, 4);
                sc.get_token(); // ,
                let item = sc.get_number();
                sc.get_token(); // ,
                let num = sc.get_number_i32();
                if let Some(slot) = def.prop.dlg_reward_items.get_mut(i) {
                    slot.item = item;
                    slot.num = num;
                }
            }
            "SetEndRewardItem" => {
                let mut e = read_item(sc, "SetEndRewardItem");
                sc.get_token(); // , or )
                if sc.is(",") {
                    e.flag = sc.get_number() as u8;
                }
                push_item(sc, &mut def.prop.end_reward_items, e, "SetEndRewardItem");
            }
            "SetEndRewardItemWithAbilityOption" => {
                let mut e = read_item(sc, "SetEndRewardItemWithAbilityOption");
                sc.get_token(); // ,
                e.ability_option = sc.get_number_i32();
                sc.get_token(); // , or )
                if sc.is(",") {
                    e.flag = sc.get_number() as u8;
                }
                push_item(
                    sc,
                    &mut def.prop.end_reward_items,
                    e,
                    "SetEndRewardItemWithAbilityOption",
                );
            }
            "SetEndRewardGold" => {
                sc.get_token(); // (
                def.prop.end_reward_gold_min = sc.get_number_i32();
                sc.get_token(); // ,
                def.prop.end_reward_gold_max = sc.get_number_i32();
            }
            "SetEndRewardPetLevelup" => def.prop.end_reward_pet_levelup = true,
            "SetEndRewardExp" => {
                sc.get_token(); // (
                def.prop.end_reward_exp_min = sc.get_number_i32();
                sc.get_token(); // ,
                def.prop.end_reward_exp_max = sc.get_number_i32();
            }
            "SetEndRewardSkillPoint" => {
                sc.get_token(); // (
                def.prop.end_reward_skill_point = sc.get_number_i32();
            }
            "SetEndRewardPKValue" => {
                sc.get_token(); // (
                def.prop.end_reward_pk_value_min = sc.get_number_i32();
                sc.get_token(); // ,
                def.prop.end_reward_pk_value_max = sc.get_number_i32();
            }
            "SetEndRewardTeleport" => {
                sc.get_token(); // (
                def.prop.end_reward_teleport = sc.get_number();
                sc.get_token(); // ,
                let x = sc.get_float();
                sc.get_token(); // ,
                let y = sc.get_float();
                sc.get_token(); // ,
                let z = sc.get_float();
                def.prop.end_reward_teleport_pos = [x, y, z];
            }
            "SetEndRewardHide" => {
                sc.get_token(); // (
                def.prop.end_reward_hide = sc.get_number() != 0;
            }
            "SetEndRemoveQuest" => {
                sc.get_token(); // (
                let mut n = 0usize;
                loop {
                    let q = sc.get_number_i32();
                    if let Some(slot) = def.prop.end_remove_quests.get_mut(n) {
                        *slot = q;
                    }
                    n += 1;
                    sc.get_token(); // , or )
                    if sc.token().starts_with(')') || n >= MAX_QUESTREMOVE || sc.finished() {
                        break;
                    }
                }
            }
            "SetEndRemoveItem" => {
                sc.get_token(); // (
                let idx = sc.get_number_i32();
                let i = slot_or_zero(sc, "SetEndRemoveItem", idx, 8);
                sc.get_token(); // ,
                let item = sc.get_number();
                sc.get_token(); // ,
                let num = sc.get_number_i32();
                if let Some(slot) = def.prop.end_remove_items.get_mut(i) {
                    slot.item = item;
                    slot.num = num;
                }
            }
            "SetEndRemoveGold" => {
                sc.get_token(); // (
                def.prop.end_remove_gold = sc.get_number_i32();
            }
            "SetRepeat" => {
                sc.get_token(); // (
                def.prop.repeat = sc.get_number() != 0;
            }
            "QuestItem" => {
                if let Some(q) = read_quest_item(sc, &mut loader.quest_item_counts, env, 0) {
                    def.items.push(q);
                }
            }
            "SetEndRewardTSP" => {
                sc.get_token(); // (
                def.prop.end_reward_tsp = sc.get_number_i32();
            }
            "SetEndRemoveTSP" => {
                sc.get_token(); // (
                def.prop.end_remove_tsp = sc.get_number_i32();
            }
            "SetDialog" => {
                sc.get_token(); // (
                let slot = sc.get_number_i32();
                sc.get_token(); // ,
                let text = sc.lang_script();
                match usize::try_from(slot) {
                    Ok(i) if i < MAX_QUEST_DIALOG => {
                        if let Some(s) = def.prop.dialog.get_mut(i) {
                            *s = Some(text);
                        }
                    }
                    Ok(_) => {} // the original ignores a slot of 32 or more
                    Err(_) => {
                        let msg = format!("SetDialog: slot {slot} is negative; ignored");
                        sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                    }
                }
            }
            "SetPatrolZoneName" => {
                sc.get_token(); // (
                let text = sc.lang_script();
                if text.len() < 64 {
                    def.prop.patrol_zone_name = text;
                } else {
                    sc.report(
                        Severity::Error,
                        DiagCode::CapacityExceeded,
                        "SetPatrolZoneName: the text is 64 bytes or longer; ignored".to_owned(),
                    );
                }
            }
            "SetHeadQuest" => {
                sc.get_token(); // (
                def.prop.head_quest = remap_head_quest(sc.get_number() as u16);
            }
            "SetQuestType" => {
                sc.get_token(); // (
                def.prop.quest_type = sc.get_number_i32() as i8;
            }
            "SetRemove" => {
                sc.get_token(); // (
                def.prop.no_remove = sc.get_number() == 0;
            }
            "state" => read_state(sc, def, loader, env),
            _ => {}
        }
    }

    /// `state N { .. }`: `SetDesc` and `QuestItem` count, `SetCond` and `SetStatus` are read and dropped. A
    /// state outside 0..16 overflowed the original's array: it is an Error and the block is read and ignored.
    fn read_state(
        sc: &mut Script<'_>,
        def: &mut Def,
        loader: &mut super::QuestLoaderState,
        env: &QuestEnv<'_>,
    ) {
        let n = sc.get_number_i32();
        let slot = usize::try_from(n).ok().filter(|i| *i < MAX_QUEST_STATE);
        if slot.is_none() {
            let msg = format!("state {n}: outside 0..{MAX_QUEST_STATE}; the block is ignored");
            sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
        }
        let state_u8 = slot.and_then(|i| u8::try_from(i).ok());
        let mut desc = String::new();
        sc.get_token(); // { or anything
        while !sc.token().starts_with('}') && !sc.finished() {
            match sc.token() {
                "QuestItem" => {
                    // a block that is ignored keeps its drops out of the per-mover count too
                    if let Some(st) = state_u8 {
                        if let Some(q) = read_quest_item(sc, &mut loader.quest_item_counts, env, st)
                        {
                            def.items.push(q);
                        }
                    } else {
                        let mut scratch = BTreeMap::new();
                        let _ = read_quest_item(sc, &mut scratch, env, 0);
                    }
                }
                "SetDesc" => {
                    sc.get_token(); // (
                    let text = sc.lang_script();
                    if text.len() < 512 {
                        desc = text;
                    } else {
                        sc.report(
                            Severity::Error,
                            DiagCode::CapacityExceeded,
                            "SetDesc: the text is 512 bytes or longer; ignored".to_owned(),
                        );
                    }
                }
                "SetCond" | "SetStatus" => {
                    sc.get_token(); // (
                    let _ = sc.lang_script();
                }
                _ => {}
            }
            sc.get_token(); // key or anything
        }
        if let Some(i) = slot
            && let Some(s) = def.prop.states.get_mut(i)
        {
            *s = Some(QuestStateDef { desc });
        }
    }

    fn add_link(map: &mut BTreeMap<String, Vec<QuestId>>, key: &str, quest: QuestId) {
        let list = map.entry(key.to_ascii_lowercase()).or_default();
        if !list.contains(&quest) {
            list.push(quest);
        }
    }

    /// The end-character fallback of `Project.cpp:2465-2496`, run when neither `SetEndCondCharacter` nor
    /// `SetEndCondMultiCharacter` was given.
    fn end_fallback(def: &mut Def) {
        if def.end_given {
            return;
        }
        match def.link.len() {
            0 => {}
            1 => {
                if let Some(first) = def.link.first().cloned() {
                    def.dst.push(first.clone());
                    def.prop.end_character = first;
                }
            }
            n => {
                // KEEP-QUIRK: the first giver goes into every slot, and the end character stays empty
                let first = def.link.first().cloned().unwrap_or_default();
                def.prop.end_multi_characters = (0..n).map(|_| (first.clone(), 0)).collect();
                def.dst.extend(def.link.iter().cloned());
            }
        }
    }

    /// `CProject::LoadPropQuest` over decoded `text`: reads every quest of the file into `tables` (a later id
    /// replaces an earlier one, also across files), advancing the goal counter and the per-mover quest-item
    /// counts of `tables.loader`.
    pub(crate) fn parse_quest_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        tables: &mut QuestTables,
        env: &QuestEnv<'_>,
        diags: &mut Diagnostics,
    ) {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut n_quest = sc.get_number();
        while !sc.finished() {
            let mut def = Def {
                prop: QuestProp::new(n_quest),
                link: Vec::new(),
                dst: Vec::new(),
                end_given: false,
                items: Vec::new(),
            };
            sc.get_token(); // {
            let mut block = 1u32;
            while block != 0 && !sc.finished() {
                sc.get_token();
                match sc.token() {
                    "{" => block += 1,
                    "}" => block -= 1,
                    _ => run_directive(&mut sc, &mut def, &mut tables.loader, env),
                }
            }
            end_fallback(&mut def);
            let id_ok = u16::try_from(n_quest).is_ok();
            if !id_ok {
                let msg = format!("quest id {n_quest} does not fit 16 bits; the quest is skipped");
                sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
            }
            // QUEST_KAWIBAWIBO01 is never added; its links and drops go with it
            if id_ok && n_quest != QUEST_KAWIBAWIBO01 {
                let Def {
                    mut prop,
                    dst,
                    items,
                    ..
                } = def;
                prop.quest_items = items;
                for (key, _) in &prop.givers {
                    if (env.npc_exists)(key) {
                        add_link(&mut tables.quest_src, key, n_quest);
                    }
                }
                for key in &dst {
                    if (env.npc_exists)(key) {
                        add_link(&mut tables.quest_dst, key, n_quest);
                    }
                }
                let _ = tables.quests.insert(n_quest, prop);
            }
            n_quest = sc.get_number();
        }
    }

    /// Cuts `text` to a `char[limit]`, reporting an over-long one.
    fn raw_text(sc: &mut Script<'_>, what: &str, limit: usize) -> String {
        let t = sc.token().to_owned();
        bounded(sc, what, &t, limit)
    }

    /// `State n { desc = "text" }` of the guild and party quest files; `desc` is a raw token.
    fn read_desc_block(sc: &mut Script<'_>, desc: &mut [Option<String>; MAX_GUILD_QUEST_STATE]) {
        let n = sc.get_number_i32();
        sc.get_token(); // key or anything
        while !sc.token().starts_with('}') && !sc.finished() {
            if sc.is("desc") {
                sc.get_token(); // =
                sc.get_token();
                let text = raw_text(sc, "desc", 260);
                match usize::try_from(n)
                    .ok()
                    .filter(|i| *i < MAX_GUILD_QUEST_STATE)
                    .and_then(|i| desc.get_mut(i))
                {
                    Some(slot) => *slot = Some(text),
                    None => {
                        let msg = format!(
                            "State {n}: outside 0..{MAX_GUILD_QUEST_STATE}; the text is ignored"
                        );
                        sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                    }
                }
            }
            sc.get_token(); // key or anything
        }
    }

    /// `Region = x1 , y1 , x2 , y2`.
    fn read_region(sc: &mut Script<'_>) -> [i32; 4] {
        sc.get_token(); // =
        let x1 = sc.get_number_i32();
        sc.get_token(); // ,
        let y1 = sc.get_number_i32();
        sc.get_token(); // ,
        let x2 = sc.get_number_i32();
        sc.get_token(); // ,
        let y2 = sc.get_number_i32();
        [x1, y1, x2, y2]
    }

    /// `CProject::LoadPropGuildQuest`: `SetAtGrow`, so the last definition of an id wins.
    pub(crate) fn parse_guild_quest_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        out: &mut IdTable<GuildQuestProp>,
        diags: &mut Diagnostics,
    ) {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut id = sc.get_number();
        while !sc.finished() {
            let mut prop = GuildQuestProp::default();
            sc.get_token();
            while !sc.token().starts_with('}') && !sc.finished() {
                match sc.token() {
                    "Title" => {
                        sc.get_token(); // =
                        sc.get_token();
                        prop.title = raw_text(&mut sc, "Title", 32);
                    }
                    "Level" => {
                        sc.get_token(); // =
                        prop.level = sc.get_number_i32();
                    }
                    "Wormon" => {
                        sc.get_token(); // =
                        prop.wormon = sc.get_number();
                    }
                    "World" => {
                        sc.get_token(); // =
                        prop.world = sc.get_number();
                    }
                    "Position" => {
                        sc.get_token(); // =
                        let x = sc.get_float();
                        sc.get_token(); // ,
                        let y = sc.get_float();
                        sc.get_token(); // ,
                        let z = sc.get_float();
                        prop.pos = [x, y, z];
                    }
                    "Region" => prop.region = Some(read_region(&mut sc)),
                    "State" => read_desc_block(&mut sc, &mut prop.desc),
                    _ => {}
                }
                sc.get_token();
            }
            if out.insert(id, prop).is_err() {
                let msg = format!("guild quest id {id} is out of range; the quest is skipped");
                sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
            }
            id = sc.get_number();
        }
    }

    /// `CProject::LoadPropPartyQuest`: `std::map::insert`, so the FIRST definition of an id wins; a `Wormon` list
    /// of more than 1000 entries ends the file (`return FALSE`), keeping the quests inserted before.
    pub(crate) fn parse_party_quest_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        out: &mut IdTable<PartyQuestProp>,
        diags: &mut Diagnostics,
    ) {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut id = sc.get_number();
        while !sc.finished() {
            let mut prop = PartyQuestProp::default();
            sc.get_token();
            while !sc.token().starts_with('}') && !sc.finished() {
                match sc.token() {
                    "Title" => {
                        sc.get_token(); // =
                        sc.get_token();
                        prop.title = raw_text(&mut sc, "Title", 32);
                    }
                    "Level" => {
                        sc.get_token(); // =
                        prop.level = sc.get_number_i32();
                    }
                    "Wormon" => {
                        prop.wormons.clear();
                        let mut count = 0usize;
                        sc.get_token();
                        while !sc.token().starts_with('}') && !sc.finished() {
                            count += 1;
                            let mover = sc.get_number();
                            sc.get_token(); // ,
                            let x = sc.get_float();
                            sc.get_token(); // ,
                            let y = sc.get_float();
                            sc.get_token(); // ,
                            let z = sc.get_float();
                            sc.get_token(); // ,
                            prop.wormons.push((mover, [x, y, z]));
                            if count > MAX_PARTY_WORMON {
                                let msg = format!(
                                    "propPartyQuest.inc: {}: more than {MAX_PARTY_WORMON} wormon entries; the file ends here",
                                    prop.title
                                );
                                sc.report(Severity::Error, DiagCode::CapacityExceeded, msg);
                                return;
                            }
                        }
                    }
                    "World" => {
                        sc.get_token(); // =
                        prop.world = sc.get_number();
                    }
                    "WorldKey" => {
                        sc.get_token(); // =
                        sc.get_token();
                        prop.world_key = raw_text(&mut sc, "WorldKey", 32);
                    }
                    "Region" => prop.region = Some(read_region(&mut sc)),
                    "State" => read_desc_block(&mut sc, &mut prop.desc),
                    _ => {}
                }
                sc.get_token();
            }
            if !out.contains(id) && out.insert(id, prop).is_err() {
                let msg = format!("party quest id {id} is out of range; the quest is skipped");
                sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
            }
            id = sc.get_number();
        }
    }
}
