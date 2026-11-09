//! `QuestIndex` (Plan/20 §9.1): the lookups the quest engine derives once from the [`QuestTables`] at game start:
//! the NPC offer lists, the quest drops by monster, the dialog conditions, the timed quests, the patrol zones and
//! the kill targets. Built from the FINAL properties: the `QuestItem` lines of a definition that a later file
//! replaced do not drop (NOTES decision; the original had appended them to the mover for good, invisible with the
//! shipped data).

use flyff_common::WorldId;
use flyff_common::det::{DetMap, DetSet};
use flyff_data::quest::QuestTables;
use flyff_data::types::{ItemId, MoverId, QuestId};

/// One quest drop (`QUESTITEM`): the killer gets `item` while the quest is at exactly `state`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct QuestDrop {
    /// The quest.
    pub quest: QuestId,
    /// The state the quest must be in.
    pub state: u8,
    /// The item.
    pub item: ItemId,
    /// `dwProbability` out of 3 000 000 000.
    pub prob: u32,
    /// `dwNumber`: the largest stack.
    pub max_num: u32,
}

/// A normalised patrol rectangle (`CRect` after `NormalizeRect`; `left <= x < right`, `top <= z < bottom`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PatrolRect {
    /// Left.
    pub left: i32,
    /// Top.
    pub top: i32,
    /// Right.
    pub right: i32,
    /// Bottom.
    pub bottom: i32,
}

/// The derived lookups (see the module documentation).
#[derive(Clone, Debug, Default)]
pub struct QuestIndex {
    /// `quest_src`: lower-cased NPC key to the quests it gives, in parse order (offer-list input).
    pub src: DetMap<String, Vec<QuestId>>,
    /// The drops of each monster, ordered (quest id, state, entry order).
    pub drops_by_mover: DetMap<MoverId, Vec<QuestDrop>>,
    /// `(quest, npc key, key text)` of every `SetEndCondDialog`, case-sensitive strings.
    pub dialog_cond: Vec<(QuestId, String, String)>,
    /// The quests with `SetEndCondLimitTime > 0`.
    pub timed: DetSet<QuestId>,
    /// `SetEndCondPatrolZone` (only a patrol world other than 0: the engine treats world 0 as no patrol).
    pub patrol: DetMap<QuestId, (WorldId, PatrolRect)>,
    /// The kill conditions: monster to `(quest, slot)`.
    pub kill_targets: DetMap<MoverId, Vec<(QuestId, u8)>>,
}

impl QuestIndex {
    /// Builds the index from the tables.
    pub fn build(t: &QuestTables) -> QuestIndex {
        let mut ix = QuestIndex {
            src: t
                .quest_src
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            ..QuestIndex::default()
        };
        for (id, p) in t.quests.iter() {
            // (state, entry order) within the quest
            let mut items = p.quest_items.clone();
            items.sort_by_key(|i| i.state);
            for i in items {
                ix.drops_by_mover
                    .entry(i.mover)
                    .or_default()
                    .push(QuestDrop {
                        quest: id,
                        state: i.state,
                        item: i.item,
                        prob: i.prob,
                        max_num: i.max_num,
                    });
            }
            if let Some(d) = &p.end_cond_dialog {
                ix.dialog_cond.push((id, d.npc.clone(), d.key.clone()));
            }
            if p.is_timed() {
                ix.timed.insert(id);
            }
            if let Some(z) = p.end_cond_patrol.filter(|z| z.world != 0) {
                let [left, top, right, bottom] = z.rect;
                ix.patrol.insert(
                    id,
                    (
                        WorldId(z.world),
                        PatrolRect {
                            left,
                            top,
                            right,
                            bottom,
                        },
                    ),
                );
            }
            for (slot, k) in p.end_cond_kill_npc.iter().enumerate() {
                if k.mover != 0 {
                    let slot = u8::try_from(slot).unwrap_or(u8::MAX);
                    ix.kill_targets.entry(k.mover).or_default().push((id, slot));
                }
            }
        }
        ix
    }
}
