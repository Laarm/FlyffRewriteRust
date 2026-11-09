//! The safe smelt (Plan/19 §12.1, `__BETTER_SAFE_UPGRADES`; src:WORLDSERVER/DPSrvr.cpp:5680-5717,
//! ItemUpgrade.cpp:424-967): `SmeltSafetyReq{target, material, protection, smelt_scroll}` and its dispatcher.
//!
//! The answer is always `SmeltSafetyNtf{result}`: 0 invalid or refused (nothing consumed), 1 success, 2 failed
//! (the target is kept), 3 already at the maximum (nothing consumed). The branch follows the material:
//!
//! | material | branch |
//! |---|---|
//! | Orichalcum (`ORICHALCUM01`, `_1`) | the general refine (`refine::safe`) |
//! | Shining Orichalcum (`ORICHALCUM02`) | the safe ultimate enchant (ITEM-13, [`UpgradeHooks::ultimate_safe`]) |
//! | moonstone | the accessory (`accessory::safe`); else, when the item can take a socket
//! (`features.no_piercing = false`), the piercing size; else it falls through to the element case, which
//! answers 0 for a moonstone (KEEP) |
//! | element card | the element (`element::safe`) |
//!
//! Before the dispatch: pacing (PL-076, answer 0), a trade or stall and a worn target (answer 0 and the legacy
//! text), items that are not all usable (answer 0 and the legacy text). A smelt scroll that is not usable counts
//! as "none". **FIX VR-022 / PL-067**: the protection item of the element branch must be `SMELPROT`.

use super::txn::{
    Outcome, UpgradeCx, audit, distinct, equipped, feedback, item_at, material, pay, take_one,
    usable,
};
use super::{accessory, element, refine};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::{Item, MAX_PIERCING_SUIT, MAX_PIERCING_WEAPON, MAX_VIS};
use crate::item::ops::{UiParam, core as ops};
use crate::item::usage::UseFacts;
use crate::mover::Mover;
use flyff_common::defs::item::II_SYS_SYS_SCR_PIEPROT;
use flyff_common::defs::itemkind::{IK2_WEAPON_DIRECT, IK2_WEAPON_MAGIC, IK3_SHIELD, IK3_SUIT};
use flyff_common::rng::GameRng;
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::chat::TEXT_GENERAL;
use flyff_rules::item::refine::{PIERCING_SIZE_COST, SPACE_10000, fails_over};
use flyff_rules::item::smelt::{
    SafeBranch, is_moonstone, is_orichalcum, is_shining_orichalcum, safe_branch,
};

/// "Unable to smelt in a vendor window or the item is equiped." (legacy literal, KEEP).
pub const TEXT_BUSY: &str = "Unable to smelt in a vendor window or the item is equiped.";
/// "Invalid items. If this continues to happen, please report it to staff" (legacy literal, KEEP).
pub const TEXT_INVALID: &str =
    "Invalid items. If this continues to happen, please report it to staff";

/// The safe ultimate enchant of ITEM-13 (`SmeltSafetyUltimate`): `(mover, cx, weapon, shining orichalcum,
/// protection, rng, fx) -> result byte`.
pub type UltimateSafeFn = fn(
    &mut Mover,
    &UpgradeCx<'_>,
    StorageId,
    StorageId,
    StorageId,
    &mut dyn GameRng,
    &mut Fx,
) -> u8;

