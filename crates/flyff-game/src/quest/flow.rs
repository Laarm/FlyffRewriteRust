//! The quest dialog flows (Plan/20 §9.8, v15 `__IMPROVE_QUEST_INTERFACE` branches; src:_Common/ScriptHelper.cpp:
//! 498-884): `LaunchQuest` = [`quest_end`] with quest 0, the predefined keys `QUEST_BEGIN`, `QUEST_BEGIN_YES`,
//! `QUEST_BEGIN_NO`, `QUEST_END`, `QUEST_END_FAIL` and `QUEST_END_COMPLETE`.
//!
//! The flows only *sequence* operations: lists and answers go out through [`FlowHost`] (the dialog host emits
//! the `DialogOp`s and builds the player's [`QuestSubject`] for the condition questions), the rules come from
//! [`QuestPredicates`]. [`NoQuestData`] knows no quest (the unit tests and the default of `NpcService`), so
//! [`quest_end`] reduces to `remove_all_key`, `#addKey`, `#noQuest` (Plan/20 §7.6); `Game::new` installs the real
//! [`QuestRules`](super::QuestRules) over the loaded tables (also on empty data: the same operations). The reward
//! order and the start effects are WP-NPC-14. A fixture implementation proves the sequences in this module's tests.

use super::QuestBook;
use super::predicates::BeginCheck;
use super::subject::QuestSubject;
use flyff_lua::HostError;
use std::sync::Arc;

/// The quest dialog slots (`QSAY_*`, `Resource/definequest.h`).
pub mod qsay {
    /// `QSAY_BEGIN1..5`: the offer text.
    pub const BEGIN: [usize; 5] = [0, 1, 2, 3, 4];
    /// `QSAY_BEGIN_YES`.
    pub const BEGIN_YES: usize = 5;
    /// `QSAY_BEGIN_NO`.
    pub const BEGIN_NO: usize = 6;
    /// `QSAY_END_COMPLETE1..3`.
    pub const END_COMPLETE: [usize; 3] = [7, 8, 9];
    /// `QSAY_END_FAILURE1..3`.
    pub const END_FAILURE: [usize; 3] = [10, 11, 12];
}

/// Whether a begin predicate may send messages (`TID_GAME_LACKSPACE`, Plan/20 §9.4 B0a).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PredicateMode {
    /// Building the offer lists: no messages.
    List,
    /// `QUEST_BEGIN_YES`: the player asked to start.
    Accept,
}

/// The quest rules the flows ask (Plan/20 §9.4-§9.5). Implemented over the quest tables by
/// [`QuestRules`](super::QuestRules). The condition predicates read the player through a [`QuestSubject`].
pub trait QuestPredicates: std::fmt::Debug + Send + Sync {
    /// `quest_src[npc_key]` in file order: the quests this NPC gives (VR-060 checks membership).
    fn offered_by(&self, npc_key: &str) -> Vec<u32>;
    /// `true` when the quest has a property record.
    fn exists(&self, q: u32) -> bool;
    /// The quest title.
    fn title(&self, q: u32) -> String;
    /// `SetBeginCondLevel(min, max)`, the sort key of the lists.
    fn level_range(&self, q: u32) -> (i32, i32);
    /// `eq_ignore_ascii_case(QuestProp.end_character, npc_key)` (VR-060: `QUEST_END_COMPLETE` checks the end NPC);
    /// an empty end character never matches.
    fn end_character_is(&self, q: u32, npc_key: &str) -> bool;
    /// `can_begin` (B0-B15) for the subject; the result says whether `TID_GAME_LACKSPACE` is due (B0a).
    fn can_begin(&self, s: &dyn QuestSubject, q: u32, mode: PredicateMode) -> BeginCheck;
    /// `is_next_level`: displayed red, never accepted.
    fn is_next_level(&self, s: &dyn QuestSubject, q: u32) -> bool;
    /// `can_end` (E1-E16). Pure: the host freezes the timer of a timed quest when it is true
    /// ([`FlowHost::can_end`]).
    fn can_end(&self, s: &dyn QuestSubject, q: u32) -> bool;
    /// The first active quest of `book` (book order) whose `SetEndCondDialog(npc, key)` names this NPC and this
    /// key; both strings are compared **case-sensitively** (`AddCondKey` and the `ScriptDlg` hook).
    fn cond_dialog_quest(&self, _book: &QuestBook, _npc_key: &str, _key: &str) -> Option<u32> {
        None
    }
    /// The quest dialog line `QuestProp.dialog[slot]` (`QSAY_*`), `None` when the slot is empty.
    fn dialog_line(&self, _q: u32, _slot: usize) -> Option<String> {
        None
    }
}

