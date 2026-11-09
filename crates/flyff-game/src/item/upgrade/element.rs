//! Element upgrade, change and removal (Plan/19 §12.3, F-229; src:WORLDSERVER/ItemUpgrade.cpp:868-967,
//! 1131-1362, DPSrvr.cpp:6079-6168; PL-067, VR-022).
//!
//! * `legacy` (`EnchantAttribute`): an element card raises the element level by one; texts as the original's.
//! * `safe` (`SmeltSafetyAttribute`): the same with inventory protection (**FIX VR-022**: the protection must
//!   be `II_SYS_SYS_SCR_SMELPROT`, the original consumed any usable item) and the optional `SMELTING2` scroll.
//! * [`on_change`]: `ChangeAttribute` with `II_SYS_SYS_SCR_SOKCHANG` keeps the level.
//! * [`on_remove`]: `RemoveAttributeReq` for 100 000 penya.

use super::refine::{BuffMods, has_item_buff, remove_item_buff};
use super::txn::{
    Outcome, UpgradeCx, audit, buff_material, destroy, distinct, equipped, feedback, item_at,
    material, pay, refuse_stone, take_one, usable,
};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::Element;
use crate::item::ops::{UiParam, core as ops};
use crate::item::usage::UseFacts;
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_SMELPROT, II_SYS_SYS_SCR_SMELTING2, II_SYS_SYS_SCR_SOKCHANG,
    II_SYS_SYS_SCR_SUPERSMELTING,
};
use flyff_common::defs::text::{
    TID_GAME_EQUIPPUT, TID_GAME_NOCHANGE_SAME_ATTRIBUTE, TID_GAME_NOTELEMENT,
    TID_GAME_NOTEQUALITEM, TID_UPGRADE_ERROR_TWOELEMENT, TID_UPGRADE_ERROR_WRONGUPLEVEL,
    TID_UPGRADE_FAIL, TID_UPGRADE_MAXOVER, TID_UPGRADE_SUCCEEFUL,
};
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::item::refine::{
    ELEMENT_BONUS_BELOW, GENERAL_BONUS_BELOW, REMOVE_ELEMENT_COST, SMELTING_BONUS, SPACE_10000,
    SUPER_SMELTING_BONUS, fails_over, legacy_failure_destroys,
};
use flyff_rules::item::smelt::{ELEMENT_END, is_ele_refinery_able, is_the_element_card};

/// The buffs the legacy element upgrade consumes: `SMELPROT` (protects), `SMELTING2` (`+1000` below level
/// 10), `SUPERSMELTING` (`+10000` below level 7).
fn take_buffs(m: &mut Mover, cx: &UpgradeCx<'_>, level: i32, fx: &mut Fx) -> BuffMods {
    let mut mods = BuffMods::default();
    if has_item_buff(m, II_SYS_SYS_SCR_SMELPROT) {
        mods.protected = true;
        mods.used.push(II_SYS_SYS_SCR_SMELPROT);
        remove_item_buff(m, cx, II_SYS_SYS_SCR_SMELPROT, fx);
    }
    if level < ELEMENT_BONUS_BELOW && has_item_buff(m, II_SYS_SYS_SCR_SMELTING2) {
        mods.bonus += SMELTING_BONUS;
        mods.used.push(II_SYS_SYS_SCR_SMELTING2);
        remove_item_buff(m, cx, II_SYS_SYS_SCR_SMELTING2, fx);
    }
    if level < GENERAL_BONUS_BELOW && has_item_buff(m, II_SYS_SYS_SCR_SUPERSMELTING) {
        mods.bonus += SUPER_SMELTING_BONUS;
        mods.used.push(II_SYS_SYS_SCR_SUPERSMELTING);
        remove_item_buff(m, cx, II_SYS_SYS_SCR_SUPERSMELTING, fx);
    }
    mods
}

