//! `ContainerRecord`: the persisted form of a `CItemContainer<CItemElem>` (Plan/13 §6.3, ref/13 §6.1).
//!
//! A container has two arrays of length `capacity`:
//!
//! * the **storage** `m_apItem[i]` (non-empty entries are [`ContainerRecord::items`], keyed by
//!   [`ItemRecord::obj_id`] = `i`, the id packets use), and
//! * the **index** `m_apIndex[s]` ([`ContainerRecord::index`]): visible slot `s` -> storage index, `-1` =
//!   `NULL_ID`. Slots `0..bag_slots` are the bag grid; for the inventory the rest are the equipment slots
//!   (`bag_slots + PARTS_*`).
//!
//! Persisting the full `index` (including empty storage entries) keeps exact legacy semantics: which
//! `obj_id` a newly added item receives depends on the slot-to-storage mapping, and taskbar item shortcuts
//! reference `obj_id`s. The legacy `objIndex` string is the inverse of `index` and is not stored.

use super::item::ItemRecord;
use serde::{Deserialize, Serialize};

/// Equipment slots appended to the inventory bag (`MAX_HUMAN_PARTS`): slot `168 + p` is part `p`.
pub const EQUIP_SLOTS: u16 = 31;

/// The persisted form of one item container.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerRecord {
    /// `m_dwItemMax`: inventory 199 (168 bag + 31 equip), bank 42, pocket 18|42, guild bank 42.
    #[serde(default)]
    pub capacity: u16,
    /// `m_apIndex`: length == `capacity`; visible slot -> storage index (`obj_id`), `-1` = none.
    #[serde(default)]
    pub index: Vec<i32>,
    /// Non-empty storage entries only, sorted by `obj_id`.
    #[serde(default)]
    pub items: Vec<ItemRecord>,
}

/// One repair done by [`ContainerRecord::validate_and_repair`]. Every repair is logged with
/// `tracing::warn!`; the loader also emits audit `persist.repair`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairNote {
    /// The stored `capacity` differs from the expected one; the expected one was adopted.
    CapacityMismatch {
        /// Stored capacity.
        found: u16,
        /// Expected capacity.
        expected: u16,
    },
    /// `index.len() != capacity`: the index was rebuilt from the items (bag slots ascending).
    IndexLengthMismatch {
        /// Stored length.
        found: usize,
        /// Expected length.
        expected: usize,
    },
    /// An index entry outside `-1..capacity`; the slot was emptied.
    BadIndexEntry {
        /// The slot.
        slot: usize,
        /// The stored value.
        value: i32,
    },
    /// A storage index referenced by two slots; the later slot was emptied.
    DuplicateIndexEntry {
        /// The later slot.
        slot: usize,
        /// The storage index.
        obj_id: i32,
    },
    /// Two items with the same `obj_id`; the later one was moved.
    DuplicateObjId {
        /// The shared id.
        obj_id: u16,
    },
    /// An item with `obj_id >= capacity`; it was moved.
    ObjIdOutOfRange {
        /// The stored id.
        obj_id: u16,
    },
    /// An item not referenced by any slot (or one of the two cases above) was moved into a free bag slot.
    Relocated {
        /// The item's `obj_id` after the move.
        obj_id: u16,
        /// The bag slot it now occupies.
        slot: usize,
    },
    /// No free bag slot (or storage id) was left: the item is handed back to the loader, which moves it to
    /// the character's mailbox as a system mail. Never silently dropped.
    Orphaned(Box<ItemRecord>),
}

impl ContainerRecord {
    /// The legacy `Clear()` index: `index[i] = i` for the bag slots, `-1` for the rest.
    pub fn default_index(capacity: u16, bag_slots: u16) -> Vec<i32> {
        let bag = bag_slots.min(capacity);
        (0..capacity)
            .map(|i| if i < bag { i32::from(i) } else { -1 })
            .collect()
    }

    /// An empty container: `capacity` storage entries, `bag_slots` visible bag slots.
    pub fn empty(capacity: u16, bag_slots: u16) -> Self {
        Self {
            capacity,
            index: Self::default_index(capacity, bag_slots),
            items: Vec::new(),
        }
    }

