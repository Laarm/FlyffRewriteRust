//! The item index and `OnAfterLoadPropItem` (Plan/14 §5.4 F3, §6.3; ref/17 §7.2.1;
//! src:_Common/Project.cpp:4704-4785).
//!
//! `ItemIndex` holds the per-`IK3` lists (`m_itemKindAry`), their `GetMinIdx`/`GetMaxIdx` table
//! (`m_minMaxIdxAry`) and the display-name map (`m_mapII`).

use crate::props::item::ItemProp;
use crate::types::{IdTable, ItemId, NULL_ID};
use flyff_common::defs::itemkind;
use flyff_common::det::DetMap;
use serde::{Deserialize, Serialize};

/// `MAX_ITEM_KIND3`.
pub const MAX_ITEM_KIND3: usize = itemkind::MAX_ITEM_KIND3 as usize;
/// `MAX_UNIQUE_SIZE`: the number of rarity values per kind in the min/max table.
pub const MAX_UNIQUE_SIZE: usize = itemkind::MAX_UNIQUE_SIZE as usize;

/// Items grouped by `IK3`, sorted by rarity, with the rarity range table and the name map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemIndex {
    /// `m_itemKindAry[IK3]`: the item ids of each kind in ascending id order, then sorted by
    /// `item_rare` with the original's exchange sort (F3). Always [`MAX_ITEM_KIND3`] lists.
    pub by_kind3: Vec<Vec<ItemId>>,
    /// `m_minMaxIdxAry[IK3][rare]` = `(first, last)` positions in `by_kind3[IK3]`; `(-1, -1)` = none.
    #[serde(with = "min_max_serde")]
    min_max: Vec<(i32, i32)>,
    /// `m_mapII`: display name to id, **first wins** (GM commands). propItem only (the skill name maps are
    /// separate, F2).
    pub by_name: DetMap<String, ItemId>,
}

impl Default for ItemIndex {
    fn default() -> Self {
        Self {
            by_kind3: vec![Vec::new(); MAX_ITEM_KIND3],
            min_max: vec![(-1, -1); MAX_ITEM_KIND3 * MAX_UNIQUE_SIZE],
            by_name: DetMap::default(),
        }
    }
}

impl ItemIndex {
    fn slot(k3: u32, rare: u32) -> Option<usize> {
        let k3 = usize::try_from(k3).ok()?;
        let rare = usize::try_from(rare).ok()?;
        (k3 < MAX_ITEM_KIND3 && rare < MAX_UNIQUE_SIZE).then(|| k3 * MAX_UNIQUE_SIZE + rare)
    }

    /// `GetMinIdx(k3, rare)`: the first position of rarity `rare` in `by_kind3[k3]`; `-1` when there is
    /// none or `rare >= 400`.
    pub fn min_idx(&self, k3: u32, rare: u32) -> i32 {
        Self::slot(k3, rare)
            .and_then(|i| self.min_max.get(i))
            .map_or(-1, |m| m.0)
    }

    /// `GetMaxIdx(k3, rare)`: the last position of rarity `rare` in `by_kind3[k3]`; `-1` when there is
    /// none or `rare >= 400`.
    pub fn max_idx(&self, k3: u32, rare: u32) -> i32 {
        Self::slot(k3, rare)
            .and_then(|i| self.min_max.get(i))
            .map_or(-1, |m| m.1)
    }

    /// The items of kind `k3` (empty for an unknown kind).
    pub fn kind3(&self, k3: u32) -> &[ItemId] {
        usize::try_from(k3)
            .ok()
            .and_then(|i| self.by_kind3.get(i))
            .map_or(&[], Vec::as_slice)
    }

    /// Recomputes `min_max` from the (already sorted) `by_kind3` lists (`items` supplies the rarity).
    pub fn rebuild_min_max(&mut self, items: &IdTable<ItemProp>) {
        self.min_max = vec![(-1, -1); MAX_ITEM_KIND3 * MAX_UNIQUE_SIZE];
        for (k3, list) in self.by_kind3.iter().enumerate() {
            let mut cur = NULL_ID;
            for (j, id) in list.iter().enumerate() {
                let rare = items.get(*id).map_or(NULL_ID, |p| p.item_rare);
                let j = i32::try_from(j).unwrap_or(i32::MAX);
                if cur != rare {
                    cur = rare;
                    if cur != NULL_ID
                        && let Some(m) =
                            Self::slot(k3 as u32, cur).and_then(|i| self.min_max.get_mut(i))
                    {
                        *m = (j, j);
                    }
                } else if cur != NULL_ID
                    && let Some(m) =
                        Self::slot(k3 as u32, cur).and_then(|i| self.min_max.get_mut(i))
                {
                    m.1 = j;
                }
            }
        }
    }
}

