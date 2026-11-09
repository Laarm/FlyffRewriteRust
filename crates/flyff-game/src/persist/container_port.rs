//! A minimal port of the `CItemContainer<CItemElem>` operations the new-character kit needs (ref/13 §6.1:
//! `Add` into empty bag slots and `DoEquip`), over [`ContainerRecord`].
//!
//! Doc 19's inventory code (WP-ITEM-*) does not exist yet. The persisted container is the pair of arrays of the
//! legacy model: the storage `m_apItem[i]` (`ItemRecord.obj_id == i`) and the visible-slot index `m_apIndex[s]`.
//! The inverse mapping `m_dwObjIndex` is not stored; it is derived: a storage entry is "shown" when some index
//! entry points at it, and an entry that is not shown has `ObjIndex == NULL_ID`.
//!
//! * `add` follows `CItemContainer::Add`: fill compatible stacks in slot order, then empty bag slots in slot
//!   order (at most `pack_max` per stack); "empty bag slot" means the storage entry the slot points at is empty
//!   (after an equip that is not the entry with the slot's own number).
//! * `equip` follows `DoEquip(srcSlot, part)`: the first empty storage entry that is not shown anywhere takes the
//!   place of the item in its bag slot, and the item's storage index moves to slot `bag_slots + part`.
//!
//! Plan/13 §6.13 gives the reference result: add lollipop, biscuit, then (add, equip) suit, gauntlet, boots,
//! sword gives `index[2] = 171`, `index[170] = 2`, `index[172] = 168`, `index[173] = 169`, `index[178] = 170`.

use flyff_db::{ContainerRecord, ItemRecord};

/// What the port needs to know about an item type (a slice of `ItemProp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemInfo {
    /// Largest stack (`dwPackMax`, at least 1).
    pub pack_max: u16,
    /// `m_nHitPoint` of a new item: the prop's `dwEndurance`, or -1 for an item without endurance.
    pub endurance: i32,
}

impl ItemInfo {
    /// What an item of unknown properties gets (no game data loaded: tests with an empty `GameData`).
    pub const UNKNOWN: ItemInfo = ItemInfo {
        pack_max: 9999,
        endurance: -1,
    };
}

/// Why an operation was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ContainerError {
    /// No room for the items.
    #[error("the container is full")]
    Full,
    /// The item to equip is not in a bag slot.
    #[error("object {0} is not in a bag slot")]
    NotInBag(u16),
    /// The equipment slot already holds an item.
    #[error("equipment slot {0} is occupied")]
    SlotOccupied(usize),
    /// No empty storage entry is left for the swap.
    #[error("no free storage entry")]
    NoStorage,
    /// A slot outside the container.
    #[error("slot {0} is outside the container")]
    OutOfRange(usize),
    /// The serial source failed.
    #[error("{0}")]
    Serial(String),
}

/// Operations over one inventory container (`bag_slots` visible bag slots, the rest equipment slots).
pub struct ContainerPort<'a> {
    c: &'a mut ContainerRecord,
    bag_slots: usize,
}

impl<'a> ContainerPort<'a> {
    /// Wraps `c`; its first `bag_slots` index entries are the bag grid.
    pub fn new(c: &'a mut ContainerRecord, bag_slots: usize) -> Self {
        Self { c, bag_slots }
    }

    fn storage_empty(&self, obj_id: usize) -> bool {
        u16::try_from(obj_id)
            .ok()
            .is_none_or(|id| self.c.item_by_obj_id(id).is_none())
    }

    fn storage_of_slot(&self, slot: usize) -> Option<usize> {
        let v = *self.c.index.get(slot)?;
        usize::try_from(v).ok()
    }

    /// The item at storage index `obj_id`.
    pub fn item(&self, obj_id: u16) -> Option<&ItemRecord> {
        self.c.item_by_obj_id(obj_id)
    }