/// A pet item's random option changed while the pet is summoned (ITEM-18): `(mover, cx, pet item, fx)`.
pub type PetOptionsFn = fn(&mut Mover, &UpgradeCx<'_>, StorageId, &mut Fx);

/// The branches of the safe smelt and the pet seams that other work packages own. `Services.items.upgrade_hooks` holds them; an
/// empty hook answers 0.
#[derive(Clone, Copy, Debug, Default)]
pub struct UpgradeHooks {
    /// ITEM-13: the Shining Orichalcum branch (`II_SYS_SYS_SCR_SMELPROT3` protects, 1 000 000-space odds).
    pub ultimate_safe: Option<UltimateSafeFn>,
    /// ITEM-18: take the stat bonus of the summoned eat or system pet off before its lines are replaced
    /// (`ResetDestParamRandomOptExtension`).
    pub pet_options_reset: Option<PetOptionsFn>,
    /// ITEM-18: apply the stat bonus of the summoned eat or system pet after its lines changed
    /// (`SetDestParamRandomOptExtension`).
    pub pet_options_set: Option<PetOptionsFn>,
}

/// Whether the item takes one more socket (`IsPierceAble( NULL_ID, TRUE )`, ItemUpgrade.cpp via Item.cpp:739).
/// With `features.no_piercing` (the repack) the gear branches do not exist and the vis `else` binds to
/// `if ( bSize )`, so the answer is always `false`.
pub fn can_take_socket(item: &Item, prop: &ItemProp, no_piercing: bool) -> bool {
    if no_piercing {
        return false;
    }
    let next = item.piercing.cards.len() + 1;
    if prop.item_kind3 == IK3_SUIT {
        next <= MAX_PIERCING_SUIT
    } else if prop.item_kind3 == IK3_SHIELD
        || matches!(prop.item_kind2, IK2_WEAPON_DIRECT | IK2_WEAPON_MAGIC)
    {
        next <= MAX_PIERCING_WEAPON
    } else if item.is_vis_pet(prop) {
        next <= MAX_VIS
    } else {
        false
    }
}

/// `SmeltSafetyPiercingSize`: 100 000 penya, a moonstone and a `II_SYS_SYS_SCR_PIEPROT` protection; the target
/// is never destroyed.
fn piercing_size(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    mat: StorageId,
    protection: StorageId,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> u8 {
    let (Some(before), Some(mat_item), Some(prot_item)) = (
        item_at(m, target).cloned(),
        item_at(m, mat).cloned(),
        item_at(m, protection).cloned(),
    ) else {
        return 0;
    };
    if !is_moonstone(mat_item.item_id) || prot_item.item_id != II_SYS_SYS_SCR_PIEPROT {
        return 0;
    }
    let Some(tp) = cx.prop(before.item_id) else {
        return 0;
    };
    let size = i32::try_from(before.piercing.cards.len()).unwrap_or(i32::MAX);
    let suit = tp.item_kind3 == IK3_SUIT;
    fx.mark_urgent();
    if !pay(m, cx, PIERCING_SIZE_COST, fx) {
        return 0;
    }
    take_one(m, cx, mat, fx);
    take_one(m, cx, protection, fx);
    let p = cx.tables.piercing_size_prob(suit, size);
    let materials = vec![
        material(mat_item.item_id, 1),
        material(prot_item.item_id, 1),
        serde_json::json!({ "gold": PIERCING_SIZE_COST }),
    ];
    let (code, result) = if fails_over(rng.random_below(SPACE_10000), p) {
        feedback(m, fx, false);
        (2, "failed")
    } else {
        feedback(m, fx, true);
        ops::update_item(
            m,
            cx.env,
            target,
            UiParam::PiercingSize,
            (size + 1) as u32,
            0,
            fx,
        );
        (1, "success")
    };
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "piercing_size",
        "safe",
        result,
        &before,
        after.as_ref(),
        materials,
    );
    code
}

/// `OnSmeltSafety` for one request: pacing, the busy and ownership checks, the dispatch. Sends the
/// `SmeltSafetyNtf` (and the legacy text of a refusal) into `fx`; returns the result byte.
pub fn smelt_safety(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::SmeltSafetyReq,
    facts: &UseFacts,
    hooks: &UpgradeHooks,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> u8 {
    let answer = |fx: &mut Fx, code: u8| {
        fx.out.to_self(pb::SmeltSafetyNtf {
            result: u32::from(code),
        });
        code
    };
    let text = |fx: &mut Fx, t: &str| {
        fx.out.to_self(pb::Text {
            kind: TEXT_GENERAL,
            text: t.to_owned(),
        });
    };
    if crate::combat::state::is_die(m) || !super::pace(m, cx) {
        return answer(fx, 0);
    }
    let main_id = StorageId::try_from(msg.item_obj_id).ok();
    if facts.trading || facts.vending || main_id.is_some_and(|id| equipped(m, id)) {
        answer(fx, 0);
        text(fx, TEXT_BUSY);
        return 0;
    }
    let (Some(target), Some(mat), Some(prot)) = (
        usable(m, msg.item_obj_id),
        usable(m, msg.material_obj_id),
        usable(m, msg.protection_obj_id),
    ) else {
        answer(fx, 0);
        text(fx, TEXT_INVALID);
        return 0;
    };
    let smelt = usable(m, msg.smelt_scroll_obj_id);
    let ids: Vec<StorageId> = [Some(target), Some(mat), Some(prot), smelt]
        .into_iter()
        .flatten()
        .collect();
    if !distinct(&ids) {
        return answer(fx, 0);
    }
    let code = dispatch(m, cx, target, mat, prot, smelt, hooks, rng, fx);
    answer(fx, code)
}

#[allow(clippy::too_many_arguments)]
fn dispatch(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    mat: StorageId,
    prot: StorageId,
    smelt: Option<StorageId>,
    hooks: &UpgradeHooks,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> u8 {
    let cfg = cx.env.cfg();
    let (Some(t), Some(mt)) = (item_at(m, target).cloned(), item_at(m, mat).cloned()) else {
        return 0;
    };
    let Some(mp) = cx.prop(mt.item_id) else {
        return 0;
    };
    match safe_branch(mp, cfg.better_safe_upgrades) {
        SafeBranch::Normal => {
            if is_orichalcum(mp.id) {
                refine::safe(m, cx, target, mat, prot, smelt, rng, fx)
            } else if is_shining_orichalcum(mp.id) {
                match hooks.ultimate_safe {
                    Some(f) => f(m, cx, target, mat, prot, rng, fx),
                    None => 0,
                }
            } else {
                0
            }
        }
        SafeBranch::Moonstone => {
            if t.is_accessory(cx.env.data()) {
                accessory::safe(m, cx, target, mat, prot, rng, fx)
            } else if cx
                .prop(t.item_id)
                .is_some_and(|tp| can_take_socket(&t, tp, cfg.no_piercing))
            {
                piercing_size(m, cx, target, mat, prot, rng, fx)
            } else {
                // KEEP: falls through into the element case, which refuses a moonstone
                element::safe(m, cx, target, mat, prot, smelt, rng, fx)
            }
        }
        SafeBranch::Element => element::safe(m, cx, target, mat, prot, smelt, rng, fx),
        SafeBranch::None => 0,
    }
}

/// The result byte as an [`Outcome`] (for the callers and the tests).
pub fn outcome_of(code: u8) -> Outcome {
    match code {
        1 => Outcome::Success,
        2 => Outcome::Failed,
        _ => Outcome::Refused,
    }
}