/// Counters and findings of [`after_load_prop_item`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AfterLoadReport {
    /// Items whose `item_kind3` is `>= MAX_ITEM_KIND3` or whose `item_rare` is a valid-looking value
    /// `>= MAX_UNIQUE_SIZE` (the original overflowed its arrays): `(item id, reason)`. They are left
    /// out of the index.
    pub out_of_range: Vec<(ItemId, &'static str)>,
}

/// `CProject::OnAfterLoadPropItem` without `LoadExcept` (F3): in ascending id order `flag == NULL_ID`
/// becomes 0, the item joins `by_kind3[k3]`, `IK3_EVENTMAIN`/`IK3_BINDS` get `IP_FLAG_BINDS`, CS and pet
/// items get their configured price; then every kind list is sorted by rarity with the original's
/// non-stable exchange sort and the min/max table is filled.
pub fn after_load_prop_item(
    items: &mut IdTable<ItemProp>,
    index: &mut ItemIndex,
    cs_item_price: u32,
    pet_item_price: u32,
) -> AfterLoadReport {
    let mut report = AfterLoadReport::default();
    let ids: Vec<ItemId> = items.ids().to_vec();
    for id in ids {
        let Some(p) = items.get_mut(id) else { continue };
        if p.flag == NULL_ID {
            p.flag = 0;
        }
        if p.item_kind3 == NULL_ID {
            continue;
        }
        let k3 = p.item_kind3;
        if k3 as usize >= MAX_ITEM_KIND3 {
            report
                .out_of_range
                .push((id, "item_kind3 >= MAX_ITEM_KIND3"));
        } else if p.item_rare != NULL_ID && p.item_rare as usize >= MAX_UNIQUE_SIZE {
            report
                .out_of_range
                .push((id, "item_rare >= MAX_UNIQUE_SIZE"));
        } else if let Some(list) = index.by_kind3.get_mut(k3 as usize) {
            list.push(id);
        }
        if k3 == itemkind::IK3_EVENTMAIN || k3 == itemkind::IK3_BINDS {
            p.flag |= crate::props::item::IP_FLAG_BINDS;
        }
        // __ALL_CS_PRICE / __ALL_PET_PRICE
        if matches!(
            k3,
            itemkind::IK3_HAT | itemkind::IK3_CLOTH | itemkind::IK3_GLOVE | itemkind::IK3_SHOES
        ) {
            p.cost = cs_item_price;
        }
        if k3 == itemkind::IK3_PET {
            p.cost = pet_item_price;
        }
    }
    for list in &mut index.by_kind3 {
        exchange_sort_by_rare(list, items);
    }
    index.rebuild_min_max(items);
    report
}

/// The original's sort: `for j in 0..n-1 { for k in j+1..n { if rare[k] < rare[j] { swap(j, k) } } }`
/// (unsigned compare, so `NULL_ID` sorts last). **Not stable**; the result is part of the data
/// (the order of equal-rarity items decides GM and drop picks).
pub fn exchange_sort_by_rare(list: &mut [ItemId], items: &IdTable<ItemProp>) {
    let mut rare: Vec<u32> = list
        .iter()
        .map(|id| items.get(*id).map_or(NULL_ID, |p| p.item_rare))
        .collect();
    let n = list.len();
    for j in 0..n.saturating_sub(1) {
        for k in (j + 1)..n {
            if rare.get(k) < rare.get(j) {
                rare.swap(j, k);
                list.swap(j, k);
            }
        }
    }
}

/// `(k3, rare, first, last)` for the used slots only: the dense table has 80,000 entries.
mod min_max_serde {
    use super::{MAX_ITEM_KIND3, MAX_UNIQUE_SIZE};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[(i32, i32)], s: S) -> Result<S::Ok, S::Error> {
        let used: Vec<(usize, usize, i32, i32)> = v
            .iter()
            .enumerate()
            .filter(|(_, m)| **m != (-1, -1))
            .map(|(i, m)| (i / MAX_UNIQUE_SIZE, i % MAX_UNIQUE_SIZE, m.0, m.1))
            .collect();
        used.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(i32, i32)>, D::Error> {
        let used: Vec<(usize, usize, i32, i32)> = Vec::deserialize(d)?;
        let mut v = vec![(-1, -1); MAX_ITEM_KIND3 * MAX_UNIQUE_SIZE];
        for (k3, rare, a, b) in used {
            if k3 < MAX_ITEM_KIND3
                && rare < MAX_UNIQUE_SIZE
                && let Some(slot) = v.get_mut(k3 * MAX_UNIQUE_SIZE + rare)
            {
                *slot = (a, b);
            }
        }
        Ok(v)
    }
}
