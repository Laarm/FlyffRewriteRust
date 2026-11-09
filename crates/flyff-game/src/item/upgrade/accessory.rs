//! Accessory (jewelry) upgrades (Plan/19 §12.4, F-230; src:WORLDSERVER/ItemUpgrade.cpp:585-865): the legacy
//! `RefineAccessory` and the safe `SmeltSafetyAccessory`.
//!
//! The six accessories of `accessory.inc` go from `+0` to `+20` with a moonstone. Odds: `Accessory_Probability`
//! of `s.txt` (out of 10 000). **Legacy**: success when `r < p` (strictly), `II_SYS_SYS_SCR_SMELPROT4` (a
//! buff) protects, a failure at `+3` or more without protection destroys the accessory; the moonstone is always
//! consumed. **Safe**: the protection item must be `II_SYS_SYS_SCR_SMELPROT4`, the attempt fails when `r > p`,
//! a failure keeps the item. No language penalty and no smelting scroll on either path.

use super::refine::{has_item_buff, remove_item_buff};
use super::txn::{
    Outcome, UpgradeCx, audit, buff_material, destroy, distinct, feedback, item_at, material,
    take_one,
};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{UiParam, core as ops};
use crate::mover::Mover;
use flyff_common::defs::item::II_SYS_SYS_SCR_SMELPROT4;
use flyff_common::defs::text::{
    TID_GAME_ACCESSORY_MAX_AAO, TID_GAME_NOTEQUALITEM, TID_UPGRADE_FAIL, TID_UPGRADE_SUCCEEFUL,
};
use flyff_common::rng::GameRng;
use flyff_rules::item::refine::{
    MAX_ACCESSORY_AO, SPACE_10000, fails_over, legacy_failure_destroys, succeeds_under,
};
use flyff_rules::item::smelt::is_moonstone;

/// `RefineAccessory`: the material must be a moonstone (`TID_GAME_NOTEQUALITEM`), the accessory below `+20`
/// (`TID_GAME_ACCESSORY_MAX_AAO`).
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
    if ao >= MAX_ACCESSORY_AO {
        fx.text(TID_GAME_ACCESSORY_MAX_AAO);
        return Outcome::Refused;
    }
    let p = cx.tables.accessory_prob(ao);
    let mut materials = Vec::new();
    let protected = has_item_buff(m, II_SYS_SYS_SCR_SMELPROT4);
    if protected {
        remove_item_buff(m, cx, II_SYS_SYS_SCR_SMELPROT4, fx);
        materials.push(buff_material(II_SYS_SYS_SCR_SMELPROT4));
    }
    materials.push(material(mat_item.item_id, 1));
    fx.mark_urgent();
    let (result, outcome) = if succeeds_under(rng.random_below(SPACE_10000), p) {
        fx.text(TID_UPGRADE_SUCCEEFUL);
        feedback(m, fx, true);
        ops::update_item(m, cx.env, target, UiParam::Ao, (ao + 1) as u32, 0, fx);
        ("success", Outcome::Success)
    } else {
        fx.text(TID_UPGRADE_FAIL);
        feedback(m, fx, false);
        if legacy_failure_destroys(ao, protected) {
            destroy(m, cx, target, fx);
            ("destroyed", Outcome::Destroyed)
        } else {
            ("failed", Outcome::Failed)
        }
    };
    take_one(m, cx, mat, fx);
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "accessory",
        "legacy",
        result,
        &before,
        after.as_ref(),
        materials,
    );
    outcome
}

/// `SmeltSafetyAccessory`: the answer byte. A material that is no moonstone or a protection that is not
/// `II_SYS_SYS_SCR_SMELPROT4` answers 0, `+20` answers 3.
pub(super) fn safe(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    mat: StorageId,
    protection: StorageId,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> u8 {
    if !distinct(&[target, mat, protection]) {
        return 0;
    }
    let (Some(before), Some(mat_item), Some(prot_item)) = (
        item_at(m, target).cloned(),
        item_at(m, mat).cloned(),
        item_at(m, protection).cloned(),
    ) else {
        return 0;
    };
    if !is_moonstone(mat_item.item_id) || prot_item.item_id != II_SYS_SYS_SCR_SMELPROT4 {
        return 0;
    }
    let ao = before.upgrade_level;
    if ao >= MAX_ACCESSORY_AO {
        return 3;
    }
    fx.mark_urgent();
    take_one(m, cx, mat, fx);
    take_one(m, cx, protection, fx);
    let materials = vec![
        material(mat_item.item_id, 1),
        material(prot_item.item_id, 1),
    ];
    let p = cx.tables.accessory_prob(ao);
    let (code, result) = if fails_over(rng.random_below(SPACE_10000), p) {
        feedback(m, fx, false);
        (2, "failed")
    } else {
        feedback(m, fx, true);
        ops::update_item(m, cx.env, target, UiParam::Ao, (ao + 1) as u32, 0, fx);
        (1, "success")
    };
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "accessory",
        "safe",
        result,
        &before,
        after.as_ref(),
        materials,
    );
    code
}