/// The M4 predicates: no quest exists, so nothing is offered, begun or completed.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoQuestData;

impl QuestPredicates for NoQuestData {
    fn offered_by(&self, _npc_key: &str) -> Vec<u32> {
        Vec::new()
    }

    fn exists(&self, _q: u32) -> bool {
        false
    }

    fn title(&self, _q: u32) -> String {
        String::new()
    }

    fn level_range(&self, _q: u32) -> (i32, i32) {
        (0, 0)
    }

    fn end_character_is(&self, _q: u32, _npc_key: &str) -> bool {
        false
    }

    fn can_begin(&self, _s: &dyn QuestSubject, _q: u32, _mode: PredicateMode) -> BeginCheck {
        BeginCheck::default()
    }

    fn is_next_level(&self, _s: &dyn QuestSubject, _q: u32) -> bool {
        false
    }

    fn can_end(&self, _s: &dyn QuestSubject, _q: u32) -> bool {
        false
    }
}

/// What the flows need from the dialog (implemented by the dialog host).
pub trait FlowHost {
    /// The character key of the NPC (lower case).
    fn npc_key(&self) -> String;
    /// The predicates in force.
    fn predicates(&self) -> Arc<dyn QuestPredicates>;
    /// The quest book of the player (`None` without a player).
    fn book(&mut self) -> Option<&mut QuestBook>;
    /// `can_begin(q, mode)`: builds the player's subject, asks the predicates and, when B0a failed and the message
    /// is due, sends `TID_GAME_LACKSPACE`. `false` without a player.
    fn can_begin(&mut self, q: u32, mode: PredicateMode) -> bool;
    /// `is_next_level(q)`.
    fn is_next_level(&mut self, q: u32) -> bool;
    /// `can_end(q)`; on `true` the timer of a timed quest freezes (KEEP: also while a list is built and also when
    /// the completion then fails).
    fn can_end(&mut self, q: u32) -> bool;
    /// Op `remove_all_key`.
    fn remove_all_key(&mut self);
    /// `run_dialog(key, value, quest)` of the same NPC (predefined keys first, then the script function).
    fn run_key(&mut self, key: &str, value: i64, quest: i64) -> Result<(), HostError>;
    /// Op `say` with the quest dialog line `QuestProp.dialog[slot]` of `quest` (nothing when the slot is empty).
    fn say_quest(&mut self, quest: u32, slot: usize);
    /// Op `add_answer{word, key, param, quest}`.
    fn add_answer(&mut self, word: &str, key: &str, param: u32, quest: u32);
    /// Op `new_quest{title, key, param, quest}`.
    fn new_quest(&mut self, title: &str, key: &str, param: u32, quest: u32);
    /// Op `curr_quest{title, key, param, quest}`.
    fn curr_quest(&mut self, title: &str, key: &str, param: u32, quest: u32);
    /// `run_quest(pc, npc, q)`: the start effects, then `set_quest_started`.
    fn run_quest(&mut self, q: u32);
    /// `end_quest(pc, q, check_cond)`: the rewards in the order of §9.7, then `set_quest_state(q, 14)`.
    fn end_quest(&mut self, q: u32, check_cond: bool) -> bool;
}