    /// Adds `count` pieces of `item_id`: existing compatible stacks first, then empty bag slots in order.
    /// `serial` is asked once per **new** stack. Returns the touched `obj_id`s in the order they were filled.
    /// Nothing changes when the container cannot hold everything (`IsFull` is evaluated first).
    pub fn add(
        &mut self,
        item_id: u32,
        count: u16,
        info: ItemInfo,
        serial: &mut dyn FnMut() -> Result<u64, String>,
    ) -> Result<Vec<u16>, ContainerError> {
        let pack_max = u32::from(info.pack_max.max(1));
        // IsFull: what the bag can absorb
        let mut room: u64 = 0;
        for slot in 0..self.bag_slots {
            match self.storage_of_slot(slot) {
                Some(storage) if self.storage_empty(storage) => room += u64::from(pack_max),
                Some(storage) => {
                    if let Some(existing) = u16::try_from(storage)
                        .ok()
                        .and_then(|id| self.c.item_by_obj_id(id))
                        && compatible(existing, item_id)
                    {
                        room += u64::from(pack_max.saturating_sub(u32::from(existing.count)));
                    }
                }
                None => {}
            }
        }
        if room < u64::from(count) {
            return Err(ContainerError::Full);
        }
        let mut left = u32::from(count);
        let mut touched = Vec::new();
        // fill compatible stacks in slot order (only worth it for stackable items)
        if pack_max > 1 {
            for slot in 0..self.bag_slots {
                if left == 0 {
                    break;
                }
                let Some(storage) = self.storage_of_slot(slot) else {
                    continue;
                };
                let Ok(id) = u16::try_from(storage) else {
                    continue;
                };
                let Some(pos) = self.c.items.iter().position(|i| i.obj_id == id) else {
                    continue;
                };
                let Some(existing) = self.c.items.get_mut(pos) else {
                    continue;
                };
                if !compatible(existing, item_id) {
                    continue;
                }
                let free = pack_max.saturating_sub(u32::from(existing.count));
                let put = free.min(left);
                if put > 0 {
                    existing.count += u16::try_from(put).unwrap_or(0);
                    left -= put;
                    touched.push(id);
                }
            }
        }
        // the remainder goes into empty bag slots
        for slot in 0..self.bag_slots {
            if left == 0 {
                break;
            }
            let Some(storage) = self.storage_of_slot(slot) else {
                continue;
            };
            if !self.storage_empty(storage) {
                continue;
            }
            let Ok(id) = u16::try_from(storage) else {
                continue;
            };
            let put = pack_max.min(left);
            let rec = ItemRecord {
                obj_id: id,
                item_id,
                count: u16::try_from(put).unwrap_or(1),
                serial: serial().map_err(ContainerError::Serial)?,
                endurance: info.endurance,
                ..ItemRecord::default()
            };
            let at = self.c.items.partition_point(|i| i.obj_id < id);
            self.c.items.insert(at, rec);
            left -= put;
            touched.push(id);
        }
        Ok(touched)
    }

    /// `DoEquip`: moves the item of storage index `obj_id` from its bag slot to equipment slot
    /// `bag_slots + part`; a free storage entry takes its place in the bag.
    pub fn equip(&mut self, obj_id: u16, part: usize) -> Result<(), ContainerError> {
        let target = self.bag_slots + part;
        if target >= self.c.index.len() {
            return Err(ContainerError::OutOfRange(target));
        }
        if self.c.index.get(target).is_some_and(|&v| v != -1) {
            return Err(ContainerError::SlotOccupied(target));
        }
        let src = (0..self.bag_slots)
            .find(|&s| self.storage_of_slot(s) == Some(usize::from(obj_id)))
            .ok_or(ContainerError::NotInBag(obj_id))?;
        let shown: Vec<bool> = {
            let mut v = vec![false; self.c.index.len()];
            for &e in &self.c.index {
                if let Some(slot) = usize::try_from(e).ok().and_then(|e| v.get_mut(e)) {
                    *slot = true;
                }
            }
            v
        };
        let free = (0..self.c.index.len())
            .find(|&i| self.storage_empty(i) && !shown.get(i).copied().unwrap_or(true))
            .ok_or(ContainerError::NoStorage)?;
        let free = i32::try_from(free).map_err(|_| ContainerError::NoStorage)?;
        if let Some(e) = self.c.index.get_mut(target) {
            *e = i32::from(obj_id);
        }
        if let Some(e) = self.c.index.get_mut(src) {
            *e = free;
        }
        Ok(())
    }

    /// The item shown in equipment slot `part`.
    pub fn equipped(&self, part: usize) -> Option<&ItemRecord> {
        self.c.item_at_slot(self.bag_slots + part)
    }
}

/// Stack compatibility (`CItemElem` same id, flags and charged state; serial, options and keep time ignored).
fn compatible(existing: &ItemRecord, item_id: u32) -> bool {
    existing.item_id == item_id && existing.flags == 0 && !existing.charged
}
