//! Food and potion recovery (Plan/16 §6.3, F-121): the pure rule of `DoUseItemFood` for HP/MP/FP items
//! (`MoverSkill.cpp:3090-3150`). The regeneration timers and the kill recovery are the game crate's
//! (`flyff_game::stats::tick`).

use crate::consts::dst::{DST_FP, DST_HP, DST_MP};
use flyff_common::defs::text::{TID_GAME_LIMITFP, TID_GAME_LIMITHP, TID_GAME_LIMITMP};

/// The text id the potion rule shows ("this potion has no full effect").
pub type TextId = u32;

/// `DoUseItemFood` for one `HP`/`MP`/`FP` slot of the item: the value to hand to `SetPointParam` (the pool
/// setter clamps it later) and, in the legacy branch, the text of the 30 % limit.
///
/// * `dst`: `DST_HP`, `DST_MP` or `DST_FP` (selects the limit text);
/// * `cur`: the current pool (`GetPointParam`), `point` the item's `nAdjParamVal`, `max` the pool maximum,
///   `item_min` the item's `dwAbilityMin` (the "full effect up to" level of the legacy rule);
/// * `remove_max_recover_rate` (`features.remove_max_recover_rate`, true in the repack): no limit at all,
///   `cur + point`. With `false` the legacy rule: `point == -1` leaves the pool alone; at or above `item_min`
///   a potion that would not fill the pool only restores `(int)(point * 0.3f)` and shows the limit text;
///   below `item_min` it adds `point`, capped at `max`.
pub fn potion_restore(
    dst: u32,
    cur: i32,
    point: i32,
    max: i32,
    item_min: i32,
    remove_max_recover_rate: bool,
) -> (i32, Option<TextId>) {
    if remove_max_recover_rate {
        return (cur.wrapping_add(point), None);
    }
    if point == -1 {
        return (cur, None);
    }
    if cur >= item_min {
        if cur as f32 + (point as f32 * 0.3_f32) >= max as f32 {
            return (max, None);
        }
        let tid = match dst {
            DST_HP => Some(TID_GAME_LIMITHP),
            DST_MP => Some(TID_GAME_LIMITMP),
            DST_FP => Some(TID_GAME_LIMITFP),
            _ => None,
        };
        let reduced = (point as f32 * 0.3_f32) as i32;
        return (cur.wrapping_add(reduced), tid);
    }
    if cur.wrapping_add(point) >= max {
        (max, None)
    } else {
        (cur.wrapping_add(point), None)
    }
}
