//! Vendor stock generation (Plan/14 §6.8.2, fix-up F7): the exact port of `CMover::ProcessRegenItem` and
//! `CMover::GenerateVendorItem` (src:_Common/Mover.cpp:1350-1450, 4709-4756; ref/13 §12.1).
//!
//! The original builds each NPC's four 100-slot shop containers on the NPC's first `Process()`; the result
//! depends only on loaded data (never on a player or a clock), so the loader precomputes it once, after
//! every character file and its `AddShopItem` price writes are in. Stock is **infinite**: buying never
//! decrements it (ref/13 §12.1).
//!
//! Per tab, in this order:
//!
//! 1. **Chip shop** (`vendor_type == 1`): every `AddVendorItem2` item becomes a full stack, except items
//!    whose chip price `refer_value1` is `< 1` as a signed value (Error `ChipCostInvalid`, skipped).
//!    A chip shop never uses its `AddVendorItem` rules (the original's `else` branch).
//! 2. **Any other type**: when the tab has rules, `GenerateVendorItem` collects the items of every rule
//!    (cap [`MAX_VENDOR_INVENTORY`] over the whole tab), the list is sorted with the original's exchange sort
//!    by `(item_kind1, item_rare)` and each item becomes a full stack.
//! 3. The `AddShopItem` items are appended in order until the container is full. A missing item is an Error
//!    (`ShopItemMissing`; the original dereferenced NULL).
//!
//! A *full stack* is `count = (short)pack_max`, `hit_point = endurance`. `CItemContainer::Add` stores nothing
//! for a count `<= 0`, so such an item adds no entry (and does not stop the generation).

use crate::character::{
    MAX_VENDOR_INVENTORY, MAX_VENDOR_INVENTORY_TAB, VendorDef, VendorRule, VendorStockEntry,
};
use crate::props::item::ItemProp;
use crate::props::item_index::{ItemIndex, MAX_UNIQUE_SIZE};
use crate::types::{IdTable, NULL_ID};
use flyff_script::{DiagCode, Diagnostics, Severity};
use std::sync::Arc;

/// Where diagnostics of one NPC are filed.
struct Report<'a> {
    key: &'a str,
    file: Option<&'a Arc<str>>,
}

impl Report<'_> {
    fn emit(&self, diags: &mut Diagnostics, sev: Severity, code: DiagCode, msg: String) {
        diags.report(sev, code, self.file, 0, format!("{}: {msg}", self.key));
    }
}

/// `CMover::GenerateVendorItem` for one rule: appends the matching items to `list` (at most `max` in
/// total). `None` of the rarity range finds an item: Warning `VendorRuleEmpty`.
fn generate_rule<'a>(
    list: &mut Vec<&'a ItemProp>,
    max: usize,
    rule: &VendorRule,
    items: &'a IdTable<ItemProp>,
    index: &ItemIndex,
    rep: &Report<'_>,
    diags: &mut Diagnostics,
) {
    if list.len() >= max {
        return;
    }
    // `GetMinIdx(ik3, j)` / `GetMaxIdx` are -1 for a rarity >= MAX_UNIQUE_SIZE, and a negative `j` becomes a
    // huge DWORD, so only 0..MAX_UNIQUE_SIZE can ever hit: the loops are clamped to that range.
    let ik3 = rule.ik3 as u32;
    let top = i32::try_from(MAX_UNIQUE_SIZE).map_or(i32::MAX, |m| m - 1);
    let lo = rule.uniq_min.max(0);
    let hi = rule.uniq_max.min(top);
    let min_idx = (lo..=hi)
        .map(|j| index.min_idx(ik3, j as u32))
        .find(|i| *i != -1)
        .unwrap_or(-1);
    let max_idx = (lo..=hi)
        .rev()
        .map(|j| index.max_idx(ik3, j as u32))
        .find(|i| *i != -1)
        .unwrap_or(-1);
    if min_idx < 0 {
        rep.emit(
            diags,
            Severity::Warning,
            DiagCode::VendorRuleEmpty,
            format!(
                "AddVendorItem rule IK3 {} rarity {}-{} finds no item",
                rule.ik3, rule.uniq_min, rule.uniq_max
            ),
        );
        return;
    }
    let kind = index.kind3(ik3);
    let (Ok(first), Ok(last)) = (usize::try_from(min_idx), usize::try_from(max_idx)) else {
        return;
    };
    for k in first..=last {
        let Some(p) = kind.get(k).and_then(|id| items.get(*id)) else {
            continue;
        };
        if p.shop_able == NULL_ID || (rule.job != -1 && p.item_job != rule.job as u32) {
            continue;
        }
        if list.len() >= max {
            break;
        }
        list.push(p);
    }
}