    /// The item stored at storage index `obj_id` (items are sorted by `obj_id`).
    pub fn item_by_obj_id(&self, obj_id: u16) -> Option<&ItemRecord> {
        self.items
            .binary_search_by_key(&obj_id, |i| i.obj_id)
            .ok()
            .and_then(|pos| self.items.get(pos))
    }

    /// The item shown in visible slot `slot`.
    pub fn item_at_slot(&self, slot: usize) -> Option<&ItemRecord> {
        let storage = *self.index.get(slot)?;
        self.item_by_obj_id(u16::try_from(storage).ok()?)
    }

    /// `true` when the container satisfies every rule of §6.3 for `expected_capacity`: `capacity` and
    /// `index.len()` equal it, index entries are `-1` or unique values below it, `obj_id`s are unique, below
    /// it and sorted, and every item is referenced by exactly one slot.
    pub fn is_valid(&self, expected_capacity: u16) -> bool {
        let cap = usize::from(expected_capacity);
        if self.capacity != expected_capacity || self.index.len() != cap {
            return false;
        }
        let mut referenced = vec![false; cap];
        for &entry in &self.index {
            if entry == -1 {
                continue;
            }
            let Some(slot) = usize::try_from(entry)
                .ok()
                .and_then(|e| referenced.get_mut(e))
            else {
                return false;
            };
            if *slot {
                return false;
            }
            *slot = true;
        }
        let mut previous: Option<u16> = None;
        for item in &self.items {
            if previous.is_some_and(|p| p >= item.obj_id) {
                return false; // duplicate or unsorted
            }
            previous = Some(item.obj_id);
            if !referenced
                .get(usize::from(item.obj_id))
                .copied()
                .unwrap_or(false)
            {
                return false;
            }
        }
        true
    }

    /// [`ContainerRecord::validate_and_repair_with`] for a container whose slots are all bag slots (banks,
    /// pockets, the guild bank). Inventories use [`ContainerRecord::validate_and_repair_with`] with the
    /// bag size (`capacity - EQUIP_SLOTS`) or `InventoryDoc::validate_and_repair`.
    pub fn validate_and_repair(&mut self, expected_capacity: u16) -> Vec<RepairNote> {
        self.validate_and_repair_with(expected_capacity, expected_capacity)
    }

