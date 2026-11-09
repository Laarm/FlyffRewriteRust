//! The collector upgrade (Plan/19 §12.4, F-230; src:WORLDSERVER/ItemUpgrade.cpp:815-865): `II_GEN_TOO_COL_NORMAL`
//! (the event collector is not refinable) goes from `+0` to `+5` with a moonstone. Odds: `Collecting_Enchant` of
//! `s.txt`, out of 1000 (100 each); success when `r < p`; a failure has no penalty and the moonstone is consumed.
//! Legacy path only: the safe smelt has no collector branch.

use super::txn::{Outcome, UpgradeCx, audit, feedback, item_at, material, take_one};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{UiParam, core as ops};
use crate::mover::Mover;
use flyff_common::defs::text::{
    TID_GAME_MAX_COLLECTOR_LEVEL, TID_GAME_NOTEQUALITEM, TID_UPGRADE_FAIL, TID_UPGRADE_SUCCEEFUL,
};
use flyff_common::rng::GameRng;
use flyff_rules::item::refine::{SPACE_1000, succeeds_under};
use flyff_rules::item::smelt::is_moonstone;

/// `RefineCollector`: a material that is no moonstone (`TID_GAME_NOTEQUALITEM`), a collector at the last level
/// (`TID_GAME_MAX_COLLECTOR_LEVEL`); a level without odds returns silently and consumes nothing.
pub(super) fn legacy(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    mat: StorageId,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Outcome {
    let (Some(before), Some(mat_item)) = (item_at(m, target).cloned(), item_at(m, mat).cloned())
    else {
        return Outcome::Ignored;
    };
    if !is_moonstone(mat_item.item_id) {
        fx.text(TID_GAME_NOTEQUALITEM);
        return Outcome::Refused;
    }
    let ao = before.upgrade_level;
    if ao >= cx.tables.max_collector() {
        fx.text(TID_GAME_MAX_COLLECTOR_LEVEL);
        return Outcome::Refused;
    }
    let p = cx.tables.collector_prob(ao);
    if p == 0 {
        return Outcome::Ignored;
    }
    fx.mark_urgent();
    let (result, outcome) = if succeeds_under(rng.random_below(SPACE_1000), p) {
        fx.text(TID_UPGRADE_SUCCEEFUL);
        feedback(m, fx, true);
        ops::update_item(m, cx.env, target, UiParam::Ao, (ao + 1) as u32, 0, fx);
        ("success", Outcome::Success)
    } else {
        fx.text(TID_UPGRADE_FAIL);
        feedback(m, fx, false);
        ("failed", Outcome::Failed)
    };
    take_one(m, cx, mat, fx);
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "collector",
        "legacy",
        result,
        &before,
        after.as_ref(),
        vec![material(mat_item.item_id, 1)],
    );
    outcome
}
