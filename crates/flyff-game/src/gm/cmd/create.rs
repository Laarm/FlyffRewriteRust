//! The item commands of the testing set (Plan/23 §4.6, §4.7, §4.9; WP-GM-13): `/createitem` (row 84),
//! `/InvenClear` (144), `/InvenRemove` (219) and `/item` (N06).
//!
//! Items are created and destroyed through the lifecycle primitives of doc 19 (`item::ops`), which keep the
//! container, the DST sources, the messages and the `item.create` / `item.destroy` audit lines in step.

use crate::ctx::PlayerRef;
use crate::gm::lookup;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::item::model::Item;
use crate::item::ops::{self, ItemOrigin, RemoveReason};
use flyff_common::defs::item::II_PET_EGG;
use flyff_common::defs::itemkind::{IK3_EGG, IK3_VIRTUAL};
use flyff_common::defs::text::TID_GAME_PET_NOWUSE;

/// Longest list `/item` prints before "… and N more".
pub const ITEM_SEARCH_LINES: usize = 20;

fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// Row 84 `/createitem (ci) <itemId or name> [num] [charged 0/1]` (`TextCmd_CreateItem`): a NUMBER is the item id,
/// anything else a name (exact, case-insensitive or the `II_*` define, PL-099). `IK3_VIRTUAL` is refused,
/// `IK3_EGG` only for `II_PET_EGG`; `num` 0 becomes 1 and is capped at the stack size of the prop (a negative
/// number counts as 1); `hit_point = -1` (the prop's endurance), `charged` is a flag. The item goes into the
/// inventory; a full inventory fails silently. Audit `item.create` with `source = "gm"`.
pub fn createitem(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let token = args.token().to_owned();
    let is_number = args.token_is_number();
    let Some(id) = lookup::item(cx.ctx.data, &token, is_number) else {
        return Ok(CmdDone::default());
    };
    let Some(prop) = cx.ctx.data.item(id) else {
        return Ok(CmdDone::default());
    };
    if prop.item_kind3 == IK3_VIRTUAL || (prop.item_kind3 == IK3_EGG && id != II_PET_EGG) {
        return Ok(CmdDone::default());
    }
    let pack_max = prop.pack_max.max(1);
    let num = args.number();
    let num = u32::try_from(num).unwrap_or(1).max(1).min(pack_max);
    let charged = args.number() != 0;
    let mut item = Item::new(id, u16::try_from(num).unwrap_or(u16::MAX));
    item.charged = charged;
    // a full inventory fails silently, like the legacy CreateItem
    let _ = ops::create_item(cx.svc, cx.ctx, who, item, ItemOrigin::Gm);
    Ok(CmdDone::on(token))
}

/// Row 144 `/InvenClear (icr)`: every item of the inventory goes, equipped ones included (audit `item.destroy`
/// with `reason = "gm"`). Refused with `TID_GAME_PET_NOWUSE` while a system pet is summoned.
pub fn inven_clear(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let Some(m) = cx.ctx.player(who) else {
        return Ok(CmdDone::default());
    };
    let Some(p) = m.player() else {
        return Ok(CmdDone::default());
    };
    if p.items.pet_obj_id.is_some() {
        cx.out.defined(TID_GAME_PET_NOWUSE, Vec::new());
        return Ok(CmdDone::default());
    }
    let all: Vec<(u16, u16)> = p
        .items
        .inventory
        .iter()
        .map(|(id, it)| (id, it.count))
        .collect();
    for (id, count) in all {
        ops::remove_item(cx.svc, cx.ctx, who, id, count, RemoveReason::Other("gm"));
    }
    Ok(CmdDone::default())
}

/// Row 219 `/InvenRemove (irm)`: the count of every usable, not equipped inventory item that is not a pet or an
/// eat pet goes to 0 (audit `item.destroy`, `reason = "gm"`).
pub fn inven_remove(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let Some(p) = cx.ctx.player(who).and_then(|m| m.player()) else {
        return Ok(CmdDone::default());
    };
    let data = cx.ctx.data;
    let all: Vec<(u16, u16)> = p
        .items
        .inventory
        .iter()
        .filter(|(id, it)| {
            it.is_usable()
                && !p.items.inventory.is_equip(*id)
                && data
                    .item(it.item_id)
                    .is_none_or(|prop| !Item::is_pet(prop) && !Item::is_eat_pet(prop))
        })
        .map(|(id, it)| (id, it.count))
        .collect();
    for (id, count) in all {
        ops::remove_item(cx.svc, cx.ctx, who, id, count, RemoveReason::Other("gm"));
    }
    Ok(CmdDone::default())
}

/// N06 `/item <search...>`: a case-insensitive substring search over the item display names and the `II_*`
/// define names; up to 20 lines `"<id>  <name>"`, then `"... and N more"`. The lines are `ChatText` (BFIX1 #006):
/// a list belongs in the chat box, the centre notices keep only 6 lines.
pub fn item_search(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let needle = args.last_full();
    if needle.trim().is_empty() {
        return Err(CmdError::Usage);
    }
    let hits = lookup::search_items(cx.ctx.data, needle.trim());
    for (id, name) in hits.iter().take(ITEM_SEARCH_LINES) {
        cx.out.chat_text(format!("{id}  {name}"));
    }
    if hits.len() > ITEM_SEARCH_LINES {
        cx.out
            .chat_text(format!("... and {} more", hits.len() - ITEM_SEARCH_LINES));
    } else if hits.is_empty() {
        cx.out.chat_text("no item found");
    }
    Ok(CmdDone::default())
}