    /// Validates the container against `expected_capacity` and repairs it (Plan/13 §6.3):
    ///
    /// 1. `capacity` adopts the expected value; an `index` of the wrong length is rebuilt from the items
    ///    (the `Clear()` layout: bag slots map to themselves, items that are then unreferenced move into free
    ///    bag slots, ascending);
    /// 2. index entries outside `-1..capacity` and repeated storage indexes are emptied;
    /// 3. items with a duplicate or out-of-range `obj_id` get a new storage index;
    /// 4. every item not referenced by a slot moves into the first bag slot whose mapped storage entry is
    ///    empty (a valid, unique `obj_id` is kept - taskbar shortcuts reference it). If no slot is free the
    ///    item is returned as [`RepairNote::Orphaned`] and removed from the container.
    ///
    /// A valid container is not modified and yields no notes; the repair is idempotent, and after it
    /// [`ContainerRecord::is_valid`] holds. Items are never lost: each is either in the container or in an
    /// `Orphaned` note.
    pub fn validate_and_repair_with(
        &mut self,
        expected_capacity: u16,
        bag_slots: u16,
    ) -> Vec<RepairNote> {
        let mut notes = Vec::new();
        let cap = usize::from(expected_capacity);
        let bag = usize::from(bag_slots.min(expected_capacity));
        if self.capacity != expected_capacity {
            notes.push(RepairNote::CapacityMismatch {
                found: self.capacity,
                expected: expected_capacity,
            });
            self.capacity = expected_capacity;
        }

        // Items: canonical order, then pull out the ones that cannot keep their obj_id.
        self.items.sort_by_key(|i| i.obj_id);
        let mut kept: Vec<ItemRecord> = Vec::with_capacity(self.items.len());
        let mut displaced: Vec<ItemRecord> = Vec::new();
        for item in std::mem::take(&mut self.items) {
            if usize::from(item.obj_id) >= cap {
                notes.push(RepairNote::ObjIdOutOfRange {
                    obj_id: item.obj_id,
                });
                displaced.push(item);
            } else if kept.last().is_some_and(|k| k.obj_id == item.obj_id) {
                notes.push(RepairNote::DuplicateObjId {
                    obj_id: item.obj_id,
                });
                displaced.push(item);
            } else {
                kept.push(item);
            }
        }

        // Index: length, entry range, uniqueness.
        if self.index.len() != cap {
            notes.push(RepairNote::IndexLengthMismatch {
                found: self.index.len(),
                expected: cap,
            });
            self.index = Self::default_index(expected_capacity, bag_slots);
        } else {
            let mut seen = vec![false; cap];
            for (slot, entry) in self.index.iter_mut().enumerate() {
                if *entry == -1 {
                    continue;
                }
                let flag = usize::try_from(*entry).ok().and_then(|e| seen.get_mut(e));
                match flag {
                    None => {
                        notes.push(RepairNote::BadIndexEntry {
                            slot,
                            value: *entry,
                        });
                        *entry = -1;
                    }
                    Some(flag) if *flag => {
                        notes.push(RepairNote::DuplicateIndexEntry {
                            slot,
                            obj_id: *entry,
                        });
                        *entry = -1;
                    }
                    Some(flag) => *flag = true,
                }
            }
        }

        // Which storage indexes are shown / hold an item now.
        let mut referenced = vec![false; cap];
        for &entry in &self.index {
            if let Some(flag) = usize::try_from(entry)
                .ok()
                .and_then(|e| referenced.get_mut(e))
            {
                *flag = true;
            }
        }
        let mut occupied = vec![false; cap];
        for item in &kept {
            if let Some(flag) = occupied.get_mut(usize::from(item.obj_id)) {
                *flag = true;
            }
        }

        // Items that must be (re)placed: unreferenced kept items first (obj_id order), then displaced ones.
        let mut to_place: Vec<(ItemRecord, bool)> = Vec::new();
        let mut placed: Vec<ItemRecord> = Vec::with_capacity(kept.len());
        for item in kept {
            if referenced
                .get(usize::from(item.obj_id))
                .copied()
                .unwrap_or(false)
            {
                placed.push(item);
            } else {
                to_place.push((item, true));
            }
        }
        to_place.extend(displaced.into_iter().map(|item| (item, false)));

        for (mut item, keeps_obj_id) in to_place {
            let free_slot = (0..bag).find(|&s| match self.index.get(s) {
                Some(&-1) => true,
                Some(&e) => usize::try_from(e)
                    .ok()
                    .and_then(|e| occupied.get(e))
                    .is_some_and(|taken| !taken),
                None => false,
            });
            let Some(slot) = free_slot else {
                notes.push(RepairNote::Orphaned(Box::new(item)));
                continue;
            };
            let current = self.index.get(slot).copied().unwrap_or(-1);
            let obj_id = if keeps_obj_id {
                item.obj_id
            } else if current >= 0 {
                // The slot already maps to an empty storage entry: the item takes it.
                u16::try_from(current).unwrap_or(item.obj_id)
            } else {
                // The slot maps to nothing: the item needs a storage index nobody shows or holds.
                let free_id = (0..cap).find(|&e| {
                    !occupied.get(e).copied().unwrap_or(true)
                        && !referenced.get(e).copied().unwrap_or(true)
                });
                match free_id.and_then(|e| u16::try_from(e).ok()) {
                    Some(id) => id,
                    None => {
                        notes.push(RepairNote::Orphaned(Box::new(item)));
                        continue;
                    }
                }
            };
            // The storage entry the slot showed before (if any) is no longer shown.
            if let Some(flag) = usize::try_from(current)
                .ok()
                .and_then(|e| referenced.get_mut(e))
            {
                *flag = false;
            }
            if let Some(entry) = self.index.get_mut(slot) {
                *entry = i32::from(obj_id);
            }
            if let Some(flag) = referenced.get_mut(usize::from(obj_id)) {
                *flag = true;
            }
            if let Some(flag) = occupied.get_mut(usize::from(obj_id)) {
                *flag = true;
            }
            item.obj_id = obj_id;
            notes.push(RepairNote::Relocated { obj_id, slot });
            placed.push(item);
        }

        placed.sort_by_key(|i| i.obj_id);
        self.items = placed;
        for note in &notes {
            tracing::warn!(?note, "item container repaired");
        }
        notes
    }
}
