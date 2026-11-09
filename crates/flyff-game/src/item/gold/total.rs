//! The total wealth of a player: penya plus Perins (Plan/19 §4.3, Plan/22 "gold.total_gold / remove_total_gold";
//! src:_Common/Mover.cpp:12138-12186, `CMover::GetTotalGold`, `RemovePerin`, `RemoveTotalGold`).
//!
//! A Perin is worth [`PERIN_VALUE`] penya. [`total_gold`] counts the usable Perin stacks of
//! the inventory (the bank is not counted) and the wallet; [`remove_total_gold`] takes an amount away, Perins
//! first, and gives the change back in penya when the last Perin was larger than the rest.

use super::PERIN_VALUE;
use super::core::{add_gold, gold, perin_count};
use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::ops::{UiParam, core as ops};
use crate::mover::Mover;

/// `GetTotalGold`: `Perins x PERIN_VALUE + penya` (usable Perins of the inventory only).
pub fn total_gold(m: &Mover) -> i64 {
    i64::from(perin_count(m)) * PERIN_VALUE + gold(m)
}

/// The usable Perin stacks of the inventory in storage order (`GetItemId( i )` for `i` in `0..GetMax()`).
fn perin_stacks(m: &Mover) -> Vec<(StorageId, u16)> {
    m.player().map_or_else(Vec::new, |p| {
        p.items
            .inventory
            .iter()
            .filter(|(_, it)| it.is_perin() && it.is_usable())
            .map(|(id, it)| (id, it.count))
            .collect()
    })
}

/// `RemovePerin(n)`: takes up to `n` Perins from the usable stacks in storage order (`UpdateItem(UI_NUM)`, an
/// emptied stack is deleted) and returns how many were taken.
pub fn remove_perin(m: &mut Mover, env: &ItemEnv<'_>, n: u32, fx: &mut Fx) -> u32 {
    let mut rest = n;
    for (id, count) in perin_stacks(m) {
        if rest == 0 {
            break;
        }
        let take = rest.min(u32::from(count));
        ops::update_item(m, env, id, UiParam::Num, u32::from(count) - take, 0, fx);
        rest -= take;
    }
    n - rest
}

/// `RemoveTotalGold(amount)` (the caller checked `amount <= total_gold(m)`; a negative amount takes nothing):
/// `amount / PERIN_VALUE` Perins go first; when the remaining penya exceed the wallet one more Perin is taken
/// and the difference is paid back in penya. Returns the number of Perins taken.
pub fn remove_total_gold(m: &mut Mover, env: &ItemEnv<'_>, amount: i64, fx: &mut Fx) -> u32 {
    if amount <= 0 {
        return 0;
    }
    let want = u32::try_from(amount / PERIN_VALUE).unwrap_or(u32::MAX);
    let taken = remove_perin(m, env, want, fx);
    let mut rest = amount - i64::from(taken) * PERIN_VALUE;
    let mut taken_total = taken;
    if rest > gold(m) {
        taken_total += remove_perin(m, env, 1, fx);
        rest -= PERIN_VALUE;
    }
    add_gold(m, env, -rest, true, fx);
    taken_total
}
