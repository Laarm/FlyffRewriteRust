//! `QuestBook`: the per-player quest state (Plan/20 §9.2, WP-NPC-12): the active records, the completed ids and the
//! tracker list of the `quests` document, the minute timer's phase, and the notes of the load normalisation.
//!
//! The book keeps the persisted document unchanged (`PlayerExt.quests.doc.*` is read by the snapshot, the join
//! message, the ctrl arrival rule and the admin watch). `set` is `CMover::SetQuest` / `__SetQuest`
//! (`MoverParam.cpp:3468-3640`) with the FIXes of PL-160; `from_doc` is the load fix-up of
//! `DbManagerFun.cpp:1262-1365` (13 §17.4 step 3) plus the timeout rule of PL-108.

use super::{MAX_QUEST, QS_END};
use flyff_common::GameMs;
use flyff_data::quest::QuestTables;
use flyff_db::QuestsDoc;
use flyff_db::model::QuestRecord;

/// `MAX_CHECKED_QUEST`: the tracker holds at most this many quests.
pub const MAX_CHECKED_QUEST: usize = 5;

/// The period of the quest minute timer (`m_timerQuestLimitTime`, 60 s).
pub const QUEST_MINUTE_MS: u64 = 60_000;

/// What the load normalisation did to the document; the caller turns the notes into audit lines
/// ([`super::emit_load_notes`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LoadNote {
    /// A state-14 record moved to the completed list (and left the active and tracker lists).
    MovedToCompleted(u16),
    /// A later record of an id that was already active was removed (the first one wins).
    DuplicateRemoved(u16),
    /// A timed, unfrozen quest stored at 0 minutes was removed (PL-108, audit `quest.remove{reason: "timeout"}`).
    TimedOut(u16),
}

/// Active, completed and tracked quests of a player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestBook {
    /// The `quests` document (13 §6.7).
    pub doc: QuestsDoc,
    /// When the next quest minute is due (`m_timerQuestLimitTime`). Not persisted: the phase restarts at the join.
    pub minute_due: GameMs,
    /// What the load did to the document; taken by [`super::emit_load_notes`]. Not persisted.
    pub load_notes: Vec<LoadNote>,
}

/// The `m_wTime` word of a new record: the limit truncated to 16 bits, bit 15 being the frozen flag.
fn limit_word(limit_min: i32) -> (u16, bool) {
    let w = limit_min as u16;
    (w & 0x7FFF, w & 0x8000 != 0)
}

impl QuestBook {
    /// The book of a loaded document (13 §17.4 step 3): see [`QuestBook::from_doc`]; the notes stay in
    /// [`QuestBook::load_notes`].
    pub fn load(doc: QuestsDoc, data: &QuestTables, now: GameMs) -> QuestBook {
        let (mut book, notes) = QuestBook::from_doc(doc, data, now);
        book.load_notes = notes;
        book
    }

