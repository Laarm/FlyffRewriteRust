//! `QuestRules` and `QuestService` (Plan/20 §9.1, WP-NPC-12/13): the [`QuestPredicates`] over the loaded quest
//! tables, installed by `Game::new` in place of the M4 [`NoQuestData`](super::NoQuestData).

use super::QuestBook;
use super::flow::{PredicateMode, QuestPredicates};
use super::index::QuestIndex;
use super::predicates::{self, BeginCheck};
use super::subject::QuestSubject;
use flyff_data::GameData;
use std::sync::Arc;

/// The quest rules over the data: the property lookups, the derived [`QuestIndex`], and the predicates with the
/// `fidelity.quest_condition_fixes` switch (read once at startup, class S).
#[derive(Debug)]
pub struct QuestRules {
    /// The loaded data (`data.quests` holds the tables).
    pub data: Arc<GameData>,
    /// The derived lookups.
    pub index: QuestIndex,
    /// `fidelity.quest_condition_fixes` (PL-159).
    pub fixes: bool,
}

impl QuestRules {
    /// Rules over `data`; the index is built once.
    pub fn new(data: Arc<GameData>, fixes: bool) -> QuestRules {
        let index = QuestIndex::build(&data.quests);
        QuestRules { data, index, fixes }
    }
}

impl QuestPredicates for QuestRules {
    fn offered_by(&self, npc_key: &str) -> Vec<u32> {
        self.index
            .src
            .get(&npc_key.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    fn exists(&self, q: u32) -> bool {
        self.data.quests.quests.contains(q)
    }

    fn title(&self, q: u32) -> String {
        self.data
            .quests
            .quests
            .get(q)
            .map(|p| p.title.clone())
            .unwrap_or_default()
    }

    fn level_range(&self, q: u32) -> (i32, i32) {
        self.data.quests.quests.get(q).map_or((0, 0), |p| {
            (
                i32::from(p.begin_cond_level_min),
                i32::from(p.begin_cond_level_max),
            )
        })
    }

    fn end_character_is(&self, q: u32, npc_key: &str) -> bool {
        self.data.quests.quests.get(q).is_some_and(|p| {
            !p.end_character.is_empty() && p.end_character.eq_ignore_ascii_case(npc_key)
        })
    }

    fn can_begin(&self, s: &dyn QuestSubject, q: u32, mode: PredicateMode) -> BeginCheck {
        match self.data.quests.quests.get(q) {
            Some(p) => predicates::can_begin(p, s, mode, self.fixes),
            None => BeginCheck::default(),
        }
    }

    fn is_next_level(&self, s: &dyn QuestSubject, q: u32) -> bool {
        self.data
            .quests
            .quests
            .get(q)
            .is_some_and(|p| predicates::is_next_level(p, s, self.fixes))
    }

    fn can_end(&self, s: &dyn QuestSubject, q: u32) -> bool {
        self.data
            .quests
            .quests
            .get(q)
            .is_some_and(|p| predicates::can_end(p, s, self.fixes))
    }

    /// `DPSrvr.cpp:968-985`: the first ACTIVE record in book order whose `SetEndCondDialog` names this NPC and this
    /// key, both compared byte for byte.
    fn cond_dialog_quest(&self, book: &QuestBook, npc_key: &str, key: &str) -> Option<u32> {
        book.doc.active.iter().find_map(|r| {
            let q = u32::from(r.quest_id);
            let d = self.data.quests.quests.get(q)?.end_cond_dialog.as_ref()?;
            (d.npc == npc_key && d.key == key).then_some(q)
        })
    }

    fn dialog_line(&self, q: u32, slot: usize) -> Option<String> {
        self.data
            .quests
            .quests
            .get(q)?
            .dialog_line(slot)
            .map(str::to_owned)
    }
}

/// `Services.quest`: the quest rules of the running game.
#[derive(Clone, Debug)]
pub struct QuestService {
    rules: Arc<QuestRules>,
}

impl Default for QuestService {
    /// Rules over an empty `GameData` (the state before `Game::new` installs the real ones).
    fn default() -> Self {
        QuestService::new(Arc::new(GameData::default()), true)
    }
}

impl QuestService {
    /// The service over the loaded data and the switch.
    pub fn new(data: Arc<GameData>, fixes: bool) -> QuestService {
        QuestService {
            rules: Arc::new(QuestRules::new(data, fixes)),
        }
    }

    /// The rules as the predicate seam of the dialog flows.
    pub fn predicates(&self) -> Arc<dyn QuestPredicates> {
        Arc::clone(&self.rules) as Arc<dyn QuestPredicates>
    }

    /// The rules (property lookups, the index).
    pub fn rules(&self) -> Arc<QuestRules> {
        Arc::clone(&self.rules)
    }

    /// The derived lookups.
    pub fn index(&self) -> &QuestIndex {
        &self.rules.index
    }

    /// The arena rectangles `[x1, y1, x2, y2]` of the guild quests (`Region =`), for Blinkpool's restriction
    /// (`SkillService.guild_quest_regions`).
    pub fn guild_quest_regions(&self) -> Vec<[i32; 4]> {
        self.rules
            .data
            .quests
            .guild_quests
            .values()
            .filter_map(|g| g.region)
            .collect()
    }
}
