//! `ItemContainer`: the exact port of `CItemContainer<CItemElem>` (Plan/19 §3.1-§3.2, WP-ITEM-02;
//! ref/13 §6.1, src:_Common/Item.h:341-949).
//!
//! Two levels of addressing, because the protocol, the taskbar shortcuts and the stack placement order
//! depend on both:
//!
//! * the **storage** `items[i]`: an item's `obj_id` is its storage index for as long as it exists; packets
//!   address items by it;
//! * the **slot index** `index[s]`: the storage index shown in visible slot `s`. Slots `0..index_num` are the
//!   bag grid, the following ones the equipment slots (`index_num + part`). `-1` = `NULL_ID`.
//!
//! `obj_index[i]` is the inverse (storage index → slot, `-1` for an entry that is shown nowhere). All
//! operations are pure on the container; callers (`item::ops`, the handlers) produce the messages.
//!
//! The operations reproduce the original's quirks on purpose (KQ-15): stacks merge on equal id, flags and
//! charged state only; two full stacks do not swap; `un_equip` fails with a full bag; `add` fills compatible
//! stacks in slot order before the empty slots.

use super::model::Item;
use crate::persist::serial::SerialError;
use flyff_common::ItemSerial;
use flyff_data::GameData;
use flyff_data::types::ItemId;
use flyff_db::{ContainerRecord, ItemRecord};
use flyff_proto::pb;

/// A storage index (`m_dwObjId`), the id packets use.
pub type StorageId = u16;
/// A visible slot index.
pub type SlotIdx = u16;

/// What a container is (diagnostics and audit; the operations do not depend on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    /// The inventory: bag slots plus the 31 equipment slots.
    Inventory,
    /// One account bank.
    Bank,
    /// A pocket (0..=2).
    Pocket(u8),
    /// The guild bank.
    GuildBank,
    /// One tab of an NPC shop.
    ShopTab,
    /// A scratch container (simulations).
    Temp,
}

/// The source of fresh item serials (19 §2.7); the production one is [`super::ops::CtxSerials`].
pub trait SerialSource {
    /// The next serial: never 0, never handed out before in this database.
    fn next_serial(&mut self) -> Result<u64, SerialError>;
}

/// A counter for tests and simulations.
#[derive(Clone, Copy, Debug)]
pub struct SeqSerials(pub u64);

impl SerialSource for SeqSerials {
    fn next_serial(&mut self) -> Result<u64, SerialError> {
        self.0 += 1;
        Ok(self.0)
    }
}

/// A source that never has a serial (paths that must not need one, e.g. the pickup of a whole stack).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSerials;

impl SerialSource for NoSerials {
    fn next_serial(&mut self) -> Result<u64, SerialError> {
        Err(SerialError("no serial source on this path".to_owned()))
    }
}

/// A source that hands out serial 0 ("assign at the next item tick"): used where no serial allocator is
/// reachable (the pickup in the world phase). `PlayerItems.serial_pending` tells the item job to fill them in
/// before the snapshot pass of the tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeferSerials;

impl SerialSource for DeferSerials {
    fn next_serial(&mut self) -> Result<u64, SerialError> {
        Ok(0)
    }
}

/// What [`ItemContainer::add`] did: the storage entries that were touched, with their **new total counts**
/// (the `CreateItem` payload: the client copies the template into each listed entry with that count).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddPlan {
    /// `(storage id, new count)` in the order the entries were filled.
    pub touched: Vec<(StorageId, u16)>,
    /// The subset of `touched` that did not exist before (new entries).
    pub created: Vec<StorageId>,
    /// The serial the first new entry got (the template's serial on the wire); 0 when the stack was merged
    /// completely and the incoming serial was retired.
    pub template_serial: ItemSerial,
}

/// Why [`ItemContainer::add`] failed; nothing changed in any case.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum AddError {
    /// The item has no prop (or id 0).
    #[error("unknown item prop")]
    NoProp,
    /// The item does not fit (`IsFull`).
    #[error("the container is full")]
    Full,
    /// A fresh serial was needed and none could be allocated.
    #[error(transparent)]
    Serial(#[from] SerialError),
}

/// What [`ItemContainer::swap`] did to the two slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapOutcome {
    /// Nothing changed (same slot, out of range, or two compatible full stacks).
    Nothing,
    /// The two slots exchanged their storage entries.
    Swapped,
    /// Compatible stacks merged: the source stack moved entirely into the destination (`removed`) or only
    /// as much as fitted.
    Merged {
        /// The source stack is gone (its storage entry is empty now).
        removed: bool,
        /// How many pieces moved.
        moved: u16,
    },
}