    /// The load normalisation in ONE pass in book order (KEEP the order of `DbManagerFun.cpp`):
    ///
    /// * a state-14 record moves to `completed` (if absent) and leaves `active` and `checked`;
    /// * any other record removes every later record of the same id, whatever its state (the first wins: so
    ///   `[q s3, q s14]` keeps s3 active while `[q s14, q s3]` completes q and keeps s3 active too);
    /// * FIX: the tracker removal is order-preserving (the original shifted with the wrong index, duplicating the
    ///   next entry and dropping the last; display only);
    /// * PL-108 / PL-160: a timed quest (`SetEndCondLimitTime > 0`) with `time_left_min == 0` that is not frozen is
    ///   removed, also with `SetRemove(0)`; ids without a quest property stay untouched.
    ///
    /// `minute_due` is `now + 60 000`.
    pub fn from_doc(
        mut doc: QuestsDoc,
        data: &QuestTables,
        now: GameMs,
    ) -> (QuestBook, Vec<LoadNote>) {
        let mut notes = Vec::new();
        let mut i = 0;
        while i < doc.active.len() {
            let Some(r) = doc.active.get(i).copied() else {
                break;
            };
            if r.state == QS_END {
                if !doc.completed.contains(&r.quest_id) {
                    doc.completed.push(r.quest_id);
                }
                doc.active.remove(i);
                doc.checked.retain(|c| *c != r.quest_id);
                notes.push(LoadNote::MovedToCompleted(r.quest_id));
            } else {
                let before = doc.active.len();
                let mut at = 0;
                doc.active.retain(|x| {
                    let keep = at <= i || x.quest_id != r.quest_id;
                    at += 1;
                    keep
                });
                for _ in doc.active.len()..before {
                    notes.push(LoadNote::DuplicateRemoved(r.quest_id));
                }
                i += 1;
            }
        }
        let timed_out: Vec<u16> = doc
            .active
            .iter()
            .filter(|r| {
                !r.timer_frozen
                    && r.time_left_min == 0
                    && data
                        .quests
                        .get(u32::from(r.quest_id))
                        .is_some_and(|p| p.is_timed())
            })
            .map(|r| r.quest_id)
            .collect();
        for q in timed_out {
            doc.active.retain(|r| r.quest_id != q);
            doc.checked.retain(|c| *c != q);
            notes.push(LoadNote::TimedOut(q));
        }
        let book = QuestBook {
            doc,
            minute_due: GameMs(now.0.saturating_add(QUEST_MINUTE_MS)),
            load_notes: Vec::new(),
        };
        (book, notes)
    }

    /// The document of the book (what the snapshot persists).
    pub fn to_doc(&self) -> QuestsDoc {
        self.doc.clone()
    }

    /// The active record of quest `q`.
    pub fn find(&self, q: u32) -> Option<&QuestRecord> {
        let q = u16::try_from(q).ok()?;
        self.doc.active.iter().find(|r| r.quest_id == q)
    }

    /// `true` when quest `q` is on the completed list (`IsCompleteQuest`).
    pub fn is_complete(&self, q: u32) -> bool {
        u16::try_from(q).is_ok_and(|q| self.doc.completed.contains(&q))
    }

    /// `true` when quest `q` is active.
    pub fn is_active(&self, q: u32) -> bool {
        self.find(q).is_some()
    }

    /// `GetQuestState(q)` (Plan/20 §6.5 #39): the state of an active quest (0..13, or 14 for a state-14 record
    /// that is still active), 14 for a completed quest, `None` otherwise (the script sees −1).
    pub fn state_of(&self, q: u32) -> Option<u8> {
        if let Some(r) = self.find(q) {
            return Some(r.state);
        }
        self.is_complete(q).then_some(QS_END)
    }

    /// `CMover::SetQuest(q, state)` (Plan/20 §9.2 steps 1-6) and the record `SetQuest` sends:
    ///
    /// 1. a completed quest that is not active cannot restart (`None`);
    /// 2. the new record is zeroed except `quest_id` and `state` (a state change resets the counters, the
    ///    timer and the flags: KEEP-QUIRK);
    /// 3. a new record past [`MAX_QUEST`] active ones is refused with a warning; otherwise it is appended with
    ///    `time_left_min = SetEndCondLimitTime` (FIX PL-160: the stored record is returned, so the client sees the
    ///    timer at once) and the minute timer restarts (`minute_due = now + 60 000`, for ANY new quest);
    /// 4. state 14 of an active quest removes the record from `active` and `checked` and joins `completed` only
    ///    when the quest property exists and is not `SetRepeat` (`pProp && !m_bRepeat`: KEEP, `/eq` of an id without
    ///    a property just drops the record); the completed list has no cap (FIX PL-160, the legacy 300);
    /// 5. any other state overwrites the record;
    /// 6. state 14 of an inactive quest is a new active record with state 14 (KEEP; the next load completes it).
    pub fn set(
        &mut self,
        q: u32,
        state: u8,
        data: &QuestTables,
        now: GameMs,
    ) -> Option<QuestRecord> {
        let q = u16::try_from(q).ok()?;
        let existing = self.doc.active.iter().position(|r| r.quest_id == q);
        if existing.is_none() && self.doc.completed.contains(&q) {
            return None;
        }
        let mut record = QuestRecord {
            quest_id: q,
            state,
            ..QuestRecord::default()
        };
        match existing {
            None => {
                if self.doc.active.len() >= MAX_QUEST {
                    tracing::warn!(
                        quest = q,
                        "the quest list is full: the quest is not started"
                    );
                    return None;
                }
                if let Some(p) = data.quests.get(u32::from(q))
                    && p.end_cond_limit_time != 0
                {
                    (record.time_left_min, record.timer_frozen) = limit_word(p.end_cond_limit_time);
                }
                self.doc.active.push(record);
                self.minute_due = GameMs(now.0.saturating_add(QUEST_MINUTE_MS));
            }
            Some(i) if state == QS_END => {
                self.doc.active.remove(i);
                self.doc.checked.retain(|c| *c != q);
                let repeat = data.quests.get(u32::from(q)).map(|p| p.repeat);
                if repeat == Some(false) && !self.doc.completed.contains(&q) {
                    self.doc.completed.push(q);
                }
            }
            Some(i) => {
                if let Some(slot) = self.doc.active.get_mut(i) {
                    *slot = record;
                }
            }
        }
        Some(record)
    }

