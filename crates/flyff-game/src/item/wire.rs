//! Plain conversions of persisted item records to protocol messages (Plan/11 §7.2, ref/08 §7.7). **Stand-in**
//! for doc 19's `item::wire` (WP-ITEM-*): the field mapping is the one `ItemRecord`'s `legacy_*` accessors
//! were written for, so it is complete for what persistence stores.

use flyff_db::{ContainerRecord, ItemRecord, PocketsDoc};
use flyff_proto::pb;

/// `CItemElem::Serialize` (ref/08 §7.7.1).
pub fn item_elem(it: &ItemRecord) -> pb::ItemElem {
    let piercing = if it.piercing_cards.is_empty()
        && it.ultimate_gems.is_empty()
        && it.vis_keep_until.is_empty()
    {
        None
    } else {
        Some(pb::Piercing {
            card_item_ids: it.piercing_cards.clone(),
            ultimate_gem_ids: it.ultimate_gems.clone(),
            vis_expire_at: it.vis_keep_until.clone(),
        })
    };
    pb::ItemElem {
        obj_id: u32::from(it.obj_id),
        item_id: it.item_id,
        serial: it.serial,
        item_text: it.text.clone(),
        num: i32::from(it.count),
        repair_number: u32::from(it.repair_number),
        hit_point: it.legacy_hit_point(),
        repair: it.repair_count,
        flag: u32::from(it.flags),
        ability_option: it.upgrade_level,
        id_guild: it.guild_id,
        item_resist: u32::from(it.legacy_element()),
        resist_ability_option: it.element_level,
        resist_sm_item_id: i32::try_from(it.element_stone_id).unwrap_or(i32::MAX),
        piercing,
        charged: it.charged,
        random_opt_item_id: it.legacy_random_option() as i64,
        keep_time_at: it.keep_until,
        pet: it.pet.as_ref().map(|p| pb::PetData {
            kind: u32::from(p.kind),
            level: u32::from(p.level),
            exp: p.exp,
            energy: u32::from(p.energy),
            life: u32::from(p.life),
            avail_level: p.avail_levels.iter().map(|&v| u32::from(v)).collect(),
            name: p.name.clone(),
        }),
        transform_vis_pet: it.vis_transformed,
    }
}

/// `CItemContainer<CItemElem>::Serialize` (ref/08 §7.7.3): the slot index, the non-empty items and the
/// inverse storage-to-slot index (`-1` = none).
pub fn container(c: &ContainerRecord) -> pb::ItemContainer {
    let max = usize::from(c.capacity);
    let mut obj_index = vec![-1_i32; max];
    for (slot, &storage) in c.index.iter().enumerate() {
        if let Ok(s) = usize::try_from(storage)
            && let Some(e) = obj_index.get_mut(s)
        {
            *e = i32::try_from(slot).unwrap_or(-1);
        }
    }
    pb::ItemContainer {
        max: u32::from(c.capacity),
        index: c.index.clone(),
        items: c.items.iter().map(item_elem).collect(),
        obj_index,
    }
}

/// `CPocketController::Serialize` of the runtime pockets: exactly three slots.
pub fn pocket_set_runtime(p: &super::inventory::PlayerItems) -> pb::PocketSet {
    pb::PocketSet {
        slots: p
            .pockets
            .iter()
            .map(|pk| pb::PocketSlot {
                exists: pk.available,
                items: Some(pk.items.to_pb()),
                expired: pk.expired,
                expire_at: pk.expires_at,
            })
            .collect(),
    }
}

/// `CPocketController::Serialize` (ref/08 §7.7.4): exactly three slots.
pub fn pocket_set(p: &PocketsDoc) -> pb::PocketSet {
    pb::PocketSet {
        slots: p
            .pockets
            .iter()
            .map(|pk| pb::PocketSlot {
                exists: pk.available,
                items: Some(container(&pk.container)),
                expired: pk.expired,
                expire_at: pk.expires_at,
            })
            .collect(),
    }
}