/// The lists of one dialog refresh (§9.8 `build_lists`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Lists {
    /// Quests the player may begin.
    pub new: Vec<u32>,
    /// Quests that begin within 5 levels (red, not clickable).
    pub next: Vec<u32>,
    /// Running quests that can be completed here.
    pub end: Vec<u32>,
    /// Other running quests of this NPC.
    pub curr: Vec<u32>,
}

/// `__QuestSort`: descending by `(level min, level max, quest id)`.
fn sort_desc(preds: &dyn QuestPredicates, list: &mut [u32]) {
    list.sort_by_key(|q| {
        let (min, max) = preds.level_range(*q);
        std::cmp::Reverse((min, max, *q))
    });
}

/// `build_lists(pc, npc, strict_end)` (Plan/20 §9.8): sorts the quests of this NPC into the four lists and emits
/// them in the order NEW, NEXT, END, CURR. Returns the lists and whether a new quest was found.
pub fn build_lists(host: &mut dyn FlowHost, strict_end: bool) -> (bool, Lists) {
    let preds = host.predicates();
    let npc_key = host.npc_key();
    let mut lists = Lists::default();
    for q in preds.offered_by(&npc_key) {
        let Some(book) = host.book() else { break };
        let active = book.find(q).map(|r| r.state);
        let complete = book.is_complete(q);
        match active {
            None if !complete => {
                if host.can_begin(q, PredicateMode::List) {
                    lists.new.push(q);
                } else if host.is_next_level(q) {
                    lists.next.push(q);
                }
            }
            Some(state) if !complete && (!strict_end || state != super::QS_END) => {
                if host.can_end(q) {
                    lists.end.push(q);
                } else {
                    lists.curr.push(q);
                }
            }
            _ => {}
        }
    }
    for l in [
        &mut lists.new,
        &mut lists.next,
        &mut lists.end,
        &mut lists.curr,
    ] {
        sort_desc(preds.as_ref(), l);
    }
    let new_found = !lists.new.is_empty();
    for q in lists.new.clone() {
        host.new_quest(&preds.title(q), "QUEST_BEGIN", 0, q);
    }
    for q in lists.next.clone() {
        host.new_quest(&preds.title(q), "QUEST_NEXT_LEVEL", 0, q);
    }
    for q in lists.end.clone().into_iter().chain(lists.curr.clone()) {
        host.curr_quest(&preds.title(q), "QUEST_END", 0, q);
    }
    (new_found, lists)
}

fn say_slots(host: &mut dyn FlowHost, q: u32, slots: &[usize]) {
    for slot in slots {
        host.say_quest(q, *slot);
    }
}

/// `LaunchQuest()` is `quest_end(host, 0, false)`; `QUEST_END` and `QUEST_END_FAIL` arrive with the quest id
/// (`butt_ok` is the second). Plan/20 §9.8.
pub fn quest_end(host: &mut dyn FlowHost, q: u32, butt_ok: bool) -> Result<(), HostError> {
    let preds = host.predicates();
    let npc_key = host.npc_key();
    host.remove_all_key();
    host.run_key("#addKey", 0, 0)?;
    let (new_found, lists) = build_lists(host, true);
    let mut greet = true;
    let mut complete_check = true;
    let state_of_q = host.book().and_then(|b| b.find(q).map(|r| r.state));
    if q != 0 && state_of_q.is_some_and(|s| s != super::QS_END) && preds.exists(q) {
        complete_check = false;
        if !butt_ok {
            let ready = preds.end_character_is(q, &npc_key) && host.can_end(q);
            if ready {
                say_slots(host, q, &qsay::END_COMPLETE);
                host.add_answer("__OK__", "QUEST_END_COMPLETE", 0, q);
            } else {
                say_slots(host, q, &qsay::END_FAILURE);
                host.add_answer("__OK__", "QUEST_END_FAIL", 0, q);
            }
            greet = false;
        }
    }
    if complete_check {
        let active: Vec<(u32, u8)> = host
            .book()
            .map(|b| {
                b.doc
                    .active
                    .iter()
                    .map(|r| (u32::from(r.quest_id), r.state))
                    .collect()
            })
            .unwrap_or_default();
        for (r, state) in active {
            let done = preds.exists(r)
                && preds.end_character_is(r, &npc_key)
                && state != super::QS_END
                && host.can_end(r);
            if done {
                say_slots(host, r, &qsay::END_COMPLETE);
                host.add_answer("__OK__", "QUEST_END_COMPLETE", 0, r);
                greet = false;
                break;
            }
        }
    }
    if greet {
        if new_found {
            if lists.new.len() == 1 && complete_check {
                if let Some(&only) = lists.new.first() {
                    quest_begin(host, only)?;
                }
            } else {
                host.run_key("#yesQuest", 0, 0)?;
            }
        } else {
            host.run_key("#noQuest", 0, 0)?;
        }
    }
    Ok(())
}

