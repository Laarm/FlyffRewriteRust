//! `CMover::ProcessPet` of the world server (Plan/19 §13.2; src:_Common/Mover.cpp:3858-3876, 11112-11413): every 16
//! passes of a player (`m_nCount & 15 == 0`, 1.07 s at 15 Hz) the stat bonus is made sure of, the energy of the
//! pet runs down (and with it the life), and without `features.sys_petleveling_mobkill` the pet earns its
//! per-minute experience.

use super::summon::{apply_avail, release};
use super::{EV_DEATH, active_id, active_pet, audit_pet, pet_mut};
use crate::buff::core::BuffCx;
use crate::buff::set::{BuffKey, BuffKind};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::PetLevel;
use crate::item::ops::{UiParam, core as ops};
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_SYS_SYS_FEED_01, II_SYS_SYS_SCR_PET_FEED_POCKET, II_SYS_SYS_SCR_PET_FEED_POCKET02,
    II_SYS_SYS_SCR_PET_TONIC_A, II_SYS_SYS_SCR_PET_TONIC_B, II_SYS_SYS_SCR_SPETGOOD,
};
use flyff_proto::pb;
use flyff_rules::item::pet::{MAX_PET_EXP, PL_EGG, PL_S};

/// The mask of the pass: every 16 counts.
pub const PASS_MASK: u32 = 15;
/// The feed pocket eats one feed every 32 counts (`m_nCount & 0x1F`).
pub const FEED_POCKET_MASK: u32 = 0x1F;
/// The per-minute experience every 1 024 counts.
pub const EXP_MASK: u32 = 1023;

/// `ProcessPet` for one player: `counter` is the player's `m_nCount`. Nothing happens off the 16-count phase.
pub fn process(m: &mut Mover, cx: &BuffCx<'_>, counter: u32, fx: &mut Fx) {
    if counter & PASS_MASK != 0 {
        return;
    }
    apply_avail(m, cx, fx);
    process_energy(m, cx, counter, fx);
    if !cx.cfg().sys_petleveling_mobkill {
        process_exp(m, cx, counter, fx);
    }
}

/// `HasBuff(BUFF_ITEM, id)`.
fn has_item_buff(m: &Mover, id: u32) -> bool {
    m.buffs.contains(BuffKey::new(BuffKind::Item, id))
}

/// The first usable `II_SYS_SYS_FEED_01` of the inventory and its count.
fn first_feed(m: &Mover) -> Option<(StorageId, u16)> {
    m.player()?
        .items
        .inventory
        .iter()
        .find(|(_, it)| it.is_usable() && it.item_id == II_SYS_SYS_FEED_01)
        .map(|(id, it)| (id, it.count))
}

