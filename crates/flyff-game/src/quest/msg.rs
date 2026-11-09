//! The quest messages (Plan/20 §9.12): the builders of `SetQuest`, `RemoveQuestNtf` and `QuestChecked`, and the
//! one `Quest` record of the wire (`SetQuest`, the join message `PlayerSelf.quests`).

use super::QuestBook;
use flyff_db::model::QuestRecord;
use flyff_proto::pb;

/// A quest record as the wire carries it (`pb::Quest`): `time = (time_left_min & 0x7FFF) | frozen << 15`, the two
/// kill counters, the patrol and dialog flags. One builder for the bit-15 rule (the join message uses it too).
pub fn quest_msg(r: &QuestRecord) -> pb::Quest {
    pb::Quest {
        state: u32::from(r.state),
        time: u32::from(r.time_left_min & 0x7FFF) | (u32::from(r.timer_frozen) << 15),
        quest_id: u32::from(r.quest_id),
        kill_npc_num: r.kills.iter().map(|k| u32::from(*k)).collect(),
        patrol: r.patrol_done,
        dialog: r.dialog_done,
    }
}

/// `SetQuest{quest}`.
pub fn set_quest(r: &QuestRecord) -> pb::SetQuest {
    pb::SetQuest {
        quest: Some(quest_msg(r)),
    }
}

/// `RemoveQuestNtf{mode, quest_id}`: mode -1 cancels one quest (the client prints `TID_GAME_REMOVEQUEST`), 0
/// removes one quietly, 1 removes all (id 0), 2 removes the completed ones (id 0).
pub fn remove_quest_ntf(mode: i32, q: u32) -> pb::RemoveQuestNtf {
    pb::RemoveQuestNtf { mode, quest_id: q }
}

/// `QuestChecked{quest_ids}`: the tracker list.
pub fn quest_checked(book: &QuestBook) -> pb::QuestChecked {
    pb::QuestChecked {
        quest_ids: book.doc.checked.iter().map(|q| u32::from(*q)).collect(),
    }
}
