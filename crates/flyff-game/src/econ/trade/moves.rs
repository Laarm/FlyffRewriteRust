//! Moving stacks between two inventories (Plan/19 §8.3, §9.4; `CVTInfo::TradeLastConfirm` and
//! `VendorSellItem`, src:_Common/MoverItem.cpp:351-518, 637-704): the primitives the trade commit and the vendor
//! purchase share.
//!
//! * [`take_stack`] removes `n` pieces of one entry from the giver. A whole stack leaves with its serial; a partial
//!   stack is **split** and the moved part gets a fresh serial from the [`PoolSerials`] (PL-050; the original
//!   copied the serial, which duplicated it).
//! * [`put_stack`] adds a taken stack to the receiver with the container's `Add` (stacks merge, serials are kept or
//!   retired by the container, 19 §2.7).
//! * The serial pool is filled **before** anything changes ([`serials_needed`]), so the commit itself has no
//!   fallible allocation in it and every move can be simulated on clones first.
//!
//! Everything works on [`ItemContainer`]s, not on movers: a commit simulates the whole exchange on clones of the
//! two inventories and installs the clones only when the simulation succeeded.

use crate::item::container::{AddPlan, ItemContainer, SerialSource, StorageId};
use crate::item::model::Item;
use crate::item::ops::UiParam;
use crate::persist::serial::SerialError;
use flyff_common::ItemSerial;
use flyff_data::GameData;
use flyff_proto::pb;

/// Pre-allocated serials: the commit takes them from here, never from the allocator.
#[derive(Clone, Debug, Default)]
pub struct PoolSerials {
    pool: Vec<u64>,
    next: usize,
}

impl PoolSerials {
    /// A pool over `serials`, handed out in order.
    pub fn new(serials: Vec<u64>) -> PoolSerials {
        PoolSerials {
            pool: serials,
            next: 0,
        }
    }

    /// How many serials are left.
    pub fn left(&self) -> usize {
        self.pool.len().saturating_sub(self.next)
    }
}

impl SerialSource for PoolSerials {
    fn next_serial(&mut self) -> Result<u64, SerialError> {
        let s = self
            .pool
            .get(self.next)
            .copied()
            .ok_or_else(|| SerialError("the serial pool of the exchange ran dry".to_owned()))?;
        self.next += 1;
        Ok(s)
    }
}

/// Why a move failed (nothing is changed in any case; the callers answer with the error form).
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum MoveError {
    /// The storage entry does not exist or does not hold enough.
    #[error("the entry {0} is gone or too small")]
    Missing(StorageId),
    /// The receiver cannot take the stack.
    #[error("the receiving inventory is full")]
    Full,
    /// A serial was needed and the pool was empty.
    #[error(transparent)]
    Serial(#[from] SerialError),
}

/// One stack that left an inventory.
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    /// The storage entry it came from.
    pub from: StorageId,
    /// What is left in that entry (0 = the entry is gone): the `UpdateItem{UI_NUM}` value.
    pub left: u16,
    /// The moved part: `count` pieces, `reserved` 0, the serial of the whole stack or a fresh one.
    pub item: Item,
}

/// One stack that arrived in an inventory.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrived {
    /// What `Add` did.
    pub plan: AddPlan,
    /// The `CreateItem` template: the moved item with the serial of the first new entry.
    pub template: Item,
    /// The pieces that arrived.
    pub count: u16,
}

/// How many fresh serials a list of `(entry, n)` moves out of `inv` needs: one per partial stack (the moved part
/// is a new item) and one per whole stack that has no serial yet (the receiving `Add` numbers it).
pub fn serials_needed(inv: &ItemContainer, moves: &[(StorageId, u16)]) -> usize {
    moves
        .iter()
        .filter(|(id, n)| {
            inv.get_at_id(*id)
                .is_some_and(|it| *n < it.count || it.serial.0 == 0)
        })
        .count()
}

/// Takes `n` pieces out of entry `id`. `keep_reserved` is the reservation the entry keeps when a part of the
/// stack stays (a vendor sells from a stack that stays registered; a trade releases it: 0).
pub fn take_stack(
    inv: &mut ItemContainer,
    id: StorageId,
    n: u16,
    keep_reserved: u16,
    serials: &mut dyn SerialSource,
) -> Result<Taken, MoveError> {
    let have = inv
        .get_at_id(id)
        .map(|it| it.count)
        .filter(|&c| n >= 1 && n <= c)
        .ok_or(MoveError::Missing(id))?;
    if n == have {
        // the whole stack: the entry empties and the item keeps its serial
        let mut item = inv.remove_at_id(id).ok_or(MoveError::Missing(id))?;
        item.reserved = 0;
        item.vendor_price = 0;
        return Ok(Taken {
            from: id,
            left: 0,
            item,
        });
    }
    // a part: the moved piece is a new item (fresh serial), the rest stays under the old one
    let serial = serials.next_serial()?;
    let it = inv.get_at_id_mut(id).ok_or(MoveError::Missing(id))?;
    let mut item = it.clone();
    it.count = have - n;
    it.reserved = keep_reserved.min(it.count);
    if it.reserved == 0 {
        it.vendor_price = 0;
    }
    item.count = n;
    item.serial = ItemSerial(serial);
    item.reserved = 0;
    item.vendor_price = 0;
    Ok(Taken {
        from: id,
        left: have - n,
        item,
    })
}

/// Adds a taken stack to `inv` (`CItemContainer::Add`): `Full` when it does not fit, nothing changes then.
pub fn put_stack(
    inv: &mut ItemContainer,
    taken: &Taken,
    data: &GameData,
    serials: &mut dyn SerialSource,
) -> Result<Arrived, MoveError> {
    let plan = inv.add(&taken.item, data, serials).map_err(|e| match e {
        crate::item::container::AddError::Full | crate::item::container::AddError::NoProp => {
            MoveError::Full
        }
        crate::item::container::AddError::Serial(s) => MoveError::Serial(s),
    })?;
    let mut template = taken.item.clone();
    template.serial = plan.template_serial;
    Ok(Arrived {
        plan,
        template,
        count: taken.item.count,
    })
}

/// The `UpdateItem{UI_NUM}` a giver gets for a taken stack (what `remove_item` sends).
pub fn update_msg(t: &Taken) -> pb::UpdateItem {
    pb::UpdateItem {
        r#type: 0,
        obj_id: u32::from(t.from),
        param: UiParam::Num as i32,
        value: u32::from(t.left),
        time: 0,
    }
}

/// The `CreateItem` a receiver gets for an arrived stack (what `create_item` sends).
pub fn create_msg(a: &Arrived) -> pb::CreateItem {
    pb::CreateItem {
        item_type: 0,
        item: Some(a.template.to_pb(0)),
        obj_ids: a.plan.touched.iter().map(|t| u32::from(t.0)).collect(),
        nums: a.plan.touched.iter().map(|t| i32::from(t.1)).collect(),
    }
}
