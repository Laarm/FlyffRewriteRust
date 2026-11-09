//! Quest item drops (Plan/20 §9.9, Plan/19 §6.2 step 3, WP-NPC-15): the `QuestItem` lines of the quest files
//! (`Mover.cpp:7515-7650`). [`QuestDrops`] implements [`QuestDropHook`]; `Game::new` installs it in
//! `Services.drops.quest`.
//!
//! One kill runs the hook once, after the event items and before the passes of the monster's own drop table. A
//! monster without quest drops draws nothing (the real-data case).

use super::QuestRules;
use super::track::valid_area;
use crate::item::drop::{DropCx, MonsterFacts, QuestDropHook, roll_3e9};
use crate::item::loot::GroundItemMeta;
use crate::item::{Item, ItemError, ItemOrigin};
use flyff_common::defs::quest::{QUEST_VALENTINE, QUEST_WHITEDAY};
use flyff_common::defs::text::TID_EVE_REAPITEM;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, ObjId, WorldId};
use flyff_data::types::{MoverId, QuestId};
use flyff_db::ItemRecord;
use std::collections::BTreeSet;
use std::sync::Arc;

/// The Valentine and White Day quests scale their drop chance with the level gap (`Mover.cpp:7531-7536`).
pub fn level_factor(player_level: i32, monster_level: i32) -> f32 {
    let d = player_level.wrapping_sub(monster_level);
    if d <= 1 {
        1.0
    } else if d <= 2 {
        0.8
    } else if d <= 4 {
        0.6
    } else if d <= 7 {
        0.3
    } else {
        0.1
    }
}

/// The probability of one entry for a player: `prob`, scaled for the two seasonal quests
/// (`(DWORD)(prob * f)`, an `f32` product).
pub fn scaled_probability(quest: QuestId, prob: u32, player_level: i32, monster_level: i32) -> u32 {
    if quest == QUEST_VALENTINE || quest == QUEST_WHITEDAY {
        (prob as f32 * level_factor(player_level, monster_level)) as u32
    } else {
        prob
    }
}

/// `QuestDropHook` over the quest index.
#[derive(Debug)]
pub struct QuestDrops {
    rules: Arc<QuestRules>,
    /// `(monster, quest)` pairs whose `dwNumber == 0` was reported (the original wrote an `Error()` per hit).
    zero_logged: BTreeSet<(MoverId, QuestId)>,
}

impl QuestDrops {
    /// The hook over the rules (`Services.quest.rules()`).
    pub fn new(rules: Arc<QuestRules>) -> QuestDrops {
        QuestDrops {
            rules,
            zero_logged: BTreeSet::new(),
        }
    }
}

/// What a recipient needs for the item and the ground drop.
struct Recipient {
    obj: ObjId,
    world: WorldId,
    pos: Vec3,
    character: CharacterId,
}

