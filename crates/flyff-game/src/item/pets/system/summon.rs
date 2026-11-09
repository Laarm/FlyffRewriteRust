//! Summoning and releasing the system pet, and its stat bonus (Plan/19 §13.1-§13.2; src:_Common/MoverSkill.cpp:4011-4060,
//! _Common/Mover.cpp:11112-11413, 11550-11605, _Common/moverbuff.cpp:32-50).
//!
//! The bonus is the `BUFF_PET` entry (`level = MAKELONG(value, dst)`, id = the item id) whose DST operations are
//! recorded under its buff source, so releasing it reverts exactly what it applied; the lines of a system-pet
//! awakening are a second source, `DstSource::Pet`.

use super::{active_pet, item_mut, pet_mut};
use crate::buff::core::{BuffCx, add as buff_add, remove_real, source_of};
use crate::buff::set::{BuffKey, BuffKind};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::PetState;
use crate::item::usage::RowResult;
use crate::mover::Mover;
use crate::stats::dst::DstSource;
use crate::stats::source::{apply_dst, apply_source, remove_source};
use flyff_common::defs::text::{TID_GAME_CANNOT_CALL_PET_ON_FLYING, TID_GAME_PET_NOWUSE};
use flyff_proto::pb;
use flyff_rules::consts::NULL_CHGPARAM;
use flyff_rules::item::pet::{PF_PET, make_long};

/// `DoUseSystemPet`: using an egg item toggles the summon. A second egg while one is summoned answers
/// `TID_GAME_PET_NOWUSE`. The item is permanent, the pipeline only shows its count again.
pub fn use_egg(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) -> RowResult {
    match m.player().and_then(|p| p.items.pet_obj_id) {
        Some(summoned) if summoned == id => release(m, cx, fx),
        Some(_) => fx.text(TID_GAME_PET_NOWUSE),
        None => {
            activate(m, cx, id, fx);
        }
    }
    RowResult::Consume
}

/// `ActivateSystemPet` (the quiz world is refused by the pipeline): not while flying
/// (`TID_GAME_CANNOT_CALL_PET_ON_FLYING`); the first use creates the egg (`CPet::InitEgg`); the summoned slot
/// is set, `PetCall` goes to everybody around, a fresh pet also gets `Pet{PF_PET}`. The stat bonus is applied at
/// once (the original waited for the next 16-tick pass). `true` when the pet is summoned.
pub fn activate(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) -> bool {
    if m.sim.act.flags.fly() {
        fx.text(TID_GAME_CANNOT_CALL_PET_ON_FLYING);
        return false;
    }
    let first = {
        let Some(item) = item_mut(m, id) else {
            return false;
        };
        if item.pet.is_none() {
            item.pet = Some(Box::new(PetState::new_egg()));
            true
        } else {
            false
        }
    };
    if let Some(p) = m.player_mut() {
        p.items.pet_obj_id = Some(id);
    }
    let tables = &cx.data().pet;
    let Some((_, _, pet)) = active_pet(m, cx.data()) else {
        return false;
    };
    fx.out.near(pb::PetCall {
        item_obj_id: u32::from(id),
        mover_index: pet.mover_index(tables),
        level: u32::from(pet.level_u8()),
        name: pet.name.clone(),
    });
    if first {
        fx.out.to_self(pb::Pet {
            pet: Some(pet.to_pb()),
            flag: PF_PET,
        });
    }
    apply_avail(m, cx, fx);
    true
}

