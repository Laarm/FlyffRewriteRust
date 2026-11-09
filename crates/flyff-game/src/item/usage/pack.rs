//! Pack items (Plan/19 §5.6, WP-ITEM-09; `CUser::DoUsePackItem`, `User.cpp:2787-2831`; `propPackItem.inc`).
//!
//! The pack is opened in step 4(c) of the use pipeline, **before** `DoUseItem` (cast time, cooldown and the IK2
//! rows do not apply to it): it needs one empty slot per entry (`TID_GAME_LACKSPACE` otherwise, the pack stays),
//! every entry is created with its upgrade level, count, the `charged` flag of its prop and the keep time of
//! the pack (`now + span * 60`), the `BINDS` flag is inherited from the pack item (`IsBinds`), each creation
//! says `TID_GAME_REAPITEM`; then the pack is used up.

use crate::item::container::{DeferSerials, StorageId};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::model::{Item, ItemFlags};
use crate::item::ops::{ItemOrigin, RemoveReason, UiParam, core as ops};
use crate::mover::Mover;
use flyff_common::defs::text::{TID_GAME_LACKSPACE, TID_GAME_REAPITEM};
use flyff_data::items_ext::giftbox::PackItem;

/// `now + span minutes` as a keep time (0 = permanent).
pub fn keep_until(now_unix: i64, span_minutes: i32) -> i64 {
    if span_minutes > 0 {
        now_unix.saturating_add(i64::from(span_minutes) * 60)
    } else {
        0
    }
}

/// What opening a pack did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackResult {
    /// Opened: the pack was used up.
    Opened,
    /// Not enough room (`TID_GAME_LACKSPACE` was queued); nothing changed.
    LackSpace,
}

/// `DoUsePackItem(pack_item, entries)` for the pack item at storage id `id`.
pub fn open(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    pack: &PackItem,
    fx: &mut Fx,
) -> PackResult {
    let Some(p) = m.player() else {
        return PackResult::LackSpace;
    };
    if p.items.inventory.empty_count() < pack.items.len() {
        fx.text(TID_GAME_LACKSPACE);
        return PackResult::LackSpace;
    }
    let (binds, pack_prop) = match p.items.inventory.get_at_id(id) {
        Some(it) => (
            env.data()
                .item(it.item_id)
                .is_some_and(|prop| it.is_binds(prop, env.cfg().only_armor_awakes)),
            env.data().item(it.item_id),
        ),
        None => return PackResult::LackSpace,
    };
    let keep = keep_until(env.now.0, pack.span);
    for &(item_id, ability_option, num) in &pack.items {
        let Some(prop) = env.data().item(item_id) else {
            continue;
        };
        let mut it = Item::new(
            item_id,
            u16::try_from(num.clamp(1, i32::from(u16::MAX))).unwrap_or(1),
        );
        it.upgrade_level = ability_option;
        it.charged = prop.charged != 0;
        it.keep_until = keep;
        if binds {
            it.flags.set(ItemFlags::BINDS, true);
        }
        if ops::create_item(m, env, &mut DeferSerials, it, ItemOrigin::Pack, fx).is_ok() {
            fx.defined(TID_GAME_REAPITEM, vec![format!("\"{}\"", prop.name)]);
        }
        // a failure would be a bug ("critical err" in the original): enough empty slots were checked
    }
    let _ = pack_prop;
    // `OnAfterUseItem`, then one pack less
    let count = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .map_or(0, |it| i32::from(it.count));
    ops::remove_item(m, env, id, 1, RemoveReason::Consume, fx);
    let _ = (count, UiParam::Num);
    PackResult::Opened
}
