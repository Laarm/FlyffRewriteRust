//! Guild cloaks leave with their owner (Plan/19 §2.5; src:_Common/Mover.cpp:2413-2444 `RemoveItemIK3(IK3_CLOAK)`).
//!
//! [`remove_guild_cloaks`] is called by the guild service when an online member leaves, is kicked or loses the guild
//! (21 §5.4, §5.5). It removes every USABLE `IK3_CLOAK` that carries a guild id from the inventory, worn or not. The
//! original unequipped a worn cloak first (`DoEquip`, which needs a free bag slot): an equipped cloak with a full bag
//! stays, silently (KEEP-QUIRK). A removed cloak is destroyed through [`remove_item`], so the owner sees the normal
//! `DeleteItem` / `DoEquipNtf` messages and the audit line `item.destroy` with the reason `guild_cloak`.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::ops::{RemoveReason, remove_item};
use flyff_common::defs::itemkind::IK3_CLOAK;

/// Removes the guild cloaks of `p`; returns how many stacks left the inventory (see the module documentation).
pub fn remove_guild_cloaks(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) -> usize {
    let cloaks: Vec<(StorageId, u16, bool)> = {
        let Some(pl) = ctx.player(p).and_then(|m| m.player()) else {
            return 0;
        };
        let inv = &pl.items.inventory;
        inv.iter()
            .filter(|(_, it)| it.is_usable() && it.guild_id != 0)
            .filter(|(_, it)| {
                ctx.data
                    .item(it.item_id)
                    .is_some_and(|prop| prop.item_kind3 == IK3_CLOAK)
            })
            .map(|(id, it)| (id, it.count, inv.is_equip(id)))
            .collect()
    };
    let mut removed = 0;
    for (id, count, worn) in cloaks {
        // a worn cloak needs a free bag slot to come off (the original's `DoEquip`): without one it stays
        let room = ctx
            .player(p)
            .and_then(|m| m.player())
            .is_some_and(|pl| pl.items.inventory.empty_count() >= 1);
        if worn && !room {
            continue;
        }
        remove_item(svc, ctx, p, id, count, RemoveReason::Other("guild_cloak"));
        removed += 1;
    }
    removed
}