/// `EnchantAttribute` (legacy `Enchant` with an `IK3_ELECARD` material). Checks in the original's order: a
/// different element (`TID_UPGRADE_ERROR_TWOELEMENT`), a target that cannot take an element
/// (`TID_GAME_NOTEQUALITEM`), a card that is not **the** card of its element (`TID_UPGRADE_ERROR_WRONGUPLEVEL`),
/// level 20 (`TID_UPGRADE_MAXOVER`).
pub(super) fn legacy(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    card: StorageId,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Outcome {
    let (Some(before), Some(card_item)) = (item_at(m, target).cloned(), item_at(m, card).cloned())
    else {
        return Outcome::Ignored;
    };
    let (Some(tp), Some(cp)) = (cx.prop(before.item_id), cx.prop(card_item.item_id)) else {
        return Outcome::Ignored;
    };
    if before.element != Element::None && before.element as u32 != cp.item_type {
        fx.text(TID_UPGRADE_ERROR_TWOELEMENT);
        return Outcome::Refused;
    }
    if !is_ele_refinery_able(tp) {
        fx.text(TID_GAME_NOTEQUALITEM);
        return Outcome::Refused;
    }
    if !is_the_element_card(cp) {
        fx.text(TID_UPGRADE_ERROR_WRONGUPLEVEL);
        return Outcome::Refused;
    }
    let level = before.element_level;
    if level >= cx.tables.max_attribute() {
        fx.text(TID_UPGRADE_MAXOVER);
        return Outcome::Refused;
    }
    let p = cx.tables.attribute_prob(level);
    let mods = take_buffs(m, cx, level, fx);
    let p = p.saturating_add(mods.bonus);
    let mut materials: Vec<_> = mods.used.iter().map(|&b| buff_material(b)).collect();
    materials.push(material(card_item.item_id, 1));
    fx.mark_urgent();
    let (result, outcome) = if fails_over(rng.random_below(SPACE_10000), p) {
        fx.text(TID_UPGRADE_FAIL);
        feedback(m, fx, false);
        if legacy_failure_destroys(level, mods.protected) {
            destroy(m, cx, target, fx);
            ("destroyed", Outcome::Destroyed)
        } else {
            ("failed", Outcome::Failed)
        }
    } else {
        fx.text(TID_UPGRADE_SUCCEEFUL);
        feedback(m, fx, true);
        ops::update_item(m, cx.env, target, UiParam::Ir, cp.item_type, 0, fx);
        ops::update_item(m, cx.env, target, UiParam::Rao, (level + 1) as u32, 0, fx);
        ("success", Outcome::Success)
    };
    take_one(m, cx, card, fx);
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "element",
        "legacy",
        result,
        &before,
        after.as_ref(),
        materials,
    );
    outcome
}

