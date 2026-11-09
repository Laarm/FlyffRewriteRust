//! Item upgrades (Plan/19 §12, WP-ITEM-11 and WP-ITEM-12; ref/13 §18, ref/07b §8; task `upgrades-a`): the
//! general refine, element, accessory and collector upgrades, the safe smelt, commercial element stones,
//! random options (stat scrolls, awakening, blessing, protected awakening, augmentation retry) and the
//! `DoUseItemTarget` scrolls.
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`txn`] | the call context, the ownership checks, feedback, audit, material consumption | ITEM-12 |
//! | [`limiter`] | [`UpgradeLimiter`] (PL-076) | ITEM-12 |
//! | [`refine`] | general refine +1..+10, legacy and safe | ITEM-12 |
//! | [`element`] | element upgrade (legacy, safe), change, removal | ITEM-12 |
//! | [`accessory`], [`collector`] | accessory and collector upgrades | ITEM-12 |
//! | [`stones`] | commercial element stones | ITEM-12 |
//! | [`safe`] | the `SmeltSafetyReq` dispatcher, the hooks of ITEM-13 | ITEM-12 |
//! | [`random_option`] | the awakening generator, stat scrolls, NPC awakening, blessing cancel, protected awakening, retry, level-down removal, the pet-awakening cancel | ITEM-11 |
//! | [`target`] | `DoUseItemTarget` | ITEM-11 |
//! | [`handlers`] | the `GameCtx` wrappers (`on_*`) registered in `flyff_game::handlers` | both |
//!
//! Every attempt is one commit block over one mover (checks, then consumption, roll and result), audited as
//! `item.upgrade` and followed by an urgent save. The rolls draw from the world's random stream, so a seeded
//! `TestWorld` replays an attempt exactly. Two reproducible-by-design rules of the plan are the fixes of the
//! trust-the-client holes: every object id is resolved against the player's own unreserved inventory
//! ([`txn::usable`]) and no entry may be named twice ([`txn::distinct`]).

pub mod accessory;
pub mod collector;
pub mod element;
pub mod handlers;
pub mod limiter;
pub mod random_option;
pub mod refine;
pub mod safe;
pub mod stones;
pub mod target;
pub mod txn;

pub use limiter::UpgradeLimiter;
pub use safe::{UltimateSafeFn, UpgradeHooks};
pub use txn::{Outcome, UpgradeCx};

use crate::item::fx::Fx;
use crate::item::usage::UseFacts;
use crate::mover::Mover;
use flyff_common::defs::text::TID_GAME_EQUIPPUT;
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::combat::element::AttributeTable;
use flyff_rules::item::smelt::{EnchantBranch, enchant_branch};
use txn::{distinct, equipped, item_at, refuse_stone, usable};

/// The pacing of PL-076: `true` when this attempt may run (and the time is remembered). Shared by `Enchant`,
/// `SmeltSafetyReq` and the ultimate and piercing-size handlers of ITEM-13/14.
pub fn pace(m: &mut Mover, cx: &UpgradeCx<'_>) -> bool {
    let min = cx.env.cfg().upgrade_min_interval_ms;
    let now = cx.now;
    m.player_mut()
        .is_some_and(|p| p.items.upgrade_limiter.accept(now, min))
}

/// The element factors of one level for doc 17 (`item::upgrade::attribute_factors(level)`, Plan/19 §12.3): the
/// damage rate, the defence rate and the element-correction rate, out of 10 000 (`GetAttributeDamageFactor`,
/// `GetAttributeDefenseFactor`, `GetAttributeAddAtkDmgFactor`). The level is clamped to the table; level 0 or
/// below gives zeros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttributeFactors {
    /// `nDamageRate`.
    pub damage: i32,
    /// `nDefenseRate`.
    pub defense: i32,
    /// `nAddAtkDmgRate`.
    pub add_atk: i32,
}

/// `attribute_factors(level)` over `table` (the combat service's cached `AttributeTable`).
pub fn attribute_factors(table: &AttributeTable, level: i32) -> AttributeFactors {
    AttributeFactors {
        damage: table.damage_factor(level),
        defense: table.defense_factor(level),
        add_atk: table.add_atk_factor(level),
    }
}

/// The legacy `Enchant{item, material}` (DPSrvr.cpp:5735 + `CItemUpgrade::OnEnchant`, ItemUpgrade.cpp:384-421).
///
/// Silent (`Ignored`): the pacing of PL-076, the quiz world, a trade or stall, an item that is missing,
/// reserved or named twice. `TID_GAME_NOTUPGRADE` for a target with a commercial stone, `TID_GAME_EQUIPPUT` for
/// a worn target; then the material's item kind picks the branch: `IK3_ELECARD` the element upgrade, an
/// Orichalcum kind the general refine, anything else the accessory or the collector refine when the target is
/// one (and nothing for any other target).
pub fn enchant(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::Enchant,
    facts: &UseFacts,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Outcome {
    if crate::combat::state::is_die(m) || !pace(m, cx) {
        return Outcome::Ignored;
    }
    if facts.quiz_world || facts.trading || facts.vending {
        return Outcome::Ignored;
    }
    let (Some(target), Some(mat)) = (usable(m, msg.item_obj_id), usable(m, msg.material_obj_id))
    else {
        return Outcome::Ignored;
    };
    if !distinct(&[target, mat]) {
        return Outcome::Ignored;
    }
    let (Some(t), Some(mt)) = (item_at(m, target).cloned(), item_at(m, mat).cloned()) else {
        return Outcome::Ignored;
    };
    if t.element_stone_id != 0 {
        refuse_stone(fx);
        return Outcome::Refused;
    }
    if equipped(m, target) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let Some(mp) = cx.prop(mt.item_id) else {
        return Outcome::Ignored;
    };
    match enchant_branch(mp, cx.env.cfg().better_safe_upgrades) {
        EnchantBranch::Element => element::legacy(m, cx, target, mat, rng, fx),
        EnchantBranch::General => refine::legacy(m, cx, target, mat, rng, fx),
        EnchantBranch::Other => {
            if t.is_accessory(cx.env.data()) {
                accessory::legacy(m, cx, target, mat, rng, fx)
            } else if t.is_collector(true) {
                collector::legacy(m, cx, target, mat, rng, fx)
            } else {
                Outcome::Ignored
            }
        }
    }
}