/// `QUEST_BEGIN(q)` (VR-060: the quest must be one of this NPC's): the offer text and the two answers. No
/// condition check, as in the original.
pub fn quest_begin(host: &mut dyn FlowHost, q: u32) -> Result<(), HostError> {
    let preds = host.predicates();
    if !preds.offered_by(&host.npc_key()).contains(&q) {
        tracing::debug!(
            quest = q,
            "QUEST_BEGIN for a quest the NPC does not give: ignored (VR-060)"
        );
        return Ok(());
    }
    say_slots(host, q, &qsay::BEGIN);
    host.add_answer("__YES__", "QUEST_BEGIN_YES", 0, q);
    host.add_answer("__NO__", "QUEST_BEGIN_NO", 0, q);
    host.run_key("#questBegin", 0, i64::from(q))
}

/// `QUEST_BEGIN_YES(q)` (VR-060 as above): validates, starts the quest and refreshes the lists. A refusal sends
/// nothing (KEEP).
pub fn quest_begin_yes(host: &mut dyn FlowHost, q: u32) -> Result<(), HostError> {
    let preds = host.predicates();
    if !preds.offered_by(&host.npc_key()).contains(&q) {
        return Ok(());
    }
    // `__QuestBeginYes`: the begin condition, then "not running and not completed" (the condition goes first: its
    // `TID_GAME_LACKSPACE` is sent whatever the book says)
    let may_begin = host.can_begin(q, PredicateMode::Accept)
        && host
            .book()
            .is_some_and(|b| b.find(q).is_none() && !b.is_complete(q));
    if !may_begin {
        return Ok(());
    }
    host.say_quest(q, qsay::BEGIN_YES);
    host.run_quest(q);
    host.remove_all_key();
    host.run_key("#addKey", 0, 0)?;
    build_lists(host, false);
    host.run_key("#questBeginYes", 0, i64::from(q))
}

/// `QUEST_BEGIN_NO(q)`.
pub fn quest_begin_no(host: &mut dyn FlowHost, q: u32) -> Result<(), HostError> {
    host.say_quest(q, qsay::BEGIN_NO);
    host.run_key("#questBeginNo", 0, i64::from(q))
}

/// `QUEST_END_COMPLETE(q)` (VR-060 FIX: the end NPC of the quest must be this NPC): the rewards, `#questEndComplete`
/// and the refreshed lists.
pub fn quest_end_complete(host: &mut dyn FlowHost, value: i64, q: u32) -> Result<(), HostError> {
    let _ = value;
    let preds = host.predicates();
    if !preds.end_character_is(q, &host.npc_key()) {
        tracing::debug!(
            quest = q,
            "QUEST_END_COMPLETE at a wrong NPC: ignored (VR-060)"
        );
        return Ok(());
    }
    let startable = host
        .book()
        .is_some_and(|b| b.find(q).is_some_and(|r| r.state != super::QS_END) && !b.is_complete(q));
    if startable && host.can_end(q) {
        if host.end_quest(q, true) {
            host.run_key("#questEndComplete", 0, i64::from(q))?;
        }
        quest_end(host, q, true)?;
    }
    Ok(())
}