impl QuestDropHook for QuestDrops {
    /// `quest::on_monster_drop`: see the module documentation. For each `QuestDrop` of the monster in index order:
    ///
    /// 1. the attacker must have the quest at EXACTLY the entry's state (else the next entry, no draw);
    /// 2. the hit roll `roll_3e9 <= p` (inclusive); on a hit the count `1 + random_below(max_num)` is drawn FIRST
    ///    (`max_num == 0` is an error and skips the entry without a draw), then item 0 stops the loop (KEEP the
    ///    `break` that followed the count draw);
    /// 3. the recipients: the party members within 64 m (3-D, same world and layer) of the monster that have the
    ///    quest at that state (the attacker only if it passes too), or the solo attacker without a range test;
    /// 4. each recipient gets the item (`TID_EVE_REAPITEM` AFTER the creation); a full bag puts it on the ground
    ///    at the recipient, held for his character, with a random angle (no text).
    fn on_monster_drop(&mut self, cx: &mut DropCx<'_>, monster: &MonsterFacts, attacker: ObjId) {
        let Some(drops) = self.rules.index.drops_by_mover.get(&monster.prop_id) else {
            return;
        };
        let level = match cx.store.mover(attacker) {
            Some(a) if a.is_player() => a.mv.level,
            _ => return,
        };
        for d in drops {
            let at_state = |id: ObjId| {
                cx.store
                    .mover(id)
                    .and_then(|m| m.player())
                    .and_then(|p| p.quests.find(d.quest))
                    .is_some_and(|r| r.state == d.state)
            };
            if !at_state(attacker) {
                continue;
            }
            let p = scaled_probability(d.quest, d.prob, level, monster.level);
            if roll_3e9(&mut *cx.rng) > p {
                continue;
            }
            if d.max_num == 0 {
                if self.zero_logged.insert((monster.prop_id, d.quest)) {
                    tracing::error!(
                        monster = monster.prop_id,
                        quest = d.quest,
                        "quest item drop with a count of 0: the entry is skipped"
                    );
                }
                continue;
            }
            let n = (1 + cx.rng.random_below(d.max_num)).min(u32::from(u16::MAX)) as u16;
            if d.item == 0 {
                tracing::warn!(
                    monster = monster.prop_id,
                    quest = d.quest,
                    "quest item drop of item 0: the quest drops of this monster stop here"
                );
                break;
            }
            let party: Option<Vec<ObjId>> = cx
                .store
                .mover(attacker)
                .and_then(|m| m.player())
                .and_then(|p| p.social.party_link.as_ref())
                .map(|l| l.online_objs().collect());
            let recipients: Vec<ObjId> = match party {
                Some(members) => members
                    .into_iter()
                    .filter(|m| in_range(cx, *m, monster) && at_state(*m))
                    .collect(),
                None => vec![attacker],
            };
            for r in recipients {
                give(cx, monster, r, d.item, n);
            }
        }
    }
}

/// `IsValidArea(member, 64)` against the monster.
fn in_range(cx: &DropCx<'_>, member: ObjId, monster: &MonsterFacts) -> bool {
    cx.store
        .mover(member)
        .is_some_and(|m| valid_area(m, (monster.world, monster.layer, monster.pos)))
}

/// One recipient: the item into the bag, or on the ground when the bag is full.
fn give(cx: &mut DropCx<'_>, monster: &MonsterFacts, member: ObjId, item_id: u32, n: u16) {
    let Some(prop) = cx.data.item(item_id) else {
        tracing::warn!(
            item = item_id,
            "a quest drop names an unknown item: skipped"
        );
        return;
    };
    let (name, endurance) = (prop.name.clone(), prop.endurance as i32);
    let Some(r) = recipient(cx, member) else {
        return;
    };
    match cx.create_item(r.obj, Item::new(item_id, n), ItemOrigin::Quest) {
        Ok(()) => {
            cx.fx(r.obj)
                .defined(TID_EVE_REAPITEM, vec![format!("\"{name}\"")]);
        }
        Err(ItemError::LackSpace) => {
            let angle = cx.rng.random_below(360) as f32;
            let mut record = ItemRecord::new(0, item_id, n, 0);
            record.endurance = endurance;
            let meta = GroundItemMeta {
                owner: None,
                holder: Some(r.character),
                drop_time: cx.now,
                from_monster: false,
                event_monster: None,
                gold: None,
                respawned: false,
            };
            cx.spawn_ground(r.world, monster.layer, r.pos, angle, record, meta);
        }
        Err(e) => tracing::warn!(error = %e, "a quest drop could not be given"),
    }
}

fn recipient(cx: &DropCx<'_>, obj: ObjId) -> Option<Recipient> {
    let m = cx.store.mover(obj)?;
    let p = m.player()?;
    Some(Recipient {
        obj,
        world: m.core.world,
        pos: m.core.pos,
        character: CharacterId(p.ident.character_id.0),
    })
}
