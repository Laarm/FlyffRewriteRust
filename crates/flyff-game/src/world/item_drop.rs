//! Ground items (`ItemDrop`, Plan/15 §16, F-090; WP-WORLD-16): the entity, [`Worlds::spawn_ground_item`] (the spawn
//! API of docs 17/19/20/22/23) and the 180 s expiry rule of `_process` (`src:_Common/Item.cpp:550-555`).
//!
//! The loot data is [`GroundItemMeta`] (doc 19 §6.5, module `item::loot`): doc 19
//! owns its semantics (`is_loot`, the pickup, the party share), the producers fill it completely at creation:
//! the player drop (`item::handlers`), the monster drop (`item::drop`), the respawn of map items
//! (`world::respawn`, `respawned = gm_index > 0`), ctrl drops (WP-WORLD-18). A ground gold seed carries its amount
//! in `meta.gold` (19 §2.1): it travels as `ItemElem.hit_point` of the `SpawnItem` with count 1 (`world::spawn`).
//!
//! The expiry runs in `_process` of every layer whether or not anybody sees the item; `SpawnItem` reaches the
//! viewers at `_add`/`_modifylink` and `DelObj` follows at the `_delete` of the tick that flagged the item.

use super::Worlds;
use super::lifecycle::AddError;
use super::object::{ObjCore, ObjKind};
use crate::item::GroundItemMeta;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, LayerId, ObjId, WorldId};
use flyff_db::ItemRecord;

pub use crate::item::GroundItemMeta as Meta;

/// A ground item disappears when `now - drop_time` is strictly greater than this (180 s,
/// `src:_Common/Item.cpp:550-555`).
pub const ITEM_EXPIRY_MS: u64 = 180_000;

/// An item lying on the ground (15 §16.1).
#[derive(Clone, Debug)]
pub struct ItemDrop {
    /// The object header; `core.index` is the item prop id.
    pub core: ObjCore,
    /// The dropped item (doc 19 owns the runtime item type; this is the persisted record).
    pub item: Box<ItemRecord>,
    /// Loot data (doc 19 §6.5).
    pub meta: GroundItemMeta,
}

impl ItemDrop {
    /// A detached drop of `item` at `pos` facing `angle`; the id, world and layer are stamped by the spawn
    /// API.
    pub fn new(item: ItemRecord, pos: Vec3, angle: f32, meta: GroundItemMeta) -> ItemDrop {
        let mut core = ObjCore::new(
            ObjId::NULL,
            ObjKind::Item,
            item.item_id,
            WorldId(0),
            LayerId(0),
        );
        core.pos = pos;
        core.angle = angle;
        ItemDrop {
            core,
            item: Box::new(item),
            meta,
        }
    }

    /// A ground gold seed: the item `seed_item` (`II_GOLD_SEED1..4`) of `amount` penya (19 §2.1). The amount lives
    /// in `meta.gold` (the record keeps it as `endurance` as well, the original's `m_nHitPoint`); the count is 1.
    pub fn gold_seed(
        seed_item: u32,
        amount: i32,
        pos: Vec3,
        angle: f32,
        mut meta: GroundItemMeta,
    ) -> ItemDrop {
        let mut item = ItemRecord::new(0, seed_item, 1, 0);
        item.endurance = amount;
        meta.gold = Some(amount);
        ItemDrop::new(item, pos, angle, meta)
    }

    /// `true` once the drop has been lying around for more than 180 s.
    pub fn expired(&self, now: GameMs) -> bool {
        now.0.saturating_sub(self.meta.drop_time.0) > ITEM_EXPIRY_MS
    }
}

impl Worlds {
    /// The ground-item spawn API of docs 17/19/20/22/23 (`world::spawn_ground_item`, 15 §16.1): builds the
    /// [`ItemDrop`] (`core.index` = the item id, `core.pos` = `pos`, `core.angle` = `angle`, `meta` stored as
    /// given) and queues it through [`Worlds::spawn_item`] (the add-time clamp raises `pos.y` to the land height).
    /// The drop is linked and shown at the next `_add` of its layer.
    pub fn spawn_ground_item(
        &mut self,
        world: WorldId,
        layer: LayerId,
        pos: Vec3,
        angle: f32,
        item: ItemRecord,
        meta: GroundItemMeta,
    ) -> Result<ObjId, AddError> {
        self.spawn_item(world, layer, ItemDrop::new(item, pos, angle, meta))
    }
}
