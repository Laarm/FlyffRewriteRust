//! `DoUseItemTarget{material, target}` - the scrolls that are applied onto another item (Plan/19 §12.6.5, F-232;
//! ref/07b §8.7; src:WORLDSERVER/DPSrvr.cpp:5510-5616). The plan names this handler `item::usage::target::
//! on_use_target`; it lives with the other random-option code of this work package.
//!
//! Both items must exist and be unreserved (a missing or foreign id is dropped silently), the target must not
//! be worn (`TID_GAME_EQUIPPUT`). The table picks the row by the **material's item id**:
//!
//! | material | action |
//! |---|---|
//! | `II_SYS_SYS_SCR_AWAKE` | an awakening item without lines gets a fresh awakening (the protected path when the flag is set) |
//! | `II_SYS_SYS_SCR_BLESSEDNESS` / `_02` | blessing lines (overwrites; unreachable with `features.only_armor_awakes`) |
//! | `II_SYS_SYS_SCR_EATPETAWAKE` / `II_SYS_SYS_SCR_PETAWAKE` | eat pet / system pet lines (likewise unreachable by default) |
//! | `II_SYS_SYS_SCR_AWAKECANCEL` / `_02` | the lines are removed, or - with the `AWAKESAFE` buff - hidden behind the safe flag |
//! | `II_SYS_SYS_SCR_PETAWAKECANCEL` | the system pet lines are removed |
//! | `II_SYS_SYS_SCR_LEVELDOWN01` / `02` | the required level drops by 5 / 10 |
//! | `II_SYS_SYS_QUE_PETRESURRECTION02_S/_A/_B` | an expired system pet of that grade comes back |
//! | anything else | nothing |
//!
//! A row that worked plays the success sound and effect (not for an invisible GM) and consumes one scroll.

use super::random_option::{generate_on, initialize_on};
use super::txn::{
    Outcome, UpgradeCx, audit, equipped, feedback, item_at, material, take_one, usable,
};
use crate::buff::BuffCx;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::core as ops;
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_SYS_SYS_QUE_PETRESURRECTION02_A, II_SYS_SYS_QUE_PETRESURRECTION02_B,
    II_SYS_SYS_QUE_PETRESURRECTION02_S, II_SYS_SYS_SCR_AWAKE, II_SYS_SYS_SCR_AWAKECANCEL,
    II_SYS_SYS_SCR_AWAKECANCEL02, II_SYS_SYS_SCR_BLESSEDNESS, II_SYS_SYS_SCR_BLESSEDNESS02,
    II_SYS_SYS_SCR_EATPETAWAKE, II_SYS_SYS_SCR_LEVELDOWN01, II_SYS_SYS_SCR_LEVELDOWN02,
    II_SYS_SYS_SCR_PETAWAKE, II_SYS_SYS_SCR_PETAWAKECANCEL,
};
use flyff_common::defs::text::{
    TID_GAME_AWAKE_OR_BLESSEDNESS01, TID_GAME_AWAKECANCEL, TID_GAME_BLESSEDNESS_INVALID_ITEM,
    TID_GAME_EQUIPPUT, TID_GAME_INVALID_TARGET_ITEM, TID_GAME_ITEM_LEVELDOWN01,
    TID_GAME_PETAWAKE_E00, TID_GAME_PETAWAKE_S00, TID_GAME_PETAWAKECANCEL_E00,
    TID_GAME_USE_BLESSEDNESS_INFO,
};
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::item::random_option::{LevelDown, RandomOptKind};

/// `DoUseItemTarget_ItemLevelDown`: the target needs equipment parts and a level limit
/// (`TID_GAME_INVALID_TARGET_ITEM`); allowed when it has no level-down or -5 and the scroll is the -10 one
/// (`TID_GAME_ITEM_LEVELDOWN01` otherwise).
fn level_down(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    target: StorageId,
    scroll_id: u32,
    fx: &mut Fx,
) -> bool {
    let Some(before) = item_at(m, target).cloned() else {
        return false;
    };
    let Some(tp) = cx.prop(before.item_id) else {
        return false;
    };
    if tp.parts == u32::MAX || tp.limit_level1 == u32::MAX {
        fx.text(TID_GAME_INVALID_TARGET_ITEM);
        return false;
    }
    let current = before.random_option.level_down();
    let ten = scroll_id == II_SYS_SYS_SCR_LEVELDOWN02;
    if current == LevelDown::None || (current == LevelDown::Minus5 && ten) {
        let mut word = before.random_option;
        word.set_level_down(if scroll_id == II_SYS_SYS_SCR_LEVELDOWN01 {
            LevelDown::Minus5
        } else {
            LevelDown::Minus10
        });
        ops::update_item_ex(m, cx.env, target, word, fx);
        let after = item_at(m, target).cloned();
        audit(
            fx,
            "level_down",
            "scroll",
            "success",
            &before,
            after.as_ref(),
            vec![material(scroll_id, 1)],
        );
        true
    } else {
        fx.text(TID_GAME_ITEM_LEVELDOWN01);
        false
    }
}