/// The original's sort of the generated list: an exchange sort (not stable) by `item_kind1`, then
/// `item_rare`, compared as unsigned values.
fn exchange_sort(list: &mut [&ItemProp]) {
    let n = list.len();
    for j in 0..n.saturating_sub(1) {
        for k in (j + 1)..n {
            let (Some(a), Some(b)) = (list.get(k), list.get(j)) else {
                continue;
            };
            if a.item_kind1 < b.item_kind1
                || (a.item_kind1 == b.item_kind1 && a.item_rare < b.item_rare)
            {
                list.swap(j, k);
            }
        }
    }
}

fn full_stack(p: &ItemProp) -> Option<VendorStockEntry> {
    let count = p.pack_max as i16;
    (count >= 1).then_some(VendorStockEntry {
        item: p.id,
        count,
        hit_point: p.endurance,
    })
}

fn push_stack(out: &mut Vec<VendorStockEntry>, p: &ItemProp) {
    if let Some(e) = full_stack(p) {
        out.push(e);
    }
}

/// The stock of one NPC (four tabs). `file` is only used to tag the diagnostics.
///
/// Errors: `ShopItemMissing` (a missing `AddVendorItem2`/`AddShopItem` item) and `ChipCostInvalid`; Warning
/// `VendorRuleEmpty` for a rule that finds nothing.
pub fn generate_vendor_stock(
    npc_key: &str,
    vendor: &VendorDef,
    items: &IdTable<ItemProp>,
    index: &ItemIndex,
    file: Option<&Arc<str>>,
    diags: &mut Diagnostics,
) -> [Vec<VendorStockEntry>; MAX_VENDOR_INVENTORY_TAB] {
    let rep = Report { key: npc_key, file };
    let mut result: [Vec<VendorStockEntry>; MAX_VENDOR_INVENTORY_TAB] = Default::default();
    for (tab, slot_out) in vendor.tabs.iter().zip(result.iter_mut()) {
        let mut out: Vec<VendorStockEntry> = Vec::new();
        if vendor.vendor_type == 1 {
            // `CItemContainer::Add` ignores what does not fit (the 100-slot cap).
            for &id in &tab.chip_items {
                let Some(p) = items.get(id) else {
                    rep.emit(
                        diags,
                        Severity::Error,
                        DiagCode::ShopItemMissing,
                        format!("AddVendorItem2 item {id} does not exist"),
                    );
                    continue;
                };
                if (p.refer_value1 as i32) < 1 {
                    rep.emit(
                        diags,
                        Severity::Error,
                        DiagCode::ChipCostInvalid,
                        format!(
                            "chip cost < 1: item {id}, refer_value1 = {}",
                            p.refer_value1 as i32
                        ),
                    );
                    continue;
                }
                if out.len() < MAX_VENDOR_INVENTORY {
                    push_stack(&mut out, p);
                }
            }
        } else if !tab.rules.is_empty() {
            let mut list: Vec<&ItemProp> = Vec::new();
            for rule in &tab.rules {
                generate_rule(
                    &mut list,
                    MAX_VENDOR_INVENTORY,
                    rule,
                    items,
                    index,
                    &rep,
                    diags,
                );
            }
            exchange_sort(&mut list);
            for p in list {
                if out.len() >= MAX_VENDOR_INVENTORY {
                    break;
                }
                push_stack(&mut out, p);
            }
        }
        for e in &tab.shop_items {
            let Some(p) = items.get(e.item) else {
                rep.emit(
                    diags,
                    Severity::Error,
                    DiagCode::ShopItemMissing,
                    format!("AddShopItem item {} does not exist", e.item),
                );
                continue;
            };
            if out.len() >= MAX_VENDOR_INVENTORY {
                break; // `Add` returned FALSE: the container is full
            }
            push_stack(&mut out, p);
        }
        *slot_out = out;
    }
    result
}
