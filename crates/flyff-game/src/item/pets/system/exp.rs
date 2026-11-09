//! Pet experience by kills, the level-up and the owner's death (Plan/19 §13.3; src:_Common/Mover.cpp:11232-11269,
//! 11416-11513, 5333-5390; src:_Common/pet.cpp:51-115).

use super::process::{expire_item, send_state};
use super::summon::{apply_avail, release, remove_bonus};
use super::{
    EV_DEATH, EV_HATCH, EV_LEVEL_UP, PetCause, active_id, active_pet, audit_pet, item_mut, pet_mut,
};
use crate::buff::core::BuffCx;
use crate::follow::Followup;
use crate::item::fx::Fx;
use crate::item::model::PetLevel;
use crate::mover::Mover;
use flyff_common::defs::attribute::{RANK_BOSS, RANK_SUPER};
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::item::pet::{self, KillExp, PF_PET_LEVEL_UP};

/// `CMover::AddPetExpOnKill( dwClass, monster level, player level )` (`features.sys_petleveling_mobkill`): the
/// summoned pet gains `pet_base_exp x grade factor x rank factor` (see `flyff_rules::item::pet::exp_after_kill`).
/// A full bar levels the pet up (an egg hatches); otherwise `PetSetExp` goes to the owner.
pub fn exp_on_kill(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    rng: &mut dyn GameRng,
    monster_class: u32,
    monster_level: i32,
    fx: &mut Fx,
) {
    if !cx.cfg().sys_petleveling_mobkill {
        return;
    }
    let player_level = m.mv.level;
    let Some((id, _, pet)) = active_pet(m, cx.data()) else {
        return;
    };
    let outcome = pet::exp_after_kill(
        pet.level_u8(),
        pet.exp,
        cx.cfg().pet_base_exp,
        monster_class == RANK_SUPER || monster_class == RANK_BOSS,
        player_level,
        monster_level,
    );
    match outcome {
        KillExp::LevelUp => {
            level_up(m, cx, rng, PetCause::Kill, fx);
        }
        KillExp::Set(exp) => {
            if let Some(p) = pet_mut(m, id) {
                p.set_exp(exp);
                let exp = p.exp;
                fx.out.to_self(pb::PetSetExp { exp });
            }
        }
    }
}

/// `CMover::PetLevelup`: an egg hatches (the kind is rolled over the hatch weights, the life is 1, the owner's
/// honor counter follows); the grade rises by one; the item becomes the item of the kind (same object, same
/// serial); the experience is 0 and the energy full; the stat of the new grade is rolled over the
/// `Pet_LevelupAvail` row. The bonus is taken off, `Pet{PF_PET_LEVEL_UP}` goes to the owner and `PetLevelup` to
/// everybody else around, then the new bonus is applied (the original waited for the next 16-tick pass).
///
/// An S pet cannot level up (FIX: `IncLevel` of the original has no cap and corrupted the pet). `true` when
/// the pet leveled.
pub fn level_up(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    rng: &mut dyn GameRng,
    cause: PetCause,
    fx: &mut Fx,
) -> bool {
    let data = cx.data();
    let tables = &data.pet;
    let Some((id, _, pet)) = active_pet(m, data) else {
        return false;
    };
    if pet.level == PetLevel::S {
        return false;
    }
    let hatched = pet.level == PetLevel::Egg;
    let kind = hatched.then(|| tables.hatch(rng.random_range(1, 10_001)));
    let Some(pet) = pet_mut(m, id) else {
        return false;
    };
    if let Some(kind) = kind {
        pet.kind = kind;
        pet.life = 1;
    }
    pet.level = PetLevel::from_u8(pet.level_u8() + 1);
    let new_item = pet.item_id(tables);
    pet.exp = 0;
    let max_energy = u32::from(pet.max_energy(tables));
    pet.set_energy(tables, max_energy);
    let level = pet.level_u8();
    let avail = tables.levelup_avail_level(level, rng.random_range(1, 10_001));
    if let Some(slot) = pet.avail_levels.get_mut(usize::from(level) - 1) {
        *slot = avail;
    }
    // the same item object takes the id of the kind; a kind without an item row keeps the old id
    if new_item != 0
        && let Some(item) = item_mut(m, id)
    {
        item.item_id = new_item;
    }
    remove_bonus(m, cx, fx);
    audit_pet(
        fx,
        if hatched { EV_HATCH } else { EV_LEVEL_UP },
        m,
        id,
        Some(cause),
        &[],
    );
    if let Some((_, _, pet)) = active_pet(m, data) {
        fx.out.to_self(pb::Pet {
            pet: Some(pet.to_pb()),
            flag: PF_PET_LEVEL_UP,
        });
        fx.out.near_excl(pb::PetLevelup {
            pet_id: pet.levelup_word(tables),
        });
    }
    if hatched {
        fx.then(Followup::EggHatched { player: m.id() });
    }
    fx.mark_urgent();
    apply_avail(m, cx, fx);
    true
}

/// `DoDie` step 5 for the summoned system pet: a player killed by a monster or an NPC (the caller checks that)
/// loses a life of a pet above grade egg - the pet is whole again with its next life, or dies (life, energy and
/// EXP 0, the item `EXPIRED`) - and the pet is released; `PetState` follows the release. With
/// `features.no_pet_life` nothing is lost. With `features.reactivate_eatpet` a pet that is still summoned is
/// released anyway. The eat pet's part of the step is `pets::eat`'s.
pub fn on_owner_killed(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    let data = cx.data();
    let cfg = cx.cfg();
    if let Some((id, _, pet)) = active_pet(m, data)
        && pet.level != PetLevel::Egg
        && !cfg.no_pet_life
    {
        let life = i32::from(pet.reported_life(false)) - 1;
        let max_energy = u32::from(pet.max_energy(&data.pet));
        if life >= 0 {
            if let Some(p) = pet_mut(m, id) {
                p.life = life as u16;
                p.set_energy(&data.pet, max_energy);
            }
        } else {
            if let Some(p) = pet_mut(m, id) {
                p.life = 0;
                p.energy = 0;
                p.exp = 0;
            }
            expire_item(m, cx, id, fx);
        }
        audit_pet(fx, EV_DEATH, m, id, None, &[]);
        release(m, cx, fx);
        send_state(m, cx, id, fx);
    }
    if cfg.reactivate_eatpet {
        if active_id(m, data).is_some() || m.player().is_some_and(|p| p.items.pet_obj_id.is_some())
        {
            release(m, cx, fx);
        }
        remove_bonus(m, cx, fx);
    }
}
