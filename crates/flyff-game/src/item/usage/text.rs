//! The `IK2_TEXT` row of the item-use dispatcher (Plan/19 §5.3 TEXT row, `MoverSkill.cpp:1467-1474`; WP-ITEM-25):
//! a quest book, scroll or letter. A text item whose property names a quest (`ItemProp.quest_id`) starts that quest
//! once, flagged on the stack itself (`m_bItemResist`, `ItemElem.text_quest_started`); the quest side is
//! [`crate::quest::track::on_text_item_used`] (the `TID_EVE_STARTQUEST` text, the book entry and its `SetQuest`, the
//! `quest.start` audit line, and `UpdateItem(.., UI_IR, 1)` even when the book refused the quest: KEEP).
//!
//! The row ALWAYS answers [`RowResult::Consume`]: the original falls through to `UseItem()` (`MoverSkill.cpp:1729`),
//! which honours `bPermanence`, and so does the pipeline here (`own_prop.permanence == 1`): the three real text items
//! (`II_SYS_SYS_QUE_BOOK` 10383 → quest 500, `..._SCROLL` 10384 → 501, `..._LETTER` 10385 → 0) are permanent and stay
//! in the inventory; a text item without `bPermanence` is consumed after the use (as the original did, also when the
//! quest had been started before). Everything is mover-level (no `Services`), so the wire order is the original's:
//! the text, `SetQuest`, `UpdateItem IR`, then the pipeline's `UpdateItem`.
//!
//! With the real data (no quest property, PD-01) the book and the scroll create the record of an unknown quest once
//! (`__SetQuest` did so without a property: the record, no text; invisible while the quest UI is hidden, PL-148) and
//! the letter does nothing: the one deliberate change on the real data, formerly `TID_SBEVE_NOTUSEITEM`.

use super::{RowCx, RowResult};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::mover::Mover;
use flyff_data::props::item::ItemProp;

/// The `IK2_TEXT` row: starts the item's quest (when it has one and the stack is not flagged yet) and consumes per
/// `bPermanence` like every other row.
pub fn use_text(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    if prop.quest_id != 0 {
        // `false` = the stack was flagged already or the item names no quest: nothing to start
        let _started = crate::quest::track::on_text_item_used(m, env, ux.id, ux.now, fx);
    }
    RowResult::Consume
}