/// `OnDoUseItemTarget`. Returns what happened; messages and the audit lines are in `fx`.
pub fn on_use_target(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::DoUseItemTarget,
    hooks: &super::UpgradeHooks,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Outcome {
    let (Some(scroll), Some(target)) =
        (usable(m, msg.material_obj_id), usable(m, msg.target_obj_id))
    else {
        return Outcome::Ignored;
    };
    if scroll == target {
        return Outcome::Ignored;
    }
    if equipped(m, target) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let Some(scroll_item) = item_at(m, scroll).cloned() else {
        return Outcome::Ignored;
    };
    let id = scroll_item.item_id;
    let ok = match id {
        II_SYS_SYS_QUE_PETRESURRECTION02_S
        | II_SYS_SYS_QUE_PETRESURRECTION02_A
        | II_SYS_SYS_QUE_PETRESURRECTION02_B => {
            // the rules of the pet row are ITEM-17's (`pets::feed::use_target_scroll`)
            let bcx = BuffCx::new(*cx.env, cx.now);
            crate::item::pets::feed::use_target_scroll(m, &bcx, id, target, fx)
        }
        II_SYS_SYS_SCR_AWAKECANCEL | II_SYS_SYS_SCR_AWAKECANCEL02 => initialize_on(
            m,
            cx,
            target,
            RandomOptKind::Awakening,
            TID_GAME_AWAKECANCEL,
            Some(id),
            hooks,
            fx,
        ),
        II_SYS_SYS_SCR_AWAKE => generate_on(
            m,
            cx,
            target,
            RandomOptKind::Awakening,
            0,
            TID_GAME_INVALID_TARGET_ITEM,
            TID_GAME_AWAKE_OR_BLESSEDNESS01,
            Some(id),
            hooks,
            rng,
            fx,
        ),
        II_SYS_SYS_SCR_BLESSEDNESS | II_SYS_SYS_SCR_BLESSEDNESS02 => generate_on(
            m,
            cx,
            target,
            RandomOptKind::Blessing,
            // `DoUseItemTarget_GenRandomOption( .., 0, TID_GAME_USE_BLESSEDNESS_INFO, TID_GAME_BLESSEDNESS_INVALID_ITEM )`:
            // no success text; the *mismatch* answer is the "use" text, the already-has-lines one the invalid-item text
            0,
            TID_GAME_USE_BLESSEDNESS_INFO,
            TID_GAME_BLESSEDNESS_INVALID_ITEM,
            Some(id),
            hooks,
            rng,
            fx,
        ),
        II_SYS_SYS_SCR_EATPETAWAKE => generate_on(
            m,
            cx,
            target,
            RandomOptKind::EatPet,
            TID_GAME_PETAWAKE_S00,
            TID_GAME_PETAWAKE_E00,
            TID_GAME_PETAWAKE_E00,
            Some(id),
            hooks,
            rng,
            fx,
        ),
        II_SYS_SYS_SCR_PETAWAKE => generate_on(
            m,
            cx,
            target,
            RandomOptKind::SystemPet,
            TID_GAME_PETAWAKE_S00,
            TID_GAME_PETAWAKE_E00,
            TID_GAME_PETAWAKE_E00,
            Some(id),
            hooks,
            rng,
            fx,
        ),
        II_SYS_SYS_SCR_PETAWAKECANCEL => initialize_on(
            m,
            cx,
            target,
            RandomOptKind::SystemPet,
            TID_GAME_PETAWAKECANCEL_E00,
            Some(id),
            hooks,
            fx,
        ),
        II_SYS_SYS_SCR_LEVELDOWN01 | II_SYS_SYS_SCR_LEVELDOWN02 => {
            level_down(m, cx, target, id, fx)
        }
        _ => return Outcome::Ignored,
    };
    if !ok {
        return Outcome::Refused;
    }
    fx.mark_urgent();
    feedback(m, fx, true);
    take_one(m, cx, scroll, fx);
    Outcome::Success
}