/// `CItemContainer<CItemElem>` (see the module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemContainer {
    kind: ContainerKind,
    index_num: u16,
    items: Vec<Option<Item>>,
    index: Vec<i32>,
    obj_index: Vec<i32>,
}

impl ItemContainer {
    /// `SetItemContainer` + `Clear()`: `index_num` bag slots plus `extra` equipment slots; slot `i < index_num`
    /// shows storage entry `i`, the others show nothing.
    pub fn new(kind: ContainerKind, index_num: u16, extra: u16) -> ItemContainer {
        let max = usize::from(index_num) + usize::from(extra);
        let mut index = vec![-1_i32; max];
        let mut obj_index = vec![-1_i32; max];
        for i in 0..usize::from(index_num) {
            index[i] = i as i32;
            obj_index[i] = i as i32;
        }
        ItemContainer {
            kind,
            index_num,
            items: vec![None; max],
            index,
            obj_index,
        }
    }

    /// The container of a persisted record. The record is validated and repaired first (a valid record, the
    /// normal case after `flyff-db`'s load, is not touched); items that cannot be placed are returned
    /// (never dropped silently) for the caller's mailbox fallback.
    pub fn from_record(
        rec: &ContainerRecord,
        kind: ContainerKind,
        index_num: u16,
        extra: u16,
    ) -> (ItemContainer, Vec<ItemRecord>) {
        let capacity = index_num.saturating_add(extra);
        let mut rec = rec.clone();
        let mut orphans = Vec::new();
        if !rec.is_valid(capacity) {
            for note in rec.validate_and_repair_with(capacity, index_num) {
                if let flyff_db::RepairNote::Orphaned(it) = note {
                    orphans.push(*it);
                }
            }
        }
        let mut c = ItemContainer::new(kind, index_num, extra);
        c.index.clone_from(&rec.index);
        for (slot, &storage) in rec.index.iter().enumerate() {
            if let Some(o) = usize::try_from(storage)
                .ok()
                .and_then(|s| c.obj_index.get_mut(s))
            {
                *o = slot as i32;
            }
        }
        // entries that are shown nowhere keep -1 (Clear() would have numbered the bag entries; the stored
        // index is authoritative)
        for (i, o) in c.obj_index.iter_mut().enumerate() {
            if !rec.index.contains(&(i as i32)) {
                *o = -1;
            }
        }
        for r in &rec.items {
            if let Some(entry) = c.items.get_mut(usize::from(r.obj_id)) {
                *entry = Some(Item::from_record(r));
            }
        }
        (c, orphans)
    }