/// `PetRelease` (server): the release penalty of the grade (all zero in the shipped `pet.inc`) and `PetState`
/// to the owner, the stat bonus off, the summoned slot cleared, `PetReleaseNtf` to everybody around. A slot
/// that names no pet (a stale handle) is only cleared.
pub fn release(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    let data = cx.data();
    let Some(summoned) = m.player().and_then(|p| p.items.pet_obj_id) else {
        return;
    };
    let level = match active_pet(m, data) {
        Some((_, _, pet)) => pet.level_u8(),
        None => {
            if let Some(p) = m.player_mut() {
                p.items.pet_obj_id = None;
            }
            remove_bonus(m, cx, fx);
            return;
        }
    };
    let (pen_exp, pen_energy) = data
        .pet
        .penalty(level)
        .map_or((0.0_f32, 0_u32), |p| (p.exp, p.energy));
    let no_pet_life = cx.cfg().no_pet_life;
    if let Some(pet) = pet_mut(m, summoned) {
        if pen_exp > 0.0 {
            // `100000 * fExp / 100.0F`: at least 0 % is kept
            let lost = ((100_000_f32 * pen_exp) / 100.0_f32) as u32;
            pet.exp -= lost.min(pet.exp);
        }
        if pen_energy > 0 {
            // an S pet keeps at least 5 points
            let hp = i64::from(pet.energy);
            let mut penalty = i64::from(pen_energy);
            if hp < 5 {
                penalty = 0;
            } else if hp - penalty < 5 {
                penalty = hp - 5;
            }
            pet.energy = u16::try_from(hp - penalty).unwrap_or(pet.energy);
        }
    }
    if let Some((_, _, pet)) = active_pet(m, data) {
        fx.out.to_self(pb::PetState {
            item_obj_id: u32::from(summoned),
            life: u32::from(pet.reported_life(no_pet_life)),
            energy: u32::from(pet.energy),
            exp: pet.exp,
        });
    }
    remove_bonus(m, cx, fx);
    if let Some(p) = m.player_mut() {
        p.items.pet_obj_id = None;
    }
    fx.out.near(pb::PetReleaseNtf {});
}

/// `HasPet()`: the `BUFF_PET` entry exists.
pub fn has_pet_buff(m: &Mover) -> bool {
    pet_buff_key(m).is_some()
}

fn pet_buff_key(m: &Mover) -> Option<BuffKey> {
    m.buffs
        .iter()
        .map(|(k, _)| k)
        .find(|k| k.kind == BuffKind::Pet)
}

/// `RemovePet()`: the `BUFF_PET` entry goes (its DST revert exactly) and with it the lines of an awakening.
pub fn remove_bonus(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    if let Some(key) = pet_buff_key(m) {
        remove_real(m, cx, key, fx);
    }
    // the ledger reverts exactly what the awakening lines applied (nothing when there were none)
    remove_source(m, cx.stats(), DstSource::Pet, true, &mut fx.out);
}

/// `ProcessPetAvail` for a summoned pet: when the bonus is not applied yet (`!HasPet()`), the kind's DST with
/// the sum of the rolled values becomes the `BUFF_PET` entry and its DST; the lines of a system-pet awakening
/// follow. A loaded pet already has its buff: only the awakening lines are (re)applied if they are missing.
/// A summoned slot that names no pet is left alone (the original's `PetRelease` finds no pet item and returns;
/// the join clears such a slot and `PetReleaseReq` / [`release`] do too).
pub fn apply_avail(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    let data = cx.data();
    let Some((id, item, pet)) = active_pet(m, data) else {
        return;
    };
    let item_id = item.item_id;
    let (dst, bonus) = pet.avail_dst(&data.pet);
    if !has_pet_buff(m) && dst > 0 {
        buff_add(
            m,
            cx,
            BuffKind::Pet,
            item_id,
            make_long(bonus as u32, dst),
            0,
            None,
            fx,
        );
        apply_dst(
            m,
            cx.stats(),
            source_of(BuffKey::new(BuffKind::Pet, item_id)),
            (dst, bonus, NULL_CHGPARAM),
            true,
            &mut fx.out,
        );
    }
    apply_lines(m, cx, id, fx);
}

/// `SetDestParamRandomOptExtension` of the pet item: the awakening lines, once.
pub(crate) fn apply_lines(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) {
    if m.mv.dst.has(DstSource::Pet) {
        return;
    }
    let Some(item) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) else {
        return;
    };
    let lines: Vec<(u32, i32, i32)> = item
        .random_option
        .lines()
        .map(|l| (u32::from(l.dst), i32::from(l.adj), NULL_CHGPARAM))
        .collect();
    if !lines.is_empty() {
        apply_source(m, cx.stats(), DstSource::Pet, &lines, true, &mut fx.out);
    }
}