    /// `RemoveQuest(q)`: deletes the quest from the active, completed and checked lists (order preserved).
    /// Returns `true` when something was removed.
    pub fn remove(&mut self, q: u32) -> bool {
        let Ok(q) = u16::try_from(q) else {
            return false;
        };
        let before = self.doc.active.len() + self.doc.completed.len() + self.doc.checked.len();
        self.doc.active.retain(|r| r.quest_id != q);
        self.doc.completed.retain(|c| *c != q);
        self.doc.checked.retain(|c| *c != q);
        before != self.doc.active.len() + self.doc.completed.len() + self.doc.checked.len()
    }

    /// `CMover::RemoveAllQuest`: clears the active, the completed AND the checked list (the client does the same on
    /// `RemoveQuestNtf` mode 1, so the plan's `remove_all_active` clears the completed quests too: NOTES).
    pub fn remove_all(&mut self) {
        self.doc.active.clear();
        self.doc.completed.clear();
        self.doc.checked.clear();
    }

    /// `CMover::RemoveCompleteQuest`: clears the completed list only.
    pub fn remove_all_completed(&mut self) {
        self.doc.completed.clear();
    }

    /// `OnCheckedQuest` (Plan/20 §9.10): `raw` is the client's `i32` (compared untruncated, stored as `u16`).
    /// Returns `false` when the request is ignored without a reply: checking with five quests already tracked
    /// (also for an entry that is listed), unchecking with an empty list. Quest existence and activeness are not
    /// validated (KEEP; display only).
    pub fn check(&mut self, raw: i32, on: bool) -> bool {
        let listed = |c: &u16| i32::from(*c) == raw;
        if on {
            if self.doc.checked.len() >= MAX_CHECKED_QUEST {
                return false;
            }
            if let Some(i) = self.doc.checked.iter().position(listed) {
                self.doc.checked.remove(i);
            }
            self.doc.checked.push(raw as u16);
        } else {
            if self.doc.checked.is_empty() {
                return false;
            }
            if let Some(i) = self.doc.checked.iter().position(listed) {
                self.doc.checked.remove(i);
            }
        }
        true
    }

    /// The freeze side effect of the end predicate (E1..E16 all true): `m_wTime |= 0x8000` of a non-zero word, so
    /// a timer that runs (or already shows the frozen bit) stops. Returns `true` when the record changed.
    pub fn freeze_timer(&mut self, q: u32) -> bool {
        let Ok(q) = u16::try_from(q) else {
            return false;
        };
        let Some(r) = self.doc.active.iter_mut().find(|r| r.quest_id == q) else {
            return false;
        };
        if r.time_left_min != 0 && !r.timer_frozen {
            r.timer_frozen = true;
            return true;
        }
        false
    }
}
