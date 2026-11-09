//! Quests (Plan/20 §9). The quest core of milestone M7 (WP-NPC-12 / -13 over the loaded `QuestTables`, WP-DATA-15):
//!
//! | module | content |
//! |---|---|
//! | [`book`] | [`QuestBook`]: the per-player records, `set`, the load normalisation, the tracker, the timer freeze |
//! | [`msg`] | the `Quest` record and the `SetQuest` / `RemoveQuestNtf` / `QuestChecked` builders |
//! | [`ops`] | the primitives of §9.6 (`set_quest_started`, `set_quest_state`, `remove_quest`, `cancel_quest`, the expiry rule) |
//! | [`handlers`] | the client requests `RemoveQuestReq` and `QuestCheck` (§9.10) |
//! | [`gm`] | the GM API for `/bq /eq /qs /rq /raq /rcq` (§9.14) |
//! | [`subject`] | [`QuestSubject`] and [`PlayerSubject`]: what the predicates read of a player |
//! | [`predicates`] | the begin, next-level and end predicates B0-B15 / E1-E16 with the PL-159 switch |
//! | [`index`] | [`QuestIndex`]: the lookups derived from the tables |
//! | [`rules`] | [`QuestRules`] (the [`QuestPredicates`] over the data) and [`QuestService`] (`Services.quest`) |
//! | [`flow`] | the dialog flows and the [`QuestPredicates`] / [`FlowHost`] seams (M4, WP-NPC-06) |
//! | [`effects`] | the start effects and the reward sequence (`run_quest`, `end_quest`, §9.6-§9.7, WP-NPC-14) |
//! | [`track`] | the per-tick row `quest_process` (patrol, minute timer, server-side expiry), the kill counters, the text item and the ctrl port (§9.9, WP-NPC-15) |
//! | [`drops`] | the quest item drops of a monster kill ([`drops::QuestDrops`], Plan/19 §6.2 step 3) |
//! | [`locator`] | `QuestHelperReqNpcPos` (the quest helper's NPC position) |
//! | [`guild`] | the guild quests: `guild_quest_set`, `guild_quest_remove`, `state_of` and the `IsGuildQuest` helpers (WP-NPC-16) |
//! | [`wormon`] | `QuestTextTime` and the module doc of the deferred Wormon processors (WP-NPC-16) |
//!
//! The shipped quest files are empty (PD-01): with the real data every NPC shows its key buttons and its
//! `#noQuest` greeting, the client hides its quest UI (PL-148), and every tracking path returns before it draws
//! or sends anything. The guild-quest state and `QuestTextTime` are WP-NPC-16; the proof on a synthetic quest set
//! (`content/quests/overlay/`) is WP-NPC-17 (`proof_tests`).

pub mod book;
pub mod drops;
pub mod effects;
pub mod flow;
pub mod gm;
pub mod guild;
pub mod handlers;
pub mod index;
pub mod locator;
pub mod msg;
pub mod ops;
pub mod predicates;
pub mod rules;
pub mod subject;
pub mod track;
pub mod wormon;

pub use book::{LoadNote, QuestBook};
pub use flow::{FlowHost, NoQuestData, PredicateMode, QuestPredicates};
pub use guild::{guild_quest_remove, guild_quest_set};
pub use index::{PatrolRect, QuestDrop, QuestIndex};
pub use ops::{RemoveReason, emit_load_notes};
pub use predicates::BeginCheck;
pub use rules::{QuestRules, QuestService};
pub use subject::{GuildView, PartyView, PetView, PlayerSubject, QuestSubject};
pub use wormon::send_text_time;

/// `QS_BEGIN`: a freshly started quest.
pub const QS_BEGIN: u8 = 0;
/// `QS_END`: the completed state.
pub const QS_END: u8 = 14;
/// `MAX_QUEST`: active quests per player.
pub const MAX_QUEST: usize = 100;
/// `QUEST_CREGUILD`: the guild creation quest (special-cased: no "quest ended" message, PL-149).
pub use flyff_common::defs::quest::QUEST_CREGUILD;

/// The dialog-condition hook of `ScriptDlg` step 10 (Plan/20 §7.2, src:WORLDSERVER/DPSrvr.cpp:968-985): the first
/// active quest whose `SetEndCondDialog` names this NPC and this key gets `dialog_done = true`. Returns the quest
/// that matched; the caller sends `SetQuest` for it.
pub fn dialog_condition_hook(
    predicates: &dyn QuestPredicates,
    book: &mut QuestBook,
    npc_key: &str,
    key: &str,
) -> Option<u16> {
    let q = predicates.cond_dialog_quest(book, npc_key, key)?;
    let q = u16::try_from(q).ok()?;
    let record = book.doc.active.iter_mut().find(|r| r.quest_id == q)?;
    record.dialog_done = true;
    Some(q)
}
