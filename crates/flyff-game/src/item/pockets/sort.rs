//! `SortInventory` (Plan/19 §3.4; ref/13 §6.6; ref/07c §30.1; `DPSrvr.cpp:12828-12868`, `features.inventory_sorting`).
//!
//! The usable items of the bag slots `0..index_num` are ordered by `item_kind3` ascending and moved, one by one,
//! to the slots `0, 1, 2, ...`: for each item `MoveItemNtf{0, current slot, j}` is sent and the slots are swapped
//! (two compatible stacks merge, as in `MoveItemReq`). The client replays the same swaps, so the answer is one
//! message per step and no refresh. **FIX (PL-153):** the original `std::sort` is unstable, so the order inside
//! one kind was arbitrary and not reproducible; the port is stable on the current slot, which makes two runs on
//! identical inventories emit identical message sequences. A reserved (trade, vendor) item is not a source but
//! the swaps may still displace it (KEEP-QUIRK: trade and vendor refer to the storage id, which does not move).

use super::with_player;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::mover::Mover;
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;
use flyff_rules::item::buffitem::sort_order;

/// The sort over one mover; returns the number of `MoveItemNtf` queued. Does nothing with the feature off.
pub fn sort_inventory(m: &mut Mover, env: &ItemEnv<'_>, fx: &mut Fx) -> usize {
    if !env.cfg().inventory_sorting {
        return 0;
    }
    let data = env.data();
    let Some(p) = m.player_mut() else { return 0 };
    let inv = &mut p.items.inventory;
    // the usable items in slot order, with their kind
    let mut kinds: Vec<(u16, u32)> = Vec::new();
    let mut ids: Vec<StorageId> = Vec::new();
    for slot in 0..inv.index_num() {
        let Some(id) = inv.obj_at_slot(slot) else {
            continue;
        };
        let Some(it) = inv.get_at_id(id).filter(|it| it.is_usable()) else {
            continue;
        };
        kinds.push((
            u16::try_from(ids.len()).unwrap_or(u16::MAX),
            data.item(it.item_id).map_or(NULL_ID, |pr| pr.item_kind3),
        ));
        ids.push(id);
    }
    // the stable order keeps the slot order inside one kind (`flyff_rules::item::buffitem::sort_order`)
    let order = sort_order(&kinds);
    let mut j: u16 = 0;
    let mut sent = 0;
    for pos in order {
        let Some(&id) = ids.get(usize::from(pos)) else {
            continue;
        };
        // an item that merged into an earlier stack is gone (`GetAtId` returns NULL in the original)
        let Some(slot) = inv.get_at_id(id).and_then(|_| inv.slot_of(id)) else {
            continue;
        };
        fx.out.to_self(pb::MoveItemNtf {
            item_type: 0,
            src_slot: u32::from(slot),
            dst_slot: u32::from(j),
        });
        sent += 1;
        inv.swap(slot, j, data);
        j += 1;
    }
    sent
}

/// `SortInventory` (C2S, no payload).
pub fn on_sort_inventory(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _: pb::SortInventory,
) {
    with_player(svc, ctx, who, |m, env, _, fx| sort_inventory(m, env, fx));
}