    /// The persisted record: the full `index` and the non-empty entries sorted by `obj_id`.
    pub fn to_record(&self) -> ContainerRecord {
        ContainerRecord {
            capacity: self.item_max(),
            index: self.index.clone(),
            items: self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| it.as_ref().map(|it| it.to_record(i as u16)))
                .collect(),
        }
    }

    /// `CItemContainer::Serialize` as a message: the slot index, the non-empty items and the inverse index.
    pub fn to_pb(&self) -> pb::ItemContainer {
        pb::ItemContainer {
            max: u32::from(self.item_max()),
            index: self.index.clone(),
            items: self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| it.as_ref().map(|it| it.to_pb(i as u16)))
                .collect(),
            obj_index: self.obj_index.clone(),
        }
    }

    /// The kind.
    pub fn kind(&self) -> ContainerKind {
        self.kind
    }

    /// `m_dwIndexNum`: the bag slots.
    pub fn index_num(&self) -> u16 {
        self.index_num
    }

    /// `m_dwItemMax`: the storage entries (bag plus equipment).
    pub fn item_max(&self) -> u16 {
        self.items.len() as u16
    }

    /// The storage index shown in `slot`, `None` for an empty slot or one out of range.
    pub fn obj_at_slot(&self, slot: u16) -> Option<StorageId> {
        let v = *self.index.get(usize::from(slot))?;
        StorageId::try_from(v).ok()
    }

    /// The slot that shows storage entry `id` (`m_dwObjIndex`).
    pub fn slot_of(&self, id: StorageId) -> Option<SlotIdx> {
        let v = *self.obj_index.get(usize::from(id))?;
        SlotIdx::try_from(v).ok()
    }

    /// `GetAtId`: the item in storage entry `id`.
    pub fn get_at_id(&self, id: StorageId) -> Option<&Item> {
        self.items.get(usize::from(id))?.as_ref()
    }

    /// `GetAtId`, mutably.
    pub fn get_at_id_mut(&mut self, id: StorageId) -> Option<&mut Item> {
        self.items.get_mut(usize::from(id))?.as_mut()
    }

    /// `GetAt`: the item shown in `slot`.
    pub fn get_at_slot(&self, slot: u16) -> Option<&Item> {
        self.get_at_id(self.obj_at_slot(slot)?)
    }

    /// `IsEquip`: the storage entry is shown in an equipment slot.
    pub fn is_equip(&self, id: StorageId) -> bool {
        self.slot_of(id).is_some_and(|s| s >= self.index_num)
    }

    /// `GetEquip(part)`: the item in equipment slot `index_num + part`.
    pub fn get_equip(&self, part: u32) -> Option<&Item> {
        let slot = u32::from(self.index_num).checked_add(part)?;
        self.get_at_slot(u16::try_from(slot).ok()?)
    }

    /// The storage index of the item in equipment part `part`.
    pub fn equip_id(&self, part: u32) -> Option<StorageId> {
        let slot = u32::from(self.index_num).checked_add(part)?;
        self.obj_at_slot(u16::try_from(slot).ok()?)
    }

    /// `GetEmptyCount`: empty storage entries shown in the bag.
    pub fn empty_count(&self) -> usize {
        self.items
            .iter()
            .zip(&self.obj_index)
            .filter(|(it, o)| it.is_none() && **o >= 0 && **o < i32::from(self.index_num))
            .count()
    }

    /// `GetCount`: occupied storage entries shown in the bag.
    pub fn count(&self) -> usize {
        self.items
            .iter()
            .zip(&self.obj_index)
            .filter(|(it, o)| it.is_some() && **o >= 0 && **o < i32::from(self.index_num))
            .count()
    }

    /// `GetAtItemNum`: the number of pieces of `item_id` over all entries, equipped ones included.
    pub fn count_of(&self, item_id: ItemId) -> u32 {
        self.items
            .iter()
            .flatten()
            .filter(|it| it.item_id == item_id)
            .map(|it| u32::from(it.count))
            .sum()
    }

    /// The occupied entries in storage order: `(storage id, item)`.
    pub fn iter(&self) -> impl Iterator<Item = (StorageId, &Item)> + '_ {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| it.as_ref().map(|it| (i as StorageId, it)))
    }

    /// The occupied entries in storage order, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (StorageId, &mut Item)> + '_ {
        self.items
            .iter_mut()
            .enumerate()
            .filter_map(|(i, it)| it.as_mut().map(|it| (i as StorageId, it)))
    }

    /// The first occupied entry holding `item_id` (`GetAtItemId`).
    pub fn find(&self, item_id: ItemId) -> Option<(StorageId, &Item)> {
        self.iter().find(|(_, it)| it.item_id == item_id)
    }

    /// `IsFull(elem, prop, n)` (Item.h:629-658): would `n` pieces of `item` not fit? Walks the bag slots in
    /// order; an empty slot absorbs `pack_max`, a compatible stack `pack_max - count`; it is not full as soon
    /// as the rest fits somewhere.
    pub fn is_full(&self, item: &Item, pack_max: u32, n: u32) -> bool {
        let pack_max = i64::from(pack_max.max(1));
        let mut left = i64::from(n);
        for slot in 0..usize::from(self.index_num) {
            let Some(id) = self.index.get(slot).and_then(|&v| usize::try_from(v).ok()) else {
                continue;
            };
            match self.items.get(id) {
                Some(None) => {
                    if left > pack_max {
                        left -= pack_max;
                    } else {
                        return false;
                    }
                }
                Some(Some(x)) if x.stacks_with(item) => {
                    let have = i64::from(x.count);
                    if have + left > pack_max {
                        left -= pack_max - have;
                    } else {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }

    /// `Add(elem)` (Item.h:660-759). `Full` when `is_full`; otherwise compatible stacks are filled in slot
    /// order (stackable items only), then the rest goes into empty bag slots in slot order, at most
    /// `pack_max` each. Serials (19 §2.7): the first **new** entry keeps the incoming serial (a fresh one if
    /// the incoming had none), the further new entries get fresh serials, and a serial of an item that was
    /// merged completely is retired. Nothing changes when an error is returned.
    pub fn add(
        &mut self,
        item: &Item,
        data: &GameData,
        serials: &mut dyn SerialSource,
    ) -> Result<AddPlan, AddError> {
        if item.item_id == 0 {
            return Err(AddError::NoProp);
        }
        let prop = data.item(item.item_id).ok_or(AddError::NoProp)?;
        let pack_max = prop.pack_max.max(1);
        let n = u32::from(item.count);
        if self.is_full(item, pack_max, n) {
            return Err(AddError::Full);
        }
        // plan: (storage, new count, is_new)
        let mut plan: Vec<(StorageId, u16, bool)> = Vec::new();
        let mut left = i64::from(n);
        let pm = i64::from(pack_max);
        if pack_max > 1 {
            for slot in 0..usize::from(self.index_num) {
                if left == 0 {
                    break;
                }
                let Some(id) = self.slot_storage(slot) else {
                    continue;
                };
                if let Some(Some(x)) = self.items.get(usize::from(id))
                    && x.stacks_with(item)
                    && i64::from(x.count) < pm
                {
                    let room = pm - i64::from(x.count);
                    let put = room.min(left);
                    plan.push((id, (i64::from(x.count) + put) as u16, false));
                    left -= put;
                }
            }
        }
        if left > 0 {
            for slot in 0..usize::from(self.index_num) {
                if left == 0 {
                    break;
                }
                let Some(id) = self.slot_storage(slot) else {
                    continue;
                };
                if matches!(self.items.get(usize::from(id)), Some(None)) {
                    let put = pm.min(left);
                    plan.push((id, put as u16, true));
                    left -= put;
                }
            }
        }
        if left > 0 {
            // IsFull said it fits; a corrupt index is the only way here
            return Err(AddError::Full);
        }
        // serials for the new entries
        let new_count = plan.iter().filter(|p| p.2).count();
        let mut fresh: Vec<u64> = Vec::new();
        let keep_first = item.serial.0 != 0;
        let need = new_count.saturating_sub(usize::from(keep_first));
        for _ in 0..need {
            fresh.push(serials.next_serial()?);
        }
        let mut fresh = fresh.into_iter();
        let mut template_serial = ItemSerial(0);
        let mut first_new = true;
        let mut touched = Vec::with_capacity(plan.len());
        let mut created = Vec::new();
        for (id, count, is_new) in plan {
            let i = usize::from(id);
            if is_new {
                let mut it = item.clone();
                it.count = count;
                it.reserved = 0;
                let serial = if first_new && keep_first {
                    item.serial.0
                } else {
                    fresh.next().unwrap_or(0)
                };
                if first_new {
                    template_serial = ItemSerial(serial);
                    first_new = false;
                }
                it.serial = ItemSerial(serial);
                if let Some(slot) = self.items.get_mut(i) {
                    *slot = Some(it);
                }
                created.push(id);
            } else if let Some(Some(x)) = self.items.get_mut(i) {
                x.count = count;
            }
            touched.push((id, count));
        }
        Ok(AddPlan {
            touched,
            created,
            template_serial,
        })
    }

    /// The storage index shown in bag slot `slot` as a [`StorageId`].
    fn slot_storage(&self, slot: usize) -> Option<StorageId> {
        let v = *self.index.get(slot)?;
        StorageId::try_from(v)
            .ok()
            .filter(|&s| usize::from(s) < self.items.len())
    }

    /// `SetAtId`: stores `item` in storage entry `id` as it is (restores and fixtures); the entry must exist and
    /// be shown somewhere. Returns `false` otherwise. No stack merging, no serial handling.
    pub fn set_at_id(&mut self, id: StorageId, item: Item) -> bool {
        let shown = self.slot_of(id).is_some();
        match self.items.get_mut(usize::from(id)) {
            Some(slot) if shown => {
                *slot = Some(item);
                true
            }
            _ => false,
        }
    }

    /// `RemoveAtId`: empties storage entry `id` and returns the item. An entry that is shown nowhere cannot
    /// be removed (the original returned early); an equipped entry frees its equipment slot.
    pub fn remove_at_id(&mut self, id: StorageId) -> Option<Item> {
        let i = usize::from(id);
        let shown = *self.obj_index.get(i)?;
        if shown < 0 {
            return None;
        }
        let it = self.items.get_mut(i)?.take();
        if shown >= i32::from(self.index_num) {
            if let Some(s) = self.index.get_mut(shown as usize) {
                *s = -1;
            }
            if let Some(o) = self.obj_index.get_mut(i) {
                *o = -1;
            }
        }
        it
    }

    /// `Swap(src, dst)` (Item.h:843-873): two slots holding compatible stacks (same id, flags and charged
    /// state, `pack_max > 1`) merge: everything moves if it fits, otherwise as much as fits (nothing when the
    /// destination is full); anything else exchanges the two slots.
    pub fn swap(&mut self, src: u16, dst: u16, data: &GameData) -> SwapOutcome {
        let max = usize::from(self.item_max());
        if src == dst || usize::from(src) >= max || usize::from(dst) >= max {
            return SwapOutcome::Nothing;
        }
        let a = self.obj_at_slot(src);
        let b = self.obj_at_slot(dst);
        if let (Some(a), Some(b)) = (a, b)
            && let (Some(x), Some(y)) = (self.get_at_id(a), self.get_at_id(b))
            && x.stacks_with(y)
            && let Some(prop) = data.item(x.item_id)
            && prop.pack_max > 1
        {
            let pack = i64::from(prop.pack_max);
            let remnant = pack - i64::from(y.count);
            let xs = i64::from(x.count);
            if remnant >= xs {
                let moved = x.count;
                if let Some(Some(y)) = self.items.get_mut(usize::from(b)) {
                    y.count = (i64::from(y.count) + xs) as u16;
                }
                self.remove_at_id(a);
                return SwapOutcome::Merged {
                    removed: true,
                    moved,
                };
            }
            let moved = remnant.max(0) as u16;
            if let Some(Some(y)) = self.items.get_mut(usize::from(b)) {
                y.count = (i64::from(y.count) + remnant) as u16;
            }
            if let Some(Some(x)) = self.items.get_mut(usize::from(a)) {
                x.count = (xs - remnant) as u16;
            }
            return SwapOutcome::Merged {
                removed: false,
                moved,
            };
        }
        self.swap_slots(src, dst);
        SwapOutcome::Swapped
    }

    /// `Swap2`: exchanges the entries shown in two slots and fixes the inverse index.
    fn swap_slots(&mut self, src: u16, dst: u16) {
        let (s, d) = (usize::from(src), usize::from(dst));
        let (Some(&a), Some(&b)) = (self.index.get(s), self.index.get(d)) else {
            return;
        };
        if let Some(x) = self.index.get_mut(s) {
            *x = b;
        }
        if let Some(x) = self.index.get_mut(d) {
            *x = a;
        }
        if let Some(o) = usize::try_from(b)
            .ok()
            .and_then(|e| self.obj_index.get_mut(e))
        {
            *o = i32::from(src);
        }
        if let Some(o) = usize::try_from(a)
            .ok()
            .and_then(|e| self.obj_index.get_mut(e))
        {
            *o = i32::from(dst);
        }
    }

    /// `DoEquip(src, part)` (Item.h:545-569): the item shown in bag slot `src` moves to equipment slot
    /// `index_num + part`; the first storage entry that is empty and shown nowhere takes its place in the
    /// bag. `false` (nothing changed) when there is no such entry, the source shows nothing, or - defensive,
    /// the planner guarantees it - the equipment slot is occupied.
    pub fn do_equip(&mut self, src: u16, part: u32) -> bool {
        let Some(dst) = u32::from(self.index_num)
            .checked_add(part)
            .and_then(|d| u16::try_from(d).ok())
        else {
            return false;
        };
        if src == dst
            || usize::from(src) >= self.items.len()
            || usize::from(dst) >= self.items.len()
        {
            return false;
        }
        let Some(item_id) = self.obj_at_slot(src) else {
            return false;
        };
        if self.obj_at_slot(dst).is_some() {
            return false;
        }
        let free = self
            .items
            .iter()
            .zip(&self.obj_index)
            .position(|(it, &o)| it.is_none() && o < 0);
        let Some(free) = free else {
            return false;
        };
        self.index[usize::from(dst)] = i32::from(item_id);
        self.index[usize::from(src)] = free as i32;
        self.obj_index[free] = i32::from(src);
        self.obj_index[usize::from(item_id)] = i32::from(dst);
        true
    }

    /// `UnEquip(part)` (Item.h:571-597): the item in equipment part `part` moves to the first bag slot whose
    /// storage entry is empty; that entry becomes unshown. `false` when the bag has no such slot (**the
    /// unequip fails with a full bag**, KQ-15) or nothing is equipped there.
    pub fn un_equip(&mut self, part: u32) -> bool {
        let Some(eq_slot) = u32::from(self.index_num)
            .checked_add(part)
            .and_then(|d| u16::try_from(d).ok())
        else {
            return false;
        };
        let Some(item_id) = self.obj_at_slot(eq_slot) else {
            return false;
        };
        for i in 0..usize::from(self.index_num) {
            let Some(e) = self.slot_storage(i) else {
                continue;
            };
            if matches!(self.items.get(usize::from(e)), Some(None)) {
                self.obj_index[usize::from(e)] = -1;
                self.index[usize::from(eq_slot)] = -1;
                self.obj_index[usize::from(item_id)] = i as i32;
                self.index[i] = i32::from(item_id);
                return true;
            }
        }
        false
    }

    /// Moves the item of equipment part `from` to equipment part `to` (the left weapon becoming the right
    /// one): both slots exchange their entries. `to` must be empty.
    pub fn move_equip(&mut self, from: u32, to: u32) -> bool {
        let (Some(f), Some(t)) = (
            u16::try_from(u32::from(self.index_num) + from).ok(),
            u16::try_from(u32::from(self.index_num) + to).ok(),
        ) else {
            return false;
        };
        if usize::from(f) >= self.index.len()
            || usize::from(t) >= self.index.len()
            || self.obj_at_slot(f).is_none()
            || self.obj_at_slot(t).is_some()
        {
            return false;
        }
        self.swap_slots(f, t);
        true
    }

    /// Exchanges the items of two equipment parts (both occupied): the right and left weapon swapping
    /// hands. `false` when either part shows nothing.
    pub fn swap_equip(&mut self, a: u32, b: u32) -> bool {
        let (Some(x), Some(y)) = (
            u16::try_from(u32::from(self.index_num) + a).ok(),
            u16::try_from(u32::from(self.index_num) + b).ok(),
        ) else {
            return false;
        };
        if x == y || self.obj_at_slot(x).is_none() || self.obj_at_slot(y).is_none() {
            return false;
        }
        self.swap_slots(x, y);
        true
    }

    /// Copies `n` pieces of the stack in entry `id` into a new item with `serial` taken from `serials`
    /// (a split, 19 §2.7): the original keeps the rest. `None` when the entry does not hold more than `n`
    /// or `n` is 0 or a serial is unavailable (nothing changes).
    pub fn split(&mut self, id: StorageId, n: u16, serials: &mut dyn SerialSource) -> Option<Item> {
        let have = self.get_at_id(id)?.count;
        if n == 0 || n >= have {
            return None;
        }
        let serial = serials.next_serial().ok()?;
        let it = self.items.get_mut(usize::from(id))?.as_mut()?;
        it.count -= n;
        let mut out = it.clone();
        out.count = n;
        out.serial = ItemSerial(serial);
        out.reserved = 0;
        Some(out)
    }

    /// The structural invariants (tests, `debug_assert`s of the callers): `index` and `obj_index` are
    /// inverse, no entry is shown twice and every occupied entry is shown.
    pub fn check_invariants(&self) -> Result<(), String> {
        if self.index.len() != self.items.len() || self.obj_index.len() != self.items.len() {
            return Err("array lengths differ".to_owned());
        }
        for (slot, &e) in self.index.iter().enumerate() {
            if e < 0 {
                continue;
            }
            let Some(&back) = self.obj_index.get(e as usize) else {
                return Err(format!("slot {slot} shows out-of-range entry {e}"));
            };
            if back != slot as i32 {
                return Err(format!("slot {slot} shows {e} but obj_index[{e}] = {back}"));
            }
        }
        for (e, &s) in self.obj_index.iter().enumerate() {
            if s >= 0 {
                match self.index.get(s as usize) {
                    Some(&x) if x == e as i32 => {}
                    other => {
                        return Err(format!("obj_index[{e}] = {s} but index[{s}] = {other:?}"));
                    }
                }
            } else if self.items.get(e).is_some_and(Option::is_some) {
                return Err(format!("occupied entry {e} is shown nowhere"));
            }
        }
        Ok(())
    }
}
