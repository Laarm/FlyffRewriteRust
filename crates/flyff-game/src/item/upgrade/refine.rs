//! The general refine +1..+10 (Plan/19 §12.2, F-227; src:WORLDSERVER/ItemUpgrade.cpp:971-1129, safe 482-583;
//! KQ-12): the legacy `EnchantGeneral` and the safe `SmeltSafetyGeneral`.
//!
//! Odds: `p = tGeneral[AO]` with the ENG penalty from the fourth refine (`flyff_rules::item::refine`); the roll
//! is `random_below(10000)` and the attempt fails when `r > p`. **Legacy**: the protection and smelting
//! *buffs* (`II_SYS_SYS_SCR_SMELPROT`, `SMELTING`, `SUPERSMELTING`) are consumed when present; a failure at
//! `+3` or more without `SMELPROT` destroys the target. **Safe**: the protection is an inventory item that must
//! be `II_SYS_SYS_SCR_SMELPROT`, the optional smelting scroll must be `II_SYS_SYS_SCR_SMELTING`; a failure
//! never destroys anything. No penya cost; a failure never lowers the level.

use super::txn::{
    Outcome, UpgradeCx, audit, buff_material, destroy, distinct, feedback, item_at, material,
    take_one,
};
use crate::buff::core::{BuffCx, remove_fake};
use crate::buff::set::{BuffKey, BuffKind};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{UiParam, core as ops};
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_SMELPROT, II_SYS_SYS_SCR_SMELTING, II_SYS_SYS_SCR_SUPERSMELTING,
};
use flyff_common::defs::text::{
    TID_GAME_NOTEQUALITEM, TID_UPGRADE_FAIL, TID_UPGRADE_MAXOVER, TID_UPGRADE_SUCCEEFUL,
};
use flyff_common::rng::GameRng;
use flyff_rules::item::refine::{
    GENERAL_BONUS_BELOW, SMELTING_BONUS, SPACE_10000, SUPER_SMELTING_BONUS, fails_over,
    legacy_failure_destroys,
};
use flyff_rules::item::smelt::{is_dice_refinery_able, is_orichalcum, is_ultimate_weapon};

/// The legacy buff modifiers a refine consumed (the audit line lists them).
#[derive(Default)]
pub(super) struct BuffMods {
    /// `II_SYS_SYS_SCR_SMELPROT` (or the path's protection) was active.
    pub protected: bool,
    /// The odds bonus the smelting buffs gave.
    pub bonus: i32,
    /// The consumed buff items.
    pub used: Vec<u32>,
}

/// `HasBuff( BUFF_ITEM, id )` for the upgrade code. **FIX**: a buff that was removed in the legacy "fake" way
/// (its effects reverted, the entry waiting for the mover's next buff pass) is gone. The original still saw it
/// until that pass, so one protection or smelting scroll could cover several quick attempts.
pub(super) fn has_item_buff(m: &Mover, id: u32) -> bool {
    m.buffs
        .get(BuffKey::new(BuffKind::Item, id))
        .is_some_and(|e| !e.removed)
}

/// `RemoveBuff( BUFF_ITEM, id )` (the legacy fake removal).
pub(super) fn remove_item_buff(m: &mut Mover, cx: &UpgradeCx<'_>, id: u32, fx: &mut Fx) {
    let bcx = BuffCx::new(*cx.env, cx.now);
    remove_fake(m, &bcx, BuffKey::new(BuffKind::Item, id), fx);
}

/// The buffs the legacy general refine reads and consumes, in the original's order: `SMELPROT` (protects,
/// removed whenever present), `SMELTING` (`+1000` and removed only below `+7`), `SUPERSMELTING` (`+10000`,
/// likewise).
fn take_general_buffs(m: &mut Mover, cx: &UpgradeCx<'_>, ao: i32, fx: &mut Fx) -> BuffMods {
    let mut mods = BuffMods::default();
    if has_item_buff(m, II_SYS_SYS_SCR_SMELPROT) {
        mods.protected = true;
        mods.used.push(II_SYS_SYS_SCR_SMELPROT);
        remove_item_buff(m, cx, II_SYS_SYS_SCR_SMELPROT, fx);
    }
    if ao < GENERAL_BONUS_BELOW && has_item_buff(m, II_SYS_SYS_SCR_SMELTING) {
        mods.bonus += SMELTING_BONUS;
        mods.used.push(II_SYS_SYS_SCR_SMELTING);
        remove_item_buff(m, cx, II_SYS_SYS_SCR_SMELTING, fx);
    }
    if ao < GENERAL_BONUS_BELOW && has_item_buff(m, II_SYS_SYS_SCR_SUPERSMELTING) {
        mods.bonus += SUPER_SMELTING_BONUS;
        mods.used.push(II_SYS_SYS_SCR_SUPERSMELTING);
        remove_item_buff(m, cx, II_SYS_SYS_SCR_SUPERSMELTING, fx);
    }
    mods
}

