//! The pet scrolls of `DoUseItemSystem` and the naming (Plan/19 §5.4, §5.9, §13.3; src:_Common/MoverSkill.cpp:1982-2030,
//! WORLDSERVER/User.cpp:8595-8640, DPSrvr.cpp:12273-12290).

use super::exp::level_up;
use super::process::send_state;
use super::{PetCause, active_pet, pet_mut};
use crate::buff::core::BuffCx;
use crate::item::fx::Fx;
use crate::item::model::PetLevel;
use crate::item::usage::system::{CONSUME, FALLTHROUGH, Sys};
use crate::mover::Mover;
use flyff_common::defs::item::II_SYS_SYS_SCR_PET_LIFE;
use flyff_common::defs::text::{
    TID_GAME_NAME_PET_E00, TID_GAME_NAME_PET_E01, TID_GAME_PET_4EGG, TID_GAME_PET_CANT_USE_TO_EGG,
    TID_GAME_PET_NOT_FOUND,
};
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::item::pet::MAX_PET_LIFE;
use flyff_rules::names::{NameKind, NameLimits, validate_name};

/// `II_SYS_SYS_SCR_PET_LIFE` (rolls `Pet_AddLifeProbability`: +1 to +5 life) and `_LIFE02` (+1): needs a
/// summoned pet (`TID_GAME_PET_NOT_FOUND`), not an egg (`TID_GAME_PET_CANT_USE_TO_EGG`), and a life below 99. The
/// life that is added to is the *reported* one, so with `features.no_pet_life` (reads 1) the scroll is used up
/// without visible effect, as in the original. `PetState` goes to the owner.
pub fn use_life_scroll(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    scroll: u32,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Sys {
    let data = cx.data();
    let Some((id, _, pet)) = active_pet(m, data) else {
        fx.text(TID_GAME_PET_NOT_FOUND);
        return FALLTHROUGH;
    };
    if pet.level == PetLevel::Egg {
        fx.text(TID_GAME_PET_CANT_USE_TO_EGG);
        return FALLTHROUGH;
    }
    let life = pet.reported_life(cx.cfg().no_pet_life);
    if life >= MAX_PET_LIFE {
        return FALLTHROUGH;
    }
    let add = if scroll == II_SYS_SYS_SCR_PET_LIFE {
        data.pet.add_life(rng.random_range(1, 101))
    } else {
        1
    };
    let new = (u32::from(life) + add).min(u32::from(MAX_PET_LIFE));
    if let Some(p) = pet_mut(m, id) {
        p.life = new as u16;
    }
    send_state(m, cx, id, fx);
    CONSUME
}

/// `II_SYS_SYS_SCR_PET_HATCH`: hatches the summoned egg now (`TID_GAME_PET_NOT_FOUND` without a pet,
/// `TID_GAME_PET_4EGG` for anything but an egg).
pub fn use_hatch_scroll(m: &mut Mover, cx: &BuffCx<'_>, rng: &mut dyn GameRng, fx: &mut Fx) -> Sys {
    let Some((_, _, pet)) = active_pet(m, cx.data()) else {
        fx.text(TID_GAME_PET_NOT_FOUND);
        return FALLTHROUGH;
    };
    if pet.level != PetLevel::Egg {
        fx.text(TID_GAME_PET_4EGG);
        return FALLTHROUGH;
    }
    level_up(m, cx, rng, PetCause::HatchScroll, fx);
    CONSUME
}

/// `DoUseItemPetNaming` (`II_SYS_SYS_SCR_PET_NAMING` with an input text): needs the summoned pet
/// (`TID_GAME_NAME_PET_E00`) and a name that passes the pet-name rules (`TID_GAME_NAME_PET_E01`); the name is
/// cut to 16 bytes and `SetPetName` goes to everybody around. The scroll is used up.
pub fn name_pet(m: &mut Mover, cx: &BuffCx<'_>, input: &str, fx: &mut Fx) -> Sys {
    let data = cx.data();
    let Some((id, _, _)) = active_pet(m, data) else {
        fx.text(TID_GAME_NAME_PET_E00);
        return FALLTHROUGH;
    };
    let Ok(name) = validate_name(
        NameKind::PetName,
        input,
        &data.name_rules,
        NameLimits::default(),
        None,
    ) else {
        fx.text(TID_GAME_NAME_PET_E01);
        return FALLTHROUGH;
    };
    let Some(p) = pet_mut(m, id) else {
        return FALLTHROUGH;
    };
    p.set_name(&name);
    let name = p.name.clone();
    fx.out.near(pb::SetPetName { name });
    CONSUME
}

/// `ClearPetName`: needs the summoned pet (`TID_GAME_NAME_PET_E00`); the name becomes empty and `SetPetName{""}`
/// goes to everybody around.
pub fn clear_name(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    let Some((id, _, _)) = active_pet(m, cx.data()) else {
        fx.text(TID_GAME_NAME_PET_E00);
        return;
    };
    if let Some(p) = pet_mut(m, id) {
        p.name.clear();
    }
    fx.out.near(pb::SetPetName {
        name: String::new(),
    });
}
