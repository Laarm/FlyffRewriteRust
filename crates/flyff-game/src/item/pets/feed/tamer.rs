//! The tamer's scrolls (Plan/19 §13.4; ref/13 §19.6; src:WORLDSERVER/DPSrvr.cpp:10315-10426): `PetTamerMistake` lowers
//! the pet one grade and (with `features.pet_auto_level_up`) levels it up again at once, which re-rolls the stat of
//! the grade; `PetTamerMiracle` re-rolls the stats of the current and the previous grade.

use crate::buff::core::BuffCx;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::PetLevel;
use crate::item::ops::{UiParam, core as ops};
use crate::item::pets::system::summon::{apply_avail, remove_bonus};
use crate::item::pets::system::{EV_LEVEL_UP, PetCause, active_pet, audit_pet, level_up, pet_mut};
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_PET_TAMER_MIRACLE, II_SYS_SYS_SCR_PET_TAMER_MISTAKE,
};
use flyff_common::defs::text::{
    TID_GAME_PET_BETWEEN_B_TO_S, TID_GAME_PET_BETWEEN_C_TO_S, TID_GAME_PET_NOT_FOUND,
};
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::item::pet::{MAX_PET_EXP, PF_PET_GET_AVAIL, PF_PET_LEVEL_DOWN};

/// The scroll `scroll` when it is the item `item_id` and usable (the original did not look at the reservation:
/// FIX, a scroll offered in a trade is not used).
fn scroll_of(m: &Mover, scroll: StorageId, item_id: u32) -> Option<u16> {
    let it = m.player()?.items.inventory.get_at_id(scroll)?;
    (it.item_id == item_id && it.is_usable()).then_some(it.count)
}

/// `OnPetTamerMistake( scroll )`: the scroll must be `II_SYS_SYS_SCR_PET_TAMER_MISTAKE` (anything else is
/// silent), a pet must be summoned (`TID_GAME_PET_NOT_FOUND`) and be grade C to S (`TID_GAME_PET_BETWEEN_C_TO_S`).
/// The stat of the current grade is cleared, the grade falls by one, the EXP is full, the bonus is taken off,
/// `Pet{PF_PET_LEVEL_DOWN}` goes to the owner and `PetLevelup` to the others, the scroll is used up - and with
/// `features.pet_auto_level_up` the pet levels up again at once (`PetLevelup`: the same grade, EXP 0, full energy
/// and a freshly rolled stat for it).
pub fn tamer_mistake(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    rng: &mut dyn GameRng,
    scroll: StorageId,
    fx: &mut Fx,
) {
    let data = cx.data();
    let Some(count) = scroll_of(m, scroll, II_SYS_SYS_SCR_PET_TAMER_MISTAKE) else {
        return;
    };
    let Some((id, _, pet)) = active_pet(m, data) else {
        fx.text(TID_GAME_PET_NOT_FOUND);
        return;
    };
    if !(PetLevel::C..=PetLevel::S).contains(&pet.level) {
        fx.text(TID_GAME_PET_BETWEEN_C_TO_S);
        return;
    }
    if let Some(p) = pet_mut(m, id) {
        let level = usize::from(p.level_u8());
        if let Some(slot) = p.avail_levels.get_mut(level - 1) {
            *slot = 0;
        }
        p.level = PetLevel::from_u8(p.level_u8() - 1);
        p.exp = MAX_PET_EXP;
    }
    remove_bonus(m, cx, fx);
    send_level_down(m, cx, id, PF_PET_LEVEL_DOWN, fx);
    ops::update_item(
        m,
        &cx.env,
        scroll,
        UiParam::Num,
        u32::from(count) - 1,
        0,
        fx,
    );
    audit_pet(
        fx,
        EV_LEVEL_UP,
        m,
        id,
        Some(PetCause::Mistake),
        &[("scroll", serde_json::json!("tamer_mistake"))],
    );
    if cx.cfg().pet_auto_level_up {
        level_up(m, cx, rng, PetCause::Mistake, fx);
    } else {
        apply_avail(m, cx, fx);
    }
    fx.mark_urgent();
}

/// `OnPetTamerMiracle( scroll )`: the scroll must be `II_SYS_SYS_SCR_PET_TAMER_MIRACLE`, a pet must be summoned
/// and be grade B to S (`TID_GAME_PET_BETWEEN_B_TO_S`). The stats of the previous and the current grade are
/// cleared and rolled again (the previous first, each over its own `Pet_LevelupAvail` row), the bonus is
/// re-applied, `Pet{PF_PET_GET_AVAIL}` goes to the owner and `PetLevelup` to the others, the scroll is used up.
pub fn tamer_miracle(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    rng: &mut dyn GameRng,
    scroll: StorageId,
    fx: &mut Fx,
) {
    let data = cx.data();
    let Some(count) = scroll_of(m, scroll, II_SYS_SYS_SCR_PET_TAMER_MIRACLE) else {
        return;
    };
    let Some((id, _, pet)) = active_pet(m, data) else {
        fx.text(TID_GAME_PET_NOT_FOUND);
        return;
    };
    if !(PetLevel::B..=PetLevel::S).contains(&pet.level) {
        fx.text(TID_GAME_PET_BETWEEN_B_TO_S);
        return;
    }
    let tables = &data.pet;
    if let Some(p) = pet_mut(m, id) {
        let level = p.level_u8();
        for grade in [level - 1, level] {
            let roll = rng.random_range(1, 10_001);
            let avail = tables.levelup_avail_level(grade, roll);
            if let Some(slot) = p.avail_levels.get_mut(usize::from(grade) - 1) {
                *slot = avail;
            }
        }
    }
    remove_bonus(m, cx, fx);
    send_level_down(m, cx, id, PF_PET_GET_AVAIL, fx);
    ops::update_item(
        m,
        &cx.env,
        scroll,
        UiParam::Num,
        u32::from(count) - 1,
        0,
        fx,
    );
    audit_pet(
        fx,
        EV_LEVEL_UP,
        m,
        id,
        Some(PetCause::Miracle),
        &[("scroll", serde_json::json!("tamer_miracle"))],
    );
    apply_avail(m, cx, fx);
    fx.mark_urgent();
}

/// `AddPet( pet, flag )` to the owner and `AddPetLevelup( MAKELONG( index, level ) )` to everybody else around.
fn send_level_down(m: &Mover, cx: &BuffCx<'_>, id: StorageId, flag: u32, fx: &mut Fx) {
    let Some(pet) = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .and_then(|it| it.pet.as_deref())
    else {
        return;
    };
    fx.out.to_self(pb::Pet {
        pet: Some(pet.to_pb()),
        flag,
    });
    fx.out.near_excl(pb::PetLevelup {
        pet_id: pet.levelup_word(&cx.data().pet),
    });
}