/// `EnchantGeneral` (legacy `Enchant` with an Orichalcum material). The caller checked: both items usable,
/// different, the target not equipped, no commercial stone. Texts: `TID_GAME_NOTEQUALITEM` (an ultimate weapon,
/// a material that is not Orichalcum01, a target that is not refinable), `TID_UPGRADE_MAXOVER` (`+10`),
/// then `TID_UPGRADE_SUCCEEFUL` / `TID_UPGRADE_FAIL`.
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
    let Some(tp) = cx.prop(before.item_id) else {
        return Outcome::Ignored;
    };
    if is_ultimate_weapon(tp) || !is_orichalcum(mat_item.item_id) || !is_dice_refinery_able(tp) {
        fx.text(TID_GAME_NOTEQUALITEM);
        return Outcome::Refused;
    }
    let ao = before.upgrade_level;
    if ao >= cx.tables.max_general() {
        fx.text(TID_UPGRADE_MAXOVER);
        return Outcome::Refused;
    }
    let mut p = cx.tables.general_prob(ao);
    let mods = take_general_buffs(m, cx, ao, fx);
    p = p.saturating_add(mods.bonus);
    let mut materials: Vec<_> = mods.used.iter().map(|&b| buff_material(b)).collect();
    materials.push(material(mat_item.item_id, 1));
    fx.mark_urgent();
    let (result, outcome) = if fails_over(rng.random_below(SPACE_10000), p) {
        fx.text(TID_UPGRADE_FAIL);
        feedback(m, fx, false);
        if legacy_failure_destroys(ao, mods.protected) {
            destroy(m, cx, target, fx);
            ("destroyed", Outcome::Destroyed)
        } else {
            ("failed", Outcome::Failed)
        }
    } else {
        fx.text(TID_UPGRADE_SUCCEEFUL);
        feedback(m, fx, true);
        ops::update_item(m, cx.env, target, UiParam::Ao, (ao + 1) as u32, 0, fx);
        ("success", Outcome::Success)
    };
    take_one(m, cx, mat, fx);
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "refine",
        "legacy",
        result,
        &before,
        after.as_ref(),
        materials,
    );
    outcome
}

/// `SmeltSafetyGeneral` + `SmeltSafetyNormal`: the answer byte of `SmeltSafetyNtf` (0 refused, 1 success, 2
/// failed, 3 at the maximum). `protection` is the inventory entry the client named; `smelt` the optional
/// smelting scroll (already resolved to a usable entry or `None`).
#[allow(clippy::too_many_arguments)]
pub(super) fn safe(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    mat: StorageId,
    protection: StorageId,
    smelt: Option<StorageId>,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> u8 {
    let ids = [Some(target), Some(mat), Some(protection), smelt];
    let ids: Vec<StorageId> = ids.into_iter().flatten().collect();
    if !distinct(&ids) {
        return 0;
    }
    let (Some(before), Some(mat_item), Some(prot_item)) = (
        item_at(m, target).cloned(),
        item_at(m, mat).cloned(),
        item_at(m, protection).cloned(),
    ) else {
        return 0;
    };
    let Some(tp) = cx.prop(before.item_id) else {
        return 0;
    };
    if !is_dice_refinery_able(tp) {
        return 0;
    }
    if is_ultimate_weapon(tp) || prot_item.item_id != II_SYS_SYS_SCR_SMELPROT {
        return 0;
    }
    let ao = before.upgrade_level;
    if ao >= cx.tables.max_general() {
        return 3;
    }
    let mut p = cx.tables.general_prob(ao);
    let mut consume_smelt = None;
    if let Some(sid) = smelt {
        match item_at(m, sid) {
            Some(s) if s.item_id == II_SYS_SYS_SCR_SMELTING => {
                if ao < GENERAL_BONUS_BELOW {
                    p = p.saturating_add(SMELTING_BONUS);
                    consume_smelt = Some((sid, s.item_id));
                }
            }
            _ => return 0,
        }
    }
    fx.mark_urgent();
    let mut materials = Vec::new();
    if let Some((sid, item_id)) = consume_smelt {
        take_one(m, cx, sid, fx);
        materials.push(material(item_id, 1));
    }
    take_one(m, cx, mat, fx);
    materials.push(material(mat_item.item_id, 1));
    take_one(m, cx, protection, fx);
    materials.push(material(prot_item.item_id, 1));
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
        "refine",
        "safe",
        result,
        &before,
        after.as_ref(),
        materials,
    );
    code
}