/// `SmeltSafetyAttribute`: the answer byte of `SmeltSafetyNtf`. The checks that answer 0 come before anything
/// is consumed: the target cannot take an element, a different element, not the card of its element, a
/// protection that is not `II_SYS_SYS_SCR_SMELPROT` (**VR-022**); level 20 answers 3; a smelting scroll that
/// is not `II_SYS_SYS_SCR_SMELTING2` answers 0.
#[allow(clippy::too_many_arguments)]
pub(super) fn safe(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    card: StorageId,
    protection: StorageId,
    smelt: Option<StorageId>,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> u8 {
    let ids: Vec<StorageId> = [Some(target), Some(card), Some(protection), smelt]
        .into_iter()
        .flatten()
        .collect();
    if !distinct(&ids) {
        return 0;
    }
    let (Some(before), Some(card_item), Some(prot_item)) = (
        item_at(m, target).cloned(),
        item_at(m, card).cloned(),
        item_at(m, protection).cloned(),
    ) else {
        return 0;
    };
    let (Some(tp), Some(cp)) = (cx.prop(before.item_id), cx.prop(card_item.item_id)) else {
        return 0;
    };
    if !is_ele_refinery_able(tp) {
        return 0;
    }
    if before.element != Element::None && before.element as u32 != cp.item_type {
        return 0;
    }
    if !is_the_element_card(cp) {
        return 0;
    }
    if prot_item.item_id != II_SYS_SYS_SCR_SMELPROT {
        return 0;
    }
    let level = before.element_level;
    if level >= cx.tables.max_attribute() {
        return 3;
    }
    let mut p = cx.tables.attribute_prob(level);
    let mut consume_smelt = None;
    if let Some(sid) = smelt {
        match item_at(m, sid) {
            Some(s) if s.item_id == II_SYS_SYS_SCR_SMELTING2 => {
                if level < ELEMENT_BONUS_BELOW {
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
    take_one(m, cx, card, fx);
    materials.push(material(card_item.item_id, 1));
    take_one(m, cx, protection, fx);
    materials.push(material(prot_item.item_id, 1));
    let (code, result) = if fails_over(rng.random_below(SPACE_10000), p) {
        feedback(m, fx, false);
        (2, "failed")
    } else {
        feedback(m, fx, true);
        ops::update_item(m, cx.env, target, UiParam::Ir, cp.item_type, 0, fx);
        ops::update_item(m, cx.env, target, UiParam::Rao, (level + 1) as u32, 0, fx);
        (1, "success")
    };
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "element",
        "safe",
        result,
        &before,
        after.as_ref(),
        materials,
    );
    code
}

/// `ChangeAttribute{target, material, attribute}` (ItemUpgrade.cpp:1298-1362). Silent unless the original
/// answered: a worn target (`TID_GAME_EQUIPPUT`), the same element (`TID_GAME_NOCHANGE_SAME_ATTRIBUTE`), an item
/// without an element (`TID_GAME_NOTELEMENT`). Success keeps the level, plays the success sound and effect and
/// consumes the scroll.
pub fn on_change(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::ChangeAttribute,
    facts: &UseFacts,
    fx: &mut Fx,
) -> Outcome {
    if facts.trading || facts.vending {
        return Outcome::Ignored;
    }
    let (Some(target), Some(scroll)) = (usable(m, msg.item_obj_id), usable(m, msg.material_obj_id))
    else {
        return Outcome::Ignored;
    };
    if !distinct(&[target, scroll]) {
        return Outcome::Ignored;
    }
    if equipped(m, target) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let (Some(before), Some(scroll_item)) =
        (item_at(m, target).cloned(), item_at(m, scroll).cloned())
    else {
        return Outcome::Ignored;
    };
    let Some(tp) = cx.prop(before.item_id) else {
        return Outcome::Ignored;
    };
    if !is_ele_refinery_able(tp) || scroll_item.item_id != II_SYS_SYS_SCR_SOKCHANG {
        return Outcome::Ignored;
    }
    let attr = msg.attribute;
    if attr <= 0 || attr as u32 >= ELEMENT_END {
        return Outcome::Ignored;
    }
    if before.element as i32 == attr {
        fx.text(TID_GAME_NOCHANGE_SAME_ATTRIBUTE);
        return Outcome::Refused;
    }
    if before.element == Element::None || before.element_level <= 0 {
        fx.text(TID_GAME_NOTELEMENT);
        return Outcome::Refused;
    }
    fx.mark_urgent();
    feedback(m, fx, true);
    ops::update_item(m, cx.env, target, UiParam::Ir, attr as u32, 0, fx);
    take_one(m, cx, scroll, fx);
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "element_change",
        "scroll",
        "success",
        &before,
        after.as_ref(),
        vec![material(scroll_item.item_id, 1)],
    );
    Outcome::Success
}

/// `RemoveAttributeReq{obj_id}` (DPSrvr.cpp:6079-6151): 100 000 penya clear the element and its level; every
/// refusal answers `RemoveAttributeNtf{false}`, some with a text first: a target that cannot take an element
/// (`TID_GAME_NOTEQUALITEM`), a commercial stone (`TID_GAME_NOTUPGRADE`), a worn item (`TID_GAME_EQUIPPUT`), too
/// little penya (`TID_GAME_LACKMONEY`). The Ntf follows the text.
pub fn on_remove(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::RemoveAttributeReq,
    facts: &UseFacts,
    fx: &mut Fx,
) -> Outcome {
    let nope = |fx: &mut Fx| fx.out.to_self(pb::RemoveAttributeNtf { success: false });
    if facts.trading || facts.vending {
        nope(fx);
        return Outcome::Ignored;
    }
    let Some(id) = usable(m, msg.item_obj_id) else {
        nope(fx);
        return Outcome::Ignored;
    };
    let Some(before) = item_at(m, id).cloned() else {
        nope(fx);
        return Outcome::Ignored;
    };
    let Some(tp) = cx.prop(before.item_id) else {
        nope(fx);
        return Outcome::Ignored;
    };
    if !is_ele_refinery_able(tp) {
        nope(fx);
        fx.text(TID_GAME_NOTEQUALITEM);
        return Outcome::Refused;
    }
    if before.element_stone_id != 0 {
        nope(fx);
        refuse_stone(fx);
        return Outcome::Refused;
    }
    if equipped(m, id) {
        nope(fx);
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    if crate::item::gold::core::gold(m) < REMOVE_ELEMENT_COST {
        nope(fx);
        fx.text(flyff_common::defs::text::TID_GAME_LACKMONEY);
        return Outcome::Refused;
    }
    if before.element == Element::None || before.element_level <= 0 {
        nope(fx);
        return Outcome::Refused;
    }
    fx.mark_urgent();
    let _ = pay(m, cx, REMOVE_ELEMENT_COST, fx);
    feedback(m, fx, true);
    ops::update_item(m, cx.env, id, UiParam::Ir, 0, 0, fx);
    ops::update_item(m, cx.env, id, UiParam::Rao, 0, 0, fx);
    fx.out.to_self(pb::RemoveAttributeNtf { success: true });
    let after = item_at(m, id).cloned();
    audit(
        fx,
        "element_remove",
        "npc",
        "success",
        &before,
        after.as_ref(),
        vec![serde_json::json!({ "gold": REMOVE_ELEMENT_COST })],
    );
    Outcome::Success
}