/// `ProcessPetEnergy`. The egg has no energy; the tonics (and `SPETGOOD` for an S pet) stop the loss; an active
/// feed pocket with a feed in the bag eats one feed per 32 counts instead of the energy. Otherwise the energy
/// falls by 1; at 0 the pet loses a life and its energy is refilled, a pet without a life left dies (life, energy
/// and EXP 0, the item `EXPIRED`, released). With `features.no_pet_life` the life reads 1: the pet refills for
/// ever and its stored life is not touched. `PetState` goes to the owner after each pass that ran down.
pub fn process_energy(m: &mut Mover, cx: &BuffCx<'_>, counter: u32, fx: &mut Fx) {
    if counter & PASS_MASK != 0 {
        return;
    }
    let data = cx.data();
    let no_pet_life = cx.cfg().no_pet_life;
    let Some((id, _, pet)) = active_pet(m, data) else {
        return;
    };
    if pet.level == PetLevel::Egg {
        return;
    }
    let level = pet.level;
    if has_item_buff(m, II_SYS_SYS_SCR_PET_TONIC_A) || has_item_buff(m, II_SYS_SYS_SCR_PET_TONIC_B)
    {
        return;
    }
    if level == PetLevel::S
        && m.buffs
            .contains(BuffKey::new(BuffKind::Item2, II_SYS_SYS_SCR_SPETGOOD))
    {
        return;
    }
    let pocket = has_item_buff(m, II_SYS_SYS_SCR_PET_FEED_POCKET)
        || has_item_buff(m, II_SYS_SYS_SCR_PET_FEED_POCKET02);
    if pocket && let Some((feed, count)) = first_feed(m) {
        if counter & FEED_POCKET_MASK == 0 {
            ops::update_item(
                m,
                &cx.env,
                feed,
                UiParam::Num,
                u32::from(count.saturating_sub(1)),
                0,
                fx,
            );
        }
        return;
    }
    let tables = &data.pet;
    let max_energy = u32::from(pet.max_energy(tables));
    let energy = i32::from(pet.energy) - 1;
    if energy <= 0 {
        let life = i32::from(pet.reported_life(no_pet_life)) - 1;
        if life >= 0 {
            // a life is spent, the pet is whole again
            if let Some(p) = pet_mut(m, id) {
                if !no_pet_life {
                    p.life = life as u16;
                }
                p.set_energy(tables, max_energy);
            }
            if !no_pet_life {
                audit_pet(fx, EV_DEATH, m, id, None, &[]);
            }
        } else {
            if let Some(p) = pet_mut(m, id) {
                p.life = 0;
                p.energy = 0;
                p.exp = 0;
            }
            // the dead pet's egg item is flagged expired
            expire_item(m, cx, id, fx);
            audit_pet(fx, EV_DEATH, m, id, None, &[]);
            release(m, cx, fx);
        }
    } else if let Some(p) = pet_mut(m, id) {
        p.energy = energy as u16;
    }
    send_state(m, cx, id, fx);
}

/// `AddPetState( item, life, energy, exp )` to the owner.
pub fn send_state(m: &Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) {
    let no_pet_life = cx.cfg().no_pet_life;
    let Some(pet) = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .and_then(|it| it.pet.as_deref())
    else {
        return;
    };
    fx.out.to_self(pb::PetState {
        item_obj_id: u32::from(id),
        life: u32::from(pet.reported_life(no_pet_life)),
        energy: u32::from(pet.energy),
        exp: pet.exp,
    });
}

/// `ProcessPetExp` (the pre-mob-kill leveling, off by default): once per 1 024 counts a pet of grade D to A
/// gains `IncrementExp[grade]` (x 1.5 under `PET_TONIC_B`) of its 100 000, up to the maximum.
pub fn process_exp(m: &mut Mover, cx: &BuffCx<'_>, counter: u32, fx: &mut Fx) {
    if counter & EXP_MASK != 0 {
        return;
    }
    let Some(id) = active_id(m, cx.data()) else {
        return;
    };
    let tonic_b = has_item_buff(m, II_SYS_SYS_SCR_PET_TONIC_B);
    let Some(pet) = pet_mut(m, id) else { return };
    let level = pet.level_u8();
    if level == PL_EGG || level == PL_S {
        return;
    }
    if pet.exp == MAX_PET_EXP {
        return;
    }
    let factor: f32 = if tonic_b { 1.5 } else { 1.0 };
    let inc = cx.data().pet.increment_exp(level) as f32;
    // `(DWORD)( exp + increment * factor )` in float
    let exp = (pet.exp as f32 + inc * factor) as u32;
    pet.set_exp(exp.min(MAX_PET_EXP));
    let exp = pet.exp;
    fx.out.to_self(pb::PetSetExp { exp });
}

/// Flags the pet item `EXPIRED` and tells the owner (`SetFlag( expired ); UpdateItem( UI_FLAG )`): a dead pet.
pub fn expire_item(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) {
    let flags = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .map(|it| it.flags.0);
    if let Some(flags) = flags {
        ops::set_item_flags(
            m,
            &cx.env,
            id,
            flags | flyff_rules::item::ITEM_FLAG_EXPIRED,
            fx,
        );
    }
}
