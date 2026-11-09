//! `UsePetFeed` (Plan/19 §13.4; ref/13 §19.5; src:WORLDSERVER/DPSrvr.cpp:10186-10247).

use crate::buff::core::BuffCx;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{UiParam, core as ops};
use crate::item::pets::system::{EV_FEED, active_pet, audit_pet, pet_mut};
use crate::mover::Mover;
use flyff_common::defs::itemkind::IK3_FEED;
use flyff_common::defs::text::{TID_GAME_PET_NOT_FOUND, TID_GAME_PETFEED_S01};
use flyff_proto::pb;
use flyff_rules::chat::TEXT_GENERAL;
use flyff_rules::item::pet::{self, PL_EGG};

/// The legacy literal an egg is refused with under `features.sys_petleveling_mobkill`.
pub const EGG_FEED_REFUSAL: &str =
    "You can not level eggs with pet feed here. Go kill some monsters!";

/// `OnUsePetFeed( feed )`: a summoned pet (`TID_GAME_PET_NOT_FOUND`) and a usable `IK3_FEED` stack (else silent).
/// With `features.sys_petleveling_mobkill` an egg cannot be fed ([`EGG_FEED_REFUSAL`]). The stack is used up to
/// the need of the pet: an egg `50 000 - exp` (its EXP rises, `PetSetExp`), a pet `(max energy - energy) / 2`
/// (two points of energy per piece, `PetFeed{energy}` to everybody around); nothing to do is silent. The pieces
/// are consumed and `TID_GAME_PETFEED_S01 [n]` tells the owner how many.
pub fn use_pet_feed(m: &mut Mover, cx: &BuffCx<'_>, feed: StorageId, fx: &mut Fx) {
    let data = cx.data();
    let Some((pid, _, pet_state)) = active_pet(m, data) else {
        fx.text(TID_GAME_PET_NOT_FOUND);
        return;
    };
    let Some(item) = m.player().and_then(|p| p.items.inventory.get_at_id(feed)) else {
        return;
    };
    if !item.is_usable()
        || data
            .item(item.item_id)
            .is_none_or(|p| p.item_kind3 != IK3_FEED)
    {
        return;
    }
    let stack = u32::from(item.count);
    let level = pet_state.level_u8();
    if cx.cfg().sys_petleveling_mobkill && level == PL_EGG {
        fx.out.to_self(pb::Text {
            kind: TEXT_GENERAL,
            text: EGG_FEED_REFUSAL.to_owned(),
        });
        return;
    }
    let tables = &data.pet;
    let max_energy = u32::from(pet_state.max_energy(tables));
    let need = pet::feed_need(
        level,
        pet_state.exp,
        u32::from(pet_state.energy),
        max_energy,
    );
    let n = pet::feed_units(stack, need);
    if n == 0 {
        return;
    }
    if level == PL_EGG {
        if let Some(p) = pet_mut(m, pid) {
            p.set_exp(p.exp.saturating_add(n));
            let exp = p.exp;
            fx.out.to_self(pb::PetSetExp { exp });
        }
    } else if let Some(p) = pet_mut(m, pid) {
        p.energy = pet::energy_after_feed(u32::from(p.energy), n, max_energy);
        let energy = u32::from(p.energy);
        fx.out.near(pb::PetFeed { energy });
    }
    ops::update_item(m, &cx.env, feed, UiParam::Num, stack - n, 0, fx);
    fx.defined(TID_GAME_PETFEED_S01, vec![n.to_string()]);
    audit_pet(fx, EV_FEED, m, pid, None, &[("fed", serde_json::json!(n))]);
}
