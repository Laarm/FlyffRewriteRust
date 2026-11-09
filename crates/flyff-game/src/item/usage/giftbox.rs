//! Gift boxes (Plan/19 §5.6, WP-ITEM-09; `CUser::DoUseGiftbox`, `User.cpp:2833-2892`; `propGiftbox.inc`).
//!
//! The box is a `IK2_SYSTEM` item that no `use_system` row claims (result 1, "not consumed"): the pipeline then
//! tries the gift box table. The **roll comes first** (`xRandom(1000000)`, the first entry whose cumulative
//! probability exceeds it - the lower bound is always 0, KEEP), then the space check (one empty slot; the box
//! is **not** consumed when it fails), then the box is used up, then the prize is created: `count = num`,
//! `upgrade_level = ability_option`, the keep time from `span`; a `flag` other than 4 sets the item flags and the
//! prop's `charged` (4 = "ignore the property"). A prize that cannot be created (a multi-slot overflow) is lost
//! (KEEP-QUIRK of ref/13 §10.6); a success says `TID_GAME_REAPITEM`.

use super::pack::keep_until;
use crate::item::container::{DeferSerials, StorageId};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::model::{Item, ItemFlags};
use crate::item::ops::{ItemOrigin, RemoveReason, core as ops};
use crate::mover::Mover;
use flyff_common::defs::text::{TID_GAME_LACKSPACE, TID_GAME_REAPITEM};
use flyff_common::rng::GameRng;
use flyff_data::items_ext::giftbox::{GIFTBOX_SPACE, GiftBox};

/// What opening a box did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxResult {
    /// Opened: the box was used up and the prize created (or lost).
    Opened,
    /// No room (`TID_GAME_LACKSPACE`); the box stays.
    LackSpace,
    /// The box has no outcome (an empty table row): not a gift box.
    NotABox,
}

/// `DoUseGiftbox` for the box at storage id `id`.
pub fn open(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    gift: &GiftBox,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> BoxResult {
    let roll = rng.random_below(GIFTBOX_SPACE);
    let Some(entry) = gift.pick(roll).copied() else {
        return BoxResult::NotABox;
    };
    let Some(p) = m.player() else {
        return BoxResult::NotABox;
    };
    if p.items.inventory.empty_count() < 1 {
        fx.text(TID_GAME_LACKSPACE);
        return BoxResult::LackSpace;
    }
    ops::remove_item(m, env, id, 1, RemoveReason::Consume, fx);
    let Some(prop) = env.data().item(entry.item) else {
        return BoxResult::Opened;
    };
    let mut it = Item::new(
        entry.item,
        u16::try_from(entry.num.clamp(1, i32::from(u16::MAX))).unwrap_or(1),
    );
    if entry.flag != 4 {
        it.flags = ItemFlags(entry.flag);
        it.charged = prop.charged != 0;
    }
    it.keep_until = keep_until(env.now.0, entry.span);
    it.upgrade_level = entry.ability_option;
    if ops::create_item(m, env, &mut DeferSerials, it, ItemOrigin::GiftBox, fx).is_ok() {
        fx.defined(TID_GAME_REAPITEM, vec![format!("\"{}\"", prop.name)]);
    }
    BoxResult::Opened
}
